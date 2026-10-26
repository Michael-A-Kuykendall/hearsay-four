//! hearsay-demo: full 5-stage pipeline demo (bead hearsay-four-5qc).
//!
//! Audio in → transcript + per-word provenance out, on real courtroom
//! audio. Classical techniques only (windowed MFCC, Mahalanobis-ish phone
//! scoring, hash-consed arena, commit window, edit-distance lexicon lookup,
//! counted bigrams): no models, no learned weights, no STT text in scoring.
//!
//! Gate: `cargo run --bin hearsay-demo -- --input <real-audio>` exits 0
//! with a non-empty transcript, and every word's provenance chain
//! terminates at frame-level acoustic evidence (re-validated here before
//! printing; a violation exits 1, never a dangling ref).

use std::path::PathBuf;

use output::SessionMeta;
use runner::{FullPipelineConfig, generic_reference_baseline, run_full_pipeline};

fn usage() -> ! {
    eprintln!(
        "usage: hearsay-demo --input <audio.mp3|wav> \
         [--max-secs N] [--chunk-secs N]\n\
         defaults: --max-secs 12 --chunk-secs 2 (one committed word per chunk)"
    );
    std::process::exit(2);
}

fn parse_secs(flag: &str, value: &str) -> f64 {
    match value.parse::<f64>() {
        Ok(v) if v > 0.0 => v,
        _ => {
            eprintln!("hearsay-demo: {flag} must be a positive number (got {value:?})");
            usage();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<PathBuf> = None;
    let mut max_secs = 12.0f64;
    let mut chunk_secs = 2.0f64;
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
            }
            "--help" | "-h" => {
                println!(
                    "hearsay-demo: full 5-stage pipeline demo\n\n\
                     usage: hearsay-demo --input <audio.mp3|wav> \
                     [--max-secs N] [--chunk-secs N]"
                );
                return;
            }
            other => {
                eprintln!("hearsay-demo: unknown arg {other:?}");
                usage();
            }
        }
        i += 1;
    }
    let Some(input) = input else {
        usage();
    };

    let pcm = match corpus::audio::decode_to_pcm16k(&input) {
        Ok(pcm) => pcm,
        Err(e) => {
            eprintln!("hearsay-demo: decode failed for {}: {e}", input.display());
            std::process::exit(1);
        }
    };

    let baseline = generic_reference_baseline();
    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: "speaker-1".to_string(),
        recording_ref: input.display().to_string(),
        started_ms: 0,
    };
    let config = FullPipelineConfig {
        chunk_secs,
        max_secs,
        window_width: 0,
    };
    let out = match run_full_pipeline(&pcm, &baseline, &codebook, session, &config) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("hearsay-demo: pipeline failed: {e}");
            std::process::exit(1);
        }
    };

    // Re-validate every word's chain before printing: termination at
    // frame-level acoustic evidence is the gate, not an assumption.
    for word in &out.transcript.words {
        if let Err(e) = word.validate() {
            eprintln!(
                "hearsay-demo: provenance contract failed for {:?}: {e:?}",
                word.text
            );
            std::process::exit(1);
        }
    }

    let text: String = out
        .transcript
        .words
        .iter()
        .map(|w| w.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    println!("hearsay-demo: OK");
    println!(
        "input: {} ({:.1}s decoded, {:.1}s processed in {} chunks, {} frames, {} arena facts)",
        input.display(),
        pcm.duration_secs(),
        config.max_secs,
        out.stats.chunk_count,
        out.stats.frame_count,
        out.stats.arena_facts,
    );
    println!(
        "transcript ({} words, condition {}): {}",
        out.stats.word_count,
        out.transcript.conditions.as_deref().unwrap_or("?"),
        text,
    );
    println!(
        "provenance: all {} chains terminate at frame-level acoustic evidence",
        out.transcript.words.len(),
    );
    for word in &out.transcript.words {
        println!("{}", word.render());
    }
}
