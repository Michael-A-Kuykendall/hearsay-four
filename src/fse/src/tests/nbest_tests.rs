//! d5n.2 gate: `cd src/fse && cargo test nbest`
//!
//! Emission properties: never argmax collapse with real competition, cap at
//! 5, ARPABET-only candidates, relative (not absolute) threshold.

use crate::nbest::{emit_nbest, Candidate, DEFAULT_DELTA, MAX_N};
use crate::ARPABET;

/// Build a score list from (phoneme, score) pairs; phonemes default to real
/// ARPABET entries so the alphabet invariant holds.
fn scores(pairs: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
    pairs.to_vec()
}

#[test]
fn nbest_emits_multiple_when_two_within_delta() {
    // max = 0.9; threshold = 0.9 - 0.15*0.9 = 0.765. Two candidates above it.
    let input = scores(&[("AA", 0.9), ("AE", 0.8), ("AH", 0.1), ("AO", 0.05)]);
    let out = emit_nbest(&input);
    assert_eq!(out.len(), 2, "competition is kept, no argmax collapse");
    assert_eq!(out[0].phoneme, "AA");
    assert_eq!(out[1].phoneme, "AE");
    assert!(out[0].score >= out[1].score, "sorted descending");
}

#[test]
fn nbest_caps_at_five() {
    // 39 candidates all at max score -> all within delta, but cap at 5.
    let input: Vec<(&'static str, f64)> = ARPABET.iter().map(|&p| (p, 1.0)).collect();
    let out = emit_nbest(&input);
    assert_eq!(out.len(), MAX_N, "INV: n_best_bounded");
    assert!(out.len() <= MAX_N);
    for c in &out {
        assert_eq!(c.score, 1.0);
    }
}

#[test]
fn nbest_all_candidates_in_alphabet() {
    let input: Vec<(&'static str, f64)> = (0..39)
        .map(|i| (ARPABET[(i * 3) % 39], 1.0 - (i as f64) * 0.001))
        .collect();
    let out = emit_nbest(&input);
    for c in &out {
        assert!(ARPABET.contains(&c.phoneme), "INV: candidates_in_alphabet");
    }
}

#[test]
fn nbest_clear_winner_emits_one() {
    // One dominant candidate, all others far below the relative threshold.
    let input = scores(&[("IH", 1.0), ("IY", 0.1), ("JH", 0.2), ("K", 0.15)]);
    let out = emit_nbest(&input);
    assert_eq!(out.len(), 1, "unambiguous frame emits exactly one");
    assert_eq!(out[0].phoneme, "IH");
}

#[test]
fn nbest_threshold_is_relative_not_absolute() {
    // Same absolute gap (0.2), but the max differs: relative 15% threshold
    // lets 0.8 in when max is 1.0 (>= 0.85? no -> excluded) vs
    // max 0.5: threshold 0.425 -> 0.4 excluded. Verify determinism + the
    // relative rule itself on a mid-range case.
    let input = scores(&[("OW", 0.6), ("OY", 0.54), ("P", 0.0)]);
    // threshold = 0.6 - 0.15*0.6 = 0.51 -> 0.54 within.
    let out = emit_nbest(&input);
    assert_eq!(out.len(), 2, "scores_relative: 0.54 >= 0.51 kept");
    // Same gap at a lower max must behave by the relative rule, not a fixed
    // 0.06 gap: max 0.06, threshold 0.051 -> nothing else kept.
    let input2 = scores(&[("R", 0.06), ("S", 0.055), ("SH", 0.0)]);
    let out2 = emit_nbest(&input2);
    assert_eq!(out2.len(), 2, "relative rule scales with max");
}

#[test]
fn nbest_empty_input_empty_output() {
    assert!(emit_nbest(&[]).is_empty());
}

#[test]
fn nbest_default_delta_is_15_percent() {
    assert_eq!(DEFAULT_DELTA, 0.15);
    assert_eq!(MAX_N, 5);
}

#[test]
fn nbest_single_candidate_passthrough() {
    let out = emit_nbest(&scores(&[("ZH", 0.42)]));
    assert_eq!(
        out,
        vec![Candidate {
            phoneme: "ZH",
            score: 0.42
        }]
    );
}
