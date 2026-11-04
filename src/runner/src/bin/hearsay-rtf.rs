//! hearsay-rtf: real-time-factor harness (bead hearsay-four-fe3, RTF-1).
//!
//! Runs the C3 streaming pipeline ([`runner::run_streaming_pipeline`]'s
//! incremental core, [`runner::StreamingPipeline`]) over single-speaker Oyez
//! clips (`data/oyez-bulk/*/audio`, gitignored, staged into the lane)
//! and reports wall-clock processing seconds per audio second (mean + p95
//! across chunks, plus the audio-weighted overall factor).
//!
//! Gate: prints mean+p95 RTF on a 60 s Oyez clip, exit 0.
//!
//! Model-free, Oyez + synthetic only, offline: counting and table lookup
//! only — no network, no STT text, no embeddings, no learned weights.

use std::path::PathBuf;
use std::time::Instant;

use output::SessionMeta;
use runner::rtf::{ChunkTiming, summarize};
use runner::{StreamingConfig, StreamingPipeline, generic_reference_baseline};

/// Usage text, printed by `--help` (stdout, exit 0) and appended to every
/// exit-2 usage error on stderr.
const USAGE: &str = "\
hearsay-rtf: real-time-factor harness (RTF-1, bead hearsay-four-fe3)\n\
\n\
usage: hearsay-rtf --input <audio.wav> [--max-secs N] [--chunk-secs N] [--json]\n\
  --input <path>   Oyez clip (WAV; single-speaker, e.g. data/oyez-bulk/*/audio/run1.wav)\n\
  --max-secs N     cap on audio processed, seconds (default 60, 0 = no cap)\n\
  --chunk-secs N   audio seconds per timed chunk (default 2)\n\
  --json           emit the RTF report as JSON instead of text\n\
\n\
stdout: mean + p95 processing-seconds per audio-second across chunks.\n\
offline: no sockets, no network crates, vendored dictionary only.";

/// Parsed CLI request.
#[derive(Debug, Clone)]
struct RtfRequest {
    input: Option<PathBuf>,
    max_secs: f64,
    chunk_secs: f64,
    json: bool,
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}

fn parse_secs(flag: &str, value: &str) -> f64 {
    match value.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => v,
        _ => {
            eprintln!("hearsay-rtf: {flag} must be a finite number >= 0 (got {value:?})");
            usage();
        }
    }
}

fn parse_args(args: &[String]) -> RtfRequest {
    let mut input: Option<PathBuf> = None;
    let mut max_secs = 60.0f64;
    let mut chunk_secs = 2.0f64;
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                i += 1;
                if i >= args.len() {
                    usage();
                }
                input = Some(PathBuf::from(&args[i]));
            }
            "--max-secs" => {
                i += 1;
                if i >= args.len() {
                    usage();
                }
                max_secs = parse_secs("--max-secs", &args[i]);
            }
            "--chunk-secs" => {
                i += 1;
                if i >= args.len() {
                    usage();
                }
                chunk_secs = parse_secs("--chunk-secs", &args[i]);
                if chunk_secs <= 0.0 {
                    eprintln!("hearsay-rtf: --chunk-secs must be positive");
                    usage();
                }
            }
            "--json" => {
                json = true;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => {
                eprintln!("hearsay-rtf: unknown arg {other:?}");
                usage();
            }
        }
        i += 1;
    }
    RtfRequest {
        input,
        max_secs,
        chunk_secs,
        json,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let req = parse_args(&args);
    let Some(input) = req.input else {
        eprintln!("hearsay-rtf: --input <audio.wav> is required");
        usage();
    };

    let full = match corpus::audio::decode_to_pcm16k(&input) {
        Ok(pcm) => pcm,
        Err(e) => {
            eprintln!("hearsay-rtf: decode failed for {}: {e}", input.display());
            std::process::exit(1);
        }
    };
    if full.frames.is_empty() {
        eprintln!("hearsay-rtf: decoded zero frames from {}", input.display());
        std::process::exit(1);
    }
    // Cap to the horizon (default 60 s): the gate clip, not the full hour.
    let cap_samples = if req.max_secs > 0.0 {
        (full.sample_rate as f64 * req.max_secs) as usize
    } else {
        full.frames.len()
    };
    let take = cap_samples.min(full.frames.len());
    let frames = &full.frames[..take];
    let audio_secs_total = take as f64 / full.sample_rate as f64;
    if frames.is_empty() {
        eprintln!("hearsay-rtf: nothing to process (cap is zero or clip empty)");
        std::process::exit(1);
    }

    let baseline = generic_reference_baseline();
    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: "speaker-1".to_string(),
        recording_ref: input.display().to_string(),
        started_ms: 0,
    };
    let chunk_samples = ((full.sample_rate as f64 * req.chunk_secs) as usize).max(1);
    let mut pipe = match StreamingPipeline::new(
        full.sample_rate,
        &baseline,
        &codebook,
        session,
        StreamingConfig {
            chunk_secs: req.chunk_secs,
            max_secs: req.max_secs,
            window_width: 0,
        },
    ) {
        Ok(pipe) => pipe,
        Err(e) => {
            eprintln!("hearsay-rtf: pipeline setup failed: {e}");
            std::process::exit(1);
        }
    };

    // Time every full chunk's incremental commit (feed of exactly one
    // chunk commits exactly one chunk: boundaries stay position-aligned
    // from stream start, exactly as the batch path enumerates).
    let mut timings: Vec<ChunkTiming> = Vec::new();
    let mut offset = 0usize;
    while offset + chunk_samples <= frames.len() {
        let chunk = &frames[offset..offset + chunk_samples];
        let start = Instant::now();
        if let Err(e) = pipe.feed(chunk) {
            eprintln!(
                "hearsay-rtf: pipeline feed failed at chunk {}: {e}",
                timings.len()
            );
            std::process::exit(1);
        }
        let processing_secs = start.elapsed().as_secs_f64();
        timings.push(ChunkTiming::new(
            chunk.len() as f64 / full.sample_rate as f64,
            processing_secs,
        ));
        offset += chunk_samples;
    }
    // Trailing partial chunk (when the horizon is not a multiple of
    // --chunk-secs): its commit happens inside `finish`, so the feed +
    // finish wall clock together are its processing time.
    let remainder = &frames[offset..];
    let finalize_start = Instant::now();
    if !remainder.is_empty()
        && let Err(e) = pipe.feed(remainder)
    {
        eprintln!("hearsay-rtf: pipeline feed failed on trailing audio: {e}");
        std::process::exit(1);
    }
    let out = match pipe.finish() {
        Ok(out) => out,
        Err(e) => {
            eprintln!("hearsay-rtf: pipeline finish failed: {e}");
            std::process::exit(1);
        }
    };
    let finalize_secs = finalize_start.elapsed().as_secs_f64();
    if !remainder.is_empty() {
        timings.push(ChunkTiming::new(
            remainder.len() as f64 / full.sample_rate as f64,
            finalize_secs,
        ));
    }
    if timings.is_empty() {
        eprintln!("hearsay-rtf: no chunks committed (clip shorter than one chunk?)");
        std::process::exit(1);
    }

    let report = match summarize(&timings) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("hearsay-rtf: RTF summary failed: {e}");
            std::process::exit(1);
        }
    };
    // Sanity: the timed audio must cover the horizon (no silent drop).
    let timed_audio: f64 = timings.iter().map(|t| t.audio_secs).sum();
    if (timed_audio - audio_secs_total).abs() > 1e-6 {
        eprintln!("hearsay-rtf: timed audio {timed_audio:.3}s != horizon {audio_secs_total:.3}s");
        std::process::exit(1);
    }

    if req.json {
        let value = serde_json::json!({
            "input": input.display().to_string(),
            "audio_secs": report.audio_secs,
            "processing_secs": report.processing_secs,
            "chunk_count": report.chunk_count,
            "chunk_secs": req.chunk_secs,
            "word_count": out.stats.word_count,
            "overall_rtf": report.overall_rtf,
            "mean_rtf": report.mean_rtf,
            "p95_rtf": report.p95_rtf,
            "chunk_rtfs": report.chunk_rtfs,
        });
        match serde_json::to_string_pretty(&value) {
            Ok(rendered) => println!("{rendered}"),
            Err(e) => {
                eprintln!("hearsay-rtf: JSON render failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    println!("hearsay-rtf: OK");
    println!(
        "input: {} ({:.1}s audio in {} chunks of {:.1}s, {} words committed)",
        input.display(),
        report.audio_secs,
        report.chunk_count,
        req.chunk_secs,
        out.stats.word_count,
    );
    println!(
        "processing: {:.3}s total wall-clock ({:.3}s audio-weighted overall RTF)",
        report.processing_secs, report.overall_rtf,
    );
    println!(
        "mean RTF: {:.4} processing-seconds per audio-second",
        report.mean_rtf
    );
    println!(
        "p95 RTF: {:.4} processing-seconds per audio-second",
        report.p95_rtf
    );
}
