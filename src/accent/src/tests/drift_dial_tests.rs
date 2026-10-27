//! D2 drift-tolerance dial tests (bead hearsay-four-dib): the calibrated
//! `DEFAULT_TOLERANCE` (τ = 0.0875 = 3 × σ_max of the D1 enrolled baseline
//! distribution; derivation in the `drift.rs` constant doc-comment) fires on
//! genuine drift, stays silent on enrollment-scale variation, and leaves the
//! retract-and-recascade path intact. Synthetic fixtures only.

use crate::derive::{PHONEME_COUNT, derive_signature};
use crate::drift::{DEFAULT_TOLERANCE, DriftDetector, UtteranceSample, retrain_after_drift};

fn zero_sig() -> Vec<f64> {
    derive_signature(&[]).displacements
}

fn sample(floor: f64) -> UtteranceSample {
    // Deviates from a zero signature by `floor` on every phoneme.
    UtteranceSample {
        displacement: vec![floor; PHONEME_COUNT],
    }
}

#[test]
fn dial_tolerance_calibrated_not_placeholder() {
    // The 2.0 placeholder is gone; the frozen calibrated dial is τ = 0.0875
    // (3 × σ_max = 3 × 0.029158, measured off the D1 enrollment fixture —
    // see the derivation + TOLERANCE provenance note on DEFAULT_TOLERANCE).
    assert!(
        (DEFAULT_TOLERANCE - 2.0).abs() > f64::EPSILON,
        "calibrated tolerance must differ from the 2.0 placeholder"
    );
    assert!(
        (DEFAULT_TOLERANCE - 0.0875).abs() < 1e-12,
        "frozen dial value τ = 0.0875, got {DEFAULT_TOLERANCE}"
    );
}

#[test]
fn dial_trigger_fires_on_drift_silent_on_stable() {
    let sig = zero_sig();
    // Drifting fixture at 0.5 feature units: above the calibrated τ (0.0875)
    // yet far below the old 2.0 placeholder — the dial is what catches it.
    let mut detector = DriftDetector::new(DEFAULT_TOLERANCE);
    assert!(!detector.observe(sample(0.5), &sig));
    assert!(!detector.observe(sample(0.5), &sig));
    assert!(
        detector.observe(sample(0.5), &sig),
        "3-of-5 drifting fixture fires at the calibrated tolerance"
    );
    // The old placeholder would have been blind to this drift.
    let mut stale = DriftDetector::new(2.0);
    for _ in 0..5 {
        assert!(
            !stale.observe(sample(0.5), &sig),
            "0.5-scale drift hides under the 2.0 placeholder"
        );
    }
    // Stable fixture at enrollment σ-scale (0.05 ≈ σ_max order): silent.
    let mut calm = DriftDetector::new(DEFAULT_TOLERANCE);
    for _ in 0..5 {
        assert!(
            !calm.observe(sample(0.05), &sig),
            "enrollment-scale variation stays silent at the new tolerance"
        );
    }
}

#[test]
fn dial_retrain_still_retracts_and_recascades() {
    use fabric::Fabric;

    // Drive the calibrated detector to a genuine trigger first.
    let sig = zero_sig();
    let mut detector = DriftDetector::new(DEFAULT_TOLERANCE);
    assert!(!detector.observe(sample(0.5), &sig));
    assert!(!detector.observe(sample(0.5), &sig));
    assert!(
        detector.observe(sample(0.5), &sig),
        "calibrated trigger fires before retraining"
    );

    // Same retract-and-recascade chain as the 91x.3 gate: drifted LEAF
    // evidence backs old baseline <- old signature <- dependent.
    let mut fabric = Fabric::new();
    let leaf = fabric.intern();
    fabric.assert_assumption(leaf);
    let old_basis = fabric.intern();
    {
        let mut p = std::collections::BTreeSet::new();
        p.insert(leaf);
        fabric.assert(old_basis, p, false);
    }
    let old_sig = fabric.intern();
    {
        let mut p = std::collections::BTreeSet::new();
        p.insert(old_basis);
        fabric.assert(old_sig, p, false);
    }
    let dependent = fabric.intern();
    {
        let mut p = std::collections::BTreeSet::new();
        p.insert(old_sig);
        fabric.assert(dependent, p, false);
    }
    assert!(fabric.is_active(old_sig));
    assert!(fabric.is_active(dependent));

    let recent = [
        crate::derive::CommittedPhoneme {
            phoneme: 0,
            displacement: 0.4,
        },
        crate::derive::CommittedPhoneme {
            phoneme: 1,
            displacement: -0.2,
        },
    ];
    let premise1 = fabric.intern();
    let premise2 = fabric.intern();
    fabric.assert_assumption(premise1);
    fabric.assert_assumption(premise2);

    let new_sig = retrain_after_drift(&mut fabric, &[leaf], &recent, &[premise1, premise2]);
    assert!(!fabric.is_active(leaf), "drifted leaf retracted");
    assert!(!fabric.is_active(old_basis), "old baseline retracted");
    assert!(!fabric.is_active(old_sig), "old signature retired");
    assert!(
        !fabric.is_active(dependent),
        "dependent recascaded off old baseline (recascade_automatic)"
    );
    assert!(
        fabric.is_active(new_sig),
        "re-derived signature live on the new baseline"
    );
}
