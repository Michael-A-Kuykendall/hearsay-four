//! R2b REAL-DATA gate (bead hearsay-four-0bv): checkpoint selection, mixture
//! splitting, and parameter provenance on real D1 audio.
//!
//! Pipeline per utterance (existing paths only, no new extractor or decoder):
//! manifest entry -> corpus::audio::decode_file (RIFF/WAV or MP3 sniff) ->
//! corpus::audio::to_mono -> fse::window_audio -> fse::extract_features
//! (39-dim frames) -> hmm::HmmSet::train_checkpoints.
//! Transcripts come from each dataset's TSV sidecar; pronunciations from the
//! VERIFIED lexicon-text table (better-voice data/phonetics, excluded from D1
//! only as audio payload) via lexicon::ingest_sample.
//!
//! Selection rule (deterministic, manifest order, fixed before first run):
//! TRAIN: first 300 entries each of bulk_tts_1k, bulk_tts_1k_trimmed,
//!   quality_checked_1k; all entries of medical_tts, phonetics_audio.
//! DEV: all entries of quality_test, libri_mini_spaced (no cap).
//! Entries are skipped with counted reasons: non-wav entry, missing file,
//! decode failure, missing transcript row, lexicon OOV, zero frames.
//! Out of scope with reason: tts_audio + data/demo (no transcript source),
//! case:* (session metadata, not utterances), _phonetic spelled-audio clips
//! (letter-name audio whose word phones would be wrong references).
//! Transcript normalization is mechanical only (uppercase, strip CMUdict
//! `(n)` variant markers, strip trailing `'` plural possessive, split
//! multiword transcripts on whitespace); every word must resolve in the
//! lexicon or the utterance is skipped. Phone inventory = TRAIN phones only,
//! so no DEV information enters model construction.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use corpus::audio::{decode_file, to_mono};
use fse::{extract_features, window_audio};
use hmm::{HmmSet, TrainConfig, UttData, paired_ci};
use lexicon::ingest_sample;

fn lane_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn better_voice_root() -> PathBuf {
    lane_root().join("../better-voice")
}

fn basename_lower(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_lowercase()
}

/// Load one transcript TSV into `map` keyed by (dataset, audio basename).
/// `text_col` is the 0-based TSV column holding the transcript; when `header`
/// is set row 0 is skipped. Rows that do not point at a .wav file are
/// ignored, as are `_phonetic.wav` spelled-audio clips.
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

/// Mechanical transcript normalization (no phonetic invention): uppercase,
/// strip CMUdict `(n)` variant markers, strip trailing `'` (plural
/// possessive, pronounced as the bare word), split multiword transcripts.
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

/// First-pronunciation phone sequence for a transcript, or None when any
/// word is out of the lexicon.
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

/// Real 39-dim feature frames: decode -> mono -> window -> fse extract.
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

struct LoadStats {
    loaded: usize,
    phones: usize,
    frames: usize,
    skipped: BTreeMap<&'static str, usize>,
}

fn bump(skipped: &mut BTreeMap<&'static str, usize>, reason: &'static str) {
    *skipped.entry(reason).or_insert(0) += 1;
}

/// Load one D1 split in manifest order. `caps` maps dataset -> max attempted
/// entries (usize::MAX = all); datasets outside `caps` are out of scope.
fn load_split(
    entries: &[serde_json::Value],
    caps: &BTreeMap<&str, usize>,
    transcripts: &HashMap<(String, String), String>,
    lex: &HashMap<String, Vec<Vec<String>>>,
    bv: &Path,
) -> (Vec<UttData>, LoadStats) {
    let mut data = Vec::new();
    let mut stats = LoadStats {
        loaded: 0,
        phones: 0,
        frames: 0,
        skipped: BTreeMap::new(),
    };
    let mut attempted: HashMap<&str, usize> = HashMap::new();
    for e in entries {
        let (Some(path), Some(dataset), Some(root)) = (
            e["path"].as_str(),
            e["dataset"].as_str(),
            e["root"].as_str(),
        ) else {
            bump(&mut stats.skipped, "malformed_entry");
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
            bump(&mut stats.skipped, "unexpected_root");
            continue;
        }
        if !path.to_lowercase().ends_with(".wav") {
            bump(&mut stats.skipped, "non_wav");
            continue;
        }
        let disk = bv.join(path);
        if !disk.is_file() {
            bump(&mut stats.skipped, "missing_file");
            continue;
        }
        let key = (dataset.to_string(), basename_lower(path));
        let Some(text) = transcripts.get(&key) else {
            bump(&mut stats.skipped, "no_transcript");
            continue;
        };
        let Some(phones) = phones_for(lex, text) else {
            bump(&mut stats.skipped, "lexicon_oov");
            continue;
        };
        let frames = match features_for(&disk) {
            Ok(f) => f,
            Err(reason) => {
                bump(&mut stats.skipped, reason);
                continue;
            }
        };
        stats.loaded += 1;
        stats.phones += phones.len();
        stats.frames += frames.len();
        data.push((path.to_string(), phones, frames));
    }
    (data, stats)
}

#[test]
fn real_data_checkpoint_gate() {
    let root = lane_root();
    let bv = better_voice_root();
    assert!(bv.is_dir(), "better-voice root missing: {}", bv.display());
    let load = |name: &str| -> serde_json::Value {
        let text =
            std::fs::read_to_string(root.join(format!("data/partitions/{name}.json"))).unwrap();
        serde_json::from_str(&text).unwrap()
    };
    let train_j = load("train");
    let dev_j = load("dev");
    let excluded_j = load("excluded");
    let excluded_sets: Vec<String> = excluded_j["excluded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["dataset"].as_str().unwrap().to_string())
        .collect();
    let ids = |v: &serde_json::Value| -> Vec<(String, String)> {
        v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["path"].as_str().unwrap().to_string(),
                    e["dataset"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    let train_entries = ids(&train_j);
    let dev_entries = ids(&dev_j);
    let train_paths: BTreeSet<&str> = train_entries.iter().map(|(p, _)| p.as_str()).collect();
    for (p, ds) in &dev_entries {
        assert!(!train_paths.contains(p.as_str()), "TRAIN/DEV disjoint (D1)");
        assert!(
            !excluded_sets.contains(ds),
            "DEV consumes only use-verified data"
        );
    }
    for (_, ds) in &train_entries {
        assert!(
            !excluded_sets.contains(ds),
            "TRAIN consumes only use-verified data"
        );
    }

    let transcripts = load_transcripts(&bv);
    let lex = load_lexicon(&bv);
    eprintln!(
        "transcript rows: {}; lexicon words: {}",
        transcripts.len(),
        lex.len()
    );

    let train_caps: BTreeMap<&str, usize> = BTreeMap::from([
        ("bulk_tts_1k", 300),
        ("bulk_tts_1k_trimmed", 300),
        ("quality_checked_1k", 300),
        ("medical_tts", usize::MAX),
        ("phonetics_audio", usize::MAX),
    ]);
    let dev_caps: BTreeMap<&str, usize> = BTreeMap::from([
        ("quality_test", usize::MAX),
        ("libri_mini_spaced", usize::MAX),
    ]);
    let (train, tstats) = load_split(
        train_j["entries"].as_array().unwrap(),
        &train_caps,
        &transcripts,
        &lex,
        &bv,
    );
    let (dev_full, dstats) = load_split(
        dev_j["entries"].as_array().unwrap(),
        &dev_caps,
        &transcripts,
        &lex,
        &bv,
    );
    eprintln!(
        "TRAIN loaded={} phones={} frames={} skipped={:?}",
        tstats.loaded, tstats.phones, tstats.frames, tstats.skipped
    );
    eprintln!(
        "DEV loaded={} phones={} frames={} skipped={:?}",
        dstats.loaded, dstats.phones, dstats.frames, dstats.skipped
    );
    assert!(
        train.len() >= 100,
        "TRAIN subset too small to be meaningful: {}",
        train.len()
    );
    assert!(
        dev_full.len() >= 20,
        "DEV subset too small to be meaningful: {}",
        dev_full.len()
    );
    let dev: Vec<(Vec<String>, Vec<Vec<f64>>)> = dev_full
        .iter()
        .map(|(_, s, o)| (s.clone(), o.clone()))
        .collect();

    // Inventory from TRAIN phones only: no DEV information enters the model.
    let mut inv: BTreeSet<String> = BTreeSet::new();
    for (_, seq, _) in &train {
        inv.extend(seq.iter().cloned());
    }
    let inv_ref: Vec<&str> = inv.iter().map(|s| s.as_str()).collect();
    eprintln!("phone inventory (TRAIN): {} {inv_ref:?}", inv.len());
    let unseen: BTreeSet<&str> = dev_full
        .iter()
        .flat_map(|(_, s, _)| s.iter().map(|p| p.as_str()))
        .filter(|p| !inv.contains(*p))
        .collect();
    eprintln!("DEV phones unseen in TRAIN: {unseen:?}");
    // Feature sanity (informational): global per-dim mean/var over TRAIN.
    {
        let dim = train[0].2[0].len();
        let mut mean = vec![0.0; dim];
        let mut sq = vec![0.0; dim];
        let mut n = 0f64;
        for (_, _, obs) in &train {
            for f in obs {
                n += 1.0;
                for (d, v) in f.iter().enumerate() {
                    mean[d] += v;
                    sq[d] += v * v;
                }
            }
        }
        let mean: Vec<f64> = mean.iter().map(|m| m / n).collect();
        let var: Vec<f64> = (0..dim).map(|d| sq[d] / n - mean[d].powi(2)).collect();
        eprintln!(
            "TRAIN feature dim={dim} mean[0..5]={:.4?} var[0..5]={:.4?} meanvar={:.4}",
            &mean[..5],
            &var[..5],
            var.iter().sum::<f64>() / dim as f64
        );
    }

    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&inv_ref, &obs, 0.01).unwrap();
    let cfg = TrainConfig {
        bw_iterations: 6,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
    };
    let run = model
        .train_checkpoints(&train, &dev, &cfg, &[2, 4, 8])
        .unwrap();
    assert!(
        run.checkpoints.len() >= 7,
        "one checkpoint per BW iteration plus flat start"
    );
    let selected = &run.checkpoints[run.selected];
    eprintln!(
        "flat PER={:.4} selected PER={:.4} (iter {}, {} mixtures)",
        run.flat_dev_per, selected.dev_per, selected.iteration, selected.mixtures
    );
    for d in &run.split_decisions {
        eprintln!(
            "split {}->{} before={:.4} after={:.4} accepted={}",
            d.from_mixtures, d.to_mixtures, d.dev_per_before, d.dev_per_after, d.accepted
        );
    }
    for (i, c) in run.checkpoints.iter().enumerate() {
        eprintln!(
            "ckpt iter={} mix={} train_ll={:.2} dev_per={:.4}{}",
            c.iteration,
            c.mixtures,
            c.train_ll,
            c.dev_per,
            if i == run.selected { " SELECTED" } else { "" }
        );
    }
    let flat_rep = run.checkpoints[0].model.phone_error_rate(&dev).unwrap();
    let sel_rep = selected.model.phone_error_rate(&dev).unwrap();
    eprintln!(
        "flat errors={}/{} sel errors={}/{}",
        flat_rep.total_errors,
        flat_rep.total_ref_phones,
        sel_rep.total_errors,
        sel_rep.total_ref_phones
    );
    let diffs: Vec<f64> = sel_rep
        .per_utterance
        .iter()
        .zip(&flat_rep.per_utterance)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, _lo, hi) = paired_ci(&diffs);
    eprintln!("paired diff mean={mean:.4} hi={hi:.4} n={}", diffs.len());
    for (i, (utt, (f, s))) in dev_full
        .iter()
        .zip(flat_rep.per_utterance.iter().zip(&sel_rep.per_utterance))
        .enumerate()
    {
        eprintln!("dev[{i}] {} flat={f:.4} sel={s:.4}", utt.0);
    }
    assert!(
        selected.dev_per < run.flat_dev_per,
        "selected checkpoint must beat flat start on real DEV PER"
    );
    assert!(
        mean < 0.0 && hi < 0.0,
        "paired CI must lie below 0 (mean {mean}, hi {hi})"
    );
    assert_eq!(
        run.split_decisions.len(),
        3,
        "splitting decision recorded for 2, 4, 8"
    );
    for d in &run.split_decisions {
        assert!(d.dev_per_before >= 0.0 && d.dev_per_after >= 0.0);
        if d.accepted {
            assert!(d.dev_per_after < d.dev_per_before);
        }
    }
    selected.model.validate_provenance().unwrap();
    model.validate_provenance().unwrap();
    // Every provenance id is a real TRAIN path or flat-start residue ("utt:N"
    // survives only in components BW never occupied); no foreign ids.
    let train_ids: BTreeSet<&str> = train.iter().map(|(id, _, _)| id.as_str()).collect();
    let is_flat = |u: &str| u.starts_with("utt:") && u[4..].chars().all(|c| c.is_ascii_digit());
    let (mut real, mut flat) = (0usize, 0usize);
    for h in selected.model.phones.values() {
        if h.tied_to.is_some() {
            continue;
        }
        for mix in &h.emissions {
            for comp in &mix.components {
                for u in &comp.stats.provenance.utterances {
                    assert!(
                        train_ids.contains(u.as_str()) || is_flat(u),
                        "foreign provenance id: {u}"
                    );
                    if train_ids.contains(u.as_str()) {
                        real += 1;
                    } else {
                        flat += 1;
                    }
                }
            }
        }
    }
    eprintln!("provenance ids: {real} real-train, {flat} flat-residue");
    assert!(
        real > 0,
        "selected model must attribute real TRAIN utterances"
    );
}
