//! R4-v2 MAP means-only adaptation plumbing on canonical run1 — NOT EVIDENCE.
//!
//! End-to-end pipe check for lane E (bead
//! hearsay-four-9ud8--adaptation-plumbing-runbook): load the R3-v2 CMVN
//! checkpoint, extract per-utterance CMVN features for the canonical run1
//! takes, collect enrollment facts, run means-only MAP (`tune_tau`), and
//! prove retraction restores the base exactly.
//!
//! Frontend recipe (committed API, mirrors the trainer id
//! `hmm-fe2.cmvn-pop-floor1e-6`): `FrontendConfig::default()` (12 MFCC +
//! energy, deltas + delta-deltas, pre-emphasis 0.97, lifter 22) with
//! `cmvn: true` — per-utterance population-std CMVN with a 1e-6 floor on
//! statics, deltas computed from the normalized statics
//! (`fse::frontend::extract_with_config`, trimmed path).
//!
//! Label: every number this test prints is plumbing exhaust, NOT a
//! milestone claim. run1 is the wrong population for R4-v2 (one repurposed
//! session over a 30-word vocabulary, not owner Harvard enrollment), so no
//! verdict here constrains R4-v2. The assertions prove the pipe runs and
//! round-trips, nothing more.
//!
//! Data (read-only, main workspace via `HEARSAY_MAIN_ROOT`, which lanes
//! without project audio must set; defaults to this checkout):
//! `sessions/canon/run1` (canonical loader only, never raw audio) and
//! `data/models/r3v2-full93-cmvn/ckpt-cmvn8-iter1.json` (override with
//! `HEARSAY_ADAPT_CKPT`). Missing data skips, exactly like the owner
//! enrollment gate pattern this mirrors.

use corpus::audio::TARGET_SAMPLE_RATE;
use fse::frontend::{FrontendConfig, extract_with_config};
use hmm::HmmSet;
use hmm::adapt::{AdaptFact, AdaptOverlay, AdaptStage, collect_fact, tune_tau};
use lexicon::ingest_sample;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn lane_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn better_voice_root() -> PathBuf {
    for env in ["BETTER_VOICE_ROOT", "HEARSAY_BETTER_VOICE_ROOT"] {
        if let Ok(v) = std::env::var(env) {
            return PathBuf::from(v);
        }
    }
    lane_root().join("../better-voice")
}

/// Main workspace: read-only source of canonical audio + checkpoints.
/// Defaults to this checkout; lanes without project audio override with
/// `HEARSAY_MAIN_ROOT`.
fn main_root() -> PathBuf {
    std::env::var("HEARSAY_MAIN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| lane_root())
}

fn norm_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|tok| {
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
            if w.is_empty() { None } else { Some(w) }
        })
        .collect()
}

fn load_lexicon(bv: &Path) -> HashMap<String, Vec<Vec<String>>> {
    let text = std::fs::read_to_string(
        bv.join("data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt"),
    )
    .expect("read phonetics table");
    let mut cmudict = String::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(word), Some(phones)) = (cols.next(), cols.next()) else {
            continue;
        };
        let word = word.trim().to_uppercase();
        if word.is_empty() || phones.trim().is_empty() {
            continue;
        }
        cmudict.push_str(&word);
        cmudict.push(' ');
        cmudict.push_str(phones.trim());
        cmudict.push('\n');
    }
    ingest_sample(&cmudict)
}

fn phones_for(lex: &HashMap<String, Vec<Vec<String>>>, text: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let words = norm_words(text);
    if words.is_empty() {
        return None;
    }
    for w in &words {
        out.extend(lex.get(w)?.first()?.clone());
    }
    Some(out)
}

/// CMVN features for one canonical take: trimmed extraction, default
/// frontend + per-utterance CMVN (the checkpoint's training frontend).
fn features_for_pcm_cmvn(pcm: &[i16], sample_rate: u32) -> Result<Vec<Vec<f64>>, &'static str> {
    if pcm.is_empty() {
        return Err("no_frames");
    }
    let cfg = FrontendConfig {
        cmvn: true,
        ..FrontendConfig::default()
    };
    let frames = extract_with_config(pcm, sample_rate, &cfg).map_err(|_| "no_frames")?;
    if frames.is_empty() || frames[0].len() != fse::FEATURE_DIM {
        return Err("no_frames");
    }
    Ok(frames)
}

fn bump(skipped: &mut BTreeMap<&'static str, usize>, reason: &'static str) {
    *skipped.entry(reason).or_insert(0) += 1;
}

struct PlumbUtt {
    id: String,
    text: String,
    phones: Vec<String>,
    frames: Vec<Vec<f64>>,
}

/// Canonical run1 takes via the RS-1 §5 canonical loader (never raw).
fn load_plumbing_clips(
    lex: &HashMap<String, Vec<Vec<String>>>,
) -> (Vec<PlumbUtt>, BTreeMap<&'static str, usize>) {
    let canon = main_root().join("sessions/canon/run1");
    if !canon.is_dir() {
        eprintln!("SKIP: canonical run1 absent at {}", canon.display());
        return (Vec::new(), BTreeMap::new());
    }
    let session = corpus::canonical::load_session(&canon)
        .unwrap_or_else(|e| panic!("load canonical run1 {}: {e}", canon.display()));
    let mut out = Vec::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    for take in &session.takes {
        let Some(phones) = phones_for(lex, &take.prompt) else {
            bump(&mut skipped, "lexicon_oov");
            continue;
        };
        let frames = match features_for_pcm_cmvn(&take.pcm, TARGET_SAMPLE_RATE) {
            Ok(f) => f,
            Err(reason) => {
                bump(&mut skipped, reason);
                continue;
            }
        };
        out.push(PlumbUtt {
            id: format!("{}.wav", take.id),
            text: take.prompt.clone(),
            phones,
            frames,
        });
    }
    (out, skipped)
}

/// The CMVN frontend recipe is fixed: default config + `cmvn` must stay
/// 39-dim, deterministic, and finite on real-valued input. Fast, always
/// runs, no data files.
#[test]
fn adapt_cmvn_frontend_recipe_shape() {
    let cfg = FrontendConfig {
        cmvn: true,
        ..FrontendConfig::default()
    };
    assert_eq!(cfg.dim(), 39);
    assert_eq!(cfg.dim(), fse::FEATURE_DIM);
    // 1 s voiced-ish sweep at 16 kHz: CMVN must stay finite + deterministic.
    let pcm: Vec<i16> = (0..16000)
        .map(|t| {
            let phase = 2.0 * std::f64::consts::PI * 220.0 * f64::from(t) / 16000.0;
            (phase.sin() * 12000.0) as i16
        })
        .collect();
    let a = extract_with_config(&pcm, 16000, &cfg).expect("cmvn extract");
    let b = extract_with_config(&pcm, 16000, &cfg).expect("cmvn extract again");
    assert!(!a.is_empty());
    assert!(a.iter().all(|f| f.len() == 39));
    assert!(a.iter().all(|f| f.iter().all(|x| x.is_finite())));
    assert_eq!(a, b);
}

/// Full MAP means-only pipe on canonical run1 with the R3-v2 CMVN
/// checkpoint. NOT EVIDENCE (wrong population for R4-v2 claims): asserts
/// the pipe runs and round-trips, never that adaptation wins.
#[test]
fn adapt_plumbing_run1_map_means_only_not_evidence() {
    eprintln!("NOT EVIDENCE: adaptation plumbing run (wrong population for R4-v2 claims)");
    let bv = better_voice_root();
    if !bv.join("data").is_dir() {
        eprintln!(
            "SKIP: better-voice checkout absent at {} (set $BETTER_VOICE_ROOT)",
            bv.display()
        );
        return;
    }
    let ckpt = std::env::var("HEARSAY_ADAPT_CKPT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| main_root().join("data/models/r3v2-full93-cmvn/ckpt-cmvn8-iter1.json"));
    if !ckpt.is_file() {
        eprintln!("SKIP: plumbing checkpoint absent at {}", ckpt.display());
        return;
    }
    let base =
        HmmSet::load_text(&ckpt).unwrap_or_else(|e| panic!("load ckpt {}: {e}", ckpt.display()));
    base.validate_provenance().unwrap();
    assert_eq!(base.dim, fse::FEATURE_DIM);
    eprintln!(
        "NOT EVIDENCE base: {} ({} phones, dim {})",
        ckpt.display(),
        base.phones.len(),
        base.dim
    );

    let lex = load_lexicon(&bv);
    let (clips, cskipped) = load_plumbing_clips(&lex);
    eprintln!(
        "NOT EVIDENCE clips loaded={} skipped={cskipped:?}",
        clips.len()
    );
    if clips.is_empty() {
        eprintln!("SKIP: no usable plumbing clips (gitignored run1 audio absent)");
        return;
    }

    // Enroll/DEV split, stratified by transcript (disjoint utterances).
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, u) in clips.iter().enumerate() {
        groups.entry(u.text.as_str()).or_default().push(i);
    }
    let mut enroll_idx: Vec<usize> = Vec::new();
    let mut dev_idx: Vec<usize> = Vec::new();
    for idxs in groups.values() {
        let mut sorted = idxs.clone();
        sorted.sort_by(|&a, &b| clips[a].id.cmp(&clips[b].id));
        let n_enroll = (2 * sorted.len()).div_ceil(3).max(1).min(sorted.len());
        enroll_idx.extend_from_slice(&sorted[..n_enroll]);
        dev_idx.extend_from_slice(&sorted[n_enroll..]);
    }
    enroll_idx.sort();
    dev_idx.sort();
    while dev_idx.len() < 20 && enroll_idx.len() > 40 {
        dev_idx.push(enroll_idx.pop().unwrap());
    }
    dev_idx.sort();
    eprintln!(
        "NOT EVIDENCE enroll={} dev={}",
        enroll_idx.len(),
        dev_idx.len()
    );
    assert!(enroll_idx.len() >= 40, "plumbing enroll too small");
    assert!(dev_idx.len() >= 20, "plumbing DEV too small");
    let enroll_ids: BTreeSet<&str> = enroll_idx.iter().map(|&i| clips[i].id.as_str()).collect();
    let dev_ids: BTreeSet<&str> = dev_idx.iter().map(|&i| clips[i].id.as_str()).collect();
    assert!(enroll_ids.intersection(&dev_ids).next().is_none());

    let mut facts: Vec<AdaptFact> = Vec::new();
    let mut enroll_skipped: BTreeMap<&str, usize> = BTreeMap::new();
    for &i in &enroll_idx {
        let u = &clips[i];
        match collect_fact(&base, &u.id, &format!("ref:{}", u.id), &u.phones, &u.frames) {
            Ok(f) => facts.push(f),
            Err(hmm::HmmError::UnknownPhone(_)) => bump(&mut enroll_skipped, "unseen_phone"),
            Err(_) => bump(&mut enroll_skipped, "align_failed"),
        }
    }
    eprintln!(
        "NOT EVIDENCE enrollment facts={} skipped={enroll_skipped:?}",
        facts.len()
    );
    assert!(facts.len() >= 30, "too few plumbing facts: {}", facts.len());

    let dev: Vec<(Vec<String>, Vec<Vec<f64>>)> = dev_idx
        .iter()
        .map(|&i| (clips[i].phones.clone(), clips[i].frames.clone()))
        .collect();

    // Small tau grid: plumbing proves the pipe, not tau science.
    let taus = vec![1.0, 10.0, 100.0];
    let d = tune_tau(&base, &facts, &dev, &taus, AdaptStage::MeansOnly).unwrap();
    assert_eq!(d.stage, AdaptStage::MeansOnly);
    assert_eq!(d.n_dev, dev.len());
    assert!(d.dev_per_base.is_finite());
    assert!(d.dev_per_adapted.is_finite());
    assert!(taus.contains(&d.best_tau));
    eprintln!(
        "NOT EVIDENCE means-only: base PER={:.4} adapted PER={:.4} tau={} diff mean={:.4} CI=[{:.4},{:.4}] keep={}",
        d.dev_per_base, d.dev_per_adapted, d.best_tau, d.mean_diff, d.ci_lo, d.ci_hi, d.keep
    );

    // Retraction on real facts restores the base exactly.
    let mut overlay = AdaptOverlay::new(base.clone(), d.best_tau, d.stage).unwrap();
    for f in &facts {
        overlay.assert_fact(f.clone()).unwrap();
    }
    let adapted = overlay.adapted_model();
    adapted.validate_provenance().unwrap();
    let ids = overlay.fact_ids();
    let (dropped, kept) = ids.split_at(ids.len() / 2);
    for id in dropped {
        overlay.retract_fact(id).unwrap();
    }
    let mut rebuilt = AdaptOverlay::new(base.clone(), d.best_tau, d.stage).unwrap();
    for f in &facts {
        if kept.contains(&f.id) {
            rebuilt.assert_fact(f.clone()).unwrap();
        }
    }
    assert_eq!(overlay.adapted_model(), rebuilt.adapted_model());
    for id in kept {
        overlay.retract_fact(id).unwrap();
    }
    assert_eq!(overlay.adapted_model(), base);
    eprintln!("NOT EVIDENCE retraction: partial-match + restore-base OK");
}
