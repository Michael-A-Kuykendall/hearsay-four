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
use runner::{
    FullPipelineConfig, FullPipelineOutput, generic_reference_baseline, run_full_pipeline,
};

/// Validate every word's chain, then serialize transcript + run stats.
///
/// Mirrors the human path's contract: termination at frame-level acoustic
/// evidence is enforced before anything is emitted, so `--json` output is
/// as trustworthy as the one-screen render.
fn emit_json(out: &FullPipelineOutput) -> Result<String, String> {
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
        },
    });
    serde_json::to_string_pretty(&value).map_err(|e| format!("JSON serialization failed: {e}"))
}

fn usage() -> ! {
    eprintln!(
        "usage: hearsay-demo --input <audio.mp3|wav> \
         [--max-secs N] [--chunk-secs N] [--json]\n\
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
            }
            "--json" => {
                json = true;
            }
            "--help" | "-h" => {
                println!(
                    "hearsay-demo: full 5-stage pipeline demo\n\n\
                     usage: hearsay-demo --input <audio.mp3|wav> \
                     [--max-secs N] [--chunk-secs N] [--json]"
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

    if json {
        match emit_json(&out) {
            Ok(rendered) => {
                println!("{rendered}");
            }
            Err(e) => {
                eprintln!("hearsay-demo: {e}");
                std::process::exit(1);
            }
        }
        return;
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

#[cfg(test)]
mod tests {
    use super::*;
    use output::Transcript;
    use runner::{FullPipelineOutput, FullPipelineStats};

    fn empty_output() -> FullPipelineOutput {
        FullPipelineOutput {
            transcript: Transcript {
                words: vec![],
                session: SessionMeta {
                    speaker_id: "test".to_string(),
                    recording_ref: "test".to_string(),
                    started_ms: 0,
                },
                conditions: None,
            },
            stats: FullPipelineStats {
                frame_count: 0,
                chunk_count: 0,
                arena_facts: 0,
                word_count: 0,
            },
        }
    }

    #[test]
    fn json_output_emits_transcript_shape() {
        let json = emit_json(&empty_output()).expect("empty transcript serializes");
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("emit_json produces valid JSON");
        assert_eq!(v["transcript"]["words"].as_array().unwrap().len(), 0);
        assert!(v.get("stats").is_some(), "stats section present");
        assert_eq!(v["stats"]["word_count"], 0);
    }
}
