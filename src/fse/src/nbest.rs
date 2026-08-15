//! N-best candidate emission per frame (d5n.2) — never argmax collapse.
//!
//! Per `docs/design/nbest-candidates.md`: emit candidates with
//! `score >= max_score - delta` where `delta` is relative (default 15% of the
//! max score), hard-capped at 5. Candidates are ARPABET phonemes
//! (`docs/design/phoneme-alphabet.md`).
//!
//! Emission invariants (d5n.3 contract fodder):
//! - `n_best_bounded` — never more than 5 candidates/frame
//! - `n_best_not_argmax` — real competition (>= 2 within delta) stays emitted
//! - `scores_relative` — threshold is relative to the max, never absolute
//! - `phoneme_in_alphabet` — every emitted phoneme is in the 39-phone set

use invariant_ppt::assert_invariant;

use crate::ARPABET;

/// Hard cap on emitted candidates per frame (nbest-candidates.md §1).
pub const MAX_N: usize = 5;
/// Default relative score gap: emit candidates within 15% of the max score.
pub const DEFAULT_DELTA: f64 = 0.15;

/// One emitted phoneme candidate for a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// ARPABET phoneme (INV: `phoneme_in_alphabet`).
    pub phoneme: &'static str,
    /// Higher-is-better similarity score in [0, 1].
    pub score: f64,
}

/// Emit the adaptive N-best set from per-phone scores.
///
/// `scores` is a list of `(phoneme, score)` pairs with higher scores better.
/// Semantics: keep every candidate with `score >= max_score - delta * max_score`
/// (relative threshold), sort descending, hard-cap at [`MAX_N`].
///
/// Empty input yields empty output (no panic). Deterministic ordering: score
/// descending, then phoneme ascending.
pub fn emit_nbest(scores: &[(&'static str, f64)]) -> Vec<Candidate> {
    if scores.is_empty() {
        return Vec::new();
    }
    let max_score = scores
        .iter()
        .map(|(_, s)| *s)
        .fold(f64::NEG_INFINITY, f64::max);
    let delta = DEFAULT_DELTA * max_score;
    let threshold = max_score - delta;

    // INV: alphabet_fixed — the alphabet is the fixed 39-phone ARPABET.
    assert_invariant(ARPABET.len() == 39, "alphabet_fixed", Some("stage1 nbest"));

    // INV: scores_relative — the threshold is derived from the observed max,
    // never a hardcoded absolute cut.
    assert_invariant(
        delta >= 0.0 && threshold <= max_score,
        "scores_relative",
        Some("stage1 nbest"),
    );

    // INV: score_in_range — candidate scores ∈ [0, 1].
    assert_invariant(
        scores.iter().all(|(_, s)| (0.0..=1.0).contains(s)),
        "score_in_range",
        Some("stage1 nbest"),
    );

    let mut within: Vec<Candidate> = scores
        .iter()
        .filter(|(_, s)| *s >= threshold)
        .map(|&(phoneme, score)| Candidate { phoneme, score })
        .collect();

    // INV: phoneme_in_alphabet — every emitted phoneme is ARPABET.
    for c in &within {
        assert_invariant(
            ARPABET.contains(&c.phoneme),
            "phoneme_in_alphabet",
            Some(&format!("stage1 nbest: {}", c.phoneme)),
        );
    }

    // Real competition = >= 2 candidates within delta.
    let had_competition = within.len() >= 2;

    within.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.phoneme.cmp(b.phoneme))
    });

    // INV: n_best_bounded — never more than MAX_N per frame.
    within.truncate(MAX_N);
    assert_invariant(
        within.len() <= MAX_N,
        "n_best_bounded",
        Some("stage1 nbest"),
    );

    // INV: n_best_not_argmax — with real competition the top-2 within-delta
    // candidates must both survive (truncation keeps the top-5 by score, so
    // a collapse to 1 here is a real bug).
    assert_invariant(
        !had_competition || within.len() >= 2,
        "n_best_not_argmax",
        Some("stage1 nbest"),
    );

    within
}
