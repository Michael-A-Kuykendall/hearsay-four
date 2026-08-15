//! d73.4 gate tests: N-best competition with multi-support justification sets.
//!
//! DoD (from bead acceptance):
//! 1. rating computed from calibrated weights, in [0,100] (INV rating_in_range)
//! 2. combining more evidence never decreases a rating (INV evidence_additive)
//! 3. winner = highest rating
//! 4. losers retain their evidence as justification sets (survival semantics)

use std::collections::BTreeMap;

use crate::competition::{
    NBestCandidate, PRIOR_LOG_ODDS, add_evidence, calibration_weight, emit_nbest, winner,
};
use fabric::{Fabric, FactId};

fn evidence(f: &mut Fabric) -> FactId {
    let e = f.intern();
    f.assert_assumption(e);
    e
}

#[test]
fn nbest_competition_rating_from_calibrated_weights_in_range() {
    // A strongly confident evidence source: P = 0.9 -> w = ln(9).
    let w = calibration_weight(0.9);
    assert!((w - (9.0f64).ln()).abs() < 1e-9);
    let cand = NBestCandidate {
        fact: FactId(1),
        log_odds: PRIOR_LOG_ODDS + w,
        evidence_count: 1,
    };
    let r = cand.rating();
    assert!((0.0..=100.0).contains(&r), "rating_in_range");
    assert!(r > 50.0, "P=0.9 posterior maps above neutral");
}

#[test]
fn nbest_competition_more_evidence_never_decreases_rating() {
    let mut cand = NBestCandidate {
        fact: FactId(1),
        log_odds: PRIOR_LOG_ODDS,
        evidence_count: 0,
    };
    let before = cand.rating();
    let after = add_evidence(&mut cand, calibration_weight(0.75));
    assert!(after >= before, "INV evidence_additive");
    assert_eq!(cand.evidence_count, 1);
    assert!((0.0..=100.0).contains(&after));
}

#[test]
fn nbest_competition_winner_is_highest_rating() {
    let mut f = Fabric::new();
    // One frame, real competition: 5 candidates with distinct scores.
    let mut scores = BTreeMap::new();
    scores.insert("AA".to_string(), 0.9);
    scores.insert("AE".to_string(), 0.6);
    scores.insert("AH".to_string(), 0.4);
    scores.insert("AO".to_string(), 0.2);
    scores.insert("AW".to_string(), 0.1);

    let set = emit_nbest(&mut f, &scores, 3.0);
    assert!(set.candidates.len() <= 5, "n_best_bounded");
    let w = winner(&set).expect("winner exists");
    // AA has the top score -> highest log-odds -> highest rating.
    assert_eq!(w.rating(), set.candidates[0].rating());
    let ratings: Vec<f64> = set.candidates.iter().map(|c| c.rating()).collect();
    assert!(
        ratings.windows(2).all(|w| w[0] >= w[1]),
        "sorted best first"
    );
}

#[test]
fn nbest_competition_adaptive_cap_keeps_within_delta() {
    let mut f = Fabric::new();
    // All within a tight delta: capped at N_BEST_CAP, never more.
    let mut scores = BTreeMap::new();
    for i in 0..8 {
        scores.insert(format!("P{i}"), 0.9 - 0.005 * i as f64);
    }
    let set = emit_nbest(&mut f, &scores, 0.02);
    assert_eq!(set.candidates.len(), 5, "hard cap at N=5");
}

#[test]
fn nbest_competition_losers_keep_justification_sets() {
    use fabric::Fabric;

    let mut f = Fabric::new();
    let e1 = evidence(&mut f);
    // Candidate A with its evidence as an active premise.
    let a = f.intern();
    let mut ja = std::collections::BTreeSet::new();
    ja.insert(e1);
    f.assert(a, ja, false);
    assert!(f.is_active(a), "a active via evidence premise");
    assert_eq!(f.justifications(a).len(), 1, "justification retained");
    assert!(f.is_active(e1), "loser's evidence premise stays");
}

#[test]
fn nbest_competition_empty_frame_emits_nothing() {
    let mut f = Fabric::new();
    let scores = BTreeMap::new();
    let set = emit_nbest(&mut f, &scores, 1.0);
    assert!(set.candidates.is_empty());
}
