//! Stage 0 calibration PPT/invariant tests (task 6lx.4).
//!
//! Enforces the Stage 0 contract invariants via the invariant_ppt C-Test
//! layer:
//! - `baseline_finite_and_non_degenerate`
//! - `baseline_comparable` (fixed-dim feature summary)
//!
//! Plus a P-Test for perturbation robustness: a small perturbation of the
//! enrollment frames must produce a baseline close to the unperturbed one.

use calibration::{Baseline, FEATURE_DIM, Frame, fit_baseline};
use invariant_ppt::{
    assert_invariant, clear_contracts, clear_invariant_log, register_contract, test_lock,
    verify_all_contracts,
};
use proptest::prelude::*;
use std::sync::MutexGuard;

fn setup(_g: &MutexGuard<'static, ()>) {
    clear_invariant_log();
    clear_contracts();
}

fn base_frames(n: usize, v: f64) -> Vec<Frame> {
    (0..n)
        .map(|_| {
            let mut f = [0.0; FEATURE_DIM];
            for x in f.iter_mut() {
                *x = v;
            }
            f
        })
        .collect()
}

fn mean_vec(b: &Baseline) -> f64 {
    b.mean.iter().sum::<f64>() / b.mean.len() as f64
}

// Property: perturbing a subset of enrollment frames by a small epsilon keeps
// the fitted baseline's mean close to the original. (Perturbation robustness.)
proptest! {
    #[test]
    fn baseline_robust_to_small_perturbation(
        n in 20usize..80,
        perturb_frames in 1usize..5,
        eps in 0.0f64..0.01,
    ) {
        let g = test_lock::lock();
        setup(&g);
        let clean = base_frames(n, 1.0);
        let mut noisy = clean.clone();
        // Perturb the first `perturb_frames` frames deterministically.
        let count = perturb_frames.min(noisy.len());
        for frame in noisy.iter_mut().take(count) {
            for x in frame.iter_mut() {
                *x += eps;
            }
        }
        let b_clean = fit_baseline(&clean).unwrap();
        let b_noisy = fit_baseline(&noisy).unwrap();
        let drift = (mean_vec(&b_clean) - mean_vec(&b_noisy)).abs();
        // With a small eps and few perturbed frames, mean drift stays tiny.
        prop_assert!(drift < 0.05);
    }
}

#[test]
fn stage0_contract_invariants_enforced() {
    let g = test_lock::lock();
    setup(&g);
    register_contract(
        "stage 0 calibration",
        &["baseline_finite_and_non_degenerate"],
    );
    let frames = base_frames(50, 1.0);
    let b = fit_baseline(&frames).expect("baseline");
    assert!(b.is_valid());
    // Explicitly assert comparability for the contract gate.
    assert_invariant(
        b.mean.len() == FEATURE_DIM,
        "baseline_comparable",
        Some("stage0"),
    );
    assert!(verify_all_contracts() >= 1);
}
