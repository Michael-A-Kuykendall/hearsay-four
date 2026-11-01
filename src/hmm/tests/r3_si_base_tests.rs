//! R3 speaker-independent base model gate (bead `hearsay-four-30y--speaker-independent-model`).
//!
//! Trains an SI base model with the merged R2 machinery (`flat_start` +
//! `train_checkpoints`: 3 log-domain Baum-Welch iterations, checkpoint
//! selection on DEV PER, mixture splitting 1->2->4->8, per-parameter
//! provenance) and reports the first SI DEV WER. TRAIN is budgeted
//! (segments <= 12 s, 350 total round-robin over speakers) because the R2
//! E-step is ~quadratic in utterance length; see `TRAIN_MAX_SEG_SEC`.
//!
//! Data (all read-only; nothing outside the lane is written):
//! - SI TRAIN (human, multi-speaker, D0 VERIFIED, D1 TRAIN partition): Oyez
//!   cases `10-218_2011` (11 speakers) + `70-112_1971` (8 speakers).
//!   `session.tsv` transcripts come from the lane checkout (committed text);
//!   `audio/run1.wav` is gitignored so it is read from the main workspace
//!   (`HEARSAY_MAIN_ROOT`, default: this checkout).
//! - SI DEV (human read speech, unseen speaker, D0 VERIFIED CC BY 4.0,
//!   D1 DEV partition): `libri_mini_spaced` (LibriSpeech speaker 103,
//!   40 utterances) from better-voice.
//! - Secondary held-out likelihood check: Oyez DEV case `24-889_2025`
//!   (forced alignment only, no decoding, no selection).
//!
//! LOCKBOX, PHASE-TEST, and every D1-excluded (NOT-VERIFIED) set are
//! asserted untouched; the test fails if any consumed path belongs to them.
//!
//! Word decoding for WER is a from-scratch word-loop Viterbi beam search
//! over a closed DEV vocabulary (uniform loop, fixed zero insertion
//! penalty, no LM, SIL loop word, deterministic BTreeMap order). R5 (trie
//! decoder with LM inside search) does not exist yet; this is the honest
//! starting-point decoder the Master Plan R3 gate asks for ("SI DEV WER
//! reported (no threshold; it is a starting point)").
//!
//! Reproduce (release; ignored by default, ~9 min):
//! `cargo test --release -p hmm --test r3_si_base_tests -- --nocapture --test-threads=1 --include-ignored`
//! with the `better-voice` sibling checkout and this repo's gitignored audio
//! present (or `HEARSAY_MAIN_ROOT` / `BETTER_VOICE_ROOT` set).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use corpus::audio::{decode_file, to_mono};
use fse::{extract_features, window_audio};
use hmm::{HmmError, HmmSet, TrainConfig, UttData, paired_ci};
use lexicon::ingest_sample;
use output::wer;

/// Oyez TRAIN cases (D1 TRAIN partition, human multi-speaker, VERIFIED).
const TRAIN_CASES: &[&str] = &["10-218_2011", "70-112_1971"];
/// Oyez DEV case (D1 DEV partition): likelihood spot-check only.
const OYEZ_DEV_CASE: &str = "24-889_2025";
/// LibriSpeech DEV set (D1 DEV partition): primary SI eval.
const LIBRI_DEV_DATASET: &str = "libri_mini_spaced";
/// Path fragments that must never be consumed (D1 sealed/excluded).
const FORBIDDEN_PATH_FRAGMENTS: &[&str] = &[
    "lockbox",
    "phase-test",
    "phase_test",
    "106844",
    "106874",
    "106764",
    "106765",
    "106804",
];

fn lane_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn better_voice_root() -> PathBuf {
    lane_root().join("../better-voice")
}

/// Root providing gitignored audio (`sessions/oyez-bulk/*/audio/run1.wav`).
/// Defaults to this checkout; lanes without project audio override with
/// `HEARSAY_MAIN_ROOT`.
fn main_root() -> PathBuf {
    std::env::var("HEARSAY_MAIN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| lane_root())
}

fn load_partition(name: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(lane_root().join(format!("data/partitions/{name}.json")))
        .unwrap_or_else(|e| panic!("read data/partitions/{name}.json: {e}"));
    serde_json::from_str(&text).expect("parse partition json")
}

/// Assert every consumed (dataset, path) is D1-legitimate for its split and
/// that no LOCKBOX / PHASE-TEST / excluded path is touched.
fn assert_partition_guards(
    train_used: &[(String, String)],
    dev_used: &[(String, String)],
    oyez_dev_used: &[(String, String)],
) {
    let train_j = load_partition("train");
    let dev_j = load_partition("dev");
    let excluded_j = load_partition("excluded");
    let excluded_sets: BTreeSet<String> = excluded_j["excluded"]
        .as_array()
        .expect("excluded list")
        .iter()
        .map(|e| e["dataset"].as_str().expect("dataset").to_string())
        .collect();
    // The guard is only meaningful if it sees the known NOT-VERIFIED sets.
    assert!(
        excluded_sets.contains("phrase_training"),
        "excluded.json must list phrase_training"
    );
    let train_paths: BTreeSet<&str> = train_j["entries"]
        .as_array()
        .expect("train entries")
        .iter()
        .map(|e| e["path"].as_str().expect("path"))
        .collect();
    let dev_paths: BTreeSet<&str> = dev_j["entries"]
        .as_array()
        .expect("dev entries")
        .iter()
        .map(|e| e["path"].as_str().expect("path"))
        .collect();
    assert!(
        train_paths.is_disjoint(&dev_paths),
        "TRAIN/DEV partitions must be disjoint (D1)"
    );
    let check_path = |path: &str| {
        let lower = path.to_lowercase();
        for frag in FORBIDDEN_PATH_FRAGMENTS {
            assert!(
                !lower.contains(frag),
                "forbidden partition fragment in consumed path: {path}"
            );
        }
    };
    for (dataset, path) in train_used {
        assert!(
            !excluded_sets.contains(dataset),
            "TRAIN consumes excluded set: {dataset}"
        );
        assert!(
            train_paths.contains(path.as_str()),
            "TRAIN path not in D1 train.json: {path}"
        );
        check_path(path);
    }
    for (dataset, path) in dev_used.iter().chain(oyez_dev_used.iter()) {
        assert!(
            !excluded_sets.contains(dataset),
            "DEV consumes excluded set: {dataset}"
        );
        check_path(path);
    }
    for (dataset, path) in dev_used.iter().chain(oyez_dev_used.iter()) {
        assert!(
            dev_paths.contains(path.as_str()),
            "DEV path not in D1 dev.json: {dataset} {path}"
        );
    }
    let train_ids: BTreeSet<&str> = train_used.iter().map(|(_, p)| p.as_str()).collect();
    for (_, path) in dev_used.iter().chain(oyez_dev_used.iter()) {
        assert!(
            !train_ids.contains(path.as_str()),
            "TRAIN/DEV consumed overlap: {path}"
        );
    }
}

// ---------- Lexicon + normalization ----------

type Lexicon = HashMap<String, Vec<Vec<String>>>;

fn load_lexicon(bv: &Path) -> Lexicon {
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

/// Resolve one surface token to (lexicon surface, first pronunciation).
/// Candidates: as-is (keeps `MR.`, `DON'T`), edge-punctuation stripped,
/// possessive `'S` dropped. Deterministic first-hit order.
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

/// Strict transcript->phones for TRAIN: `None` when any token is OOV.
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

fn basename_lower(p: &str) -> String {
    p.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(p)
        .to_lowercase()
}

fn load_libri_transcripts(bv: &Path) -> HashMap<String, String> {
    let path = bv.join("data/manifests/libri_mini_spaced.tsv");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut map = HashMap::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let base = basename_lower(cols[0]);
        if !base.ends_with(".wav") {
            continue;
        }
        map.insert(base, cols[1].trim().to_string());
    }
    map
}

// ---------- Audio ----------

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

fn features_for_slice(mono: &[i16], sample_rate: u32) -> Result<Vec<Vec<f64>>, &'static str> {
    if mono.is_empty() {
        return Err("no_frames");
    }
    let windows = window_audio(mono, sample_rate);
    if windows.is_empty() {
        return Err("no_frames");
    }
    let frames = extract_features(&windows, sample_rate).map_err(|_| "no_frames")?;
    if frames.is_empty() || frames[0].len() != fse::FEATURE_DIM {
        return Err("no_frames");
    }
    Ok(frames.iter().map(|f| f.to_vec()).collect())
}

struct OyezSeg {
    id: String,
    speaker: String,
    start_sec: f64,
    stop_sec: f64,
    text: String,
}

fn load_oyez_session(path: &Path, case: &str) -> Vec<OyezSeg> {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut segs = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        if i == 0 {
            continue; // header
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 7 {
            continue;
        }
        let (Ok(start), Ok(stop)) = (cols[2].parse::<f64>(), cols[3].parse::<f64>()) else {
            continue;
        };
        if stop <= start {
            continue;
        }
        segs.push(OyezSeg {
            id: format!("{case}/seg{i}"),
            speaker: cols[4].trim().to_string(),
            start_sec: start,
            stop_sec: stop,
            text: cols[6].trim().to_string(),
        });
    }
    segs
}

struct OyezLoad {
    data: Vec<UttData>,
    speakers: BTreeSet<String>,
    skipped: BTreeMap<&'static str, usize>,
    session_paths: Vec<(String, String)>,
}

fn bump(skipped: &mut BTreeMap<&'static str, usize>, reason: &'static str) {
    *skipped.entry(reason).or_insert(0) += 1;
}

/// TRAIN data budget (R3 scope). The R2 E-step visits every (frame, state)
/// pair, so cost is roughly quadratic in utterance length; the uncapped
/// Oyez set (47 s max segment, 6 iters) needs ~80 min in release. The cap
/// keeps most segments at a fraction of the cost and selects round-robin
/// over speakers (sorted) so all 19 speakers and all 39 phones survive
/// (verified in the gate log). Full-data scale-up is follow-up work.
const TRAIN_MAX_SEG_SEC: f64 = 12.0;
const TRAIN_MAX_SEGS: usize = 350;

/// Load Oyez cases as TRAIN-style (phones, frames) pairs. `session.tsv` is
/// read from the lane checkout (committed text); `audio/run1.wav` is
/// gitignored and comes from the main workspace. No synthetic fallback.
/// When `cap` is set, keep at most `cap` segments of at most
/// `TRAIN_MAX_SEG_SEC` seconds, round-robin over speakers (deterministic).
fn load_oyez_cases(cases: &[&str], lex: &Lexicon, cap: Option<usize>) -> OyezLoad {
    let lane = lane_root();
    let main = main_root();
    let mut data = Vec::new();
    let mut speakers = BTreeSet::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut session_paths = Vec::new();
    for case in cases {
        let rel_session = format!("sessions/oyez-bulk/{case}/session.tsv");
        let session_path = if lane.join(&rel_session).is_file() {
            lane.join(&rel_session)
        } else {
            main.join(&rel_session)
        };
        assert!(
            session_path.is_file(),
            "missing oyez session.tsv: {}",
            session_path.display()
        );
        session_paths.push((format!("case:{case}"), rel_session));
        let rel_audio = format!("sessions/oyez-bulk/{case}/audio/run1.wav");
        let audio_path = if main.join(&rel_audio).is_file() {
            main.join(&rel_audio)
        } else {
            lane.join(&rel_audio)
        };
        assert!(
            audio_path.is_file(),
            "missing oyez audio (read-only, main workspace): {}",
            audio_path.display()
        );
        let pcm = decode_file(&audio_path).expect("decode oyez run1.wav");
        let mono = to_mono(&pcm);
        let sr = pcm.sample_rate as f64;
        eprintln!(
            "oyez {case}: {} samples @ {} Hz ({} s)",
            mono.len(),
            pcm.sample_rate,
            mono.len() as f64 / sr
        );
        // Eligibility needs no audio: duration + lexicon coverage.
        let mut eligible: Vec<(OyezSeg, Vec<String>)> = Vec::new();
        for seg in load_oyez_session(&session_path, case) {
            if cap.is_some() && seg.stop_sec - seg.start_sec > TRAIN_MAX_SEG_SEC {
                bump(&mut skipped, "too_long");
                continue;
            }
            let Some(phones) = phones_for_train(lex, &seg.text) else {
                bump(&mut skipped, "lexicon_oov");
                continue;
            };
            eligible.push((seg, phones));
        }
        // Capped: round-robin over speakers (BTreeMap order = sorted =
        // deterministic), budget split evenly across cases (balanced eras).
        let selected: Vec<(OyezSeg, Vec<String>)> = match cap {
            None => eligible,
            Some(n) => {
                let per_case = n.div_ceil(cases.len());
                let mut by_speaker: BTreeMap<String, Vec<(OyezSeg, Vec<String>)>> = BTreeMap::new();
                for (seg, phones) in eligible {
                    by_speaker
                        .entry(seg.speaker.clone())
                        .or_default()
                        .push((seg, phones));
                }
                let mut iters: Vec<std::vec::IntoIter<(OyezSeg, Vec<String>)>> =
                    by_speaker.into_values().map(|v| v.into_iter()).collect();
                let mut out = Vec::new();
                while out.len() < per_case {
                    let mut took = false;
                    for it in iters.iter_mut() {
                        if out.len() >= per_case {
                            break;
                        }
                        if let Some(item) = it.next() {
                            out.push(item);
                            took = true;
                        }
                    }
                    if !took {
                        break;
                    }
                }
                out
            }
        };
        for (seg, phones) in selected {
            let s = (seg.start_sec * sr) as usize;
            let e = (seg.stop_sec * sr).ceil() as usize;
            if s >= e || e > mono.len() {
                bump(&mut skipped, "bad_window");
                continue;
            }
            let frames = match features_for_slice(&mono[s..e], pcm.sample_rate) {
                Ok(f) => f,
                Err(reason) => {
                    bump(&mut skipped, reason);
                    continue;
                }
            };
            speakers.insert(seg.speaker.clone());
            data.push((seg.id, phones, frames));
        }
    }
    OyezLoad {
        data,
        speakers,
        skipped,
        session_paths,
    }
}

struct LibriUtt {
    id: String,
    ref_words: Vec<String>,
    phones: Option<Vec<String>>,
    frames: Vec<Vec<f64>>,
}

struct LibriLoad {
    utts: Vec<LibriUtt>,
    skipped: BTreeMap<&'static str, usize>,
    dev_paths: Vec<(String, String)>,
}

fn load_libri_dev(lex: &Lexicon) -> LibriLoad {
    let bv = better_voice_root();
    assert!(bv.is_dir(), "better-voice root missing: {}", bv.display());
    let dev_j = load_partition("dev");
    let transcripts = load_libri_transcripts(&bv);
    let mut utts = Vec::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut dev_paths = Vec::new();
    let mut entries: Vec<(String, String)> = dev_j["entries"]
        .as_array()
        .expect("dev entries")
        .iter()
        .filter(|e| e["dataset"].as_str() == Some(LIBRI_DEV_DATASET))
        .map(|e| {
            (
                e["path"].as_str().expect("path").to_string(),
                e["root"].as_str().expect("root").to_string(),
            )
        })
        .collect();
    entries.sort();
    for (path, root) in entries {
        assert_eq!(root, "better-voice", "libri root must be better-voice");
        if !path.to_lowercase().ends_with(".wav") {
            bump(&mut skipped, "non_wav");
            continue;
        }
        let disk = bv.join(&path);
        if !disk.is_file() {
            bump(&mut skipped, "missing_file");
            continue;
        }
        let Some(text) = transcripts.get(&basename_lower(&path)) else {
            bump(&mut skipped, "no_transcript");
            continue;
        };
        let frames = match features_for(&disk) {
            Ok(f) => f,
            Err(reason) => {
                bump(&mut skipped, reason);
                continue;
            }
        };
        dev_paths.push((LIBRI_DEV_DATASET.to_string(), path.clone()));
        utts.push(LibriUtt {
            id: path,
            ref_words: text.split_whitespace().map(str::to_string).collect(),
            phones: phones_for_train(lex, text),
            frames,
        });
    }
    LibriLoad {
        utts,
        skipped,
        dev_paths,
    }
}

// ---------- Word-loop decoder (R3 starting point; R5 builds the real one) ----------
//
// Uniform word loop over a closed vocabulary, Viterbi beam search, no LM.
// Within-phone transitions mirror the R2a decode convention (self/next 0.5;
// learned transitions exist in the model but the R2a decode path hardcodes
// 0.5, so this decoder does the same). All iteration is over BTreeMaps in
// index order with strict `>` max, so decoding is fully deterministic.
// Beam width, max-active cap, and insertion penalty are fixed a priori and
// were NOT tuned on DEV.

/// Fixed decode beam (log units, a priori).
const WORD_BEAM: f64 = 200.0;
/// Fixed cap on active states per frame (a priori).
const WORD_MAX_ACTIVE: usize = 3000;
/// Fixed word insertion penalty in log units (a priori: untuned, zero).
const WORD_PENALTY: f64 = 0.0;

#[derive(Clone)]
struct WordPron {
    surface: String,
    phones: Vec<String>,
}

struct WordNet {
    words: Vec<WordPron>,
    st_word: Vec<usize>,
    st_cache: Vec<usize>,
    starts: Vec<usize>,
    exits: Vec<usize>,
    local: Vec<Vec<(usize, f64)>>,
    uniq: Vec<(String, usize)>,
}

fn build_word_net(model: &HmmSet, vocab: &[WordPron]) -> WordNet {
    assert!(!vocab.is_empty(), "decoder vocabulary must be non-empty");
    let half = 0.5f64.ln();
    let mut st_word = Vec::new();
    let mut st_key: Vec<(String, usize)> = Vec::new();
    // (word, phone_pos, hstate) -> state index.
    let mut index: Vec<Vec<Vec<usize>>> = Vec::new();
    for (w, pron) in vocab.iter().enumerate() {
        assert!(!pron.phones.is_empty(), "empty pronunciation");
        let mut w_idx = Vec::new();
        for ph in &pron.phones {
            let h = model
                .phones
                .get(ph)
                .unwrap_or_else(|| panic!("vocab phone missing from model: {ph}"));
            let mut p_idx = Vec::new();
            for s in 0..h.n_states {
                p_idx.push(st_word.len());
                st_word.push(w);
                st_key.push((ph.clone(), s));
            }
            w_idx.push(p_idx);
        }
        index.push(w_idx);
    }
    let mut uniq: Vec<(String, usize)> = {
        let mut set = BTreeSet::new();
        for k in &st_key {
            set.insert(k.clone());
        }
        set.into_iter().collect()
    };
    uniq.sort();
    let cache_of: HashMap<&(String, usize), usize> =
        uniq.iter().enumerate().map(|(i, k)| (k, i)).collect();
    let st_cache: Vec<usize> = st_key.iter().map(|k| cache_of[k]).collect();
    let mut starts = Vec::with_capacity(vocab.len());
    let mut exits = Vec::with_capacity(vocab.len());
    let mut local: Vec<Vec<(usize, f64)>> = vec![Vec::new(); st_word.len()];
    for (w, pron) in vocab.iter().enumerate() {
        starts.push(index[w][0][0]);
        let last_p = pron.phones.len() - 1;
        exits.push(*index[w][last_p].last().expect("phone states"));
        for (p, ph) in pron.phones.iter().enumerate() {
            let n = model.phones[ph].n_states;
            for s in 0..n {
                let me = index[w][p][s];
                let mut succ = vec![(me, half)];
                if s + 1 < n {
                    succ.push((index[w][p][s + 1], half));
                } else if p + 1 < pron.phones.len() {
                    succ.push((index[w][p + 1][0], half));
                }
                local[me] = succ;
            }
        }
    }
    WordNet {
        words: vocab.to_vec(),
        st_word,
        st_cache,
        starts,
        exits,
        local,
        uniq,
    }
}

/// Viterbi lattice: network state -> log score.
type Lattice = BTreeMap<usize, f64>;
/// Backpointers: network state -> (previous state, word completed here).
type Backpointers = BTreeMap<usize, (usize, Option<usize>)>;

fn decode_words(model: &HmmSet, net: &WordNet, obs: &[Vec<f64>]) -> Result<Vec<String>, HmmError> {
    if obs.is_empty() {
        return Err(HmmError::EmptyData);
    }
    let loop_log = -((net.words.len() as f64).ln()) + WORD_PENALTY;
    let mut emit = vec![0.0f64; net.uniq.len()];
    let fill_emit = |frame: &[f64], emit: &mut [f64]| -> Result<(), HmmError> {
        for (i, (ph, st)) in net.uniq.iter().enumerate() {
            emit[i] = model.emission(ph, *st)?.log_prob(frame);
        }
        Ok(())
    };
    // One exact propagation step; returns (next_scores, backpointers).
    let propagate = |active: &Lattice, emit: &[f64]| -> (Lattice, Backpointers) {
        let mut next: Lattice = BTreeMap::new();
        let mut bp: Backpointers = BTreeMap::new();
        let mut relax = |to: usize, score: f64, from: usize, w: Option<usize>| {
            if score > *next.get(&to).unwrap_or(&f64::NEG_INFINITY) {
                next.insert(to, score);
                bp.insert(to, (from, w));
            }
        };
        for (&s, &sc) in active.iter() {
            for &(to, ltr) in &net.local[s] {
                relax(to, sc + ltr + emit[net.st_cache[to]], s, None);
            }
        }
        // Word exits in bulk: the loop cost is uniform, so one argmax exit
        // is exactly optimal for every word start.
        let mut best_exit = (f64::NEG_INFINITY, usize::MAX);
        for &e in &net.exits {
            if let Some(&sc) = active.get(&e)
                && sc > best_exit.0
            {
                best_exit = (sc, e);
            }
        }
        if best_exit.1 != usize::MAX {
            let ew = net.st_word[best_exit.1];
            for &s in &net.starts {
                relax(
                    s,
                    best_exit.0 + loop_log + emit[net.st_cache[s]],
                    best_exit.1,
                    Some(ew),
                );
            }
        }
        (next, bp)
    };
    fill_emit(&obs[0], &mut emit)?;
    let mut active: BTreeMap<usize, f64> = BTreeMap::new();
    for &s in &net.starts {
        active.insert(s, loop_log + emit[net.st_cache[s]]);
    }
    let mut bps: Vec<BTreeMap<usize, (usize, Option<usize>)>> = vec![BTreeMap::new()];
    for frame in obs.iter().skip(1) {
        fill_emit(frame, &mut emit)?;
        let (full_next, full_bp) = propagate(&active, &emit);
        let best = full_next
            .values()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let mut next = full_next;
        let mut bp = full_bp;
        next.retain(|_, &mut sc| sc >= best - WORD_BEAM);
        if next.len() > WORD_MAX_ACTIVE {
            let mut v: Vec<(usize, f64)> = next.into_iter().collect();
            v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
            v.truncate(WORD_MAX_ACTIVE);
            next = v.into_iter().collect();
        }
        if next.is_empty() {
            // Beam collapse (not observed): fall back to the exact
            // unpruned step rather than failing or guessing.
            let (unpruned_next, unpruned_bp) = propagate(&active, &emit);
            next = unpruned_next;
            bp = unpruned_bp;
        }
        // Drop backpointers for pruned states (traceback only follows live ones).
        bp.retain(|s, _| next.contains_key(s));
        bps.push(bp);
        active = next;
    }
    // Prefer ending on a word boundary; fall back to the global best.
    let mut best = (f64::NEG_INFINITY, usize::MAX);
    for &e in &net.exits {
        if let Some(&sc) = active.get(&e)
            && sc > best.0
        {
            best = (sc, e);
        }
    }
    if best.1 == usize::MAX {
        for (&s, &sc) in active.iter() {
            if sc > best.0 {
                best = (sc, s);
            }
        }
    }
    assert!(best.1 != usize::MAX, "decoder died with empty lattice");
    let mut cur = best.1;
    let mut words_rev = Vec::new();
    for t in (1..obs.len()).rev() {
        let (prev, w) = bps[t].get(&cur).copied().unwrap_or((cur, None));
        if let Some(wi) = w {
            words_rev.push(net.words[wi].surface.clone());
        }
        cur = prev;
    }
    words_rev.reverse();
    words_rev.retain(|w| w != hmm::SIL);
    Ok(words_rev)
}

/// 9-minute release gate: ignored by default so `cargo test --workspace`
/// stays fast (debug would take hours). Run explicitly (see header doc).
#[test]
#[ignore]
fn r3_speaker_independent_base_gate() {
    let bv = better_voice_root();
    assert!(bv.is_dir(), "better-voice root missing: {}", bv.display());
    let lex = load_lexicon(&bv);
    eprintln!("lexicon words: {}", lex.len());

    // SI TRAIN: Oyez human multi-speaker (D1 TRAIN), budgeted (see consts).
    let oyez = load_oyez_cases(TRAIN_CASES, &lex, Some(TRAIN_MAX_SEGS));
    let train = oyez.data;
    let tframes: usize = train.iter().map(|(_, _, o)| o.len()).sum();
    let tphones: usize = train.iter().map(|(_, s, _)| s.len()).sum();
    eprintln!(
        "TRAIN loaded={} phones={} frames={} speakers={} skipped={:?} (budget: {} segs, {} s)",
        train.len(),
        tphones,
        tframes,
        oyez.speakers.len(),
        oyez.skipped,
        TRAIN_MAX_SEGS,
        TRAIN_MAX_SEG_SEC
    );
    eprintln!("TRAIN speakers: {:?}", oyez.speakers);
    assert!(
        train.len() >= 100,
        "TRAIN subset too small to be meaningful: {}",
        train.len()
    );
    assert!(
        oyez.speakers.len() >= 5,
        "SI TRAIN must span speakers, got {:?}",
        oyez.speakers
    );

    // SI DEV: LibriSpeech human read speech, unseen speaker (D1 DEV).
    let libri = load_libri_dev(&lex);
    eprintln!(
        "DEV libri loaded={} skipped={:?}",
        libri.utts.len(),
        libri.skipped
    );
    assert_eq!(
        libri.utts.len(),
        40,
        "libri_mini_spaced must load all 40 D1 DEV utterances"
    );
    let covered: Vec<&LibriUtt> = libri.utts.iter().filter(|u| u.phones.is_some()).collect();
    eprintln!(
        "DEV libri lexicon-covered={} (PER/likelihood subset)",
        covered.len()
    );
    assert!(
        covered.len() >= 10,
        "covered DEV subset too small: {}",
        covered.len()
    );

    // Secondary held-out likelihood check: Oyez DEV case (no decode, no selection).
    let oyez_dev = load_oyez_cases(&[OYEZ_DEV_CASE], &lex, None);
    eprintln!(
        "DEV oyez loaded={} speakers={} skipped={:?}",
        oyez_dev.data.len(),
        oyez_dev.speakers.len(),
        oyez_dev.skipped
    );
    assert!(
        oyez_dev.data.len() >= 50,
        "oyez DEV subset too small: {}",
        oyez_dev.data.len()
    );

    assert_partition_guards(
        &oyez.session_paths,
        &libri.dev_paths,
        &oyez_dev.session_paths,
    );

    // Phone inventory from TRAIN only: no DEV information enters the model.
    let mut inv: BTreeSet<String> = BTreeSet::new();
    for (_, seq, _) in &train {
        inv.extend(seq.iter().cloned());
    }
    let inv_ref: Vec<&str> = inv.iter().map(String::as_str).collect();
    eprintln!("phone inventory (TRAIN): {} {inv_ref:?}", inv.len());
    let unseen: BTreeSet<&str> = covered
        .iter()
        .flat_map(|u| u.phones.as_ref().unwrap().iter().map(String::as_str))
        .filter(|p| !inv.contains(*p))
        .collect();
    eprintln!("DEV phones unseen in TRAIN: {unseen:?}");

    // Train with the merged R2 machinery (same config shape as the R2b gate).
    let dev_per: Vec<(Vec<String>, Vec<Vec<f64>>)> = covered
        .iter()
        .map(|u| (u.phones.clone().unwrap(), u.frames.clone()))
        .collect();
    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&inv_ref, &obs, 0.01).unwrap();
    // 3 BW iterations (R2b used 6): with continuous-speech segments the
    // E-step cost is ~quadratic in utterance length; 3 keeps the gate at
    // ~15 min release while early iterations capture most of the movement.
    let cfg = TrainConfig {
        bw_iterations: 3,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
        estep_chunk: 2000,
    };
    let run = model
        .train_checkpoints(&train, &dev_per, &cfg, &[2, 4, 8])
        .unwrap();
    assert!(
        run.checkpoints.len() > cfg.bw_iterations,
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

    // Gate: all expected (TRAIN) phones represented in the trained model.
    for p in &inv {
        assert!(
            selected.model.phones.contains_key(p),
            "expected phone missing from model: {p}"
        );
    }
    assert!(selected.model.phones.contains_key(hmm::SIL), "SIL missing");
    assert!(selected.model.phones.contains_key(hmm::SP), "SP missing");
    eprintln!(
        "model phones: {} ({} TRAIN + SIL/SP)",
        selected.model.phones.len(),
        inv.len()
    );

    // Gate: model saves and loads (text-readable JSON + lossless binary twin).
    let model_dir = lane_root().join("models/si_base");
    std::fs::create_dir_all(&model_dir).expect("create models/si_base");
    let text_path = model_dir.join("model.json");
    let bin_path = model_dir.join("model.bin");
    selected
        .model
        .save_text(&text_path)
        .expect("save model.json");
    std::fs::write(&bin_path, selected.model.to_binary()).expect("save model.bin");
    let re_text = HmmSet::load_text(&text_path).expect("load model.json");
    let re_bin =
        HmmSet::from_binary(&std::fs::read(&bin_path).expect("read model.bin")).expect("model.bin");
    assert_eq!(re_text, selected.model, "text round-trip must be lossless");
    assert_eq!(re_bin, selected.model, "binary round-trip must be lossless");
    eprintln!(
        "saved {} ({} bytes) + {} ({} bytes)",
        text_path.display(),
        std::fs::metadata(&text_path).unwrap().len(),
        bin_path.display(),
        std::fs::metadata(&bin_path).unwrap().len()
    );

    // Gate: held-out utterances produce finite likelihoods (both DEV sets).
    let mut ll_min = f64::INFINITY;
    for u in &covered {
        let (_, ll) = selected
            .model
            .forced_align(u.phones.as_ref().unwrap(), &u.frames)
            .unwrap();
        assert!(ll.is_finite(), "non-finite libri DEV likelihood: {}", u.id);
        ll_min = ll_min.min(ll);
    }
    let mut oyez_ll = 0usize;
    for (id, seq, frames) in &oyez_dev.data {
        let (_, ll) = selected.model.forced_align(seq, frames).unwrap();
        assert!(ll.is_finite(), "non-finite oyez DEV likelihood: {id}");
        ll_min = ll_min.min(ll);
        oyez_ll += 1;
    }
    eprintln!(
        "finite DEV likelihoods: {} libri + {} oyez (min ll={:.2})",
        covered.len(),
        oyez_ll,
        ll_min
    );

    // Gate: decoder smoke (free phone decode runs and emits phones).
    for u in covered.iter().take(3) {
        let hyp = selected.model.decode_phones(&u.frames).unwrap();
        assert!(!hyp.is_empty(), "empty phone decode: {}", u.id);
        eprintln!("phone-smoke {}: {} phones", u.id, hyp.len());
    }

    // Gate: SI DEV WER (closed DEV vocab, uniform loop, no LM).
    let mut vocab_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut oov_tokens = 0usize;
    let mut total_tokens = 0usize;
    for u in &libri.utts {
        for t in &u.ref_words {
            total_tokens += 1;
            match lex_lookup(&lex, t) {
                Some((surface, pron))
                    if pron.iter().all(|p| selected.model.phones.contains_key(p)) =>
                {
                    vocab_map.entry(surface).or_insert(pron);
                }
                _ => oov_tokens += 1,
            }
        }
    }
    vocab_map.insert(hmm::SIL.to_string(), vec![hmm::SIL.to_string()]);
    let vocab: Vec<WordPron> = vocab_map
        .into_iter()
        .map(|(surface, phones)| WordPron { surface, phones })
        .collect();
    eprintln!(
        "decoder vocab={} (incl SIL) oov_tokens={}/{} ({:.1}%)",
        vocab.len(),
        oov_tokens,
        total_tokens,
        100.0 * oov_tokens as f64 / total_tokens as f64
    );
    let net = build_word_net(&selected.model, &vocab);
    eprintln!("decoder states={}", net.st_word.len());
    let (mut sub, mut del, mut ins, mut refn) = (0usize, 0usize, 0usize, 0usize);
    let mut hyp_words_total = 0usize;
    for (i, u) in libri.utts.iter().enumerate() {
        let hyp = decode_words(&selected.model, &net, &u.frames).unwrap();
        hyp_words_total += hyp.len();
        let score = wer(&u.ref_words, &hyp);
        sub += score.substitutions;
        del += score.deletions;
        ins += score.insertions;
        refn += score.reference_len;
        if i < 5 {
            eprintln!(
                "dev[{i}] ref={} hyp={}",
                u.ref_words.join(" "),
                hyp.join(" ")
            );
        }
    }
    assert!(
        hyp_words_total > 0,
        "word decoder produced no words on 40 DEV utterances"
    );
    let corpus_wer = (sub + del + ins) as f64 / refn as f64;
    assert!(corpus_wer.is_finite(), "WER must be finite");
    eprintln!("SI DEV WER={corpus_wer:.4} (S={sub} D={del} I={ins} N={refn})");

    // PER + paired CI vs flat start (reported; R3 has no improvement threshold).
    let flat_rep = run.checkpoints[0].model.phone_error_rate(&dev_per).unwrap();
    let sel_rep = selected.model.phone_error_rate(&dev_per).unwrap();
    let diffs: Vec<f64> = sel_rep
        .per_utterance
        .iter()
        .zip(&flat_rep.per_utterance)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, _lo, hi) = paired_ci(&diffs);
    eprintln!(
        "SI DEV PER flat={:.4} selected={:.4} paired diff mean={mean:.4} hi={hi:.4} n={}",
        flat_rep.per,
        sel_rep.per,
        diffs.len()
    );

    // Provenance: every parameter attributed; real TRAIN utterances present.
    selected.model.validate_provenance().unwrap();
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
    assert_eq!(
        run.split_decisions.len(),
        3,
        "splitting decision recorded for 2, 4, 8"
    );
}
