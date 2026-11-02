//! hearsay-transcribe: file transcription on the current best model.
//!
//! Bead `hearsay-four-xels--demo-file-transcription`: WAV/MP3 in, transcript
//! out, decoded with the 8-mix AM + tiered lexicon + R1-v2 general LM (see
//! [`runner::demo_transcribe`] for the single config values). Classical
//! pipeline only: no neural net, transformer, embedding model, or weights.
//!
//! Gate: `hearsay-transcribe --demo` exits 0 with transcript output for each
//! fixed demo file. An empty decode fails loudly (exit 1), never silently.

use std::path::PathBuf;
use std::time::Instant;

use runner::demo_transcribe::{
    DEMO_FILES, DEMO_WAV_ROOT, DemoConfig, load_transcriber, utt_id_from_path,
};

struct Input {
    utt_id: String,
    path: PathBuf,
}

struct Request {
    inputs: Vec<Input>,
    cfg: DemoConfig,
    json: bool,
    out_dir: Option<PathBuf>,
}

fn usage() -> ! {
    eprintln!(
        "usage: hearsay-transcribe [--demo | --input <audio.mp3|wav> ...] [--json] [--out-dir <dir>]\n\
         \t[--am <model.json>] [--lm <lm>] [--tier1 <master.tsv>] [--tier2 <lex.txt>]\n\
         \t[--book <txt>] [--top-n N] [--beam F] [--max-tokens N] [--lookahead-weight F]\n\
         \t[--lm-weight F] [--word-penalty F]\n\
         defaults: --am/--lm/--tier1/--tier2 from demo_transcribe DEFAULT_* (single config values),\n\
         \t--top-n 20000 --lm-weight 6 --word-penalty 0. --demo transcribes the fixed DEMO_FILES list."
    );
    std::process::exit(2);
}

fn parse_args(args: &[String]) -> Result<Request, String> {
    let mut inputs: Vec<Input> = Vec::new();
    let mut cfg = DemoConfig::default();
    let mut json = false;
    let mut out_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--demo" => {
                for (id, rel) in &DEMO_FILES {
                    inputs.push(Input {
                        utt_id: id.to_string(),
                        path: PathBuf::from(DEMO_WAV_ROOT).join(rel),
                    });
                }
            }
            "--input" => {
                i += 1;
                if i >= args.len() {
                    return Err("--input needs a path".to_string());
                }
                let path = PathBuf::from(&args[i]);
                let utt_id = utt_id_from_path(&path);
                inputs.push(Input { utt_id, path });
            }
            "--json" => json = true,
            "--out-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err("--out-dir needs a directory".to_string());
                }
                out_dir = Some(PathBuf::from(&args[i]));
            }
            "--am" => {
                i += 1;
                if i >= args.len() {
                    return Err("--am needs a path".to_string());
                }
                cfg.am = PathBuf::from(&args[i]);
            }
            "--lm" => {
                i += 1;
                if i >= args.len() {
                    return Err("--lm needs a path".to_string());
                }
                cfg.lm = PathBuf::from(&args[i]);
            }
            "--tier1" => {
                i += 1;
                if i >= args.len() {
                    return Err("--tier1 needs a path".to_string());
                }
                cfg.tier1 = PathBuf::from(&args[i]);
            }
            "--tier2" => {
                i += 1;
                if i >= args.len() {
                    return Err("--tier2 needs a path".to_string());
                }
                cfg.tier2 = PathBuf::from(&args[i]);
            }
            "--book" => {
                i += 1;
                if i >= args.len() {
                    return Err("--book needs a path".to_string());
                }
                cfg.book = Some(PathBuf::from(&args[i]));
            }
            "--top-n" => {
                i += 1;
                if i >= args.len() {
                    return Err("--top-n needs a number".to_string());
                }
                cfg.top_n = args[i]
                    .parse::<usize>()
                    .map_err(|_| format!("bad --top-n {:?}", args[i]))?;
            }
            "--beam" => {
                i += 1;
                if i >= args.len() {
                    return Err("--beam needs a number".to_string());
                }
                cfg.beam = Some(
                    args[i]
                        .parse::<f64>()
                        .map_err(|_| format!("bad --beam {:?}", args[i]))?,
                );
            }
            "--max-tokens" => {
                i += 1;
                if i >= args.len() {
                    return Err("--max-tokens needs a number".to_string());
                }
                cfg.max_tokens = Some(
                    args[i]
                        .parse::<usize>()
                        .map_err(|_| format!("bad --max-tokens {:?}", args[i]))?,
                );
            }
            "--lookahead-weight" => {
                i += 1;
                if i >= args.len() {
                    return Err("--lookahead-weight needs a number".to_string());
                }
                cfg.lookahead_weight = Some(
                    args[i]
                        .parse::<f64>()
                        .map_err(|_| format!("bad --lookahead-weight {:?}", args[i]))?,
                );
            }
            "--lm-weight" => {
                i += 1;
                if i >= args.len() {
                    return Err("--lm-weight needs a number".to_string());
                }
                cfg.lm_weight = Some(
                    args[i]
                        .parse::<f64>()
                        .map_err(|_| format!("bad --lm-weight {:?}", args[i]))?,
                );
            }
            "--word-penalty" => {
                i += 1;
                if i >= args.len() {
                    return Err("--word-penalty needs a number".to_string());
                }
                cfg.word_penalty = Some(
                    args[i]
                        .parse::<f64>()
                        .map_err(|_| format!("bad --word-penalty {:?}", args[i]))?,
                );
            }
            "--help" | "-h" => {
                println!(
                    "hearsay-transcribe: file transcription on the current best model\n\n\
                     usage: hearsay-transcribe [--demo | --input <audio> ...] [--json] [--out-dir <dir>]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown arg {other:?}")),
        }
        i += 1;
    }
    if inputs.is_empty() {
        return Err("no inputs: pass --demo and/or --input <audio>".to_string());
    }
    Ok(Request {
        inputs,
        cfg,
        json,
        out_dir,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let req = match parse_args(&args) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("hearsay-transcribe: {e}");
            usage();
        }
    };

    let t0 = Instant::now();
    let tx = match load_transcriber(&req.cfg) {
        Ok(tx) => tx,
        Err(e) => {
            eprintln!("hearsay-transcribe: load failed: {e}");
            std::process::exit(1);
        }
    };
    let r = &tx.vocab_report;
    eprintln!(
        "hearsay-transcribe: am {} lm {} ({}) load {:.1}s",
        req.cfg.am.display(),
        req.cfg.lm.display(),
        tx.flavor,
        t0.elapsed().as_secs_f64()
    );
    eprintln!(
        "vocab top_n={} kept_words={} kept_prons={} tier1={} tier2={} oov_lm={} oov_book_only={} decoder_vocab={} has_lm={} skipped={}",
        r.top_n_requested,
        r.kept_words,
        r.kept_prons,
        r.tier1_words,
        r.tier2_words,
        r.oov_lm,
        r.oov_book_only,
        tx.vocab_size(),
        tx.has_lm(),
        tx.skipped_words.len()
    );

    if let Some(dir) = &req.out_dir
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!(
            "hearsay-transcribe: cannot create --out-dir {}: {e}",
            dir.display()
        );
        std::process::exit(1);
    }

    let mut rendered: Vec<serde_json::Value> = Vec::with_capacity(req.inputs.len());
    for input in &req.inputs {
        let pcm = match corpus::audio::decode_to_pcm16k(&input.path) {
            Ok(pcm) => pcm,
            Err(e) => {
                eprintln!(
                    "hearsay-transcribe: decode failed for {}: {e}",
                    input.path.display()
                );
                std::process::exit(1);
            }
        };
        let audio_secs = pcm.duration_secs();
        let started = Instant::now();
        let lattice = tx.decode(&pcm.frames);
        let decode_secs = started.elapsed().as_secs_f64();
        if lattice.tokens.is_empty() {
            eprintln!(
                "hearsay-transcribe: empty decode for {} (no words completed)",
                input.path.display()
            );
            std::process::exit(1);
        }
        eprintln!(
            "decoded {}: {} words, {:.2}s audio in {:.2}s",
            input.utt_id,
            lattice.tokens.len(),
            audio_secs,
            decode_secs
        );
        let value = tx.transcript_json_with_why(
            &input.utt_id,
            &input.path.display().to_string(),
            audio_secs,
            decode_secs,
            &lattice,
        );
        if let Some(dir) = &req.out_dir {
            let out_path = dir.join(format!("{}.json", input.utt_id));
            let text = serde_json::to_string_pretty(&value).unwrap_or_else(|e| {
                eprintln!("hearsay-transcribe: JSON serialization failed: {e}");
                std::process::exit(1);
            });
            if let Err(e) = std::fs::write(&out_path, &text) {
                eprintln!(
                    "hearsay-transcribe: write failed for {}: {e}",
                    out_path.display()
                );
                std::process::exit(1);
            }
        }
        rendered.push(value);
    }

    if req.json {
        let text = serde_json::to_string_pretty(&rendered).unwrap_or_else(|e| {
            eprintln!("hearsay-transcribe: JSON serialization failed: {e}");
            std::process::exit(1);
        });
        println!("{text}");
    } else {
        println!("hearsay-transcribe: OK ({} files)", rendered.len());
        for value in &rendered {
            let id = value.get("utt_id").and_then(|v| v.as_str()).unwrap_or("?");
            let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
            println!("{id}: {text}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn demo_flag_expands_fixed_list() {
        let req = parse_args(&argv(&["--demo"])).expect("--demo parses");
        assert_eq!(req.inputs.len(), DEMO_FILES.len());
        assert_eq!(req.inputs[0].utt_id, DEMO_FILES[0].0);
        assert_eq!(req.cfg.top_n, runner::demo_transcribe::DEFAULT_TOP_N);
        assert!(!req.json);
    }

    #[test]
    fn input_flag_derives_utt_id_from_stem() {
        let req = parse_args(&argv(&["--input", "a/b/utt-1.wav"])).expect("--input parses");
        assert_eq!(req.inputs.len(), 1);
        assert_eq!(req.inputs[0].utt_id, "utt-1");
    }

    #[test]
    fn overrides_replace_single_defaults() {
        let req = parse_args(&argv(&[
            "--demo",
            "--am",
            "m.json",
            "--top-n",
            "100",
            "--json",
            "--lm-weight",
            "6",
            "--word-penalty",
            "0",
        ]))
        .expect("overrides parse");
        assert_eq!(req.cfg.am, PathBuf::from("m.json"));
        assert_eq!(req.cfg.top_n, 100);
        assert!(req.json);
        assert_eq!(req.cfg.lm_weight, Some(6.0));
        assert_eq!(req.cfg.word_penalty, Some(0.0));
    }

    #[test]
    fn missing_inputs_is_an_error() {
        assert!(parse_args(&argv(&[])).is_err());
        assert!(parse_args(&argv(&["--bogus"])).is_err());
    }
}
