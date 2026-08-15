//! 39c.6 gate: `cd src/ && cargo test --manifest-path accent/Cargo.toml --all-features`
//!
//! Contract test 'failfast measurement' over the invariants from
//! failfast-separation-criterion.md §7 (measurement_symmetry,
//! buckets_exhaustive, verdict_pre_registered), plus proptest fuzz over
//! utterance subsets. Serialized via `invariant_ppt::test_lock`.

use proptest::prelude::*;

use invariant_ppt::{
    clear_contracts, clear_invariant_log, register_contract, test_lock, verify_all_contracts,
};

use crate::measure::{SessionRow, apply_rule, measure_session};
use crate::{ConditionA, ConditionB, FEATURE_DIM};
use calibration::{Frame, fit_baseline};

/// All three contract invariants for the fail-fast measurement (doc §3).
const CONTRACT_INVARIANTS: &[&str] = &[
    "measurement_symmetry",
    "buckets_exhaustive",
    "verdict_pre_registered",
];

fn synthetic_frames(n: usize, dim_value: f64) -> Vec<Frame> {
    (0..n)
        .map(|_| {
            let mut f = [0.0; FEATURE_DIM];
            for x in f.iter_mut() {
                *x = dim_value;
            }
            f
        })
        .collect()
}

/// A 2-bucket synthetic session that lets B separate down. Enough PCM for
/// finite, distinct per-utterance scores under both conditions.
fn synthetic_rows() -> Vec<SessionRow> {
    let mut rows = Vec::new();
    for b in 0..2 {
        for idx in 0..3 {
            rows.push(SessionRow {
                bucket: b,
                idx,
                start_sec: (b * 3 + idx) as f64 * 0.25 + 0.2,
                stop_sec: (b * 3 + idx) as f64 * 0.25 + 0.5,
                text: "word".to_string(),
            });
        }
    }
    rows
}

fn synthetic_pcm(seconds: f64) -> Vec<i16> {
    (0..(seconds as usize * 16_000))
        .map(|i| ((i as i16) % 7) - 3)
        .collect()
}

/// C-Test: the fail-fast measurement actively enforces all three invariants.
#[test]
fn contract_failfast_measurement_invariants_enforced() {
    let _guard = test_lock::lock();
    clear_invariant_log();
    clear_contracts();
    register_contract("failfast measurement", CONTRACT_INVARIANTS);

    let enroll = synthetic_frames(100, 1.0);
    let baseline = fit_baseline(&enroll).expect("baseline");
    let a = ConditionA::new(baseline.clone());
    let mut b = ConditionB::new(baseline);

    let rows = synthetic_rows();
    let pcm = synthetic_pcm(4.0);
    let stats = measure_session(&a, &mut b, &pcm, 16_000, &rows).expect("measure");
    // Fires measurement_symmetry (same frame feed) + buckets_exhaustive.
    let _ = apply_rule(&stats); // fires verdict_pre_registered

    // Every required invariant must have fired.
    for inv in CONTRACT_INVARIANTS {
        assert!(
            invariant_ppt::has_invariant(inv),
            "invariant {inv} was not recorded by the measurement"
        );
    }
    assert!(
        verify_all_contracts() >= 1,
        "'failfast measurement' contract verified"
    );
}

// P-Test (proptest): any well-formed utterance set yields finite bucket means
// and a deterministic verdict (the pre-registered rule is not revised per
// outcome). Covers 1..4 buckets, including the single-bucket degenerate case.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(25))]
    #[test]
    fn ppt_arbitrary_bucketed_sessions_are_finite_and_deterministic(
        n_buckets in 1usize..4,
        per_bucket in 1usize..5,
        signal in 2i16..20,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("failfast measurement", CONTRACT_INVARIANTS);

        let mut rows = Vec::new();
        let mut t = 0.25f64;
        for b in 0..n_buckets {
            for idx in 0..per_bucket {
                rows.push(SessionRow {
                    bucket: b,
                    idx,
                    start_sec: t,
                    stop_sec: t + 0.3,
                    text: "word".to_string(),
                });
                t += 0.35;
            }
        }
        let sig_max = signal.max(1);
        // Pad the buffer 2s past the last stop so no slice exceeds it.
        let total_secs = t + 2.0;
        let pcm: Vec<i16> = (0..(total_secs as usize * 16_000))
            .map(|i| ((i as i16) % sig_max) - (sig_max / 2))
            .collect();

        let enroll = synthetic_frames(100, 1.0);
        let baseline = fit_baseline(&enroll).expect("baseline");
        let a = ConditionA::new(baseline.clone());
        let mut b = ConditionB::new(baseline);

        let stats = measure_session(&a, &mut b, &pcm, 16_000, &rows).expect("measure");
        prop_assert!(!stats.is_empty());
        for s in &stats {
            prop_assert!(s.mean_a.is_finite());
            prop_assert!(s.mean_b.is_finite());
            prop_assert!(s.count > 0);
        }

        let v1 = apply_rule(&stats);
        let v2 = apply_rule(&stats);
        prop_assert_eq!(v1.pass, v2.pass, "pre-registered rule is deterministic");
        prop_assert!(verify_all_contracts() >= 1);
    }
}
