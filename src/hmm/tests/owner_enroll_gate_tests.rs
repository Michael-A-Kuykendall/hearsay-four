//! Owner enrollment + MAP adaptation real-data gate (bead
//! hearsay-four-jem--owner-enrollment-adaptation; SI revalidation bead
//! hearsay-four-erv--enroll-si-revalidation).
//!
//! Base: the committed speaker-independent model
//! (`models/si_base/model.json`, bead
//! hearsay-four-30y--speaker-independent-model). Enrollment + owner DEV:
//! owner clips from `sessions/canon/run1` via the RS-1 §5 canonical loader
//! (read-only from the main workspace — the
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
use corpus::audio::TARGET_SAMPLE_RATE;
use fse::{extract_features, window_audio};
use hmm::HmmSet;
use hmm::adapt::{AdaptFact, AdaptOverlay, collect_fact, select_stage};
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

fn bump(skipped: &mut BTreeMap<&'static str, usize>, reason: &'static str) {
    *skipped.entry(reason).or_insert(0) += 1;
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
        let frames = match features_for_pcm(&take.pcm, TARGET_SAMPLE_RATE) {
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

#[test]
fn owner_enrollment_map_gate() {
    let root = lane_root();
    let bv = better_voice_root();
    assert!(bv.is_dir(), "better-voice root missing: {}", bv.display());
    // NOTE: phase-test.json and lockbox.json are deliberately never loaded
    // here (lane data rule); the gate decides on owner DEV only.

    // SI base (bead hearsay-four-30y--speaker-independent-model): the
    // committed speaker-independent artifact — not an improvised in-lane
    // base. SI revalidation bead hearsay-four-erv--enroll-si-revalidation.
    let si_path = root.join("models/si_base/model.json");
    let base = HmmSet::load_text(&si_path)
        .unwrap_or_else(|e| panic!("load SI base {}: {e}", si_path.display()));
    base.validate_provenance().unwrap();
    eprintln!(
        "base: SI {} ({} phones)",
        si_path.display(),
        base.phones.len()
    );

    let lex = load_lexicon(&bv);
    eprintln!("lexicon words: {}", lex.len());

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
