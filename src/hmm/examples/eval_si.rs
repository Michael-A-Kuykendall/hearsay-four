//! R3-v2 dev eval: PER (+ finite-likelihood check) of a saved HmmSet on a
//! LibriSpeech subset manifest (dev-clean).
//! Usage: eval_si <model.json> <manifest.tsv> <wav_root>
//! Prints `dev_per=...`, `dev_oov_dropped=...`, `dev_ll_min=...`.
//! Census/lexicon helpers sourced from r2_perf.rs (same crate).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use corpus::audio::{decode_file, to_mono};
use fse::{FEATURE_DIM, extract_features, window_audio};
use hmm::HmmSet;
use lexicon::{TieredLexicon, load_tiered};

type Lexicon = HashMap<String, Vec<Vec<String>>>;

fn lexicon_table_path() -> PathBuf {
    if let Ok(root) = std::env::var("BETTER_VOICE_ROOT") {
        let cand =
            PathBuf::from(root).join("data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt");
        if cand.is_file() {
            return cand;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../better-voice/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt")
}

fn librispeech_lexicon_path() -> PathBuf {
    if let Ok(path) = std::env::var("LIBRISPEECH_LEXICON") {
        let cand = PathBuf::from(path);
        if cand.is_file() {
            return cand;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/lexicon/librispeech-lexicon.txt")
}

fn norm_token(tok: &str) -> String {
    let mut w = tok.trim().to_uppercase();
    if w.ends_with(')')
        && let Some(open) = w.rfind('(')
        && w[open + 1..w.len() - 1].chars().all(|c| c.is_ascii_digit())
    {
        w.truncate(open);
    }
    while w.ends_with('\'') {
        w.pop();
    }
    w
}

fn strip_edge_punct(w: &str) -> String {
    w.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '(' | ')' | '[' | ']' | '-' | '—' | '–'
        )
    })
    .to_string()
}

fn lex_lookup(lex: &Lexicon, tok: &str) -> Option<(String, Vec<String>)> {
    let w = norm_token(tok);
    if w.is_empty() {
        return None;
    }
    let mut cands = vec![w.clone()];
    let stripped = strip_edge_punct(&w);
    if stripped != w {
        cands.push(stripped);
    }
    let mut extra = Vec::new();
    for c in &cands {
        if let Some(base) = c.strip_suffix("'S")
            && base.len() >= 2
        {
            extra.push(base.to_string());
        }
    }
    cands.extend(extra);
    for c in cands {
        if c.is_empty() {
            continue;
        }
        if let Some(prons) = lex.get(&c)
            && let Some(first) = prons.first()
        {
            return Some((c, first.clone()));
        }
    }
    None
}

fn phones_for_train(lex: &Lexicon, text: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let toks: Vec<&str> = text.split_whitespace().collect();
    if toks.is_empty() {
        return None;
    }
    for t in toks {
        out.extend(lex_lookup(lex, t)?.1);
    }
    Some(out)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 4,
        "usage: eval_si <model.json> <manifest.tsv> <wav_root>"
    );
    let model = HmmSet::load_text(&PathBuf::from(&args[1])).expect("load model");
    let tiered: TieredLexicon = load_tiered(&lexicon_table_path(), &librispeech_lexicon_path())
        .expect("tiered lexicon");
    let lex: Lexicon = tiered.table;
    let text = std::fs::read_to_string(&args[2]).expect("manifest");
    let wav_root = PathBuf::from(&args[3]);
    let mut pairs: Vec<(Vec<String>, Vec<Vec<f64>>)> = Vec::new();
    let mut oov_dropped = 0usize;
    let mut ll_min = f64::INFINITY;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(4, '\t');
        let (id, rel, sec, txt) = (
            cols.next().unwrap_or(""),
            cols.next().unwrap_or(""),
            cols.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
            cols.next().unwrap_or(""),
        );
        if id.is_empty() || rel.is_empty() || sec <= 0.0 {
            continue;
        }
        let Some(phones) = phones_for_train(&lex, txt) else {
            oov_dropped += 1;
            continue;
        };
        let pcm = match decode_file(&wav_root.join(rel)) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mono = to_mono(&pcm);
        if mono.is_empty() {
            continue;
        }
        let windows = window_audio(&mono, pcm.sample_rate);
        if windows.is_empty() {
            continue;
        }
        let frames = match extract_features(&windows, pcm.sample_rate) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if frames.is_empty() || frames[0].len() != FEATURE_DIM {
            continue;
        }
        let obs: Vec<Vec<f64>> = frames.iter().map(|f| f.to_vec()).collect();
        let (_, ll) = model.forced_align(&phones, &obs).expect("forced_align");
        assert!(ll.is_finite(), "non-finite dev likelihood: {id}");
        ll_min = ll_min.min(ll);
        pairs.push((phones, obs));
    }
    let per = model.phone_error_rate(&pairs).expect("phone_error_rate").per;
    println!("dev_n={}", pairs.len());
    println!("dev_oov_dropped={oov_dropped}");
    println!("dev_ll_min={ll_min:.1}");
    println!("dev_per={per:.4}");
}
