//! N-best competition with multi-support justification sets (d73.4).
//!
//! Confidence math (hearsay-confidence-formulas.md §3): each knowledge
//! source's evidence has a calibration weight
//! `w_i = ln(P(h|e_i) / (1 − P(h|e_i)))`; a candidate's combined log-odds is
//! `L(h) = L0(h) + Σ w_i` over independent supporting evidence (INV
//! `evidence_additive`), and its 0–100 credibility rating is `100·σ(L)`
//! (INV `rating_in_range`).
//!
//! The N-best cap is adaptive per frame (nbest-candidates.md §2): emit
//! candidates within `δ` of the max, hard-capped at `N=5` (INV
//! `n_best_bounded`, `scores_relative`).
//!
//! Losers retain their evidence as justification sets (INV
//! `retraction_preserves_provenance`, survival via independent support per
//! aem.13) — the fabric keeps every justification.

use invariant_ppt::assert_invariant;

use crate::window::sigmoid;
use fabric::{Fabric, FactId};
use std::collections::BTreeMap;

/// Hard cap on emitted candidates per frame (nbest-candidates.md §1).
pub const N_BEST_CAP: usize = 5;
/// Prior log-odds for a candidate with no prior information (uniform).
pub const PRIOR_LOG_ODDS: f64 = 0.0;

/// A candidate in the N-best competition for a position.
#[derive(Debug, Clone)]
pub struct NBestCandidate {
    /// Fabric fact holding this alternative's justification sets.
    pub fact: FactId,
    /// Combined log-odds `L = L0 + Σ w_i`.
    pub log_odds: f64,
    /// Number of supporting evidence channels so far.
    pub evidence_count: usize,
}

impl NBestCandidate {
    /// Credibility rating on the Hearsay-II 0–100 scale.
    pub fn rating(&self) -> f64 {
        rating_of(self.log_odds)
    }
}

/// Convert a calibrated evidence posterior `P(h|e)` to a log-odds weight.
///
/// `w = ln(P / (1 − P))`. Clamps the input to (0,1) so the weight is always
/// finite (INV `logodds_finite`).
pub fn calibration_weight(posterior: f64) -> f64 {
    assert_invariant(
        (0.0..=1.0).contains(&posterior),
        "posterior_in_unit_interval",
        Some("d73.4"),
    );
    let p = posterior.clamp(1e-9, 1.0 - 1e-9);
    (p / (1.0 - p)).ln()
}

fn rating_of(log_odds: f64) -> f64 {
    let r = 100.0 * sigmoid(log_odds);
    assert_invariant(r.is_finite(), "rating_in_range", Some("d73.4"));
    assert_invariant((0.0..=100.0).contains(&r), "rating_in_range", Some("d73.4"));
    r
}

/// One frame of N-best emission: the adaptive-capped candidate set.
#[derive(Debug, Clone, Default)]
pub struct NBestSet {
    /// Candidates this frame (≤ `N_BEST_CAP`), sorted best first.
    pub candidates: Vec<NBestCandidate>,
}

/// Adaptive emission (nbest-candidates.md §2): keep every candidate within
/// `delta` of the best score, capped at 5.
///
/// `delta` is a log-odds gap; candidates within `[max − delta, max]` are
/// emitted.
pub fn emit_nbest(fabric: &mut Fabric, scores: &BTreeMap<String, f64>, delta: f64) -> NBestSet {
    assert_invariant(delta >= 0.0, "delta_nonnegative", Some("d73.4"));
    let mut best: Option<(String, f64)> = None;
    for (name, score) in scores {
        if best.as_ref().is_none_or(|(_, s)| *score > *s) {
            best = Some((name.clone(), *score));
        }
    }
    let Some((best_name, best_score)) = best else {
        return NBestSet::default();
    };

    let mut in_window: Vec<(String, f64)> = scores
        .iter()
        .filter(|(name, score)| name.as_str() != best_name && **score >= best_score - delta)
        .map(|(n, s)| (n.to_owned(), *s))
        .collect();
    in_window.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Best first, capped at N.
    let mut top: Vec<(String, f64)> = Vec::new();
    top.push((best_name.clone(), best_score));
    top.extend(in_window.into_iter().take(N_BEST_CAP.saturating_sub(1)));
    debug_assert!(top.len() <= N_BEST_CAP, "n_best_bounded");

    let candidates: Vec<NBestCandidate> = top
        .into_iter()
        .map(|(_name, score)| {
            let fact = fabric.intern();
            // Calibration-consistent (INV `calibration_consistent`): every raw
            // Stage-1 evidence strength is mapped through its calibration
            // weight before entering the log-odds combination — no raw
            // un-calibrated rating combines (aem.7 §5).
            assert_invariant(
                (0.0..=1.0).contains(&score),
                "calibration_consistent",
                Some("d73.4"),
            );
            let calibrated = calibration_weight(score);
            let log_odds = PRIOR_LOG_ODDS + calibrated;
            NBestCandidate {
                fact,
                log_odds,
                evidence_count: 1,
            }
        })
        .collect();

    assert_invariant(
        candidates.len() <= N_BEST_CAP,
        "n_best_bounded",
        Some("d73.4"),
    );
    NBestSet { candidates }
}

/// Combine an additional evidence channel into a candidate (additive log-odds).
///
/// `w` is a calibration weight (positive = supporting evidence, negative =
/// contradicting evidence). Returns the new rating.
pub fn add_evidence(candidate: &mut NBestCandidate, w: f64) -> f64 {
    assert_invariant(w.is_finite(), "logodds_finite", Some("d73.4"));
    candidate.log_odds += w;
    candidate.evidence_count += 1;
    // INV: combining more *supporting* evidence never decreases a rating
    // (aem.7 §5 `evidence_additive` — monotone in supporting evidence).
    assert_invariant(
        w < 0.0 || candidate.rating() >= rating_of(candidate.log_odds - w),
        "evidence_additive",
        Some("d73.4"),
    );
    candidate.rating()
}

/// The winner of an N-best set: the candidate with the highest rating.
pub fn winner(set: &NBestSet) -> Option<&NBestCandidate> {
    set.candidates.iter().max_by(|a, b| {
        a.log_odds
            .partial_cmp(&b.log_odds)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}
