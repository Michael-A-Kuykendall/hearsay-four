//! R3-v2 dev eval: PER (+ finite-likelihood check) of a saved HmmSet on a
//! LibriSpeech subset manifest (dev-clean).
//! Usage: eval_si <model.json> <manifest.tsv> <wav_root>
//! Prints `dev_per=...`, `dev_oov_dropped=...`, `dev_ll_min=...`.
//! Census/lexicon helpers sourced from r2_perf.rs (same crate).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use corpus::audio::{decode_file, to_mono};
use fse::featcache::{cache_path, load as cache_load, store as cache_store};
use fse::instrument::sha256_hex;
use fse::{
    FEATURE_DIM, HMM_FRONTEND_ID, HMM_FRONTEND_ID_RAW, extract_features, extract_features_cmvn,
    window_audio,
};
use hmm::{HmmSet, check_frontend};
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
/// Cache-aware feature fetch: hit returns frames, miss computes, stores,
/// and returns. `raw` selects the hmm-fe1 namespace (pre-CMVN models);
/// CMVN uses [`HMM_FRONTEND_ID`]. Duplicated in train_si (not diverged).
fn cached_frames(
    cache_root: &Option<PathBuf>,
    raw: bool,
    wav_path: &Path,
    rate: u32,
    mono: &[i16],
) -> Option<Vec<fse::Frame>> {
    let fid = if raw {
        HMM_FRONTEND_ID_RAW
    } else {
        HMM_FRONTEND_ID
    };
    if let Some(root) = cache_root {
        let bytes = std::fs::read(wav_path).ok()?;
        let sha = sha256_hex(&bytes);
        let path = cache_path(root, fid, &sha);
        if let Some(frames) = cache_load(&path, rate) {
            return Some(frames);
        }
        let windows = window_audio(mono, rate);
        if windows.is_empty() {
            return None;
        }
        let frames = if raw {
            extract_features(&windows, rate).ok()?
        } else {
            extract_features_cmvn(&windows, rate).ok()?
        };
        let _ = cache_store(&path, rate, &frames);
        return Some(frames);
    }
    let windows = window_audio(mono, rate);
    if windows.is_empty() {
        return None;
    }
    if raw {
        extract_features(&windows, rate).ok()
    } else {
        extract_features_cmvn(&windows, rate).ok()
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Optional --cache <dir> / --per-utt <file> pairs (position-independent;
    // stripped before positional parse).
    let mut cache_root: Option<PathBuf> = None;
    let mut per_utt_path: Option<PathBuf> = None;
    let mut argv: Vec<String> = Vec::with_capacity(args.len());
    let mut arg_it = args.into_iter();
    argv.push(arg_it.next().expect("argv0"));
    while let Some(a) = arg_it.next() {
        if a == "--cache" {
            cache_root = Some(PathBuf::from(arg_it.next().expect("--cache needs <dir>")));
        } else if a == "--per-utt" {
            per_utt_path = Some(PathBuf::from(arg_it.next().expect("--per-utt needs <file>")));
        } else {
            argv.push(a);
        }
    }
    let args = argv;
    assert!(
        args.len() == 4 || args.len() == 5,
        "usage: eval_si <model.json> <manifest.tsv> <wav_root> [--raw] [--cache <dir>] [--per-utt <file>]"
    );
    let raw = args.len() == 5 && args[4] == "--raw";
    assert!(args.len() == 4 || raw, "5th arg must be --raw");
    let model = HmmSet::load_text(&PathBuf::from(&args[1])).expect("load model");
    let selected_frontend = if raw {
        HMM_FRONTEND_ID_RAW
    } else {
        HMM_FRONTEND_ID
    };
    check_frontend(&model.frontend_id, selected_frontend, &args[1]).expect("frontend guard");
    let tiered: TieredLexicon =
        load_tiered(&lexicon_table_path(), &librispeech_lexicon_path()).expect("tiered lexicon");
    let lex: Lexicon = tiered.table;
    let text = std::fs::read_to_string(&args[2]).expect("manifest");
    let wav_root = PathBuf::from(&args[3]);
    let mut pairs: Vec<(Vec<String>, Vec<Vec<f64>>)> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    let mut oov_dropped = 0usize;
    let mut ll_min = f64::INFINITY;
    // Parse is cheap and sequential; decode+align below is parallel.
    let mut rows: Vec<(String, String, Vec<String>)> = Vec::new();
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
        rows.push((id.to_string(), rel.to_string(), phones));
    }
    {
        use rayon::prelude::*;
        let done: Vec<Option<(String, Vec<String>, Vec<Vec<f64>>, f64)>> = rows
            .par_iter()
            .map(|(id, rel, phones)| {
                let wav_path = wav_root.join(rel);
                let pcm = decode_file(&wav_path).ok()?;
                let mono = to_mono(&pcm);
                if mono.is_empty() {
                    return None;
                }
                let frames =
                    cached_frames(&cache_root, raw, &wav_path, pcm.sample_rate, &mono)?;
                if frames.is_empty() || frames[0].len() != FEATURE_DIM {
                    return None;
                }
                let obs: Vec<Vec<f64>> = frames.iter().map(|f| f.to_vec()).collect();
                let (_, ll) = model.forced_align(phones, &obs).expect("forced_align");
                assert!(ll.is_finite(), "non-finite dev likelihood: {id}");
                Some((id.clone(), phones.clone(), obs, ll))
            })
            .collect();
        for item in done.into_iter().flatten() {
            let (id, phones, obs, ll) = item;
            ll_min = ll_min.min(ll);
            ids.push(id);
            pairs.push((phones, obs));
        }
    }
    let report = model.phone_error_rate(&pairs).expect("phone_error_rate");
    let per = report.per;
    if let Some(path) = &per_utt_path {
        assert_eq!(
            ids.len(),
            report.per_utterance.len(),
            "id/per-utt length mismatch"
        );
        let mut out = String::new();
        for (id, pu) in ids.iter().zip(report.per_utterance.iter()) {
            out.push_str(&format!("{id}\t{pu:.6}\n"));
        }
        std::fs::write(path, out).expect("write per-utt");
    }
    println!("dev_n={}", pairs.len());
    println!("dev_oov_dropped={oov_dropped}");
    println!("dev_ll_min={ll_min:.1}");
    println!("dev_per={per:.4}");
}
