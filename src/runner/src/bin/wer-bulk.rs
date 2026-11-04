//! wer-bulk: absolute WER driver on the Oyez bulk sessions
//! (bead hearsay-four-4b7, WER-1).
//!
//! Transcribes `data/oyez-bulk/*/audio/run1.wav` end to end through the
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
//! `--root ../../data/oyez-bulk`).
//!
//! Two modes: full-file (default, `--span-secs 0`) transcribes each case end
//! to end and scores against the full-file reference (kept for overnight
//! runs); span-sliced (`--span-secs S --num-spans K`) transcribes K bounded
//! audio slices per case — each on a FRESH pipeline so fabric cost stays
//! bounded — and scores each hypothesis against its MATCHING reference span
//! (timed words from `extract_words` filtered to the span), so slice WER is
//! a real accuracy number instead of capped-hyp-vs-full-ref deletions.
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
usage: wer-bulk --root <data/oyez-bulk> [--chunk-secs N] [--max-secs N]\n\
  [--span-secs S --num-spans K | --span-starts a,b,c]\n\
  --root <dir>   bulk session dir holding <case>/session.tsv +\n\
                 <case>/audio/run1.wav (resolves relative to CWD)\n\
  --chunk-secs N audio seconds per committed word (default 2)\n\
  --max-secs N   cap audio per case, seconds (default 0 = full file;\n\
                 full-file path, kept for overnight runs; ignored in\n\
                 span mode)\n\
  --span-secs S  span-sliced mode: transcribe spans of S audio seconds\n\
                 per case, each scored against its MATCHING reference\n\
                 span (default 0 = full-file mode)\n\
  --num-spans K  spans per case in span mode, spread start/middle/end\n\
                 across the file (default 3; ignored if --span-starts)\n\
  --span-starts  explicit span starts in seconds, e.g. \"0,1200,2400\"\n\
                 (implies span mode; overrides --num-spans)\n\
\n\
stdout: per-case + overall WER lines; <root>/WER-REPORT.md: the report.\n\
offline: WAV + TSV in, markdown out; no sockets, no downloads.";

struct Request {
    root: PathBuf,
    chunk_secs: f64,
    max_secs: f64,
    span_secs: f64,
    num_spans: usize,
    span_starts: Vec<f64>,
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
    let mut span_secs = 0.0f64;
    let mut num_spans = 3usize;
    let mut span_starts: Vec<f64> = Vec::new();
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
            "--span-secs" => {
                i += 1;
                match args.get(i) {
                    Some(v) => {
                        span_secs = parse_secs("--span-secs", v)?;
                        if span_secs <= 0.0 {
                            return Err(Failure::usage("--span-secs must be positive".to_string()));
                        }
                    }
                    None => {
                        return Err(Failure::usage("--span-secs needs a value".to_string()));
                    }
                }
            }
            "--num-spans" => {
                i += 1;
                match args.get(i) {
                    Some(v) => match v.parse::<usize>() {
                        Ok(k) if k >= 1 => num_spans = k,
                        _ => {
                            return Err(Failure::usage(format!(
                                "--num-spans must be an integer >= 1 (got {v:?})"
                            )));
                        }
                    },
                    None => {
                        return Err(Failure::usage("--num-spans needs a value".to_string()));
                    }
                }
            }
            "--span-starts" => {
                i += 1;
                match args.get(i) {
                    Some(v) => {
                        span_starts.clear();
                        for part in v.split(',') {
                            match part.trim().parse::<f64>() {
                                Ok(s) if s.is_finite() && s >= 0.0 => span_starts.push(s),
                                _ => {
                                    return Err(Failure::usage(format!(
                                        "--span-starts must be comma-separated seconds >= 0 (got {part:?})"
                                    )));
                                }
                            }
                        }
                        if span_starts.is_empty() {
                            return Err(Failure::usage(
                                "--span-starts needs at least one start".to_string(),
                            ));
                        }
                    }
                    None => {
                        return Err(Failure::usage("--span-starts needs a value".to_string()));
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
            span_secs,
            num_spans,
            span_starts,
        })),
        None => Err(Failure::usage(
            "--root <data/oyez-bulk> is required".to_string(),
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

/// One transcribed span scored against its MATCHING reference span.
/// `start_sec` is the span offset in the case audio; the reference is the
/// timed words from `extract_words` whose word-start falls in
/// `[start_sec, start_sec + span_secs)`, so slice WER is a real accuracy
/// number (never capped-hypothesis vs full-file-reference deletions).
#[derive(Debug, Clone)]
struct SpanScore {
    label: String,
    start_sec: f64,
    span_secs: f64,
    reference_len: usize,
    hyp_len: usize,
    substitutions: usize,
    deletions: usize,
    insertions: usize,
    wer: f64,
}

impl SpanScore {
    /// Total word errors (S+D+I) for this span.
    fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }
}

/// Span mode is on when `--span-secs` (or `--span-starts`) was passed.
fn is_span_mode(req: &Request) -> bool {
    req.span_secs > 0.0 || !req.span_starts.is_empty()
}

/// Spread `count` span starts across a file of `duration_secs`: first span
/// at 0, last span ending exactly at the file end, rest evenly between, so
/// bounded spans sample the whole argument (not just the opening minutes).
/// Pure (no I/O): unit-tested below.
fn spread_span_starts(duration_secs: f64, span_secs: f64, count: usize) -> Vec<f64> {
    if count <= 1 || duration_secs <= span_secs {
        return vec![0.0];
    }
    let last = duration_secs - span_secs;
    (0..count)
        .map(|i| last * i as f64 / (count - 1) as f64)
        .collect()
}

/// Reference words whose word-START falls inside the span
/// `[start_ms, end_ms)`: the matching reference span for a transcribed
/// audio slice. Pure (no I/O): unit-tested below.
fn reference_words_in_span(
    timed: &[(String, u64, u64)],
    start_ms: u64,
    end_ms: u64,
) -> Vec<String> {
    timed
        .iter()
        .filter(|(_, s, _)| *s >= start_ms && *s < end_ms)
        .map(|(word, _, _)| word.clone())
        .collect()
}

/// Transcribe one audio slice `[start_sec, start_sec + span_secs)` on a
/// FRESH pipeline (bounded fabric: per-span cost, never the full-file
/// superlinear blowup) and score the hypothesis against the MATCHING
/// reference span. The ONLY I/O is the two input files via the caller.
fn score_span(
    label: &str,
    wav_path: &Path,
    pcm: &Pcm16kAudio,
    timed: &[(String, u64, u64)],
    start_sec: f64,
    span_secs: f64,
    chunk_secs: f64,
) -> Result<SpanScore, String> {
    let rate = pcm.sample_rate as f64;
    let start_sample = (rate * start_sec) as usize;
    if start_sample >= pcm.frames.len() {
        return Err(format!(
            "{label}: span start {start_sec}s past end of audio"
        ));
    }
    let end_sample = ((rate * (start_sec + span_secs)) as usize).min(pcm.frames.len());
    let slice = Pcm16kAudio {
        sample_rate: pcm.sample_rate,
        channels: pcm.channels,
        frames: pcm.frames[start_sample..end_sample].to_vec(),
    };
    let actual_span_secs = (end_sample - start_sample) as f64 / rate;
    let baseline = generic_reference_baseline();
    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: format!("{label}@span{start_sec:.0}"),
        recording_ref: format!("{}#t={start_sec:.0}", wav_path.display()),
        started_ms: (start_sec * 1000.0).round() as u64,
    };
    let config = StreamingConfig {
        chunk_secs,
        max_secs: 0.0,
        window_width: 0,
    };
    let out = run_streaming_pipeline(&slice, &baseline, &codebook, session, &config)
        .map_err(|e| format!("{label}@span{start_sec:.0}: pipeline: {e}"))?;
    for word in &out.transcript.words {
        word.validate().map_err(|failure| {
            format!(
                "{label}@span{start_sec:.0}: chain invalid for {:?}: {failure:?}",
                word.text
            )
        })?;
    }
    let hypothesis: Vec<String> = out
        .transcript
        .words
        .iter()
        .map(|w| w.text.clone())
        .collect();
    if std::env::var("WER_BULK_DEBUG_WORDS").is_ok() {
        eprintln!("{label}@span{start_sec:.0}: hyp=[{}]", hypothesis.join(" "));
    }
    let start_ms = (start_sec * 1000.0).round().max(0.0) as u64;
    let end_ms = ((start_sec + actual_span_secs) * 1000.0).round().max(0.0) as u64;
    let reference = reference_words_in_span(timed, start_ms, end_ms);
    if reference.is_empty() {
        return Err(format!(
            "{label}@span{start_sec:.0}: no reference words in span (empty ground truth slice)"
        ));
    }
    let score = output::wer(&reference, &hypothesis);
    Ok(SpanScore {
        label: label.to_string(),
        start_sec,
        span_secs: actual_span_secs,
        reference_len: score.reference_len,
        hyp_len: hypothesis.len(),
        substitutions: score.substitutions,
        deletions: score.deletions,
        insertions: score.insertions,
        wer: score.wer,
    })
}

/// Transcribe one case's bounded spans and score each against its matching
/// reference span. Pure pipeline + A1 ground truth; the ONLY I/O is the two
/// input files.
fn score_case_spans(root: &Path, label: &str, req: &Request) -> Result<Vec<SpanScore>, String> {
    let case_dir = root.join(label);
    let tsv_text = std::fs::read_to_string(case_dir.join("session.tsv"))
        .map_err(|e| format!("read {label}/session.tsv: {e}"))?;
    let rows = parse_session_tsv(&tsv_text)?;
    let timed: Vec<(String, u64, u64)> = corpus::ground_truth::extract_words(&rows);
    if timed.is_empty() {
        return Err(format!("{label}: no reference words in session.tsv"));
    }
    let wav_path = case_dir.join("audio/run1.wav");
    let pcm = corpus::audio::decode_to_pcm16k(&wav_path)
        .map_err(|e| format!("decode {label}/audio/run1.wav: {e}"))?;
    let duration_secs = pcm.duration_secs();
    let starts: Vec<f64> = if req.span_starts.is_empty() {
        spread_span_starts(duration_secs, req.span_secs, req.num_spans)
    } else {
        req.span_starts.clone()
    };
    let mut spans = Vec::with_capacity(starts.len());
    for start in starts {
        spans.push(score_span(
            label,
            &wav_path,
            &pcm,
            &timed,
            start,
            req.span_secs,
            req.chunk_secs,
        )?);
    }
    Ok(spans)
}

/// Micro-average over spans: total errors / total span reference words.
fn overall_span_wer(spans: &[SpanScore]) -> (usize, usize, usize, usize, usize, f64) {
    let (mut s, mut d, mut ins, mut n, mut h) = (0, 0, 0, 0, 0);
    for span in spans {
        s += span.substitutions;
        d += span.deletions;
        ins += span.insertions;
        n += span.reference_len;
        h += span.hyp_len;
    }
    let wer = if n == 0 {
        0.0
    } else {
        (s + d + ins) as f64 / n as f64
    };
    (s, d, ins, n, h, wer)
}

/// Render `<root>/WER-REPORT.md` for a span-sliced run: one row per span
/// plus the micro-average, labeled span-sliced (never full-file).
/// Deterministic: no timestamps, no absolute paths.
fn render_span_report(spans: &[SpanScore], req: &Request) -> String {
    let (s, d, ins, n, h, wer) = overall_span_wer(spans);
    let starts_note: String = if req.span_starts.is_empty() {
        format!(
            "span starts spread start/middle/end across each file ({} spans/case)",
            spans.len().max(1) / CASES.len().max(1)
        )
    } else {
        format!(
            "explicit span starts: {}",
            req.span_starts
                .iter()
                .map(|v| format!("{v:.0}s"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut out = String::from(
        "# WER-1: Absolute WER of the full system on Oyez bulk (span-sliced)\n\
         \n\
         Bead: `hearsay-four-4b7`. The full system (`hearsay-serve` pipeline:\n\
         windowed MFCC, phone scoring, hash-consed arena, commit window,\n\
         edit-distance lexicon lookup over the vendored closed vocabulary,\n\
         counted bigrams) transcribes bounded audio spans of each bulk case's\n\
         `audio/run1.wav` — each span on a FRESH pipeline so fabric cost\n\
         stays bounded — and each span hypothesis is scored against its\n\
         MATCHING reference span (timed words from `extract_words` whose\n\
         word-start falls in the span) with `output::wer`.\n\
         Model-free, Oyez audio + synthetic fixtures only, fully offline.\n\
         \n\
         This is a SPAN-SLICED measurement, not full-file: full-file\n\
         transcription is kept as the overnight driver path (`--span-secs 0`).\n\
         \n",
    );
    out.push_str(&format!(
        "Reproduce (from `src/runner`):\n\
         `cargo run --release --bin wer-bulk -- --root ../../data/oyez-bulk --span-secs {} --num-spans {}`\n\
         \n\
         Pipeline: chunk_secs={} (one committed word per chunk), generic\n\
         reference baseline, SPAN-SLICED: {}s spans per case, {}.\n\
         \n",
        req.span_secs, req.num_spans, req.chunk_secs, req.span_secs, starts_note
    ));
    out.push_str(
        "| case | span start (s) | span (s) | ref words (N) | hyp words | S | D | I | WER |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for span in spans {
        out.push_str(&format!(
            "| {} | {:.0} | {:.0} | {} | {} | {} | {} | {} | {:.4} |\n",
            span.label,
            span.start_sec,
            span.span_secs,
            span.reference_len,
            span.hyp_len,
            span.substitutions,
            span.deletions,
            span.insertions,
            span.wer
        ));
    }
    out.push_str(&format!(
        "\nOverall span micro-average (total errors / total span ref words): \
         S={s} D={d} I={ins} N={n} hyp={h} WER={wer:.4}.\n\
         \nS = substitutions, D = deletions, I = insertions, \
         WER = (S+D+I)/N.\n"
    ));
    out
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
         `cargo run --release --bin wer-bulk -- --root ../../data/oyez-bulk`\n\
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

/// Span-sliced run: bounded spans per case, each scored against its
/// matching reference span; one thread per case (wall time ~ max, not sum).
fn run_spans(req: &Request) -> Result<String, Failure> {
    if !req.span_starts.is_empty() && req.span_secs <= 0.0 {
        return Err(Failure::usage(
            "--span-starts needs --span-secs S (span length)".to_string(),
        ));
    }
    let joined: Result<Vec<Vec<SpanScore>>, String> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(CASES.len());
        for label in CASES {
            handles.push(scope.spawn(move || score_case_spans(&req.root, label, req)));
        }
        let mut per_case = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.join() {
                Ok(Ok(spans)) => per_case.push(spans),
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("worker thread panicked".to_string()),
            }
        }
        Ok(per_case)
    });
    let mut per_case = joined.map_err(Failure::failed)?;
    // Restore oldest-term-first case order (threads finish in any order),
    // spans within a case stay in start order.
    per_case.sort_by_key(|spans| {
        spans
            .first()
            .and_then(|s| CASES.iter().position(|l| *l == s.label))
            .unwrap_or(usize::MAX)
    });
    for spans in &mut per_case {
        spans.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    }
    let spans: Vec<SpanScore> = per_case.into_iter().flatten().collect();

    let report = render_span_report(&spans, req);
    let report_path = req.root.join("WER-REPORT.md");
    std::fs::write(&report_path, &report)
        .map_err(|e| Failure::failed(format!("write {}: {e}", report_path.display())))?;
    let mut summary = String::new();
    for span in &spans {
        summary.push_str(&format!(
            "{}@span{:.0}: N={} hyp={} S={} D={} I={} err={} WER={:.4}\n",
            span.label,
            span.start_sec,
            span.reference_len,
            span.hyp_len,
            span.substitutions,
            span.deletions,
            span.insertions,
            span.errors(),
            span.wer
        ));
    }
    let (s, d, ins, n, _, wer) = overall_span_wer(&spans);
    summary.push_str(&format!(
        "overall span micro-average: S={s} D={d} I={ins} N={n} WER={wer:.4}\nreport: {}\n",
        report_path.display()
    ));
    Ok(summary)
}

fn run(req: &Request) -> Result<String, Failure> {
    if !req.root.is_dir() {
        return Err(Failure::usage(format!(
            "--root {} is not a directory",
            req.root.display()
        )));
    }
    if is_span_mode(req) {
        return run_spans(req);
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
            root: PathBuf::from("data/oyez-bulk"),
            chunk_secs: 2.0,
            max_secs: 0.0,
            span_secs: 0.0,
            num_spans: 3,
            span_starts: Vec::new(),
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
            root: PathBuf::from("data/oyez-bulk"),
            chunk_secs: 2.0,
            max_secs: 0.0,
            span_secs: 0.0,
            num_spans: 3,
            span_starts: Vec::new(),
        };
        let capped = Request {
            root: PathBuf::from("data/oyez-bulk"),
            chunk_secs: full.chunk_secs,
            max_secs: 60.0,
            span_secs: 0.0,
            num_spans: 3,
            span_starts: Vec::new(),
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

    /// Span alignment: a hypothesis span scored against its MATCHING
    /// reference span gives the exact known WER (one substitution in three
    /// span words: S=1 D=0 I=0, WER = 1/3) — while scoring the same span
    /// hypothesis against the FULL reference inflates deletions, which is
    /// the bug span alignment fixes.
    #[test]
    fn span_hypothesis_against_matching_span_is_exact() {
        let rows = parse_session_tsv(SYNTHETIC_TSV).expect("synthetic TSV parses");
        let timed = corpus::ground_truth::extract_words(&rows);
        assert_eq!(timed.len(), 6, "six timed reference words");
        // Synthetic segments: "Hello world" @ [0,2000ms), "Yes." @
        // [2000,3000ms), "The CAT sat!" @ [3000,6000ms). The [0,3000ms)
        // span covers exactly [hello, world, yes].
        let span_ref = reference_words_in_span(&timed, 0, 3000);
        assert_eq!(
            span_ref,
            vec!["hello", "world", "yes"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            "span filter keeps exactly the matching words"
        );
        let hypothesis: Vec<String> = ["hello", "world", "no"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let aligned = output::wer(&span_ref, &hypothesis);
        assert_eq!(aligned.substitutions, 1);
        assert_eq!(aligned.deletions, 0);
        assert_eq!(aligned.insertions, 0);
        assert_eq!(aligned.reference_len, 3);
        assert_eq!(aligned.wer, 1.0 / 3.0);
        // Same hypothesis against the FULL six-word reference: the three
        // missing span-external words count as deletions — the inflated
        // number span alignment avoids.
        let full_ref: Vec<String> = timed.iter().map(|(word, _, _)| word.clone()).collect();
        let misaligned = output::wer(&full_ref, &hypothesis);
        assert_eq!(misaligned.reference_len, 6);
        assert!(
            misaligned.deletions >= 3,
            "full-ref scoring inflates deletions: {misaligned:?}"
        );
    }

    /// Span starts spread start/middle/end across the file (first at 0,
    /// last ending exactly at the file end), so bounded spans sample the
    /// whole argument instead of just the opening minutes.
    #[test]
    fn span_starts_spread_across_the_file() {
        assert_eq!(
            spread_span_starts(3600.0, 60.0, 3),
            vec![0.0, 1770.0, 3540.0]
        );
        assert_eq!(spread_span_starts(100.0, 60.0, 1), vec![0.0]);
        assert_eq!(
            spread_span_starts(30.0, 60.0, 3),
            vec![0.0],
            "span longer than file collapses to one span at 0"
        );
    }

    /// The span report labels itself span-sliced (never full-file) and
    /// carries every per-span row plus the micro-average line.
    #[test]
    fn span_report_says_span_sliced_with_every_row() {
        let req = Request {
            root: PathBuf::from("data/oyez-bulk"),
            chunk_secs: 2.0,
            max_secs: 0.0,
            span_secs: 30.0,
            num_spans: 3,
            span_starts: Vec::new(),
        };
        let spans = vec![
            SpanScore {
                label: "70-112_1971".to_string(),
                start_sec: 0.0,
                span_secs: 30.0,
                reference_len: 60,
                hyp_len: 15,
                substitutions: 5,
                deletions: 45,
                insertions: 0,
                wer: 50.0 / 60.0,
            },
            SpanScore {
                label: "70-112_1971".to_string(),
                start_sec: 1770.0,
                span_secs: 30.0,
                reference_len: 60,
                hyp_len: 15,
                substitutions: 5,
                deletions: 45,
                insertions: 0,
                wer: 50.0 / 60.0,
            },
        ];
        let report = render_span_report(&spans, &req);
        assert!(
            report.contains("span-sliced") || report.contains("SPAN-SLICED"),
            "span report labels itself span-sliced"
        );
        assert!(
            !report.contains("full-file transcription (no `--max-secs` cap)"),
            "span report must not claim full-file"
        );
        assert!(report.contains("| 70-112_1971 | 0 |"), "first span row");
        assert!(report.contains("| 70-112_1971 | 1770 |"), "second span row");
        assert!(
            report.contains("Overall span micro-average"),
            "micro-average line present"
        );
        let (s, d, ins, n, _, wer) = overall_span_wer(&spans);
        assert_eq!((s, d, ins, n), (10, 90, 0, 120));
        assert_eq!(wer, 100.0 / 120.0);
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
