//! Owner enrollment + MAP adaptation real-data gate (bead
//! hearsay-four-jem--owner-enrollment-adaptation).
//!
//! Base: D1 TRAIN via the merged checkpoint/mixture/provenance machinery
//! (`train_checkpoints`, use-verified sets only). Enrollment + owner DEV:
//! owner clips from `sessions/run1` (read-only from the main workspace — the
//! lane worktree does not carry project audio), split stratified by transcript
//! so enroll and owner-DEV utterances are disjoint.
//!
//! DATA RULE: LOCKBOX and PHASE-TEST partitions are off limits in this lane,
//! so PHASE-TEST is NOT reported here; the gate decides keep/reject on owner
//! DEV only (PHASE-TEST reporting is deferred to a lane cleared for it).
//!
//! Gate: owner DEV PER (adapted) < owner DEV PER (base) with a paired CI
//! excluding 0 (PER is the phone-stage proxy for the bead's WER gate — no
//! word decoder exists yet); retraction on real facts restores the base.
use corpus::audio::{decode_file, to_mono};
use fse::{extract_features, window_audio};
use hmm::adapt::{AdaptFact, AdaptOverlay, collect_fact, select_stage};
use hmm::{HmmSet, TrainConfig, UttData};
use lexicon::ingest_sample;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn lane_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn better_voice_root() -> PathBuf {
    lane_root().join("../better-voice")
}

/// Main workspace: read-only source of owner audio (lane has none).
fn main_root() -> PathBuf {
    lane_root().join("../hearsay-four")
}

fn basename_lower(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_lowercase()
}

fn load_tsv(
    map: &mut HashMap<(String, String), String>,
    dataset: &str,
    path: &Path,
    text_col: usize,
    header: bool,
) {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    for (i, raw) in text.lines().enumerate() {
        if header && i == 0 {
            continue;
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() <= text_col {
            continue;
        }
        let base = basename_lower(cols[0]);
        if !base.ends_with(".wav") || base.ends_with("_phonetic.wav") {
            continue;
        }
        map.insert(
            (dataset.to_string(), base),
            cols[text_col].trim().to_string(),
        );
    }
}

fn load_transcripts(bv: &Path) -> HashMap<(String, String), String> {
    let mut map = HashMap::new();
    let specs: &[(&str, &str, usize, bool)] = &[
        ("bulk_tts_1k", "data/bulk_tts_1k/train_1k.tsv", 1, true),
        ("bulk_tts_1k", "data/bulk_tts_1k/val_1k.tsv", 1, true),
        (
            "bulk_tts_1k_trimmed",
            "data/bulk_tts_1k_trimmed/train_1k_trimmed.tsv",
            2,
            false,
        ),
        (
            "quality_checked_1k",
            "data/quality_checked_1k/train.tsv",
            2,
            false,
        ),
        (
            "quality_checked_1k",
            "data/quality_checked_1k/val.tsv",
            2,
            false,
        ),
        ("quality_test", "data/quality_test/train.tsv", 2, false),
        ("quality_test", "data/quality_test/val.tsv", 2, false),
        (
            "medical_tts",
            "data/medical_tts/training_manifest.tsv",
            1,
            true,
        ),
        (
            "phonetics_audio",
            "data/phonetics_audio/phonetics_training_manifest.tsv",
            1,
            true,
        ),
        (
            "libri_mini_spaced",
            "data/manifests/libri_mini_spaced.tsv",
            1,
            false,
        ),
    ];
    for (dataset, rel, col, header) in specs {
        load_tsv(&mut map, dataset, &bv.join(rel), *col, *header);
    }
    map
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

fn features_for(path: &Path) -> Result<Vec<Vec<f64>>, &'static str> {
    let pcm = decode_file(path).map_err(|_| "decode")?;
    let mono = to_mono(&pcm);
    if mono.is_empty() {
        return Err("no_frames");
    }
    let windows = window_audio(&mono, pcm.sample_rate);
    if windows.is_empty() {
        return Err("no_frames");
    }
    let frames = extract_features(&windows, pcm.sample_rate).map_err(|_| "no_frames")?;
    if frames.is_empty() || frames[0].len() != fse::FEATURE_DIM {
        return Err("no_frames");
    }
    Ok(frames.iter().map(|f| f.to_vec()).collect())
}

fn bump(skipped: &mut BTreeMap<&'static str, usize>, reason: &'static str) {
    *skipped.entry(reason).or_insert(0) += 1;
}

fn load_split(
    entries: &[serde_json::Value],
    caps: &BTreeMap<&str, usize>,
    transcripts: &HashMap<(String, String), String>,
    lex: &HashMap<String, Vec<Vec<String>>>,
    bv: &Path,
) -> (Vec<UttData>, usize, BTreeMap<&'static str, usize>) {
    let mut data = Vec::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut attempted: HashMap<&str, usize> = HashMap::new();
    for e in entries {
        let (Some(path), Some(dataset), Some(root)) = (
            e["path"].as_str(),
            e["dataset"].as_str(),
            e["root"].as_str(),
        ) else {
            bump(&mut skipped, "malformed_entry");
            continue;
        };
        let Some(cap) = caps.get(dataset) else {
            continue;
        };
        let n = attempted.entry(dataset).or_insert(0);
        if *n >= *cap {
            continue;
        }
        *n += 1;
        if root != "better-voice" {
            bump(&mut skipped, "unexpected_root");
            continue;
        }
        if !path.to_lowercase().ends_with(".wav") {
            bump(&mut skipped, "non_wav");
            continue;
        }
        let disk = bv.join(path);
        if !disk.is_file() {
            bump(&mut skipped, "missing_file");
            continue;
        }
        let key = (dataset.to_string(), basename_lower(path));
        let Some(text) = transcripts.get(&key) else {
            bump(&mut skipped, "no_transcript");
            continue;
        };
        let Some(phones) = phones_for(lex, text) else {
            bump(&mut skipped, "lexicon_oov");
            continue;
        };
        let frames = match features_for(&disk) {
            Ok(f) => f,
            Err(reason) => {
                bump(&mut skipped, reason);
                continue;
            }
        };
        data.push((path.to_string(), phones, frames));
    }
    let loaded = data.len();
    (data, loaded, skipped)
}

struct OwnerUtt {
    id: String,
    text: String,
    phones: Vec<String>,
    frames: Vec<Vec<f64>>,
}

/// Owner clips: manifest has no header; columns are file, transcript, ...;
/// audio resolves under the main workspace (read-only).
fn load_owner_clips(
    lex: &HashMap<String, Vec<Vec<String>>>,
) -> (Vec<OwnerUtt>, BTreeMap<&'static str, usize>) {
    let run1 = main_root().join("sessions/run1");
    assert!(
        run1.is_dir(),
        "main workspace run1 missing: {}",
        run1.display()
    );
    let text = std::fs::read_to_string(run1.join("manifest.tsv")).expect("read owner manifest");
    let mut out = Vec::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            bump(&mut skipped, "malformed_row");
            continue;
        }
        let (file, transcript) = (cols[0].trim(), cols[1].trim());
        let disk = run1.join(file);
        if !disk.is_file() {
            bump(&mut skipped, "missing_file");
            continue;
        }
        let Some(phones) = phones_for(lex, transcript) else {
            bump(&mut skipped, "lexicon_oov");
            continue;
        };
        let frames = match features_for(&disk) {
            Ok(f) => f,
            Err(reason) => {
                bump(&mut skipped, reason);
                continue;
            }
        };
        out.push(OwnerUtt {
            id: file.to_string(),
            text: transcript.to_string(),
            phones,
            frames,
        });
    }
    (out, skipped)
}

#[test]
fn owner_enrollment_map_gate() {
    let root = lane_root();
    let bv = better_voice_root();
    assert!(bv.is_dir(), "better-voice root missing: {}", bv.display());
    let load = |name: &str| -> serde_json::Value {
        let text =
            std::fs::read_to_string(root.join(format!("data/partitions/{name}.json"))).unwrap();
        serde_json::from_str(&text).unwrap()
    };
    // NOTE: phase-test.json and lockbox.json are deliberately never loaded
    // here (lane data rule); the gate decides on owner DEV only.
    let train_j = load("train");
    let dev_j = load("dev");
    let excluded_j = load("excluded");
    let excluded_sets: Vec<String> = excluded_j["excluded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["dataset"].as_str().unwrap().to_string())
        .collect();

    let transcripts = load_transcripts(&bv);
    let lex = load_lexicon(&bv);
    eprintln!(
        "transcript rows: {}; lexicon words: {}",
        transcripts.len(),
        lex.len()
    );

    // Base training subset (smaller than the R2b full gate for runtime; same
    // use-verified datasets, same machinery).
    let train_caps: BTreeMap<&str, usize> = BTreeMap::from([
        ("bulk_tts_1k", 150),
        ("bulk_tts_1k_trimmed", 150),
        ("quality_checked_1k", 150),
        ("medical_tts", usize::MAX),
        ("phonetics_audio", usize::MAX),
    ]);
    let dev_caps: BTreeMap<&str, usize> = BTreeMap::from([
        ("quality_test", usize::MAX),
        ("libri_mini_spaced", usize::MAX),
    ]);
    let (train, tloaded, tskipped) = load_split(
        train_j["entries"].as_array().unwrap(),
        &train_caps,
        &transcripts,
        &lex,
        &bv,
    );
    let (dev_full, dloaded, dskipped) = load_split(
        dev_j["entries"].as_array().unwrap(),
        &dev_caps,
        &transcripts,
        &lex,
        &bv,
    );
    eprintln!("TRAIN loaded={tloaded} skipped={tskipped:?}");
    eprintln!("D1-DEV loaded={dloaded} skipped={dskipped:?}");
    assert!(
        train.len() >= 100,
        "TRAIN subset too small: {}",
        train.len()
    );
    assert!(
        dev_full.len() >= 20,
        "D1-DEV subset too small: {}",
        dev_full.len()
    );
    for (id, _, _) in train.iter().chain(dev_full.iter()) {
        let ds = train_j["entries"]
            .as_array()
            .unwrap()
            .iter()
            .chain(dev_j["entries"].as_array().unwrap().iter())
            .find(|e| e["path"].as_str() == Some(id.as_str()))
            .and_then(|e| e["dataset"].as_str())
            .unwrap_or("?");
        assert!(
            !excluded_sets.iter().any(|x| x == ds),
            "consumed use-verified data only, got {ds}"
        );
    }
    let d1_dev: Vec<(Vec<String>, Vec<Vec<f64>>)> = dev_full
        .iter()
        .map(|(_, s, o)| (s.clone(), o.clone()))
        .collect();

    let mut inv: BTreeSet<String> = BTreeSet::new();
    for (_, seq, _) in &train {
        inv.extend(seq.iter().cloned());
    }
    let inv_ref: Vec<&str> = inv.iter().map(|s| s.as_str()).collect();
    eprintln!("phone inventory (TRAIN): {} phones", inv.len());

    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&inv_ref, &obs, 0.01).unwrap();
    let cfg = TrainConfig {
        bw_iterations: 4,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
    };
    let run = model
        .train_checkpoints(&train, &d1_dev, &cfg, &[2])
        .unwrap();
    let selected = &run.checkpoints[run.selected];
    eprintln!(
        "base: flat PER={:.4} selected PER={:.4} (iter {}, {} mixtures)",
        run.flat_dev_per, selected.dev_per, selected.iteration, selected.mixtures
    );
    let base = selected.model.clone();
    base.validate_provenance().unwrap();

    // Owner enrollment / owner-DEV split, stratified by transcript (disjoint
    // utterances; the 1h-recording human gate is on the owner — this gate
    // runs on the recorded run1 clips available).
    let (owner, oskipped) = load_owner_clips(&lex);
    eprintln!("owner clips loaded={} skipped={oskipped:?}", owner.len());
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, u) in owner.iter().enumerate() {
        groups.entry(u.text.as_str()).or_default().push(i);
    }
    let mut enroll_idx: Vec<usize> = Vec::new();
    let mut dev_idx: Vec<usize> = Vec::new();
    for idxs in groups.values() {
        let mut sorted = idxs.clone();
        sorted.sort_by(|&a, &b| owner[a].id.cmp(&owner[b].id));
        let n_enroll = (2 * sorted.len()).div_ceil(3).max(1).min(sorted.len());
        enroll_idx.extend_from_slice(&sorted[..n_enroll]);
        dev_idx.extend_from_slice(&sorted[n_enroll..]);
    }
    // Guarantee a non-trivial owner-DEV even when every transcript group is a
    // singleton: spill the tail of enroll into dev.
    enroll_idx.sort();
    dev_idx.sort();
    while dev_idx.len() < 20 && enroll_idx.len() > 40 {
        dev_idx.push(enroll_idx.pop().unwrap());
    }
    dev_idx.sort();
    eprintln!("owner enroll={} dev={}", enroll_idx.len(), dev_idx.len());
    assert!(enroll_idx.len() >= 40, "owner enroll too small");
    assert!(dev_idx.len() >= 20, "owner DEV too small");
    let enroll_ids: BTreeSet<&str> = enroll_idx.iter().map(|&i| owner[i].id.as_str()).collect();
    let dev_ids: BTreeSet<&str> = dev_idx.iter().map(|&i| owner[i].id.as_str()).collect();
    assert!(enroll_ids.intersection(&dev_ids).next().is_none());

    let mut facts: Vec<AdaptFact> = Vec::new();
    let mut enroll_skipped: BTreeMap<&str, usize> = BTreeMap::new();
    for &i in &enroll_idx {
        let u = &owner[i];
        match collect_fact(&base, &u.id, &format!("ref:{}", u.id), &u.phones, &u.frames) {
            Ok(f) => facts.push(f),
            Err(hmm::HmmError::UnknownPhone(_)) => bump(&mut enroll_skipped, "unseen_phone"),
            Err(_) => bump(&mut enroll_skipped, "align_failed"),
        }
    }
    eprintln!(
        "enrollment facts={} skipped={enroll_skipped:?}",
        facts.len()
    );
    assert!(
        facts.len() >= 30,
        "too few enrollment facts: {}",
        facts.len()
    );

    let owner_dev: Vec<(Vec<String>, Vec<Vec<f64>>)> = dev_idx
        .iter()
        .map(|&i| (owner[i].phones.clone(), owner[i].frames.clone()))
        .collect();

    // Grid extends past 100 because the best tau sat at the grid edge in the
    // first run (a grid-edge win means the grid was misspecified, not that
    // the edge value is optimal).
    let taus = vec![0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0];
    let sel = select_stage(&base, &facts, &owner_dev, &taus).unwrap();
    let d = sel.decision();
    eprintln!(
        "stage1: base PER={:.4} adapted PER={:.4} tau={} diff mean={:.4} CI=[{:.4},{:.4}] keep={}",
        sel.stage1.dev_per_base,
        sel.stage1.dev_per_adapted,
        sel.stage1.best_tau,
        sel.stage1.mean_diff,
        sel.stage1.ci_lo,
        sel.stage1.ci_hi,
        sel.stage1.keep
    );
    eprintln!(
        "stage2: base PER={:.4} adapted PER={:.4} tau={} diff mean={:.4} CI=[{:.4},{:.4}] keep={}",
        sel.stage2.dev_per_base,
        sel.stage2.dev_per_adapted,
        sel.stage2.best_tau,
        sel.stage2.mean_diff,
        sel.stage2.ci_lo,
        sel.stage2.ci_hi,
        sel.stage2.keep
    );
    eprintln!(
        "selected={:?} stage2-vs-stage1 diff mean={:.4} CI=[{:.4},{:.4}]",
        sel.selected, sel.stage2_mean_diff, sel.stage2_ci_lo, sel.stage2_ci_hi
    );

    // Stage rule: variances kept only on a DEV win with CI excluding 0.
    if sel.selected == hmm::adapt::AdaptStage::MeansAndVariances {
        assert!(sel.stage2_mean_diff < 0.0 && sel.stage2_ci_hi < 0.0);
    }

    // THE GATE: adapted owner-DEV PER beats base with CI excluding 0.
    assert!(
        d.dev_per_adapted < d.dev_per_base,
        "adapted owner-DEV PER ({:.4}) must beat base ({:.4})",
        d.dev_per_adapted,
        d.dev_per_base
    );
    assert!(
        d.ci_hi < 0.0,
        "paired CI must exclude 0 (mean {:.4} CI [{:.4},{:.4}])",
        d.mean_diff,
        d.ci_lo,
        d.ci_hi
    );
    assert!(d.keep);

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
    eprintln!("retraction on real facts: partial-match + restore-base OK");
}
