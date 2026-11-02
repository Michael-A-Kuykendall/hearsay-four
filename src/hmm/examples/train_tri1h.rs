//! train_tri1h: lane B wave 1b — triphone 1 h pipeline, green run.
//!
//! Clone the main CMVN monophone -> align a fixed 1 h subset once ->
//! accumulate triphone stats -> grow decision trees -> tie states ->
//! Baum-Welch 4 iters with mixtures 1->2->4->8 (retie after every round).
//! Prints triphone PER next to monophone PER on the same PER subset.
//!
//! Features come from the frozen fe2 CMVN cache (HFC1 files keyed by
//! sha256 of the wav bytes); no MFCC is recomputed here.
//!
//! usage: train_tri1h <mono_model.json> <manifest.tsv> <wav_root> <feat_cache>
//!        <out_dir> --subset-size <N> --subset-seed <S> [--per-utts <K>]
//!        [--gain <G>] [--minocc <O>] [--max-leaves <L>]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use fse::instrument::sha256_hex;
use hmm::triphone::{
    MAX_PROV_UTTS, TyingConfig, accumulate_stats, build_tied_model, build_tying,
    cap_model_provenance, center_of_unit, default_questions, merge_stats, retie_averaging,
    sample_subset, unit_keys_for_word,
};
use hmm::{HmmSet, SIL, SP, TrainConfig, UttData, phone_edit_distance};
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

fn lex_lookup(lex: &Lexicon, tok: &str) -> Option<Vec<String>> {
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
            return Some(first.clone());
        }
    }
    None
}

/// Per-word phone strings; None if any word is OOV (mirrors train_si's
/// usability rule, keeping word boundaries for word-internal triphones).
fn phones_for_words(lex: &Lexicon, text: &str) -> Option<Vec<Vec<String>>> {
    let toks: Vec<&str> = text.split_whitespace().collect();
    if toks.is_empty() {
        return None;
    }
    toks.iter().map(|t| lex_lookup(lex, t)).collect()
}

struct Row {
    id: String,
    rel: String,
    seconds: f64,
    text: String,
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

/// Frozen HFC1 feature cache: magic + u32 dim + u32 nframes + u32 sr,
/// then nframes*dim f64 LE. Keyed by sha256 of the wav file bytes.
fn load_cached_frames(
    cache_dir: &Path,
    wav_path: &Path,
    dim: usize,
) -> Result<Vec<Vec<f64>>, String> {
    let bytes = std::fs::read(wav_path).map_err(|e| format!("read wav: {e}"))?;
    let h = sha256_hex(&bytes);
    let p = cache_dir.join(&h[..2]).join(format!("{h}.bin"));
    let b = std::fs::read(&p).map_err(|e| format!("cache miss {}: {e}", p.display()))?;
    if b.len() < 16 || &b[0..4] != b"HFC1" {
        return Err(format!("bad magic {}", p.display()));
    }
    let get = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (dim_f, nf) = (get(4), get(8));
    if dim_f != dim {
        return Err(format!("dim {dim_f} != model {dim}"));
    }
    if b.len() < 16 + nf * dim_f * 8 {
        return Err(format!("truncated {}", p.display()));
    }
    let mut out = Vec::with_capacity(nf);
    for f in 0..nf {
        let mut row = Vec::with_capacity(dim_f);
        for d in 0..dim_f {
            let o = 16 + (f * dim_f + d) * 8;
            row.push(f64::from_le_bytes(b[o..o + 8].try_into().unwrap()));
        }
        out.push(row);
    }
    Ok(out)
}

fn peak_rss_mb() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("VmHWM:").map(|v| {
                    let kb: f64 = v
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0);
                    format!("{:.0}", kb / 1024.0)
                })
            })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

struct Utt {
    id: String,
    seq: Vec<String>,
    spans: Vec<(usize, usize)>,
    units: Vec<String>,
    obs: Vec<Vec<f64>>,
}

/// Inventory-constrained triphone decode: free phone loop over the monophone
/// inventory plus this utterance's own triphone/biphone units (the full
/// cross-product loop is infeasible; the constraint is recorded in run.json).
/// Decoded units map to centers before scoring.
fn triphone_per(model: &HmmSet, utts: &[Utt]) -> (f64, usize, usize) {
    let mono: Vec<String> = model
        .phones
        .keys()
        .filter(|k| !k.contains('-') && !k.contains('+'))
        .cloned()
        .collect();
    let strip = |seq: &[String]| -> Vec<String> {
        seq.iter()
            .filter(|p| *p != SIL && *p != SP)
            .cloned()
            .collect()
    };
    let (mut errors, mut total) = (0usize, 0usize);
    for (ui, u) in utts.iter().enumerate() {
        let mut units: BTreeSet<String> = mono.iter().cloned().collect();
        units.extend(u.units.iter().cloned());
        let nunits = units.len() as f64;
        let mut states: Vec<(String, usize)> = Vec::new();
        for name in &units {
            for s in 0..model.phones[name].n_states {
                states.push((name.clone(), s));
            }
        }
        let n = states.len();
        let neg = f64::NEG_INFINITY;
        let start_log = -nunits.ln();
        let jump_log = 0.5f64.ln() - nunits.ln();
        let half = 0.5f64.ln();
        let log_emit: Vec<Vec<f64>> = u
            .obs
            .iter()
            .map(|o| {
                states
                    .iter()
                    .map(|(p, s)| model.emission(p, *s).unwrap().log_prob(o))
                    .collect()
            })
            .collect();
        let mut delta = vec![neg; n];
        for (j, st) in states.iter().enumerate() {
            if st.1 == 0 {
                delta[j] = start_log + log_emit[0][j];
            }
        }
        let mut psi = vec![vec![0usize; n]; u.obs.len()];
        for t in 1..u.obs.len() {
            let mut nd = vec![neg; n];
            // Best cross-unit entry per frame: any last-state -> any first-state.
            let mut best_jump = neg;
            let mut arg_jump = 0usize;
            for (i, (ip, is)) in states.iter().enumerate() {
                if delta[i] == neg {
                    continue;
                }
                if *is == model.phones[ip].n_states - 1 && delta[i] > best_jump {
                    best_jump = delta[i];
                    arg_jump = i;
                }
            }
            for j in 0..n {
                let e = log_emit[t][j];
                let (mut best, mut arg) = (delta[j] + half, j);
                let (jp, js) = &states[j];
                if *js > 0 {
                    let i = j - 1;
                    if states[i].0 == *jp && delta[i] + half > best {
                        best = delta[i] + half;
                        arg = i;
                    }
                } else if best_jump + jump_log > best {
                    best = best_jump + jump_log;
                    arg = arg_jump;
                }
                nd[j] = best + e;
                psi[t][j] = arg;
            }
            delta = nd;
        }
        let (mut best, mut last) = (neg, 0usize);
        for (j, &dj) in delta.iter().enumerate() {
            if dj > best {
                best = dj;
                last = j;
            }
        }
        let mut path = vec![0usize; u.obs.len()];
        path[u.obs.len() - 1] = last;
        for t in (1..u.obs.len()).rev() {
            path[t - 1] = psi[t][path[t]];
        }
        let mut hyp_units: Vec<String> = Vec::new();
        for &idx in &path {
            let p = states[idx].0.clone();
            if hyp_units.last() != Some(&p) {
                hyp_units.push(p);
            }
        }
        let hyp: Vec<String> = hyp_units
            .iter()
            .map(|k| center_of_unit(k).to_string())
            .collect();
        let reference = strip(&u.seq);
        let hyp = strip(&hyp);
        errors += phone_edit_distance(&reference, &hyp);
        total += reference.len();
        if (ui + 1) % 10 == 0 {
            println!("train_tri1h tri-per decoded {}/{}", ui + 1, utts.len());
        }
    }
    let per = if total == 0 {
        0.0
    } else {
        errors as f64 / total as f64
    };
    (per, errors, total)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() >= 6,
        "usage: train_tri1h <mono_model.json> <manifest.tsv> <wav_root> <feat_cache> <out_dir> --subset-size <N> --subset-seed <S> [--per-utts <K>] [--gain <G>] [--minocc <O>] [--max-leaves <L>]"
    );
    let (mono_path, manifest_path, wav_root, cache_dir, out_dir) = (
        PathBuf::from(&args[1]),
        PathBuf::from(&args[2]),
        PathBuf::from(&args[3]),
        PathBuf::from(&args[4]),
        PathBuf::from(&args[5]),
    );
    let subset_size: usize = flag(&args, "--subset-size")
        .expect("--subset-size")
        .parse()
        .expect("size");
    let subset_seed: u64 = flag(&args, "--subset-seed")
        .expect("--subset-seed")
        .parse()
        .expect("seed");
    let per_utts: usize = flag(&args, "--per-utts")
        .map(|s| s.parse().expect("per-utts"))
        .unwrap_or(30);
    let tying = TyingConfig {
        gain_threshold: flag(&args, "--gain")
            .map(|s| s.parse().expect("gain"))
            .unwrap_or(10.0),
        min_occupancy: flag(&args, "--minocc")
            .map(|s| s.parse().expect("minocc"))
            .unwrap_or(10.0),
        max_leaves: flag(&args, "--max-leaves")
            .map(|s| s.parse().expect("max-leaves"))
            .unwrap_or(32),
        var_floor: 1e-6,
    };
    std::fs::create_dir_all(&out_dir).expect("out_dir");
    println!(
        "train_tri1h threads={} tier1={TIER1_IDENTITY}",
        rayon::current_num_threads()
    );

    let tiered: TieredLexicon =
        load_tiered(lexicon_table_path(), librispeech_lexicon_path()).expect("tiered lexicon");
    let lex: Lexicon = tiered.table;

    // ---- manifest + deterministic subset ----
    let text = std::fs::read_to_string(&manifest_path).expect("read manifest");
    let mut usable: Vec<Row> = Vec::new();
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
        if phones_for_words(&lex, &txt).is_none() {
            n_oov += 1;
            continue;
        }
        usable.push(Row {
            id,
            rel,
            seconds: sec,
            text: txt,
        });
    }
    let picks = sample_subset(usable.len(), subset_size, subset_seed);
    let kept_sec: f64 = picks.iter().map(|&i| usable[i].seconds).sum();
    println!(
        "train_tri1h subset rows={n_rows} oov_dropped={n_oov} usable={} kept_utts={} kept_h={:.3} seed={subset_seed}",
        usable.len(),
        picks.len(),
        kept_sec / 3600.0
    );
    {
        let mut s = String::new();
        for &i in &picks {
            let r = &usable[i];
            s.push_str(&format!("{}\t{}\t{}\t{}\n", r.id, r.rel, r.seconds, r.text));
        }
        std::fs::write(out_dir.join("subset.tsv"), s).expect("subset.tsv");
    }

    // ---- model + cached features ----
    let t0 = Instant::now();
    let mut mono = HmmSet::load_text(&mono_path).expect("load mono model");
    println!(
        "train_tri1h mono load s={:.0} dim={} phones={}",
        t0.elapsed().as_secs_f64(),
        mono.dim,
        mono.phones.len()
    );
    // The full93 checkpoint carries ~740 MB of provenance utterance ids;
    // cap them before the clone into the tied model (RSS gate < 8 GB).
    let (prov_before, prov_after) = cap_model_provenance(&mut mono);
    println!("train_tri1h prov_cap={MAX_PROV_UTTS} prov_ids {prov_before}->{prov_after}");
    let mut utts: Vec<Utt> = Vec::new();
    let mut n_cache_miss = 0usize;
    for &i in &picks {
        let r = &usable[i];
        let words = phones_for_words(&lex, &r.text).expect("usable");
        let mut seq = Vec::new();
        let mut spans = Vec::new();
        for w in &words {
            let start = seq.len();
            seq.extend(w.iter().cloned());
            spans.push((start, seq.len()));
        }
        let obs = match load_cached_frames(&cache_dir, &wav_root.join(&r.rel), mono.dim) {
            Ok(o) if !o.is_empty() => o,
            _ => {
                n_cache_miss += 1;
                continue;
            }
        };
        let mut units = Vec::new();
        for w in &words {
            units.extend(unit_keys_for_word(w));
        }
        utts.push(Utt {
            id: r.id.clone(),
            seq,
            spans,
            units,
            obs,
        });
    }
    let frames: usize = utts.iter().map(|u| u.obs.len()).sum();
    println!(
        "train_tri1h feats utts={} frames={frames} cache_miss={n_cache_miss} s={:.0}",
        utts.len(),
        t0.elapsed().as_secs_f64()
    );
    assert!(!utts.is_empty(), "no usable utterances");

    // ---- align once with the monophone, accumulate triphone stats ----
    let t1 = Instant::now();
    let mut pooled: BTreeMap<(hmm::triphone::Triphone, usize), hmm::triphone::TriStateStats> =
        BTreeMap::new();
    let mut align_ll = 0.0;
    for u in &utts {
        let (path, ll) = mono.forced_align(&u.seq, &u.obs).expect("forced_align");
        align_ll += ll;
        let one =
            accumulate_stats(&mono, &u.id, &u.seq, &u.spans, &path, &u.obs).expect("accumulate");
        merge_stats(&mut pooled, one);
    }
    let n_full = pooled
        .keys()
        .filter(|(t, _)| t.left.is_some() && t.right.is_some())
        .count();
    println!(
        "train_tri1h align ll={align_ll:.1} tri_states={} full_tri_states={n_full} s={:.0}",
        pooled.len(),
        t1.elapsed().as_secs_f64()
    );

    // ---- trees + tying ----
    let t2 = Instant::now();
    let stats_vec: Vec<_> = pooled.values().cloned().collect();
    let tied = build_tying(&stats_vec, &default_questions(), &tying);
    let max_depth = tied.iter().map(|t| t.path.len()).max().unwrap_or(0);
    std::fs::write(
        out_dir.join("tying.json"),
        serde_json::to_string_pretty(&tied).unwrap(),
    )
    .expect("tying.json");
    println!(
        "train_tri1h tying tied={} max_depth={max_depth} s={:.0}",
        tied.len(),
        t2.elapsed().as_secs_f64()
    );

    // ---- tied model + BW 1->2->4->8 ----
    let mut units: BTreeSet<String> = BTreeSet::new();
    for u in &utts {
        units.extend(u.units.iter().cloned());
    }
    let mut tri = build_tied_model(&mono, &pooled, &tied, &units).expect("tied model");
    tri.validate_provenance().expect("tied provenance");
    println!(
        "train_tri1h tied_model units={} phones={}",
        units.len(),
        tri.phones.len()
    );
    let tri_data: Vec<UttData> = utts
        .iter()
        .map(|u| (u.id.clone(), u.units.clone(), u.obs.clone()))
        .collect();
    // Small E-step chunks: one chunk of per-utterance partials is held at
    // once, so peak transient stays ~45 MB instead of ~500 MB (RSS gate).
    let one = TrainConfig {
        bw_iterations: 1,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
        estep_chunk: 32,
    };
    let mut bw_ll = Vec::new();
    for &mix in &[1usize, 2, 4, 8] {
        if mix > 1 {
            tri.split_mixtures_to(mix);
            println!("train_tri1h split to {mix} mixes");
        }
        let tb = Instant::now();
        let ll = tri.baum_welch_ids(&tri_data, &one).expect("baum_welch");
        tri.validate_provenance().expect("bw provenance");
        retie_averaging(&mut tri, &tied).expect("retie");
        bw_ll.push(ll);
        println!(
            "train_tri1h mix={mix} ll={ll:.1} s={:.0}",
            tb.elapsed().as_secs_f64()
        );
    }
    // Cap BW/retie provenance unions before serializing (validate stays green).
    let (tprov_before, tprov_after) = cap_model_provenance(&mut tri);
    tri.validate_provenance().expect("final provenance");
    println!("train_tri1h tri prov_ids {tprov_before}->{tprov_after}");
    tri.save_text(&out_dir.join("tri-final.json"))
        .expect("save final");

    // ---- PER: monophone vs triphone on the same PER subset ----
    let per_n = per_utts.min(utts.len());
    let per_utts_slice = &utts[..per_n];
    let t3 = Instant::now();
    let pairs: Vec<(Vec<String>, Vec<Vec<f64>>)> = per_utts_slice
        .iter()
        .map(|u| (u.seq.clone(), u.obs.clone()))
        .collect();
    let mono_rep = mono.phone_error_rate(&pairs).expect("mono per");
    println!(
        "train_tri1h mono_per={:.4} errors={} ref_phones={} s={:.0}",
        mono_rep.per,
        mono_rep.total_errors,
        mono_rep.total_ref_phones,
        t3.elapsed().as_secs_f64()
    );
    let t4 = Instant::now();
    let (tri_per, tri_err, tri_tot) = triphone_per(&tri, per_utts_slice);
    println!(
        "train_tri1h tri_per={tri_per:.4} errors={tri_err} ref_phones={tri_tot} s={:.0}",
        t4.elapsed().as_secs_f64()
    );

    let run = serde_json::json!({
        "mono_model": mono_path.display().to_string(),
        "manifest": manifest_path.display().to_string(),
        "subset_size": subset_size,
        "subset_seed": subset_seed,
        "kept_utts": utts.len(),
        "kept_h": kept_sec / 3600.0,
        "cache_miss": n_cache_miss,
        "frames": frames,
        "align_ll": align_ll,
        "tri_states": pooled.len(),
        "full_tri_states": n_full,
        "tied_states": tied.len(),
        "max_depth": max_depth,
        "tying": { "gain": tying.gain_threshold, "minocc": tying.min_occupancy, "max_leaves": tying.max_leaves },
        "prov_cap": MAX_PROV_UTTS,
        "estep_chunk": one.estep_chunk,
        "units": units.len(),
        "bw_ll": bw_ll,
        "per_utts": per_n,
        "mono_per": mono_rep.per,
        "mono_errors": mono_rep.total_errors,
        "tri_per": tri_per,
        "tri_errors": tri_err,
        "decode": "inventory-constrained free loop (mono inventory + utterance units), centers scored",
        "peak_rss_mb": peak_rss_mb(),
    });
    std::fs::write(
        out_dir.join("run.json"),
        serde_json::to_string_pretty(&run).unwrap(),
    )
    .expect("run.json");
    println!(
        "train_tri1h DONE {} peak_rss_mb={}",
        out_dir.display(),
        peak_rss_mb()
    );
}
