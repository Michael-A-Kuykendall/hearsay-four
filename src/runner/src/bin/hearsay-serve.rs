//! hearsay-serve: offline transcription service (bead hearsay-four-j6r, E1).
//!
//! stdin/file WAV in → stdout transcript-JSON out, on real courtroom
//! audio. Classical techniques only (windowed MFCC, Mahalanobis-ish phone
//! scoring, hash-consed arena, commit window, edit-distance lexicon lookup,
//! counted bigrams): no models, no learned weights, no STT text in scoring.
//!
//! Pipeline composition: D1 enrolled baseline (`--enroll <wav>`, fit through
//! the same front-end path as scoring, falling back to the generic reference
//! baseline) + B2 fuzzy closed vocabulary (the vendored sample dictionary
//! inside the streaming pipeline — lookup only, never downloaded) + C3 true
//! online streaming pipeline ([`run_streaming_pipeline`], incremental
//! per-chunk commit with bounded memory).
//!
//! Contract (same as hearsay-demo): every word's provenance chain is
//! re-validated before anything is printed — a violation exits 1, never a
//! dangling ref and never partial JSON on stdout. Missing input exits 2.
//!
//! Offline by construction: no sockets are opened, no network crates are
//! linked, and the only pronunciation data is the vendored dictionary.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use corpus::audio::{Pcm16kAudio, TARGET_CHANNELS, TARGET_SAMPLE_RATE};
use output::SessionMeta;
use runner::{
    StreamingConfig, StreamingOutput, generic_reference_baseline, run_streaming_pipeline,
};

/// Usage text, printed by `--help` (stdout, exit 0) and appended to every
/// exit-2 usage error on stderr.
const USAGE: &str = "\
hearsay-serve: offline transcription service (E1, bead hearsay-four-j6r)\n\
\n\
usage: hearsay-serve [--input <audio.wav|->] [--enroll <enroll.wav>] \\\n\
                     [--max-secs N] [--chunk-secs N]\n\
  --input <path>   WAV file to transcribe; `-` or omitted reads WAV bytes\n\
                   from stdin (pipe, never a TTY prompt)\n\
  --enroll <path>  D1 enrollment clip: the scoring baseline is fit from this\n\
                   audio through the front-end path instead of the generic\n\
                   reference baseline\n\
  --max-secs N     cap on audio processed, seconds (default 12, 0 = no cap)\n\
  --chunk-secs N   audio seconds per committed word (default 2)\n\
\n\
defaults: --max-secs 12 --chunk-secs 2 (one committed word per chunk)\n\
stdout: transcript JSON (`transcript` + `stats`); stderr: errors only.\n\
offline: no sockets, no network crates, vendored dictionary only.";

/// Parsed CLI request.
#[derive(Debug, Clone)]
struct ServeRequest {
    /// `None` (or `Some("-")`) means: read WAV bytes from stdin.
    input: Option<PathBuf>,
    /// D1 enrollment clip for the scoring baseline (`None` = generic ref).
    enroll: Option<PathBuf>,
    max_secs: f64,
    chunk_secs: f64,
}

/// `--help` vs a runnable request.
enum ServeAction {
    Help,
    Run(ServeRequest),
}

/// A serve failure: process exit `code` with `stderr` text (stdout stays
/// empty — never partial JSON).
struct ServeFailure {
    code: i32,
    stderr: String,
}

impl ServeFailure {
    fn usage(msg: String) -> Self {
        Self {
            code: 2,
            stderr: format!("hearsay-serve: {msg}\n{USAGE}\n"),
        }
    }

    fn failed(msg: String) -> Self {
        Self {
            code: 1,
            stderr: format!("hearsay-serve: {msg}\n"),
        }
    }
}

fn parse_secs(flag: &str, value: &str) -> Result<f64, ServeFailure> {
    match value.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(v),
        _ => Err(ServeFailure::usage(format!(
            "{flag} must be a finite number >= 0 (got {value:?})"
        ))),
    }
}

fn parse_args(args: &[String]) -> Result<ServeAction, ServeFailure> {
    let mut input: Option<PathBuf> = None;
    let mut enroll: Option<PathBuf> = None;
    let mut max_secs = 12.0f64;
    let mut chunk_secs = 2.0f64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                i += 1;
                if i >= args.len() {
                    return Err(ServeFailure::usage("--input needs a value".to_string()));
                }
                input = Some(PathBuf::from(&args[i]));
            }
            "--enroll" => {
                i += 1;
                if i >= args.len() {
                    return Err(ServeFailure::usage("--enroll needs a value".to_string()));
                }
                enroll = Some(PathBuf::from(&args[i]));
            }
            "--max-secs" => {
                i += 1;
                if i >= args.len() {
                    return Err(ServeFailure::usage("--max-secs needs a value".to_string()));
                }
                max_secs = parse_secs("--max-secs", &args[i])?;
            }
            "--chunk-secs" => {
                i += 1;
                if i >= args.len() {
                    return Err(ServeFailure::usage(
                        "--chunk-secs needs a value".to_string(),
                    ));
                }
                chunk_secs = parse_secs("--chunk-secs", &args[i])?;
                if chunk_secs <= 0.0 {
                    return Err(ServeFailure::usage(
                        "--chunk-secs must be positive".to_string(),
                    ));
                }
            }
            "--help" | "-h" => {
                return Ok(ServeAction::Help);
            }
            other => {
                return Err(ServeFailure::usage(format!("unknown arg {other:?}")));
            }
        }
        i += 1;
    }
    Ok(ServeAction::Run(ServeRequest {
        input,
        enroll,
        max_secs,
        chunk_secs,
    }))
}

/// Decode WAV bytes (stdin path) into canonical 16 kHz mono PCM: `hound`
/// parses the RIFF container, then the same mono-collapse + linear-resample
/// the file path uses, so stdin and file agree exactly.
fn decode_stdin_wav(bytes: &[u8]) -> Result<Pcm16kAudio, String> {
    let reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("not a 16-bit PCM WAV: {e}"))?;
    let spec = reader.spec();
    if spec.bits_per_sample != 16 {
        return Err(format!(
            "unsupported bits_per_sample {} (stdin WAV must be 16-bit PCM)",
            spec.bits_per_sample
        ));
    }
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| format!("stdin WAV sample error: {e}"))?;
    let audio = corpus::audio::PcmAudio {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        samples,
    };
    let mono = corpus::audio::to_mono(&audio);
    let frames = corpus::audio::resample_linear(&mono, audio.sample_rate, TARGET_SAMPLE_RATE);
    if frames.is_empty() {
        return Err("decoded zero frames".to_string());
    }
    Ok(Pcm16kAudio {
        sample_rate: TARGET_SAMPLE_RATE,
        channels: TARGET_CHANNELS,
        frames,
    })
}

/// D1 enrollment-as-baseline wiring: fit a Gaussian baseline from an
/// enrollment clip through the same front-end path as scoring
/// (window → extract → fit). See `accent::baseline_from_frames` for the
/// consumer-side twin; this is the service-side entry.
fn enrolled_baseline(pcm: &Pcm16kAudio) -> Result<calibration::Baseline, String> {
    if pcm.frames.is_empty() {
        return Err("no enrollment samples".to_string());
    }
    let windows = fse::window_audio(&pcm.frames, pcm.sample_rate);
    if windows.is_empty() {
        return Err("enrollment produced no windows (clip too short)".to_string());
    }
    let frames = fse::extract_features(&windows, pcm.sample_rate)
        .map_err(|e| format!("enrollment feature extraction failed: {e}"))?;
    calibration::fit_baseline(&frames).ok_or_else(|| "enrollment produced no frames".to_string())
}

/// Validate every word's chain, then serialize transcript + streaming stats.
///
/// Mirrors the demo's contract: termination at frame-level acoustic evidence
/// is enforced before anything is emitted, so stdout JSON is as trustworthy
/// as the one-screen render. `Err` means the caller must exit 1 with empty
/// stdout — never partial JSON.
fn render_json(out: &StreamingOutput) -> Result<String, String> {
    for word in &out.transcript.words {
        word.validate().map_err(|failure| {
            format!(
                "provenance contract failed for {:?}: {failure:?}",
                word.text
            )
        })?;
    }
    let value = serde_json::json!({
        "transcript": &out.transcript,
        "stats": {
            "frame_count": out.stats.frame_count,
            "chunk_count": out.stats.chunk_count,
            "arena_facts": out.stats.arena_facts,
            "word_count": out.stats.word_count,
            "evicted_facts": out.stats.evicted_facts,
            "active_facts": out.stats.active_facts,
            "total_facts": out.stats.total_facts,
            "repair_count": out.stats.repair_count,
        },
    });
    serde_json::to_string_pretty(&value).map_err(|e| format!("JSON serialization failed: {e}"))
}

/// Finalize one validated output: `(stdout, stderr, exit_code)`. `Ok` render
/// is the ONLY path with non-empty stdout; every failure is `(empty, msg, 1)`.
fn emit(out: &StreamingOutput) -> (String, String, i32) {
    match render_json(out) {
        Ok(rendered) => (format!("{rendered}\n"), String::new(), 0),
        Err(e) => {
            let failure = ServeFailure::failed(e);
            (String::new(), failure.stderr, failure.code)
        }
    }
}

/// Serve one request: `(stdout_text, stderr_text, exit_code)`.
///
/// `stdin_bytes`: `None` = stdin unavailable (TTY — a missing-input error
/// when no `--input` file was given); `Some(bytes)` = piped stdin content
/// (empty = missing input, exit 2).
fn serve(req: &ServeRequest, stdin_bytes: Option<&[u8]>) -> (String, String, i32) {
    let fail = |failure: ServeFailure| (String::new(), failure.stderr, failure.code);

    // Input: file WAV, or stdin WAV bytes (`-` or omitted `--input`).
    let wants_stdin = match &req.input {
        None => true,
        Some(path) => path.as_os_str() == "-",
    };
    let (pcm, recording_ref) = if wants_stdin {
        match stdin_bytes {
            None | Some([]) => {
                return fail(ServeFailure::usage(
                    "no input: pass --input <audio.wav> or pipe WAV bytes on stdin".to_string(),
                ));
            }
            Some(bytes) => match decode_stdin_wav(bytes) {
                Ok(pcm) => (pcm, "stdin".to_string()),
                Err(e) => return fail(ServeFailure::failed(format!("stdin decode failed: {e}"))),
            },
        }
    } else {
        let path = req.input.as_ref().expect("file input selected");
        match corpus::audio::decode_to_pcm16k(path) {
            Ok(pcm) => (pcm, path.display().to_string()),
            Err(e) => {
                return fail(ServeFailure::failed(format!(
                    "decode failed for {}: {e}",
                    path.display()
                )));
            }
        }
    };

    // Baseline: D1 enrolled clip when given, else the generic reference.
    let baseline = match &req.enroll {
        Some(path) => {
            let enroll_pcm = match corpus::audio::decode_to_pcm16k(path) {
                Ok(pcm) => pcm,
                Err(e) => {
                    return fail(ServeFailure::failed(format!(
                        "enroll decode failed for {}: {e}",
                        path.display()
                    )));
                }
            };
            match enrolled_baseline(&enroll_pcm) {
                Ok(baseline) => baseline,
                Err(e) => return fail(ServeFailure::failed(format!("enrollment failed: {e}"))),
            }
        }
        None => generic_reference_baseline(),
    };

    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: "speaker-1".to_string(),
        recording_ref,
        started_ms: 0,
    };
    let config = StreamingConfig {
        chunk_secs: req.chunk_secs,
        max_secs: req.max_secs,
        window_width: 0,
    };
    // C3 streaming pipeline over the B2 vendored closed vocabulary (the
    // dictionary lives inside the pipeline — lookup only, never fetched).
    let out = match run_streaming_pipeline(&pcm, &baseline, &codebook, session, &config) {
        Ok(out) => out,
        Err(e) => return fail(ServeFailure::failed(format!("pipeline failed: {e}"))),
    };

    // Re-validate every chain before print: a violation exits 1 with empty
    // stdout, never partial JSON.
    emit(&out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let req = match parse_args(&args) {
        Ok(ServeAction::Help) => {
            println!("{USAGE}");
            return;
        }
        Ok(ServeAction::Run(req)) => req,
        Err(failure) => {
            eprint!("{}", failure.stderr);
            std::process::exit(failure.code);
        }
    };
    let wants_stdin = match &req.input {
        None => true,
        Some(path) => path.as_os_str() == "-",
    };
    // A TTY is not piped audio: treat it as missing input (exit 2) instead
    // of blocking on a read that never arrives.
    let stdin_bytes: Option<Vec<u8>> = if !wants_stdin || std::io::stdin().is_terminal() {
        None
    } else {
        let mut buf = Vec::new();
        match std::io::stdin().read_to_end(&mut buf) {
            Ok(_) => Some(buf),
            Err(e) => {
                eprintln!("hearsay-serve: stdin read failed: {e}");
                std::process::exit(1);
            }
        }
    };
    let (stdout_text, stderr_text, code) = serve(&req, stdin_bytes.as_deref());
    if !stdout_text.is_empty() {
        print!("{stdout_text}");
    }
    if !stderr_text.is_empty() {
        eprint!("{stderr_text}");
    }
    std::process::exit(code);
}

#[cfg(test)]
mod serve_tests {
    //! Offline service gates (bead hearsay-four-j6r, E1).
    //!
    //! Model-free, fixture WAVs + Oyez sessions only, offline: counting and
    //! table lookup only — no sockets, no network. The fixture is the
    //! committed `data/demo/sample.wav` courtroom clip (8 s mono 16 kHz).
    use super::*;
    use output::Transcript;

    /// Committed courtroom fixture, relative to the runner package root
    /// (cargo test cwd), exactly like the existing runner suite's fixtures.
    const FIXTURE: &str = "../../data/demo/sample.wav";

    fn fixture_request() -> ServeRequest {
        ServeRequest {
            input: Some(PathBuf::from(FIXTURE)),
            enroll: None,
            // First 4 s (2 chunks): enough to prove end-to-end transcription
            // without paying for the full 8 s clip in every gate run.
            max_secs: 4.0,
            chunk_secs: 2.0,
        }
    }

    fn transcript_of(stdout: &str) -> Transcript {
        let v: serde_json::Value =
            serde_json::from_str(stdout).expect("stdout is valid transcript JSON");
        assert!(v.get("transcript").is_some(), "transcript section present");
        assert!(v.get("stats").is_some(), "stats section present");
        serde_json::from_value(v["transcript"].clone()).expect("transcript shape deserializes")
    }

    /// (1) Fixture WAV in → valid transcript JSON out, and every word's
    /// chain passes `WordRecord::validate` (the pre-print contract).
    #[test]
    fn serve_fixture_wav_produces_valid_json() {
        let (stdout, stderr, code) = serve(&fixture_request(), None);
        assert_eq!(code, 0, "fixture transcribes clean (stderr: {stderr})");
        assert!(stderr.is_empty(), "no stderr on success (got {stderr:?})");
        let transcript = transcript_of(&stdout);
        assert!(
            !transcript.words.is_empty(),
            "transcript non-empty on courtroom fixture"
        );
        for word in &transcript.words {
            word.validate()
                .expect("chain terminates at frame-level acoustic evidence");
        }
        let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout re-parses as JSON");
        assert_eq!(
            v["stats"]["word_count"].as_u64().unwrap_or(0) as usize,
            transcript.words.len(),
            "stats agree with the transcript"
        );
    }

    /// (2) A chain violation exits 1 with NO partial JSON on stdout: the
    /// render is all-or-nothing before print, so stderr carries the failure
    /// and stdout stays empty.
    #[test]
    fn serve_chain_violation_yields_exit_1_without_partial_json() {
        let pcm = corpus::audio::decode_to_pcm16k(std::path::Path::new(FIXTURE))
            .expect("fixture decodes");
        let take = ((pcm.sample_rate as f64 * 4.0) as usize).min(pcm.frames.len());
        let pcm = Pcm16kAudio {
            sample_rate: pcm.sample_rate,
            channels: pcm.channels,
            frames: pcm.frames[..take].to_vec(),
        };
        let out = run_streaming_pipeline(
            &pcm,
            &generic_reference_baseline(),
            &fse::default_codebook(),
            SessionMeta {
                speaker_id: "test".to_string(),
                recording_ref: "violation-fixture".to_string(),
                started_ms: 0,
            },
            &StreamingConfig {
                chunk_secs: 2.0,
                max_secs: 4.0,
                window_width: 0,
            },
        )
        .expect("pipeline on fixture");
        assert!(
            !out.transcript.words.is_empty(),
            "violation fixture needs at least one word"
        );
        // Break the grounding: strip every layer so validation must fail.
        let mut bad = out;
        bad.transcript.words[0].provenance.layers.clear();
        assert!(render_json(&bad).is_err(), "stripped chain must not render");
        let (stdout, stderr, code) = emit(&bad);
        assert_eq!(code, 1, "chain violation exits 1");
        assert!(
            stdout.is_empty(),
            "no partial JSON on stdout (got {stdout:?})"
        );
        assert!(
            stderr.contains("provenance contract failed"),
            "stderr names the contract (got {stderr:?})"
        );
    }

    /// (3) Missing input (no `--input`, no piped stdin — or empty piped
    /// stdin) is a usage error: stderr explains, stdout stays empty, exit 2.
    #[test]
    fn serve_missing_input_exits_2_with_stderr() {
        let req = ServeRequest {
            input: None,
            enroll: None,
            max_secs: 4.0,
            chunk_secs: 2.0,
        };
        let (stdout, stderr, code) = serve(&req, None);
        assert_eq!(code, 2, "missing input exits 2");
        assert!(stdout.is_empty(), "no stdout on usage error");
        assert!(
            stderr.contains("--input"),
            "stderr names the missing input (got {stderr:?})"
        );
        let (stdout_empty, stderr_empty, code_empty) = serve(&req, Some(&[]));
        assert_eq!(code_empty, 2, "empty piped stdin exits 2");
        assert!(stdout_empty.is_empty(), "no stdout on empty stdin");
        assert!(!stderr_empty.is_empty(), "stderr explains the empty stdin");
    }

    /// (4) D1 enrolled baseline path: `--enroll` fits the scoring baseline
    /// from enrollment audio through the front-end path (window → extract →
    /// fit), and the enrolled run still transcribes the fixture to valid,
    /// fully validated transcript JSON.
    #[test]
    fn serve_enrolled_baseline_transcribes_fixture() {
        // The fit itself is a real enrollment baseline, not the generic ref.
        let enroll_pcm = corpus::audio::decode_to_pcm16k(std::path::Path::new(FIXTURE))
            .expect("enrollment clip decodes");
        let enrolled = enrolled_baseline(&enroll_pcm).expect("enrollment fits");
        assert!(enrolled.is_valid(), "enrolled baseline is a valid payload");
        // End to end through the service request with `--enroll` set.
        let req = ServeRequest {
            enroll: Some(PathBuf::from(FIXTURE)),
            ..fixture_request()
        };
        let (stdout, stderr, code) = serve(&req, None);
        assert_eq!(code, 0, "enrolled run transcribes clean (stderr: {stderr})");
        let transcript = transcript_of(&stdout);
        assert!(
            !transcript.words.is_empty(),
            "enrolled transcript non-empty"
        );
        for word in &transcript.words {
            word.validate()
                .expect("enrolled chain terminates at acoustic evidence");
        }
    }

    /// (5) C1 rating invariants enforced on the serve path (bead
    /// hearsay-four-aff, E3): a served transcript whose node rating or
    /// propagated source rating leaves the Hearsay-II 0-100 range is
    /// rejected before print — exit 1 with empty stdout, never partial
    /// JSON. The violation fixture is the real pipeline output with one
    /// rating pushed out of range.
    #[test]
    fn serve_out_of_range_rating_yields_exit_1_without_partial_json() {
        let pcm = corpus::audio::decode_to_pcm16k(std::path::Path::new(FIXTURE))
            .expect("fixture decodes");
        let take = ((pcm.sample_rate as f64 * 4.0) as usize).min(pcm.frames.len());
        let pcm = Pcm16kAudio {
            sample_rate: pcm.sample_rate,
            channels: pcm.channels,
            frames: pcm.frames[..take].to_vec(),
        };
        let run = || {
            run_streaming_pipeline(
                &pcm,
                &generic_reference_baseline(),
                &fse::default_codebook(),
                SessionMeta {
                    speaker_id: "test".to_string(),
                    recording_ref: "rating-violation-fixture".to_string(),
                    started_ms: 0,
                },
                &StreamingConfig {
                    chunk_secs: 2.0,
                    max_secs: 4.0,
                    window_width: 0,
                },
            )
            .expect("pipeline on fixture")
        };
        let good = run();
        assert!(
            !good.transcript.words.is_empty(),
            "rating fixture needs at least one word"
        );
        assert!(
            render_json(&good).is_ok(),
            "unmodified pipeline output renders"
        );

        // Case A: a chain node rating pushed above 100 (the lexical layer
        // always carries exactly one node — see the chain built above the
        // `WordRecord::committed_from_live` call in the runner).
        let mut bad_node = good.clone();
        bad_node.transcript.words[0].provenance.layers[2].nodes[0].rating = 150.0;
        assert!(
            render_json(&bad_node).is_err(),
            "node rating 150.0 must not render"
        );
        let (stdout, stderr, code) = emit(&bad_node);
        assert_eq!(code, 1, "out-of-range node rating exits 1");
        assert!(stdout.is_empty(), "no partial JSON on stdout");
        assert!(
            stderr.contains("provenance contract failed"),
            "stderr names the contract (got {stderr:?})"
        );

        // Case B: the propagated stage-3 source rating pushed above 100.
        let mut bad_source = run();
        bad_source.transcript.words[0].source_rating = 150.0;
        assert!(
            render_json(&bad_source).is_err(),
            "source rating 150.0 must not render"
        );
        let (stdout, stderr, code) = emit(&bad_source);
        assert_eq!(code, 1, "out-of-range source rating exits 1");
        assert!(stdout.is_empty(), "no partial JSON on stdout");
        assert!(
            stderr.contains("provenance contract failed"),
            "stderr names the contract (got {stderr:?})"
        );
    }
}
