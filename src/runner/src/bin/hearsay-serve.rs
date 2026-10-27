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
//! dangling ref and never partial JSON on stdout. Missing or undecodable
//! input (MP3/WAV sniff fails, corrupt bytes) exits 2, also with empty
//! stdout.
//!
//! Offline by construction: no sockets are opened, no network crates are
//! linked, and the only pronunciation data is the vendored dictionary.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use corpus::audio::Pcm16kAudio;
use output::SessionMeta;
use runner::live::{LiveError, LiveSource, PcmFrameStream, run_live_session};
use runner::{
    StreamingConfig, StreamingOutput, generic_reference_baseline, run_streaming_pipeline,
};

/// Usage text, printed by `--help` (stdout, exit 0) and appended to every
/// exit-2 usage error on stderr.
const USAGE: &str = "\
hearsay-serve: offline transcription service (E1, bead hearsay-four-j6r;
               live ingest MIC-1, bead hearsay-four-4zx)\n\
\n\
usage: hearsay-serve [--input <audio.mp3|wav|->] [--enroll <enroll.mp3|wav>] \\\n\
                     [--max-secs N] [--chunk-secs N] [--live [--live-frame-ms N]]\n\
  --input <path>   MP3 or WAV file to transcribe (container sniffed from\n\
                   content, not the extension); `-` or omitted reads\n\
                   MP3/WAV bytes from stdin (pipe, never a TTY prompt)\n\
  --enroll <path>  D1 enrollment clip (MP3 or WAV, sniffed like --input):\n\
                   the scoring baseline is fit from this audio through\n\
                   the front-end path instead of the generic reference\n\
                   baseline\n\
  --max-secs N     cap on audio processed, seconds (default 12, 0 = no cap)\n\
  --chunk-secs N   audio seconds per committed word (default 2)
  --live           live-ingest mode (MIC-1): stream the input in mic-sized
                   frames through the C3 pipeline incrementally (same words
                   as batch), printing provisional words to stderr as they
                   commit, then the final transcript JSON on stdout
  --live-frame-ms N
                   synthetic-live frame size in ms (default 100, positive
                   integer); the hermetic gate replays a fixture file in
                   these frames instead of a physical mic\n\
\n\
live exits: 0 transcribed; 1 pipeline/provenance failure (empty stdout);
        2 input/device error — incl. a dead live device — (empty stdout,
        never a silent stall).
defaults: --max-secs 12 --chunk-secs 2 (one committed word per chunk)\n\
stdout: transcript JSON (`transcript` + `stats`); stderr: errors only.\n\
offline: no sockets, no network crates, vendored dictionary only.";

/// Parsed CLI request.
#[derive(Debug, Clone)]
struct ServeRequest {
    /// `None` (or `Some("-")`) means: read MP3/WAV bytes from stdin.
    input: Option<PathBuf>,
    /// D1 enrollment clip for the scoring baseline (`None` = generic ref).
    enroll: Option<PathBuf>,
    max_secs: f64,
    chunk_secs: f64,
    /// Live-ingest mode: stream the input in mic-sized frames through the
    /// C3 pipeline incrementally (MIC-1).
    live: bool,
    /// Synthetic-live frame size in ms for `--live` (default 100).
    live_frame_ms: u64,
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

    /// Undecodable input (missing file, sniff failure, corrupt MP3/WAV):
    /// exit 2 like a usage error — the request never reached the pipeline —
    /// but without the usage dump, since the CLI shape was fine.
    fn input(msg: String) -> Self {
        Self {
            code: 2,
            stderr: format!("hearsay-serve: {msg}\n"),
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
    let mut live = false;
    let mut live_frame_ms = 100u64;
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
            "--live" => {
                live = true;
            }
            "--live-frame-ms" => {
                i += 1;
                if i >= args.len() {
                    return Err(ServeFailure::usage(
                        "--live-frame-ms needs a value".to_string(),
                    ));
                }
                match args[i].parse::<u64>() {
                    Ok(v) if v > 0 => {
                        live_frame_ms = v;
                    }
                    _ => {
                        return Err(ServeFailure::usage(format!(
                            "--live-frame-ms must be a positive integer of milliseconds (got {:?})",
                            args[i]
                        )));
                    }
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
        live,
        live_frame_ms,
    }))
}

/// Decode piped-stdin bytes into canonical 16 kHz mono PCM, sniffing the
/// container exactly like the file path ([`corpus::audio::decode_bytes`]:
/// RIFF bytes decode as WAV, MPEG-sync/ID3 bytes as MP3) and sharing the
/// same mono-collapse + linear-resample core, so stdin and file agree
/// exactly for identical audio bytes.
fn decode_stdin_audio(bytes: &[u8]) -> Result<Pcm16kAudio, String> {
    corpus::audio::decode_bytes_to_pcm16k(bytes)
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

/// Decode the request input: file MP3/WAV (sniffed), or stdin MP3/WAV bytes
/// (`-` or omitted `--input`). Any input that never decodes — missing file,
/// unknown container, corrupt bytes — is an input error (exit 2): the
/// request never reached the pipeline, and stdout stays empty (never
/// partial JSON). Shared by the batch and `--live` paths so both agree
/// exactly on what counts as input.
fn decode_input(
    req: &ServeRequest,
    stdin_bytes: Option<&[u8]>,
) -> Result<(Pcm16kAudio, String), ServeFailure> {
    let wants_stdin = match &req.input {
        None => true,
        Some(path) => path.as_os_str() == "-",
    };
    if wants_stdin {
        match stdin_bytes {
            None | Some([]) => Err(ServeFailure::usage(
                "no input: pass --input <audio.mp3|wav> or pipe MP3/WAV bytes on stdin".to_string(),
            )),
            Some(bytes) => match decode_stdin_audio(bytes) {
                Ok(pcm) => Ok((pcm, "stdin".to_string())),
                Err(e) => Err(ServeFailure::input(format!("stdin decode failed: {e}"))),
            },
        }
    } else {
        let path = req.input.as_ref().expect("file input selected");
        match corpus::audio::decode_to_pcm16k(path) {
            Ok(pcm) => Ok((pcm, path.display().to_string())),
            Err(e) => Err(ServeFailure::input(format!(
                "decode failed for {}: {e}",
                path.display()
            ))),
        }
    }
}

/// Baseline for a request: D1 enrolled clip when given, else the generic
/// reference. An undecodable enrollment clip is an input error (exit 2),
/// like the main input: scoring never started.
fn load_baseline(req: &ServeRequest) -> Result<calibration::Baseline, ServeFailure> {
    match &req.enroll {
        Some(path) => {
            let enroll_pcm = match corpus::audio::decode_to_pcm16k(path) {
                Ok(pcm) => pcm,
                Err(e) => {
                    return Err(ServeFailure::input(format!(
                        "enroll decode failed for {}: {e}",
                        path.display()
                    )));
                }
            };
            match enrolled_baseline(&enroll_pcm) {
                Ok(baseline) => Ok(baseline),
                Err(e) => Err(ServeFailure::input(format!("enrollment failed: {e}"))),
            }
        }
        None => Ok(generic_reference_baseline()),
    }
}

/// Serve one request: `(stdout_text, stderr_text, exit_code)`.
///
/// `stdin_bytes`: `None` = stdin unavailable (TTY — a missing-input error
/// when no `--input` file was given); `Some(bytes)` = piped stdin content
/// (empty = missing input, exit 2).
fn serve(req: &ServeRequest, stdin_bytes: Option<&[u8]>) -> (String, String, i32) {
    let fail = |failure: ServeFailure| (String::new(), failure.stderr, failure.code);

    let (pcm, recording_ref) = match decode_input(req, stdin_bytes) {
        Ok(v) => v,
        Err(failure) => return fail(failure),
    };
    let baseline = match load_baseline(req) {
        Ok(baseline) => baseline,
        Err(failure) => return fail(failure),
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
    let (stdout_text, stderr_text, code) = if req.live {
        serve_live(&req, stdin_bytes.as_deref())
    } else {
        serve(&req, stdin_bytes.as_deref())
    };
    if !stdout_text.is_empty() {
        print!("{stdout_text}");
    }
    if !stderr_text.is_empty() {
        eprint!("{stderr_text}");
    }
    std::process::exit(code);
}

/// Serve one request in live-ingest mode (MIC-1, bead hearsay-four-4zx):
/// the decoded input is streamed through the C3 pipeline in mic-sized
/// frames via [`run_live_session`] — the same incremental path a mic frame
/// source drives in production — instead of one batch call.
///
/// `(stdout_text, stderr_text, exit_code)` with the batch contract:
/// provisional words go to stderr as they commit (`live: provisional …`),
/// the final validated transcript JSON goes to stdout, and stdout stays
/// empty on any failure. A dead/missing live device exits 2 with a message
/// (never a silent stall); a pipeline failure on valid input exits 1.
fn serve_live(req: &ServeRequest, stdin_bytes: Option<&[u8]>) -> (String, String, i32) {
    let fail = |failure: ServeFailure| (String::new(), failure.stderr, failure.code);
    let (pcm, recording_ref) = match decode_input(req, stdin_bytes) {
        Ok(v) => v,
        Err(failure) => return fail(failure),
    };
    let baseline = match load_baseline(req) {
        Ok(baseline) => baseline,
        Err(failure) => return fail(failure),
    };
    // Hermetic synthetic-live input: the decoded fixture replayed in
    // mic-sized frames (no physical mic in the gate; a mic frame source
    // implements the same `LiveSource` seam in production).
    let stream = match PcmFrameStream::new(&pcm, req.live_frame_ms) {
        Ok(stream) => stream,
        Err(e) => return fail(ServeFailure::input(format!("live frame setup failed: {e}"))),
    };
    serve_live_from_source(stream, &baseline, req, &recording_ref)
}

/// Live-ingest core over any [`LiveSource`]: the hermetic gate drives this
/// directly with fixture replay ([`PcmFrameStream`]) and device-failure
/// injection (`runner::live::FailAfter`), so both gate cases run without a
/// physical mic.
fn serve_live_from_source<S: LiveSource>(
    source: S,
    baseline: &calibration::Baseline,
    req: &ServeRequest,
    recording_ref: &str,
) -> (String, String, i32) {
    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: "speaker-1".to_string(),
        recording_ref: recording_ref.to_string(),
        started_ms: 0,
    };
    let config = StreamingConfig {
        chunk_secs: req.chunk_secs,
        max_secs: req.max_secs,
        window_width: 0,
    };
    let report = match run_live_session(source, baseline, &codebook, session, config) {
        Ok(report) => report,
        Err(LiveError::Capture(msg)) => {
            return (String::new(), format!("hearsay-serve: {msg}\n"), 2);
        }
        Err(LiveError::Pipeline(msg)) => {
            return (String::new(), format!("hearsay-serve: {msg}\n"), 1);
        }
    };
    // Provisional words to stderr as they committed (every snapshot but the
    // last), then the final line; the validated transcript JSON goes to
    // stdout via `emit` under the same all-or-nothing contract as batch.
    let mut stderr = String::new();
    let (provisional, final_snapshot) = report
        .provisional
        .split_at(report.provisional.len().saturating_sub(1));
    for snapshot in provisional {
        stderr.push_str(&format!(
            "live: provisional ({} words): {}\n",
            snapshot.len(),
            snapshot.join(" ")
        ));
    }
    if let Some(final_words) = final_snapshot.first() {
        stderr.push_str(&format!(
            "live: final ({} words): {}\n",
            final_words.len(),
            final_words.join(" ")
        ));
    }
    let (stdout, emit_stderr, code) = emit(&report.output);
    stderr.push_str(&emit_stderr);
    (stdout, stderr, code)
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
            live: false,
            live_frame_ms: 100,
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
            live: false,
            live_frame_ms: 100,
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

    /// MP3-1 fixtures (bead hearsay-four-gmv): the Oyez MP3 clip, its
    /// decode-derived WAV twin, and the synthetic corrupt fixture.
    const MP3_FIXTURE: &str = "../../data/demo/serve-mp3.mp3";
    const MP3_TWIN_WAV: &str = "../../data/demo/serve-mp3-twin.wav";
    const CORRUPT_MP3: &str = "../../data/demo/corrupt.mp3";

    /// Parse stdout transcript JSON and redact the run-specific
    /// `recording_ref`, so two runs over identical PCM compare
    /// byte-identical everywhere else (words, provenance, stats).
    fn normalized_json(stdout: &str) -> serde_json::Value {
        let mut v: serde_json::Value =
            serde_json::from_str(stdout).expect("stdout is valid transcript JSON");
        if let Some(session) = v.pointer_mut("/transcript/session/recording_ref") {
            *session = serde_json::Value::String("RECORDING_REF".to_string());
        } else {
            panic!("transcript session recording_ref present");
        }
        v
    }

    fn mp3_request(input: &str) -> ServeRequest {
        ServeRequest {
            input: Some(PathBuf::from(input)),
            enroll: None,
            // First 4 s (2 chunks): enough to prove end-to-end parity.
            max_secs: 4.0,
            chunk_secs: 2.0,
            live: false,
            live_frame_ms: 100,
        }
    }

    /// MP3-1 (bead hearsay-four-gmv): the Oyez MP3 clip served via
    /// `--input` transcribes byte-identically (modulo `recording_ref`)
    /// to its WAV twin — the wired MP3 decode reproduces the WAV path.
    #[test]
    fn serve_mp3_file_matches_wav_twin_transcript() {
        let (mp3_out, mp3_err, mp3_code) = serve(&mp3_request(MP3_FIXTURE), None);
        assert_eq!(
            mp3_code, 0,
            "MP3 fixture transcribes clean (stderr: {mp3_err})"
        );
        let (wav_out, wav_err, wav_code) = serve(&mp3_request(MP3_TWIN_WAV), None);
        assert_eq!(
            wav_code, 0,
            "WAV twin transcribes clean (stderr: {wav_err})"
        );
        let mp3_json = normalized_json(&mp3_out);
        let wav_json = normalized_json(&wav_out);
        assert_eq!(
            mp3_json, wav_json,
            "MP3 transcript is byte-identical to its WAV twin (modulo recording_ref)"
        );
        let transcript = transcript_of(&mp3_out);
        assert!(
            !transcript.words.is_empty(),
            "twin transcript non-empty on courtroom audio"
        );
        for word in &transcript.words {
            word.validate()
                .expect("twin chain terminates at frame-level acoustic evidence");
        }
    }

    /// MP3-1: piped-stdin bytes are sniffed (MP3 vs WAV) and transcribe
    /// byte-identically (modulo `recording_ref`) to the same bytes served
    /// via `--input` — stdin and file agree exactly either way.
    #[test]
    fn serve_stdin_bytes_match_file_for_both_containers() {
        for fixture in [MP3_FIXTURE, MP3_TWIN_WAV] {
            let bytes = std::fs::read(fixture).expect("fixture bytes readable");
            let req = ServeRequest {
                input: None,
                ..mp3_request(fixture)
            };
            let (stdin_out, stdin_err, stdin_code) = serve(&req, Some(&bytes));
            assert_eq!(
                stdin_code, 0,
                "stdin {fixture} transcribes clean (stderr: {stdin_err})"
            );
            let (file_out, file_err, file_code) = serve(&mp3_request(fixture), None);
            assert_eq!(
                file_code, 0,
                "file {fixture} transcribes clean (stderr: {file_err})"
            );
            assert_eq!(
                normalized_json(&stdin_out),
                normalized_json(&file_out),
                "stdin bytes and --input file agree exactly for {fixture}"
            );
        }
    }

    /// MP3-1: a corrupt MP3 exits 2 with an error and never partial JSON —
    /// via `--input` and via piped-stdin bytes alike. Undecodable input
    /// never reaches the pipeline, so this is an input error (2), not a
    /// pipeline failure (1).
    #[test]
    fn serve_corrupt_mp3_exits_2_without_partial_json() {
        let (stdout, stderr, code) = serve(&mp3_request(CORRUPT_MP3), None);
        assert_eq!(code, 2, "corrupt MP3 file exits 2");
        assert!(stdout.is_empty(), "no partial JSON on stdout");
        assert!(
            stderr.contains("decode failed"),
            "stderr names the decode failure (got {stderr:?})"
        );
        let bytes = std::fs::read(CORRUPT_MP3).expect("corrupt fixture readable");
        let req = ServeRequest {
            input: None,
            ..mp3_request(CORRUPT_MP3)
        };
        let (stdin_out, stdin_err, stdin_code) = serve(&req, Some(&bytes));
        assert_eq!(stdin_code, 2, "corrupt MP3 stdin bytes exit 2");
        assert!(stdin_out.is_empty(), "no partial JSON on stdout");
        assert!(!stdin_err.is_empty(), "stderr explains the failure");
        // Unknown container bytes are an input error too, never a guess.
        let (guess_out, guess_err, guess_code) = serve(&req, Some(&[0x00, 0x01, 0x02, 0x03]));
        assert_eq!(guess_code, 2, "unknown bytes exit 2");
        assert!(guess_out.is_empty(), "no partial JSON on stdout");
        assert!(!guess_err.is_empty(), "stderr explains the failure");
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

    /// MIC-1 gate (bead hearsay-four-4zx): hermetic live run over the
    /// fixture streamed as synthetic live input (100 ms mic-sized frames,
    /// no physical mic) transcribes fully in order (exit 0), every word
    /// validating, with provisional + final progress on stderr.
    ///
    /// Model-free, fixture audio + synthetic only, offline.
    #[test]
    fn serve_live_fixture_streams_all_words_in_order_exit_0() {
        let live_req = ServeRequest {
            live: true,
            live_frame_ms: 100,
            ..fixture_request()
        };
        let (live_out, live_err, live_code) = serve_live(&live_req, None);
        assert_eq!(live_code, 0, "live run exits 0 (stderr: {live_err})");
        let live_transcript = transcript_of(&live_out);
        assert!(
            !live_transcript.words.is_empty(),
            "live transcript non-empty"
        );
        for word in &live_transcript.words {
            word.validate()
                .expect("live chain terminates at acoustic evidence");
        }
        // Same fixture through the batch path: identical words in order.
        let (batch_out, _, batch_code) = serve(&fixture_request(), None);
        assert_eq!(batch_code, 0, "batch fixture run exits 0");
        let batch_transcript = transcript_of(&batch_out);
        let live_words: Vec<&str> = live_transcript
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        let batch_words: Vec<&str> = batch_transcript
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        assert_eq!(
            live_words, batch_words,
            "live commits all fixture words in order"
        );
        assert!(
            live_err.contains("live: final"),
            "stderr prints the final line (got {live_err:?})"
        );
        assert!(
            live_err.contains("live: provisional"),
            "stderr prints provisional commits (got {live_err:?})"
        );
    }

    /// MIC-1 gate (bead hearsay-four-4zx): a dead or dying live device
    /// surfaces as exit 2 with a message on stderr and empty stdout — never
    /// a silent stall — immediately (zero frames) and mid-stream (after a
    /// good frame already landed).
    #[test]
    fn serve_live_device_error_exits_2_without_stall() {
        use runner::live::FailAfter;

        let full = corpus::audio::decode_to_pcm16k(std::path::Path::new(FIXTURE))
            .expect("fixture decodes");
        let take = ((full.sample_rate as f64 * 4.0) as usize).min(full.frames.len());
        let fixture_pcm = || Pcm16kAudio {
            sample_rate: full.sample_rate,
            channels: full.channels,
            frames: full.frames[..take].to_vec(),
        };
        let live_req = ServeRequest {
            live: true,
            live_frame_ms: 100,
            ..fixture_request()
        };
        let baseline = generic_reference_baseline();

        // Immediate: the device fails on the first frame.
        let dead = FailAfter::new(
            PcmFrameStream::new(&fixture_pcm(), 100).expect("frame stream"),
            0,
            "no default input device",
        );
        let (stdout, stderr, code) =
            serve_live_from_source(dead, &baseline, &live_req, "device-error-fixture");
        assert_eq!(code, 2, "dead device exits 2");
        assert!(stdout.is_empty(), "no partial JSON on stdout");
        assert!(
            stderr.contains("no default input device"),
            "stderr names the failure (got {stderr:?})"
        );

        // Mid-stream: one good frame lands, then the device drops.
        let dying = FailAfter::new(
            PcmFrameStream::new(&fixture_pcm(), 100).expect("frame stream"),
            1,
            "capture stream error: device disconnected",
        );
        let (mid_out, mid_err, mid_code) =
            serve_live_from_source(dying, &baseline, &live_req, "device-error-fixture");
        assert_eq!(mid_code, 2, "mid-stream failure exits 2");
        assert!(mid_out.is_empty(), "no partial JSON on stdout");
        assert!(
            mid_err.contains("device disconnected"),
            "stderr names the failure (got {mid_err:?})"
        );
    }

    /// MIC-1 hermetic-backend check (bead hearsay-four-4zx): the
    /// `CaptureBackend` stub (`StubCapture`, `test-utils` feature) produces
    /// a valid enrollment alongside the live run — the gate exercises the
    /// no-hardware backend seam, never a physical mic.
    #[test]
    fn stub_capture_backend_stays_valid_for_hermetic_live_gate() {
        use calibration::enrollment::{StubCapture, capture_enrollment};

        let mut backend = StubCapture::default();
        let enrollment = capture_enrollment(&mut backend);
        assert!(
            enrollment.prompt_is_fixed(),
            "stub enrollment carries the fixed prompt"
        );
        assert!(enrollment.is_valid(), "stub enrollment frames are valid");
    }
}
