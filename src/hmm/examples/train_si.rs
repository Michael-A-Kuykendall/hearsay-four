//! R3-v2 SI trainer: flat-start -> Viterbi align -> BW schedules with
//! per-iteration checkpoints (1->2->4 mixtures), speaker-balanced subset.
//!
//! Usage:
//!   train_si <manifest.tsv> <wav_root> <out_dir> <target_h> <i1> <i2> <i4> <i8> [resume]
//! With [resume] = checkpoint path: loads model + <out_dir>/schedule.json,
//! continues the schedule. Checkpoints are HmmSet text (lossless round-trip).
//!
//! Census/lexicon helpers are sourced from r2_perf.rs (R2, same crate);
//! candidate for lib extraction, intentionally duplicated, not diverged.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::audio::{decode_file, to_mono};
use fse::{FEATURE_DIM, extract_features, window_audio};
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

struct Row {
    id: String,
    rel: String,
    seconds: f64,
    text: String,
}

fn speaker_of(id: &str) -> String {
    id.split('-').next().unwrap_or(id).to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 9 || args.len() == 10,
        "usage: train_si <manifest.tsv> <wav_root> <out_dir> <target_h> <i1> <i2> <i4> <i8> [resume_ckpt]"
    );
    let (manifest_path, wav_root, out_dir) =
        (PathBuf::from(&args[1]), PathBuf::from(&args[2]), PathBuf::from(&args[3]));
    let target_h: f64 = args[4].parse().expect("target_h");
    let iters: [usize; 4] = [
        args[5].parse().expect("i1"),
        args[6].parse().expect("i2"),
        args[7].parse().expect("i4"),
        args[8].parse().expect("i8"),
    ];
    let resume = args.get(9).map(PathBuf::from);
    std::fs::create_dir_all(&out_dir).expect("out_dir");
    println!("train_si threads={} target_h={target_h} iters={iters:?}", rayon::current_num_threads());
    println!("train_si tier1={TIER1_IDENTITY}");

    let tiered = load_lexicon();
    let lex: Lexicon = tiered.table;

    // ---- manifest + speaker-balanced selection (usable rows only) ----
    let text = std::fs::read_to_string(&manifest_path).expect("read manifest");
    let mut by_spk: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    let mut n_rows = 0usize;
    let mut n_oov = 0usize;
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
        n_rows += 1;
        if phones_for_train(&lex, &txt).is_none() {
            n_oov += 1;
            continue;
        }
        by_spk
            .entry(speaker_of(&id))
            .or_default()
            .push(Row { id, rel, seconds: sec, text: txt });
    }
    // Water-filling round-robin over speakers (deterministic manifest order).
    let budget = target_h * 3600.0;
    let mut rows: Vec<&Row> = Vec::new();
    let mut kept_sec = 0.0;
    let mut idx = vec![0usize; by_spk.len()];
    let keys: Vec<String> = by_spk.keys().cloned().collect();
    loop {
        let mut progress = false;
        for (s, k) in keys.iter().enumerate() {
            let v = &by_spk[k];
            if idx[s] < v.len() && kept_sec + v[idx[s]].seconds <= budget {
                rows.push(&v[idx[s]]);
                kept_sec += v[idx[s]].seconds;
                idx[s] += 1;
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    // Greedy top-up: first-fit remaining rows in manifest order.
    if kept_sec < budget {
        for k in &keys {
            for r in &by_spk[k] {
                if kept_sec + r.seconds <= budget && !rows.iter().any(|x| x.id == r.id) {
                    rows.push(r);
                    kept_sec += r.seconds;
                }
            }
        }
    }
    let mut spk_used = BTreeSet::new();
    for r in &rows {
        spk_used.insert(speaker_of(&r.id));
    }
    println!(
        "train_si select rows={n_rows} oov_dropped={n_oov} kept_utts={} kept_h={:.3} speakers={}",
        rows.len(),
        kept_sec / 3600.0,
        spk_used.len()
    );
    let subset_path = out_dir.join("subset.tsv");
    {
        let mut s = String::new();
        for r in &rows {
            s.push_str(&format!("{}\t{}\t{}\t{}\n", r.id, r.rel, r.seconds, r.text));
        }
        std::fs::write(&subset_path, s).expect("subset.tsv");
    }

    // ---- MFCC (same block as r2_perf) ----
    let t0 = Instant::now();
    let mut data: Vec<UttData> = Vec::new();
    let mut audio_sec = 0.0;
    for r in &rows {
        let pcm = match decode_file(&wav_root.join(&r.rel)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("train_si SKIP {} decode: {e}", r.id);
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
        let frames = match extract_features(&windows, pcm.sample_rate) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if frames.is_empty() || frames[0].len() != FEATURE_DIM {
            continue;
        }
        let Some(phones) = phones_for_train(&lex, &r.text) else {
            continue;
        };
        audio_sec += r.seconds;
        data.push((
            r.id.clone(),
            phones,
            frames.iter().map(|f| f.to_vec()).collect(),
        ));
    }
    println!(
        "train_si mfcc utts={} audio_h={:.3} s={:.0}",
        data.len(),
        audio_sec / 3600.0,
        t0.elapsed().as_secs_f64()
    );
    assert!(!data.is_empty(), "no usable utterances");

    // ---- model: resume or flat start ----
    let mut inv: BTreeSet<String> = BTreeSet::new();
    for (_, seq, _) in &data {
        inv.extend(seq.iter().cloned());
    }
    let inv_ref: Vec<&str> = inv.iter().map(String::as_str).collect();
    let sched_path = out_dir.join("schedule.json");
    let (mut model, mut stage_done) = match &resume {
        Some(p) => {
            let m = HmmSet::load_text(p).expect("load checkpoint");
            let sched: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&sched_path).expect("schedule.json"))
                    .expect("parse schedule");
            let expect_ckpt = sched["ckpt"].as_str().unwrap_or("");
            let got_ckpt = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            assert_eq!(
                got_ckpt, expect_ckpt,
                "resume checkpoint {got_ckpt} != schedule latest {expect_ckpt}: pass the newest checkpoint"
            );
            let done = [
                sched["done1"].as_u64().unwrap_or(0) as usize,
                sched["done2"].as_u64().unwrap_or(0) as usize,
                sched["done4"].as_u64().unwrap_or(0) as usize,
                sched["done8"].as_u64().unwrap_or(0) as usize,
            ];
            let sched_iters: Vec<usize> = sched["iters"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as usize).collect())
                .unwrap_or_default();
            assert_eq!(
                sched_iters,
                iters.to_vec(),
                "resume iters {iters:?} != schedule iters {sched_iters:?}"
            );
            println!("train_si resume {} done={done:?}", p.display());
            (m, done)
        }
        None => {
            let obs: Vec<Vec<Vec<f64>>> =
                data.iter().map(|(_, _, o)| o.clone()).collect();
            let m = HmmSet::flat_start(&inv_ref, &obs, 0.01).expect("flat_start");
            let t2 = Instant::now();
            let mut align_ll = 0.0;
            for (_, seq, o) in &data {
                let (_, ll) = m.forced_align(seq, o).expect("forced_align");
                align_ll += ll;
            }
            println!(
                "train_si viterbi_align ll={align_ll:.1} s={:.0}",
                t2.elapsed().as_secs_f64()
            );
            m.save_text(&out_dir.join("ckpt-flat.json")).expect("ckpt flat");
            (m, [0, 0, 0, 0])
        }
    };

    let cfg = TrainConfig {
        bw_iterations: 1,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
    };
    let mixes = [1usize, 2, 4, 8];
    for (s, &mix) in mixes.iter().enumerate() {
        // Split once on first entry to stages 2/4: fresh runs always split;
        // resume splits only when entering an unstarted stage (done[s]==0
        // means the loaded checkpoint is the tail of the previous stage at
        // the lower mix; mid-stage resume skips the split). Prior stages
        // must be complete before any split.
        if mix > 1 && stage_done[s] == 0 {
            assert!(
                done_before(&stage_done, s, &iters),
                "cannot enter stage {mix}: prior stages incomplete"
            );
            model.split_mixtures_to(mix);
            println!("train_si split to {mix} mixes");
        }
        while stage_done[s] < iters[s] {
            let tb = Instant::now();
            let ll = model.baum_welch_ids(&data, &cfg).expect("baum_welch");
            stage_done[s] += 1;
            let el = tb.elapsed().as_secs_f64();
            let name = format!("ckpt-{mix}mix-iter{}.json", stage_done[s]);
            model.save_text(&out_dir.join(&name)).expect("checkpoint");
            let sched = serde_json::json!({
                "mixes": mix, "done1": stage_done[0],
                "done2": stage_done[1], "done4": stage_done[2],
                "done8": stage_done[3],
                "iters": iters, "ll": ll, "ckpt": name,
            });
            std::fs::write(&sched_path, serde_json::to_string_pretty(&sched).unwrap())
                .expect("schedule");
            println!("train_si mix={mix} iter={}/{} ll={ll:.1} s={el:.0} {name}", stage_done[s], iters[s]);
        }
    }
    model
        .save_text(&out_dir.join("model.json"))
        .expect("final model");
    println!("train_si DONE {}", out_dir.display());
}

fn done_before(done: &[usize; 4], s: usize, iters: &[usize; 4]) -> bool {
    (0..s).all(|i| done[i] >= iters[i])
}
