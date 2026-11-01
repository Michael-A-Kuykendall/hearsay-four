//! train_si_cmvn_retrain: Viterbi-align with a legacy (raw-feature) model,
//! single-pass re-estimate on CMVN features, then Baum-Welch iterations.
//!
//! usage: train_si_cmvn_retrain <subset.tsv> <wav_root> <out_dir>
//!        <cache_root> <align_ckpt> <iters>
//!
//! Two streaming passes bound RAM (raw and CMVN features are never held
//! together): (1) raw features -> forced_align -> state paths; (2) CMVN
//! features from cache (compute-and-fill on miss) -> viterbi_reestimate
//! -> ckpt-cmvn8-iter0 -> <iters> BW iters with per-iter checkpoints.
//! Lexicon helpers intentionally duplicated from train_si.rs (same crate).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::audio::{decode_file, to_mono};
use fse::featcache::{cache_path, load as cache_load, store as cache_store};
use fse::instrument::sha256_hex;
use fse::{
    FEATURE_DIM, HMM_FRONTEND_ID, extract_features, extract_features_cmvn, window_audio,
};
use hmm::{HmmSet, TrainConfig, UttData};
use lexicon::{TIER1_IDENTITY, TieredLexicon, load_tiered};

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

fn load_lexicon() -> TieredLexicon {
    let tier1 = lexicon_table_path();
    let tier2 = librispeech_lexicon_path();
    load_tiered(&tier1, &tier2).unwrap_or_else(|e| {
        panic!(
            "read tiered lexicon {} + {}: {e}",
            tier1.display(),
            tier2.display()
        )
    })
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

fn raw_obs(wav_root: &Path, rel: &str) -> Option<(Vec<Vec<f64>>, u32)> {
    let pcm = decode_file(&wav_root.join(rel)).ok()?;
    let mono = to_mono(&pcm);
    if mono.is_empty() {
        return None;
    }
    let windows = window_audio(&mono, pcm.sample_rate);
    if windows.is_empty() {
        return None;
    }
    let frames = extract_features(&windows, pcm.sample_rate).ok()?;
    if frames.is_empty() || frames[0].len() != FEATURE_DIM {
        return None;
    }
    Some((frames.iter().map(|f| f.to_vec()).collect(), pcm.sample_rate))
}

fn cmvn_obs(cache_root: &Path, wav_root: &Path, rel: &str) -> Option<Vec<Vec<f64>>> {
    let wav_path = wav_root.join(rel);
    let bytes = std::fs::read(&wav_path).ok()?;
    let sha = sha256_hex(&bytes);
    let path = cache_path(cache_root, HMM_FRONTEND_ID, &sha);
    let pcm = decode_file(&wav_path).ok()?;
    if let Some(frames) = cache_load(&path, pcm.sample_rate) {
        return Some(frames.iter().map(|f| f.to_vec()).collect());
    }
    let mono = to_mono(&pcm);
    if mono.is_empty() {
        return None;
    }
    let windows = window_audio(&mono, pcm.sample_rate);
    if windows.is_empty() {
        return None;
    }
    let frames = extract_features_cmvn(&windows, pcm.sample_rate).ok()?;
    if frames.is_empty() || frames[0].len() != FEATURE_DIM {
        return None;
    }
    let _ = cache_store(&path, pcm.sample_rate, &frames);
    Some(frames.iter().map(|f| f.to_vec()).collect())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 7,
        "usage: train_si_cmvn_retrain <subset.tsv> <wav_root> <out_dir> <cache_root> <align_ckpt> <iters>"
    );
    let (subset_path, wav_root, out_dir, cache_root, align_ckpt) = (
        PathBuf::from(&args[1]),
        PathBuf::from(&args[2]),
        PathBuf::from(&args[3]),
        PathBuf::from(&args[4]),
        PathBuf::from(&args[5]),
    );
    let iters: usize = args[6].parse().expect("iters");
    assert!(iters > 0, "need iters>0");
    std::fs::create_dir_all(&out_dir).expect("out_dir");
    println!(
        "cmvn_retrain threads={} iters={iters} frontend={HMM_FRONTEND_ID}",
        rayon::current_num_threads()
    );
    println!("cmvn_retrain tier1={TIER1_IDENTITY}");

    let tiered = load_lexicon();
    let lex: Lexicon = tiered.table;
    let align_model = HmmSet::load_text(&align_ckpt).expect("load align ckpt");

    let text = std::fs::read_to_string(&subset_path).expect("read subset");
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(4, '\t');
        let (id, rel, sec, text) = (
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
            cols.next().unwrap_or("").to_string(),
        );
        if id.is_empty() || rel.is_empty() || sec <= 0.0 {
            continue;
        }
        rows.push((id, rel, text));
    }
    println!("cmvn_retrain rows={}", rows.len());
    assert!(!rows.is_empty(), "no usable rows");

    // ---- pass 1: raw features -> Viterbi paths (parallel; ordered) ----
    let t0 = Instant::now();
    let mut aligned: Vec<(String, Vec<String>, Vec<usize>)> = Vec::new();
    let mut align_ll = 0.0;
    {
        use rayon::prelude::*;
        let done: Vec<Option<(String, Vec<String>, Vec<usize>, f64)>> = rows
            .par_iter()
            .map(|(id, rel, txt)| {
                let (Some((obs, _)), Some(seq)) =
                    (raw_obs(&wav_root, rel), phones_for_train(&lex, txt))
                else {
                    return None;
                };
                match align_model.forced_align(&seq, &obs) {
                    Ok((path, ll)) => Some((id.clone(), seq, path, ll)),
                    Err(_) => None,
                }
            })
            .collect();
        for item in done.into_iter().flatten() {
            let (id, seq, path, ll) = item;
            align_ll += ll;
            aligned.push((id, seq, path));
        }
    }
    let skip1 = rows.len() - aligned.len();
    println!(
        "cmvn_retrain align utts={} skip={skip1} ll={align_ll:.1} s={:.0}",
        aligned.len(),
        t0.elapsed().as_secs_f64()
    );
    assert!(!aligned.is_empty(), "no aligned utterances");

    // ---- pass 2: CMVN features (cache) ----
    let t0 = Instant::now();
    let mut data: Vec<UttData> = Vec::new();
    let mut paths: Vec<Vec<usize>> = Vec::new();
    let mut skip2 = 0usize;
    let by_id: HashMap<&str, &(String, String, String)> =
        rows.iter().map(|r| (r.0.as_str(), r)).collect();
    for (id, seq, path) in &aligned {
        let rel = &by_id[id.as_str()].1;
        match cmvn_obs(&cache_root, &wav_root, rel) {
            Some(obs) if obs.len() == path.len() => {
                data.push((id.clone(), seq.clone(), obs));
                paths.push(path.clone());
            }
            _ => skip2 += 1,
        }
    }
    println!(
        "cmvn_retrain cmvn utts={} skip={skip2} s={:.0}",
        data.len(),
        t0.elapsed().as_secs_f64()
    );
    assert!(!data.is_empty(), "no CMVN utterances");
    drop(by_id);

    // ---- single-pass re-estimate, then BW iters ----
    let mut model = align_model;
    let used = model
        .viterbi_reestimate(&data, &paths, 0.01)
        .expect("viterbi_reestimate");
    // Re-estimated on CMVN features; the align model may be legacy/raw.
    model.frontend_id = Some(HMM_FRONTEND_ID.to_string());
    model
        .save_text(&out_dir.join("ckpt-cmvn8-iter0.json"))
        .expect("ckpt iter0");
    println!("cmvn_retrain reestimate frames={used} ckpt-cmvn8-iter0.json");

    let cfg = TrainConfig {
        bw_iterations: 1,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
        estep_chunk: 2000,
    };
    for i in 1..=iters {
        let tb = Instant::now();
        let ll = model.baum_welch_ids(&data, &cfg).expect("baum_welch");
        let el = tb.elapsed().as_secs_f64();
        let name = format!("ckpt-cmvn8-iter{i}.json");
        model.save_text(&out_dir.join(&name)).expect("checkpoint");
        println!("cmvn_retrain mix=8 iter={i}/{iters} ll={ll:.1} s={el:.0} {name}");
    }
    model
        .save_text(&out_dir.join("model.json"))
        .expect("final model");
    println!("cmvn_retrain DONE {}", out_dir.display());
}
