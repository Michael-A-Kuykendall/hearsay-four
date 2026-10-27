//! wer-bulk: absolute WER driver on the Oyez bulk sessions
//! (bead hearsay-four-4b7, WER-1).
//!
//! Transcribes `sessions/oyez-bulk/*/audio/run1.wav` end to end through the
//! same classical pipeline `hearsay-serve` uses
//! ([`runner::run_streaming_pipeline`]: windowed MFCC, phone scoring,
//! hash-consed arena, commit window, edit-distance lexicon lookup, counted
//! bigrams — no models, no learned weights, no network) and scores the
//! committed words against the A1 ground-truth word list extracted from the
//! committed `session.tsv` manifest
//! ([`corpus::ground_truth::extract_words`]) with [`output::wer`].
//!
//! Writes per-case and overall WER (S/D/I breakdown) to
//! `<root>/WER-REPORT.md`. `--root` resolves relative to the process CWD,
//! exactly like `failfast_run --root` (run from `src/runner` with
//! `--root ../../sessions/oyez-bulk`).
//!
//! Offline by construction: WAV + TSV in, markdown out. Exit 0 writes the
//! report; exit 1 is a pipeline/scoring failure; exit 2 is usage or I/O.

use std::path::{Path, PathBuf};

use corpus::audio::Pcm16kAudio;
use corpus::ground_truth::TimedSegment;
use output::SessionMeta;
use runner::{StreamingConfig, generic_reference_baseline, run_streaming_pipeline};

/// Bulk cases, oldest term first (matches `MANIFEST.json` case order).
const CASES: &[&str] = &["70-112_1971", "10-218_2011", "24-889_2025"];

/// Usage text, printed by `--help` (stdout, exit 0) and appended to every
/// exit-2 usage error on stderr.
const USAGE: &str = "\
wer-bulk: absolute WER on Oyez bulk sessions (WER-1, bead hearsay-four-4b7)\n\
\n\
usage: wer-bulk --root <sessions/oyez-bulk> [--chunk-secs N] [--max-secs N]\n\
  --root <dir>   bulk session dir holding <case>/session.tsv +\n\
                 <case>/audio/run1.wav (resolves relative to CWD)\n\
  --chunk-secs N audio seconds per committed word (default 2)\n\
  --max-secs N   cap audio per case, seconds (default 0 = full file)\n\
\n\
stdout: per-case + overall WER lines; <root>/WER-REPORT.md: the report.\n\
offline: WAV + TSV in, markdown out; no sockets, no downloads.";

struct Request {
    root: PathBuf,
    chunk_secs: f64,
    max_secs: f64,
}

enum Action {
    Help,
    Run(Request),
}

struct Failure {
    code: i32,
    stderr: String,
}

impl Failure {
    fn usage(msg: String) -> Self {
        Self {
            code: 2,
            stderr: format!("wer-bulk: {msg}\n{USAGE}\n"),
        }
    }

    fn failed(msg: String) -> Self {
        Self {
            code: 1,
            stderr: format!("wer-bulk: {msg}\n"),
        }
    }
}

fn parse_secs(flag: &str, value: &str) -> Result<f64, Failure> {
    match value.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(v),
        _ => Err(Failure::usage(format!(
            "{flag} must be a finite number >= 0 (got {value:?})"
        ))),
    }
}

fn parse_args(args: &[String]) -> Result<Action, Failure> {
    let mut root: Option<PathBuf> = None;
    let mut chunk_secs = 2.0f64;
    let mut max_secs = 0.0f64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                i += 1;
                match args.get(i) {
                    Some(d) => root = Some(PathBuf::from(d)),
                    None => {
                        return Err(Failure::usage("--root needs a directory".to_string()));
                    }
                }
            }
            "--chunk-secs" => {
                i += 1;
                match args.get(i) {
                    Some(v) => {
                        chunk_secs = parse_secs("--chunk-secs", v)?;
                        if chunk_secs <= 0.0 {
                            return Err(Failure::usage(
                                "--chunk-secs must be positive".to_string(),
                            ));
                        }
                    }
                    None => {
                        return Err(Failure::usage("--chunk-secs needs a value".to_string()));
                    }
                }
            }
            "--max-secs" => {
                i += 1;
                match args.get(i) {
                    Some(v) => max_secs = parse_secs("--max-secs", v)?,
                    None => {
                        return Err(Failure::usage("--max-secs needs a value".to_string()));
                    }
                }
            }
            "--help" | "-h" => return Ok(Action::Help),
            other => {
                return Err(Failure::usage(format!("unknown arg {other:?}")));
            }
        }
        i += 1;
    }
    match root {
        Some(root) => Ok(Action::Run(Request {
            root,
            chunk_secs,
            max_secs,
        })),
        None => Err(Failure::usage(
            "--root <sessions/oyez-bulk> is required".to_string(),
        )),
    }
}

/// One manifest row of `session.tsv`: `bucket, idx, start_sec, stop_sec,
/// speaker, drift_tags, text` (emitted by `scripts/oyez_ingest.py`).
fn parse_session_tsv(text: &str) -> Result<Vec<TimedSegment>, String> {
    let mut lines = text.lines();
    let header = lines.next().ok_or("session.tsv is empty")?;
    if !header.starts_with("bucket\t") {
        return Err(format!("session.tsv bad header: {header:?}"));
    }
    let mut rows = Vec::new();
    for (lineno, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(7, '\t');
        let bucket = cols.next().unwrap_or("");
        let idx = cols.next().unwrap_or("");
        let start = cols
            .next()
            .ok_or(format!("line {}: missing start_sec", lineno + 2))?;
        let stop = cols
            .next()
            .ok_or(format!("line {}: missing stop_sec", lineno + 2))?;
        // speaker + drift_tags are carried for provenance only.
        let _speaker = cols.next().unwrap_or("");
        let _tags = cols.next().unwrap_or("");
        let word_text = cols.next().unwrap_or("");
        let start_sec: f64 = start.parse().map_err(|_| {
            format!(
                "line {} ({bucket}/{idx}): bad start_sec {start:?}",
                lineno + 2
            )
        })?;
        let stop_sec: f64 = stop.parse().map_err(|_| {
            format!(
                "line {} ({bucket}/{idx}): bad stop_sec {stop:?}",
                lineno + 2
            )
        })?;
        if !start_sec.is_finite() || !stop_sec.is_finite() {
            return Err(format!("line {}: non-finite timing", lineno + 2));
        }
        rows.push(TimedSegment::new(start_sec, stop_sec, word_text));
    }
    if rows.is_empty() {
        return Err("session.tsv has no data rows".to_string());
    }
    Ok(rows)
}

/// Per-case WER outcome: the S/D/I counts that add up to the report.
#[derive(Debug, Clone)]
struct CaseScore {
    label: String,
    reference_len: usize,
    hyp_len: usize,
    substitutions: usize,
    deletions: usize,
    insertions: usize,
    wer: f64,
    audio_secs: f64,
}

impl CaseScore {
    /// Total word errors (S+D+I) for this case.
    fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }
}

/// Transcribe one case end to end and score it: `(score, hyp_words)`.
/// Pure pipeline + A1 ground truth; the ONLY I/O is the two input files.
fn score_case(root: &Path, label: &str, req: &Request) -> Result<CaseScore, String> {
    let case_dir = root.join(label);
    let tsv_text = std::fs::read_to_string(case_dir.join("session.tsv"))
        .map_err(|e| format!("read {label}/session.tsv: {e}"))?;
    let rows = parse_session_tsv(&tsv_text)?;
    let reference: Vec<String> = corpus::ground_truth::extract_words(&rows)
        .into_iter()
        .map(|(word, _, _)| word)
        .collect();
    if reference.is_empty() {
        return Err(format!("{label}: no reference words in session.tsv"));
    }

    let wav_path = case_dir.join("audio/run1.wav");
    let pcm = corpus::audio::decode_to_pcm16k(&wav_path)
        .map_err(|e| format!("decode {label}/audio/run1.wav: {e}"))?;
    let audio_secs = pcm.duration_secs();
    let pcm = if req.max_secs > 0.0 {
        let limit = ((pcm.sample_rate as f64 * req.max_secs) as usize).min(pcm.frames.len());
        Pcm16kAudio {
            sample_rate: pcm.sample_rate,
            channels: pcm.channels,
            frames: pcm.frames[..limit].to_vec(),
        }
    } else {
        pcm
    };

    let baseline = generic_reference_baseline();
    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: label.to_string(),
        recording_ref: wav_path.display().to_string(),
        started_ms: 0,
    };
    let config = StreamingConfig {
        chunk_secs: req.chunk_secs,
        max_secs: 0.0,
        window_width: 0,
    };
    let out = run_streaming_pipeline(&pcm, &baseline, &codebook, session, &config)
        .map_err(|e| format!("{label}: pipeline: {e}"))?;
    for word in &out.transcript.words {
        word.validate().map_err(|failure| {
            format!("{label}: chain invalid for {:?}: {failure:?}", word.text)
        })?;
    }
    let hypothesis: Vec<String> = out
        .transcript
        .words
        .iter()
        .map(|w| w.text.clone())
        .collect();
    let score = output::wer(&reference, &hypothesis);
    Ok(CaseScore {
        label: label.to_string(),
        reference_len: score.reference_len,
        hyp_len: hypothesis.len(),
        substitutions: score.substitutions,
        deletions: score.deletions,
        insertions: score.insertions,
        wer: score.wer,
        audio_secs,
    })
}

/// Overall WER from per-case counts: total errors / total reference words
/// (micro-average, so long cases weigh proportionally — NOT the mean of
/// per-case rates).
fn overall_wer(cases: &[CaseScore]) -> (usize, usize, usize, usize, usize, f64) {
    let (mut s, mut d, mut ins, mut n, mut h) = (0, 0, 0, 0, 0);
    for c in cases {
        s += c.substitutions;
        d += c.deletions;
        ins += c.insertions;
        n += c.reference_len;
        h += c.hyp_len;
    }
    let wer = if n == 0 {
        0.0
    } else {
        (s + d + ins) as f64 / n as f64
    };
    (s, d, ins, n, h, wer)
}

/// Render `<root>/WER-REPORT.md` from per-case scores (case order = oldest
/// term first). Deterministic: no timestamps, no absolute paths.
fn render_report(cases: &[CaseScore], req: &Request) -> String {
    let (s, d, ins, n, h, wer) = overall_wer(cases);
    let mut out = String::from(
        "# WER-1: Absolute WER of the full system on Oyez bulk\n\
         \n\
         Bead: `hearsay-four-4b7`. The full system (`hearsay-serve` pipeline:\n\
         windowed MFCC, phone scoring, hash-consed arena, commit window,\n\
         edit-distance lexicon lookup over the vendored closed vocabulary,\n\
         counted bigrams) transcribes each bulk case's `audio/run1.wav` end\n\
         to end; output words are scored against the A1 ground-truth word\n\
         list extracted from the committed `session.tsv` manifest\n\
         (`corpus::ground_truth::extract_words`) with `output::wer`.\n\
         Model-free, Oyez audio + synthetic fixtures only, fully offline.\n\
         \n\
         Reproduce (from `src/runner`):\n\
         `cargo run --release --bin wer-bulk -- --root ../../sessions/oyez-bulk`\n\
         \n",
    );
    if req.max_secs > 0.0 {
        out.push_str(&format!(
            "Pipeline: chunk_secs={} (one committed word per chunk), generic\n\
             reference baseline, CAPPED SMOKE RUN: first {}s of audio per\n\
             case only (NOT full-file; `audio (s)` below is the full-file\n\
             duration, `hyp words` covers the cap).\n\
             \n",
            req.chunk_secs, req.max_secs
        ));
    } else {
        out.push_str(&format!(
            "Pipeline: chunk_secs={} (one committed word per chunk), generic\n\
             reference baseline, full-file transcription (no `--max-secs` cap).\n\
             \n",
            req.chunk_secs
        ));
    }
    out.push_str(
        "| case | audio (s) | ref words (N) | hyp words | S | D | I | WER |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for c in cases {
        out.push_str(&format!(
            "| {} | {:.1} | {} | {} | {} | {} | {} | {:.4} |\n",
            c.label,
            c.audio_secs,
            c.reference_len,
            c.hyp_len,
            c.substitutions,
            c.deletions,
            c.insertions,
            c.wer
        ));
    }
    out.push_str(&format!(
        "\nOverall (micro-average: total errors / total ref words): \
         S={s} D={d} I={ins} N={n} hyp={h} WER={wer:.4}.\n\
         \nS = substitutions, D = deletions, I = insertions, \
         WER = (S+D+I)/N.\n"
    ));
    out
}

fn run(req: &Request) -> Result<String, Failure> {
    if !req.root.is_dir() {
        return Err(Failure::usage(format!(
            "--root {} is not a directory",
            req.root.display()
        )));
    }
    // One thread per case: the per-case pipeline state is independent
    // (baseline/codebook shared read-only); wall time ~ max, not sum.
    let joined: Result<Vec<CaseScore>, String> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(CASES.len());
        for label in CASES {
            handles.push(scope.spawn(move || score_case(&req.root, label, req)));
        }
        let mut cases = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.join() {
                Ok(Ok(score)) => cases.push(score),
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("worker thread panicked".to_string()),
            }
        }
        Ok(cases)
    });
    let mut cases = joined.map_err(Failure::failed)?;
    // Restore oldest-term-first order (threads finish in any order).
    cases.sort_by_key(|c| {
        CASES
            .iter()
            .position(|l| *l == c.label)
            .unwrap_or(usize::MAX)
    });

    let report = render_report(&cases, req);
    let report_path = req.root.join("WER-REPORT.md");
    std::fs::write(&report_path, &report)
        .map_err(|e| Failure::failed(format!("write {}: {e}", report_path.display())))?;
    let mut summary = String::new();
    for c in &cases {
        summary.push_str(&format!(
            "{}: N={} hyp={} S={} D={} I={} err={} WER={:.4}\n",
            c.label,
            c.reference_len,
            c.hyp_len,
            c.substitutions,
            c.deletions,
            c.insertions,
            c.errors(),
            c.wer
        ));
    }
    let (s, d, ins, n, _, wer) = overall_wer(&cases);
    summary.push_str(&format!(
        "overall: S={s} D={d} I={ins} N={n} WER={wer:.4}\nreport: {}\n",
        report_path.display()
    ));
    Ok(summary)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(Action::Help) => {
            println!("{USAGE}");
        }
        Ok(Action::Run(req)) => match run(&req) {
            Ok(summary) => {
                print!("{summary}");
            }
            Err(failure) => {
                eprint!("{}", failure.stderr);
                std::process::exit(failure.code);
            }
        },
        Err(failure) => {
            eprint!("{}", failure.stderr);
            std::process::exit(failure.code);
        }
    }
}

#[cfg(test)]
mod wer_harness_tests {
    //! WER-1 test harness (bead hearsay-four-4b7): synthetic fixtures with
    //! exact known WER. Model-free, offline, no audio files: the scoring path
    //! under test (`parse_session_tsv` -> `extract_words` -> `output::wer`
    //! -> `overall_wer` -> `render_report`) is exactly the path [`run`]
    //! uses on bulk data.

    use super::*;

    /// Synthetic `session.tsv` with an exact known word list: three rows,
    /// 2 + 1 + 3 normalized words = 6 reference words.
    const SYNTHETIC_TSV: &str = "\
bucket\tidx\tstart_sec\tstop_sec\tspeaker\tdrift_tags\ttext\n\
0\t0\t0.000\t2.000\tJustice A\tsource:synthetic\tHello world\n\
0\t1\t2.000\t3.000\tJustice B\tsource:synthetic\tYes.\n\
1\t2\t3.000\t6.000\tJustice A\tsource:synthetic\tThe CAT sat!\n";

    fn synthetic_reference() -> Vec<String> {
        let rows = parse_session_tsv(SYNTHETIC_TSV).expect("synthetic TSV parses");
        assert_eq!(rows.len(), 3, "all three synthetic rows kept");
        corpus::ground_truth::extract_words(&rows)
            .into_iter()
            .map(|(word, _, _)| word)
            .collect()
    }

    /// The fixture reference is exactly
    /// [hello, world, yes, the, cat, sat] (lowercased, punct stripped).
    #[test]
    fn synthetic_fixture_reference_words_are_exact() {
        assert_eq!(
            synthetic_reference(),
            vec!["hello", "world", "yes", "the", "cat", "sat"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
    }

    /// One substitution in six: S=1 D=0 I=0, WER = 1/6.
    #[test]
    fn synthetic_fixture_one_substitution_is_exact() {
        let reference = synthetic_reference();
        let hypothesis: Vec<String> = ["hello", "world", "yes", "the", "dog", "sat"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let score = output::wer(&reference, &hypothesis);
        assert_eq!(score.substitutions, 1);
        assert_eq!(score.deletions, 0);
        assert_eq!(score.insertions, 0);
        assert_eq!(score.reference_len, 6);
        assert_eq!(score.wer, 1.0 / 6.0);
    }

    /// One deletion + one insertion in six: S=0 D=1 I=1, WER = 2/6.
    #[test]
    fn synthetic_fixture_deletion_plus_insertion_is_exact() {
        let reference = synthetic_reference();
        // Drop "yes" (deletion), add trailing "please" (insertion).
        let hypothesis: Vec<String> = ["hello", "world", "the", "cat", "sat", "please"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let score = output::wer(&reference, &hypothesis);
        assert_eq!(score.substitutions, 0);
        assert_eq!(score.deletions, 1);
        assert_eq!(score.insertions, 1);
        assert_eq!(score.reference_len, 6);
        assert_eq!(score.wer, 2.0 / 6.0);
    }

    /// Overall is the micro-average over cases, not the mean of rates:
    /// case A (S=1,N=6) + case B (S=1,D=1,I=1,N=3) = 5 errors / 9 words.
    #[test]
    fn overall_is_micro_average_not_mean_of_rates() {
        let cases = vec![
            CaseScore {
                label: "a".to_string(),
                reference_len: 6,
                hyp_len: 6,
                substitutions: 1,
                deletions: 0,
                insertions: 0,
                wer: 1.0 / 6.0,
                audio_secs: 6.0,
            },
            CaseScore {
                label: "b".to_string(),
                reference_len: 3,
                hyp_len: 3,
                substitutions: 1,
                deletions: 1,
                insertions: 1,
                wer: 1.0,
                audio_secs: 3.0,
            },
        ];
        let (s, d, ins, n, h, wer) = overall_wer(&cases);
        assert_eq!((s, d, ins, n, h), (2, 1, 1, 9, 9));
        assert_eq!(wer, 4.0 / 9.0);
    }

    /// The report renders every case row plus the overall line, so a
    /// 3-case bulk run always carries its 3 per-case WER numbers + overall.
    #[test]
    fn report_carries_every_case_row_plus_overall() {
        let req = Request {
            root: PathBuf::from("sessions/oyez-bulk"),
            chunk_secs: 2.0,
            max_secs: 0.0,
        };
        let cases = vec![
            CaseScore {
                label: "70-112_1971".to_string(),
                reference_len: 6,
                hyp_len: 6,
                substitutions: 1,
                deletions: 0,
                insertions: 0,
                wer: 1.0 / 6.0,
                audio_secs: 6.0,
            },
            CaseScore {
                label: "10-218_2011".to_string(),
                reference_len: 6,
                hyp_len: 5,
                substitutions: 0,
                deletions: 1,
                insertions: 0,
                wer: 1.0 / 6.0,
                audio_secs: 6.0,
            },
            CaseScore {
                label: "24-889_2025".to_string(),
                reference_len: 6,
                hyp_len: 7,
                substitutions: 0,
                deletions: 0,
                insertions: 1,
                wer: 1.0 / 6.0,
                audio_secs: 6.0,
            },
        ];
        let report = render_report(&cases, &req);
        for label in ["70-112_1971", "10-218_2011", "24-889_2025"] {
            assert!(
                report.contains(&format!("| {label} |")),
                "report carries {label}"
            );
        }
        assert!(
            report.contains("Overall (micro-average"),
            "overall line present"
        );
        assert!(report.contains("WER=0.1667"), "overall WER exact (3/18)");
    }

    /// A capped smoke run labels itself as capped, never as full-file.
    #[test]
    fn capped_report_says_capped() {
        let full = Request {
            root: PathBuf::from("sessions/oyez-bulk"),
            chunk_secs: 2.0,
            max_secs: 0.0,
        };
        let capped = Request {
            root: PathBuf::from("sessions/oyez-bulk"),
            chunk_secs: full.chunk_secs,
            max_secs: 60.0,
        };
        let cases = vec![CaseScore {
            label: "70-112_1971".to_string(),
            reference_len: 6,
            hyp_len: 6,
            substitutions: 1,
            deletions: 0,
            insertions: 0,
            wer: 1.0 / 6.0,
            audio_secs: 3576.0,
        }];
        let full_report = render_report(&cases, &full);
        let capped_report = render_report(&cases, &capped);
        assert!(
            full_report.contains("full-file transcription"),
            "full run claims full-file"
        );
        assert!(
            !capped_report.contains("full-file transcription (no `--max-secs` cap)"),
            "capped run must not claim full-file"
        );
        assert!(
            capped_report.contains("CAPPED SMOKE RUN"),
            "capped run labels itself"
        );
    }

    /// Malformed manifests fail loudly (never an empty silent PASS): bad
    /// header, bad timing, and empty data are all `Err`.
    #[test]
    fn malformed_manifests_fail_loudly() {
        assert!(parse_session_tsv("").is_err(), "empty file");
        assert!(
            parse_session_tsv("bucket,idx,start\n0,0,0\n").is_err(),
            "bad header"
        );
        assert!(
            parse_session_tsv(
                "bucket\tidx\tstart_sec\tstop_sec\tspeaker\tdrift_tags\ttext\n\
                 0\t0\tabc\t1.0\tX\tt\tHi\n"
            )
            .is_err(),
            "bad start_sec"
        );
        assert!(
            parse_session_tsv("bucket\tidx\tstart_sec\tstop_sec\tspeaker\tdrift_tags\ttext\n")
                .is_err(),
            "header only"
        );
    }
}
