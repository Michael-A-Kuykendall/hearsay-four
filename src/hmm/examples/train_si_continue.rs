//! train_si_continue: extend a finished train_si run to more mixtures.
//!
//! Loads a stage-final checkpoint, splits to <target_mix> mixtures, and runs
//! <iters> Baum-Welch iterations with per-iter checkpoints. Viterbi realign is
//! NOT needed: the loaded model is already aligned and BW re-estimates.
//!
//! Rows come from the parent run's subset.tsv (identical utterance set, no
//! reselection). Lexicon/census helpers intentionally duplicated from
//! train_si.rs (same crate), not diverged.
//!
//! usage: train_si_continue <subset.tsv> <wav_root> <out_dir> <start_ckpt>
//!        <target_mix> <iters>
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::audio::{decode_file, to_mono};
use fse::{FEATURE_DIM, HMM_FRONTEND_ID, extract_features_cmvn, window_audio};
use hmm::{HmmSet, TrainConfig, UttData, check_frontend};
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 7,
        "usage: train_si_continue <subset.tsv> <wav_root> <out_dir> <start_ckpt> <target_mix> <iters>"
    );
    let (subset_path, wav_root, out_dir, start_ckpt) = (
        PathBuf::from(&args[1]),
        PathBuf::from(&args[2]),
        PathBuf::from(&args[3]),
        PathBuf::from(&args[4]),
    );
    let target_mix: usize = args[5].parse().expect("target_mix");
    let iters: usize = args[6].parse().expect("iters");
    assert!(target_mix > 1 && iters > 0, "need target_mix>1 iters>0");
    std::fs::create_dir_all(&out_dir).expect("out_dir");
    println!(
        "train_si_continue threads={} target_mix={target_mix} iters={iters}",
        rayon::current_num_threads()
    );
    println!("train_si_continue tier1={TIER1_IDENTITY}");

    let tiered = load_lexicon();
    let lex: Lexicon = tiered.table;

    // ---- rows: parent subset.tsv verbatim (id, rel, sec, text) ----
    let text = std::fs::read_to_string(&subset_path).expect("read subset");
    let mut rows: Vec<(String, String, f64, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(4, '\t');
        let (id, rel, sec, txt) = (
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
            cols.next().unwrap_or("").to_string(),
        );
        if id.is_empty() || rel.is_empty() || sec <= 0.0 {
            continue;
        }
        rows.push((id, rel, sec, txt));
    }
    println!("train_si_continue rows={}", rows.len());
    assert!(!rows.is_empty(), "no usable rows");

    // ---- MFCC (same block as train_si) ----
    let t0 = Instant::now();
    let mut data: Vec<UttData> = Vec::new();
    let mut audio_sec = 0.0;
    for (id, rel, sec, txt) in &rows {
        let pcm = match decode_file(&wav_root.join(rel)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("train_si_continue SKIP {id} decode: {e}");
                continue;
            }
        };
        let mono = to_mono(&pcm);
        if mono.is_empty() {
            continue;
        }
        let windows = window_audio(&mono, pcm.sample_rate);
        if windows.is_empty() {
            continue;
        }
        let frames = match extract_features_cmvn(&windows, pcm.sample_rate) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if frames.is_empty() || frames[0].len() != FEATURE_DIM {
            continue;
        }
        let Some(phones) = phones_for_train(&lex, txt) else {
            continue;
        };
        audio_sec += sec;
        data.push((
            id.clone(),
            phones,
            frames.iter().map(|f| f.to_vec()).collect(),
        ));
    }
    println!(
        "train_si_continue mfcc utts={} audio_h={:.3} s={:.0}",
        data.len(),
        audio_sec / 3600.0,
        t0.elapsed().as_secs_f64()
    );
    assert!(!data.is_empty(), "no usable utterances");

    // ---- split + iterate ----
    let mut model = HmmSet::load_text(&start_ckpt).expect("load checkpoint");
    // Continued training uses this binary's (CMVN) features.
    check_frontend(
        &model.frontend_id,
        HMM_FRONTEND_ID,
        &start_ckpt.to_string_lossy(),
    )
    .expect("frontend guard");
    if model.frontend_id.is_none() {
        model.frontend_id = Some(HMM_FRONTEND_ID.to_string());
    }
    model.split_mixtures_to(target_mix);
    println!("train_si_continue split to {target_mix} mixes");
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
        let name = format!("ckpt-{target_mix}mix-iter{i}.json");
        model.save_text(&out_dir.join(&name)).expect("checkpoint");
        println!("train_si_continue mix={target_mix} iter={i}/{iters} ll={ll:.1} s={el:.0} {name}");
    }
    model
        .save_text(&out_dir.join("model.json"))
        .expect("final model");
    println!("train_si_continue DONE {}", out_dir.display());
}
