//! 91x.2 gate tests: feedback into Stage 1 scoring for subsequent utterances.
//!
//! DoD:
//! - EMA update sharpens the signature toward the speaker's realized mean
//!   (INV `feedback_converges`)
//! - repeated consistent evidence converges monotonically
//! - signature stays bounded (INV `signature_bounded`, no divergence)
//! - adjusted score = raw + Δ·β applied to a Stage-1 frame

use crate::derive::{AccentSignature, CommittedPhoneme, PHONEME_COUNT, derive_signature};
use crate::feedback::{EMA_ALPHA, adjusted_score, apply_feedback, ema_update};

fn zero_signature() -> AccentSignature {
    derive_signature(&[])
}

/// Repeatedly EMA a fixed per-phoneme displacement `d` and assert convergence.
#[test]
fn feedback_ema_converges_monotonically() {
    let mut sig = zero_signature();
    let d = [0.5f64; PHONEME_COUNT];
    let mut prev: Vec<f64> = sig.displacements.clone();
    // Feed consistent evidence; the target (0.5) dominates exponentially.
    for _ in 0..200 {
        ema_update(&mut sig, &d, EMA_ALPHA);
        // Monotone toward the target: value only ever approaches 0.5.
        for ((new, old), &target) in sig.displacements.iter().zip(prev.iter()).zip(d.iter()) {
            assert!(
                (target - new).abs() <= (target - old).abs() + 1e-12,
                "feedback_converges: |target-new| never increases"
            );
        }
        prev = sig.displacements.clone();
    }
    // Converged near the realized mean (0.8^200 is negligible).
    assert!((sig.delta(0) - 0.5).abs() < 1e-6);
}

#[test]
fn feedback_signature_stays_bounded() {
    let mut sig = zero_signature();
    // Extreme single sample: the bounded EMA caps the per-step move.
    let wild = [-50.0f64; PHONEME_COUNT];
    ema_update(&mut sig, &wild, EMA_ALPHA);
    for v in &sig.displacements {
        assert!(v.is_finite(), "signature_bounded");
        assert!(v.abs() <= 50.0 * EMA_ALPHA + 1e-9, "single-step EMA bound");
    }
    // Repeated extreme samples still finite (no divergence).
    for _ in 0..200 {
        ema_update(&mut sig, &wild, EMA_ALPHA);
    }
    for v in &sig.displacements {
        assert!(v.is_finite(), "signature_bounded after many updates");
    }
}

#[test]
fn feedback_adjusted_score_is_raw_plus_bias() {
    // Δ_5 = 0.25, β = 1.0 -> adjusted = raw + 0.25.
    let sig = derive_signature(&[CommittedPhoneme {
        phoneme: 5,
        displacement: 0.25,
    }]);
    let raw = 0.7;
    let adjusted = adjusted_score(&sig, 5, raw, 1.0);
    assert!(
        (adjusted - (raw + 0.25)).abs() < 1e-12,
        "adjust = raw + Δ·β"
    );
}

#[test]
fn feedback_beta_amplifies_or_attenuates() {
    let sig = derive_signature(&[CommittedPhoneme {
        phoneme: 0,
        displacement: 0.1,
    }]);
    let with_beta2 = adjusted_score(&sig, 0, 0.5, 2.0);
    let with_beta_half = adjusted_score(&sig, 0, 0.5, 0.5);
    assert!((with_beta2 - (0.5 + 0.2)).abs() < 1e-12);
    assert!((with_beta_half - (0.5 + 0.05)).abs() < 1e-12);
}

#[test]
fn feedback_applies_to_all_phone_scores() {
    // A realistic Stage-1 (fse::score_phones-shaped) candidate list.
    let sig = derive_signature(&[
        CommittedPhoneme {
            phoneme: 0,
            displacement: 0.3,
        }, // AA
        CommittedPhoneme {
            phoneme: 23,
            displacement: -0.2,
        }, // K
    ]);
    let scored = vec![("AA", 0.8), ("K", 0.6), ("T", 0.9)];
    let adjusted = apply_feedback(&sig, &scored, 1.0);
    assert_eq!(adjusted.len(), 3);
    // AA: 0.8 + 0.3 = 1.1; K: 0.6 - 0.2 = 0.4; T (Δ=0): 0.9.
    assert!((adjusted[0].1 - 1.1).abs() < 1e-12);
    assert!((adjusted[1].1 - 0.4).abs() < 1e-12);
    assert!((adjusted[2].1 - 0.9).abs() < 1e-12);
    assert_eq!(adjusted[0].0, "AA");
}
