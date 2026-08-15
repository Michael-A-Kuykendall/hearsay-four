//! 91x.3 gate tests: mid-session drift as live retraction-and-recascade.
//!
//! DoD:
//! - a single outlier does NOT trigger retraction (INV `drift_trigger_guarded`)
//! - sustained deviation (3-of-5) triggers retraction + re-derivation
//! - dependents on the old baseline are recascaded automatically (INV
//!   `recascade_automatic`, no orphaned baseline facts)

use crate::derive::{PHONEME_COUNT, derive_signature};
use crate::drift::{
    DEFAULT_TOLERANCE, DriftDetector, UtteranceSample, WINDOW, retrain_after_drift,
};

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
fn drift_single_outlier_does_not_trigger() {
    let sig = zero_sig();
    let mut detector = DriftDetector::new(DEFAULT_TOLERANCE);
    assert!(
        !detector.observe(sample(10.0), &sig),
        "single outlier alone does not trigger (drift_trigger_guarded)"
    );
    for _ in 1..WINDOW {
        assert!(
            !detector.observe(sample(0.05), &sig),
            "converged samples never fire"
        );
    }
}

#[test]
fn drift_two_deviating_do_not_trigger_three_do() {
    let sig = zero_sig();
    let mut detector = DriftDetector::new(DEFAULT_TOLERANCE);
    // 2 in-convergence then 2 deviating: still under the 3-of-5 threshold.
    assert!(!detector.observe(sample(0.05), &sig));
    assert!(!detector.observe(sample(0.05), &sig));
    assert!(
        !detector.observe(sample(9.0), &sig),
        "2 in window not enough"
    );
    assert!(
        !detector.observe(sample(9.0), &sig),
        "still only 2 deviating"
    );
    // The 3rd deviating utterance crosses the trigger.
    assert!(
        detector.observe(sample(9.0), &sig),
        "3-of-5 fires the trigger"
    );
}

#[test]
fn drift_sustained_three_of_five_triggers() {
    let sig = zero_sig();
    let mut detector = DriftDetector::new(DEFAULT_TOLERANCE);
    assert!(!detector.observe(sample(0.05), &sig));
    assert!(!detector.observe(sample(0.05), &sig));
    assert!(
        !detector.observe(sample(9.0), &sig),
        "only 1 deviating so far — guarded"
    );
    assert!(
        !detector.observe(sample(9.0), &sig),
        "still only 2 deviating — guarded"
    );
    assert!(
        detector.observe(sample(9.0), &sig),
        "3rd deviating utterance fires"
    );
}

#[test]
fn drift_recascades_dependents_automatically() {
    use fabric::Fabric;

    let mut fabric = Fabric::new();
    // The drifted LEAF evidence (an obtained observation the old baseline was
    // built on). Everything downstream derives from it, and retracting this
    // leaf must cascade the whole old chain out (INV recascade_automatic).
    let leaf = fabric.intern();
    fabric.assert_assumption(leaf);
    // old baseline <- leaf; old signature <- old baseline; dependent <- sig.
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
    assert!(
        fabric.is_active(old_sig),
        "old signature active (supported)"
    );
    assert!(
        fabric.is_active(dependent),
        "dependent live on old baseline"
    );

    // Recent committed facts for re-derivation.
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

    let _new_sig = retrain_after_drift(&mut fabric, &[leaf], &recent, &[premise1, premise2]);
    // The drifted leaf is retracted; everything derived from it — baseline,
    // old signature, and dependent — recascales off automatically. No orphaned
    // baseline-dependent fact remains active.
    assert!(!fabric.is_active(leaf), "drifted leaf retracted");
    assert!(!fabric.is_active(old_basis), "old baseline retracted");
    assert!(!fabric.is_active(old_sig), "old signature retired");
    assert!(
        !fabric.is_active(dependent),
        "dependent recascaled off old baseline (recascade_automatic)"
    );
}

#[test]
fn drift_zero_drift_never_triggers() {
    let sig = zero_sig();
    let mut detector = DriftDetector::new(DEFAULT_TOLERANCE);
    for _ in 0..20 {
        assert!(
            !detector.observe(sample(0.0), &sig),
            "no drift never triggers"
        );
    }
}
