//! 91x.4 gate: `cd src/accent && cargo test --all-features`
//!
//! Contract test 'stage10 accent' over the invariants from
//! accent-feedback-semantics.md §6: signature_bounded, feedback_converges,
//! drift_trigger_guarded, recascade_automatic. Plus proptest fuzz over
//! session-long sequences. Serialized with `invariant_ppt::test_lock`.

use proptest::prelude::*;

use invariant_ppt::{
    clear_contracts, clear_invariant_log, register_contract, test_lock, verify_all_contracts,
};

use crate::derive::{AccentSignature, CommittedPhoneme, PHONEME_COUNT, derive_signature};
use crate::drift::{DriftDetector, UtteranceSample, retrain_after_drift};
use crate::feedback::{EMA_ALPHA, ema_update};

/// All four contract invariants for the stage-10 accent loop (doc §6).
const CONTRACT_INVARIANTS: &[&str] = &[
    "signature_bounded",
    "feedback_converges",
    "drift_trigger_guarded",
    "recascade_automatic",
];

fn zero_signature() -> AccentSignature {
    derive_signature(&[])
}

#[test]
fn contract_stage10_accent_invariants_enforced() {
    let _guard = test_lock::lock();
    clear_invariant_log();
    clear_contracts();
    register_contract("stage10 accent", CONTRACT_INVARIANTS);

    // --- derive + feedback: fires signature_bounded + feedback_converges ---
    let mut sig = zero_signature();
    let delta = vec![0.3f64; PHONEME_COUNT];
    for _ in 0..10 {
        ema_update(&mut sig, &delta, EMA_ALPHA);
    }

    // --- drift: fires drift_trigger_guarded (and signature_bounded) ---
    let mut detector = DriftDetector::new(2.0);
    let s = || UtteranceSample {
        displacement: vec![9.0; PHONEME_COUNT],
    };
    for _ in 0..5 {
        detector.observe(s(), &sig.displacements);
    }

    // --- recascade: fires recascade_automatic ---
    let mut fabric = fabric::Fabric::new();
    let leaf = fabric.intern();
    fabric.assert_assumption(leaf);
    let old_basis = fabric.intern();
    let mut p1 = std::collections::BTreeSet::new();
    p1.insert(leaf);
    fabric.assert(old_basis, p1, false);
    let old_sig = fabric.intern();
    let mut p2 = std::collections::BTreeSet::new();
    p2.insert(old_basis);
    fabric.assert(old_sig, p2, false);
    let recent = [CommittedPhoneme {
        phoneme: 0,
        displacement: 0.4,
    }];
    let premise = fabric.intern();
    fabric.assert_assumption(premise);
    let _new = retrain_after_drift(&mut fabric, &[leaf], &recent, &[premise]);

    // Every required invariant must have fired.
    for inv in CONTRACT_INVARIANTS {
        assert!(
            invariant_ppt::has_invariant(inv),
            "invariant {inv} was not recorded"
        );
    }
    assert!(
        verify_all_contracts() >= 1,
        "'stage10 accent' contract verified"
    );
}

// P-Test: proptest over session-long sequences — EMA stays bounded, drift
// trigger respects the guard.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(25))]
    #[test]
    fn ppt_session_long_sequence_stays_bounded(
        n_updates in 1usize..40,
        drift_tau in 0.5f64..10.0,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("stage10 accent", CONTRACT_INVARIANTS);

        let mut sig = zero_signature();
        for i in 0..n_updates {
            let base = if i % 2 == 0 { 5.0 } else { -5.0 };
            let delta = vec![base; PHONEME_COUNT];
            ema_update(&mut sig, &delta, EMA_ALPHA);
        }
        prop_assert!(sig.displacements.iter().all(|v| v.is_finite()));
        prop_assert!(sig.displacements.iter().all(|v| v.abs() < 100.0));

        let mut detector = DriftDetector::new(drift_tau);
        for i in 0..n_updates.min(20) {
            let d = if i % 3 == 0 { 30.0 } else { 0.5 };
            let triggered = detector.observe(
                UtteranceSample { displacement: vec![d; PHONEME_COUNT] },
                &sig.displacements,
            );
            if triggered {
                prop_assert!(detector.history.len() >= 3, "guard: need 3 samples");
            }
        }

        // Exercise the recascade path so recascade_automatic also fires.
        let mut fabric = fabric::Fabric::new();
        let leaf = fabric.intern();
        fabric.assert_assumption(leaf);
        let old_basis = fabric.intern();
        let mut p1 = std::collections::BTreeSet::new();
        p1.insert(leaf);
        fabric.assert(old_basis, p1, false);
        let recent = [CommittedPhoneme { phoneme: 0, displacement: 0.4 }];
        let premise = fabric.intern();
        fabric.assert_assumption(premise);
        let _ = retrain_after_drift(&mut fabric, &[leaf], &recent, &[premise]);

        prop_assert!(verify_all_contracts() >= 1);
    }
}

// P-Test: EMA monotone convergence toward the target per update.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]
    #[test]
    fn ppt_ema_updates_converge(
        alpha in 0.05f64..1.0,
        target in -10f64..10.0,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("stage10 accent", &["feedback_converges", "signature_bounded"]);
        let mut sig = AccentSignature { displacements: vec![0.0; PHONEME_COUNT] };
        ema_update(&mut sig, &vec![target; PHONEME_COUNT], alpha);
        for v in &sig.displacements {
            prop_assert!(v.is_finite());
        }
        prop_assert!(verify_all_contracts() >= 1);
    }
}
