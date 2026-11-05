//! Phase-test scoring for enrollment models (bead `hearsay-four-pts--phase-test-scoring`).
//!
//! Follow-up to the enrollment-adaptation bead
//! (`hearsay-four-jem--owner-enrollment-adaptation`), whose gate requires
//! PHASE-TEST reported once. READING (resolved, see
//! `docs/results/PHASE-TEST-SCORING.md`): D1 PHASE-TEST partition — the
//! three SCOTUS cases under `data/corpus/court/scotus` — per the jem close
//! reason ("Phase-test deferred to hearsay-four-pts--phase-test-scoring
//! (no word timestamps in partition)"). The owner 5th-take is A1's gate
//! instrument (record via `scripts/record_phase_test.sh`, bead
//! `hearsay-four-b7p--dtw-acoustic-gate`), unrecorded; it is not jem's
//! PHASE-TEST and nothing here records audio.
//!
//! Pipeline: rebuild the jem base + MAP-adapted pair EXACTLY (same TRAIN
//! subset, `TrainConfig`, owner enroll/DEV split, tau grid; tau + stage
//! frozen on owner DEV), then score both models ONCE on three
//! deterministic phase-test segments: the first HEAD_WINDOW_SEC seconds
//! of each case audio plus the duration-proportional head slice of its
//! transcript words (approximation level B — absolute PER is inflated by
//! unknown slice-boundary error; paired base-vs-adapted diffs share
//! identical references, so the comparison is unbiased by the slicing).
//! OOV slice words are skipped (counted), never guessed. No tuning on
//! phase-test: scoring calls `phone_error_rate` only.
//!
//! Framing: SCOTUS speakers are off-target for owner adaptation, so
//! adapted ≈ base is expected and collapse would be the failure mode.
//! The test asserts structural scoring properties only — it makes NO
//! win claim on phase-test (the win gate lives on owner DEV, mirrored
//! here to prove the scored pair is the jem-verdict pair).
//!
//! DATA RULE: transcripts come from the lane checkout (committed text);
//! `audio.mp3` files are gitignored and read read-only from the main
//! workspace (`HEARSAY_MAIN_ROOT`, default: this checkout).
//! Nothing outside the lane is written. Consumed groups must equal the
//! partition's group set and no consumed path may carry a sealed-case fragment.
//!
//! Reproduce (release; ignored by default — heavy real-data gate):
//! `cargo test --release -p hmm --test phase_test_scoring_tests -- --nocapture --test-threads=1 --include-ignored`
//! with the `better-voice` sibling checkout and this repo's gitignored audio
//! present (or `HEARSAY_MAIN_ROOT` / `BETTER_VOICE_ROOT` set).

use corpus::audio::{decode_file, to_mono};
use fse::{extract_features, window_audio};
use hmm::adapt::{AdaptFact, AdaptOverlay, collect_fact, select_stage};
use hmm::{HmmSet, TrainConfig, UttData, paired_ci};
use lexicon::ingest_sample;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Phase-test audio head window (seconds). Bounded for runtime: free
/// phone-loop Viterbi costs ~quadratic in total states per frame, and a
/// full hour-long argument is far outside the scoring budget. The
/// transcript slice is the same fraction of the case duration, so the
/// window fully determines the segment (no tunable alignment).
const HEAD_WINDOW_SEC: u64 = 60;

/// Path fragments that must never appear in a consumed phase-test path
/// (sealed cases; mirrors the `r3_si_base_tests` guard style — bare ids,
/// never partition paths, so the D1 seal scan stays green).
const FORBIDDEN_PATH_FRAGMENTS: &[&str] = &["lockbox", "106844", "106874"];

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

/// Main workspace: read-only source of gitignored audio. Defaults to this
/// checkout; lanes without project audio override with `HEARSAY_MAIN_ROOT`.
fn main_root() -> PathBuf {
    std::env::var("HEARSAY_MAIN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| lane_root())
}

fn basename_lower(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_lowercase()
}

// Data-loading helpers below mirror `owner_enroll_gate_tests.rs` (jem)
// verbatim: the scored pair must be built by the identical procedure.

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

fn features_for_pcm(pcm: &[i16], sample_rate: u32) -> Result<Vec<Vec<f64>>, &'static str> {
    if pcm.is_empty() {
        return Err("no_frames");
    }
    let windows = window_audio(pcm, sample_rate);
    if windows.is_empty() {
        return Err("no_frames");
    }
    let frames = extract_features(&windows, sample_rate).map_err(|_| "no_frames")?;
    if frames.is_empty() || frames[0].len() != fse::FEATURE_DIM {
        return Err("no_frames");
    }
    Ok(frames.iter().map(|f| f.to_vec()).collect())
}

/// Path-based features for NON-owner audio (SCOTUS eval segments). Owner
/// audio never enters here (RS-1 §5: it loads via `load_session` + PCM).
fn features_for(path: &Path) -> Result<Vec<Vec<f64>>, &'static str> {
    let pcm = decode_file(path).map_err(|_| "decode")?;
    let mono = to_mono(&pcm);
    features_for_pcm(&mono, pcm.sample_rate)
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

/// Owner clips via the RS-1 §5 canonical loader; canonical audio resolves
/// under the main workspace (read-only).
fn load_owner_clips(
    lex: &HashMap<String, Vec<Vec<String>>>,
) -> (Vec<OwnerUtt>, BTreeMap<&'static str, usize>) {
    // RS-1 §5: owner audio ONLY through the canonical loader (never raw).
    let canon = main_root().join("sessions/canon/run1");
    assert!(
        canon.is_dir(),
        "main workspace canonical run1 missing: {} (run canon-session on main first)",
        canon.display()
    );
    let session = corpus::canonical::load_session(&canon)
        .unwrap_or_else(|e| panic!("load canonical run1 {}: {e}", canon.display()));
    let mut out = Vec::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    for take in &session.takes {
        let Some(phones) = phones_for(lex, &take.prompt) else {
            bump(&mut skipped, "lexicon_oov");
            continue;
        };
        let frames = match features_for_pcm(&take.pcm, corpus::audio::TARGET_SAMPLE_RATE) {
            Ok(f) => f,
            Err(reason) => {
                bump(&mut skipped, reason);
                continue;
            }
        };
        out.push(OwnerUtt {
            id: format!("{}.wav", take.id),
            text: take.prompt.clone(),
            phones,
            frames,
        });
    }
    (out, skipped)
}

/// One deterministic phase-test segment: HEAD_WINDOW_SEC audio head +
/// duration-proportional transcript head slice (level B).
struct PhaseSeg {
    group: String,
    audio_path: String,
    ref_phones: Vec<String>,
    frames: Vec<Vec<f64>>,
    slice_words: usize,
    oov_skipped: usize,
    total_words: usize,
    duration_sec: f64,
    sample_rate: u32,
}

/// Deterministic phase-test segments (approximation level B): for each
/// `transcript.json` listed in the D1 PHASE-TEST partition (sorted by
/// path), the first HEAD_WINDOW_SEC seconds of the case audio plus the
/// duration-proportional head slice of the transcript words. OOV words
/// are skipped (counted), never guessed. Returns the segments and the
/// consumed group set for the partition guard.
fn load_phase_test_segments(
    lex: &HashMap<String, Vec<Vec<String>>>,
) -> (Vec<PhaseSeg>, BTreeSet<String>) {
    let root = lane_root();
    let main = main_root();
    let text = std::fs::read_to_string(root.join("data/partitions/phase-test.json"))
        .expect("read data/partitions/phase-test.json");
    let part: serde_json::Value = serde_json::from_str(&text).expect("parse phase-test.json");
    let entries = part["entries"].as_array().expect("phase-test entries");
    let partition_groups: BTreeSet<String> = entries
        .iter()
        .map(|e| e["group"].as_str().expect("group").to_string())
        .collect();
    let mut transcript_paths: Vec<(&str, &str)> = entries
        .iter()
        .filter(|e| {
            e["root"].as_str() == Some("repo")
                && e["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("transcript.json"))
        })
        .map(|e| {
            (
                e["path"].as_str().expect("path"),
                e["group"].as_str().expect("group"),
            )
        })
        .collect();
    transcript_paths.sort();
    assert_eq!(
        transcript_paths.len(),
        3,
        "D1 PHASE-TEST must list exactly 3 SCOTUS transcripts"
    );
    let mut segs = Vec::new();
    let mut groups = BTreeSet::new();
    for (rel, group) in transcript_paths {
        groups.insert(group.to_string());
        let tpath = root.join(rel);
        assert!(
            tpath.is_file(),
            "committed transcript missing: {}",
            tpath.display()
        );
        let ttext = std::fs::read_to_string(&tpath).expect("read transcript");
        let tj: serde_json::Value = serde_json::from_str(&ttext).expect("parse transcript");
        let transcript = corpus::courtlistener::reference_transcript_text(&tj)
            .expect("reference transcript string");
        let duration = tj["duration"].as_f64().expect("duration number");
        assert!(
            duration > HEAD_WINDOW_SEC as f64,
            "{group}: duration {duration} must exceed head window {HEAD_WINDOW_SEC}s"
        );
        let words: Vec<&str> = transcript.split_whitespace().collect();
        let n_slice = ((words.len() as f64) * (HEAD_WINDOW_SEC as f64) / duration).floor() as usize;
        assert!(
            n_slice >= 50,
            "{group}: proportional slice too small ({n_slice} words)"
        );
        let case_dir = Path::new(rel).parent().expect("case dir");
        let apath = main.join(case_dir).join("audio.mp3");
        assert!(
            apath.is_file(),
            "phase-test audio missing (read-only, main workspace): {}",
            apath.display()
        );
        let pcm = decode_file(&apath).unwrap_or_else(|e| panic!("decode {}: {e}", apath.display()));
        let mono = to_mono(&pcm);
        let head_samples = (HEAD_WINDOW_SEC as usize) * (pcm.sample_rate as usize);
        assert!(
            mono.len() >= head_samples,
            "{group}: decoded {} samples, need {head_samples} for {HEAD_WINDOW_SEC}s head",
            mono.len()
        );
        let windows = window_audio(&mono[..head_samples], pcm.sample_rate);
        assert!(!windows.is_empty(), "{group}: head window yields no frames");
        let feats = extract_features(&windows, pcm.sample_rate).expect("phase-test features");
        assert!(
            !feats.is_empty() && feats[0].len() == fse::FEATURE_DIM,
            "{group}: bad feature dim"
        );
        let frames: Vec<Vec<f64>> = feats.iter().map(|f| f.to_vec()).collect();
        let mut ref_phones = Vec::new();
        let mut oov = 0usize;
        for w in &words[..n_slice] {
            let toks = norm_words(w);
            let pron = toks
                .first()
                .and_then(|tok| lex.get(tok))
                .and_then(|v| v.first());
            match pron {
                Some(p) => ref_phones.extend(p.clone()),
                None => oov += 1,
            }
        }
        let n_phones = ref_phones.len();
        assert!(
            n_phones >= 100,
            "{group}: reference too small ({n_phones} phones after {oov} OOV skips)"
        );
        segs.push(PhaseSeg {
            group: group.to_string(),
            audio_path: apath.display().to_string(),
            ref_phones,
            frames,
            slice_words: n_slice,
            oov_skipped: oov,
            total_words: words.len(),
            duration_sec: duration,
            sample_rate: pcm.sample_rate,
        });
    }
    assert_eq!(
        groups, partition_groups,
        "consumed groups must equal the partition group set"
    );
    (segs, groups)
}

#[test]
#[ignore] // heavy real-data gate: release run, see header reproduce line
fn phase_test_base_vs_adapted_report() {
    let root = lane_root();
    let bv = better_voice_root();
    assert!(
        bv.join("data").is_dir(),
        "better-voice root missing: {}",
        bv.display()
    );
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

    let transcripts = load_transcripts(&bv);
    let lex = load_lexicon(&bv);
    eprintln!(
        "transcript rows: {}; lexicon words: {}",
        transcripts.len(),
        lex.len()
    );

    // Base training subset — IDENTICAL to the jem gate (same datasets,
    // same caps, same machinery): the scored pair must be the jem pair.
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
        estep_chunk: 2000,
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

    // Owner enrollment / owner-DEV split — IDENTICAL to the jem gate
    // (stratified by transcript; enroll and owner-DEV disjoint).
    let (owner, oskipped) = load_owner_clips(&lex);
    eprintln!("owner clips loaded={} skipped={oskipped:?}", owner.len());
    let mut egroups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, u) in owner.iter().enumerate() {
        egroups.entry(u.text.as_str()).or_default().push(i);
    }
    let mut enroll_idx: Vec<usize> = Vec::new();
    let mut dev_idx: Vec<usize> = Vec::new();
    for idxs in egroups.values() {
        let mut sorted = idxs.clone();
        sorted.sort_by(|&a, &b| owner[a].id.cmp(&owner[b].id));
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

    // Tau grid + stage selection on owner DEV ONLY — IDENTICAL to jem.
    // Nothing below this point tunes on phase-test.
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

    // THE JEM VERDICT, mirrored: the scored pair must be the jem pair.
    // (The win gate lives on owner DEV; phase-test below only reports.)
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

    let mut overlay = AdaptOverlay::new(base.clone(), d.best_tau, d.stage).unwrap();
    for f in &facts {
        overlay.assert_fact(f.clone()).unwrap();
    }
    let adapted = overlay.adapted_model();
    adapted.validate_provenance().unwrap();
    eprintln!(
        "adapted model: tau={} stage={:?} (frozen on owner DEV)",
        d.best_tau, d.stage
    );

    // ---- PHASE-TEST: score the frozen pair exactly once. No tuning
    // below this point — `phone_error_rate` is pure scoring, and tau /
    // stage / keep were all decided on owner DEV above.
    let (segs, groups) = load_phase_test_segments(&lex);
    for s in &segs {
        for frag in FORBIDDEN_PATH_FRAGMENTS {
            assert!(
                !s.audio_path.contains(*frag),
                "consumed sealed path: {}",
                s.audio_path
            );
        }
    }
    eprintln!("phase-test groups: {groups:?}");
    for s in &segs {
        eprintln!(
            "phase-test seg {}: slice {}/{} words oov_skipped={} ref_phones={} frames={} rate={}Hz dur={:.1}s",
            s.group,
            s.slice_words,
            s.total_words,
            s.oov_skipped,
            s.ref_phones.len(),
            s.frames.len(),
            s.sample_rate,
            s.duration_sec
        );
    }
    let scoring: Vec<(Vec<String>, Vec<Vec<f64>>)> = segs
        .iter()
        .map(|s| (s.ref_phones.clone(), s.frames.clone()))
        .collect();
    let base_rep = base
        .phone_error_rate(&scoring)
        .expect("base phase-test PER");
    let adapt_rep = adapted
        .phone_error_rate(&scoring)
        .expect("adapted phase-test PER");
    for (i, s) in segs.iter().enumerate() {
        let b = base_rep.per_utterance[i];
        let a = adapt_rep.per_utterance[i];
        eprintln!(
            "phase-test {}: base PER={b:.4} adapted PER={a:.4} ({} ref phones, {} frames)",
            s.group,
            s.ref_phones.len(),
            s.frames.len()
        );
    }
    let diffs: Vec<f64> = adapt_rep
        .per_utterance
        .iter()
        .zip(&base_rep.per_utterance)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, lo, hi) = paired_ci(&diffs);
    eprintln!(
        "PHASE-TEST base PER={:.4} adapted PER={:.4} (n={} segs, {} ref phones) diff mean={:.4} CI=[{:.4},{:.4}]",
        base_rep.per,
        adapt_rep.per,
        diffs.len(),
        base_rep.total_ref_phones,
        mean,
        lo,
        hi
    );
    eprintln!(
        "PHASE-TEST errors base={} adapted={} (descriptive only; n=3, no inferential claim)",
        base_rep.total_errors, adapt_rep.total_errors
    );

    // Structural asserts only — NO win claim on phase-test (off-target
    // condition for owner adaptation; adapted ≈ base is expected).
    assert!(
        base_rep.per.is_finite() && adapt_rep.per.is_finite(),
        "phase-test PERs must be finite"
    );
    assert!(
        base_rep.total_ref_phones >= 300,
        "phase-test reference too small: {}",
        base_rep.total_ref_phones
    );
    assert_eq!(
        adapt_rep.total_ref_phones, base_rep.total_ref_phones,
        "both models must score identical references"
    );
    assert_eq!(diffs.len(), 3, "one paired diff per phase-test case");
}
