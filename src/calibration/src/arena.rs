//! Per-speaker baseline into the arena — the Stage 0 → fabric handoff.
//!
//! Task 6lx.3: the calibrated baseline enters the arena as a *single derived
//! fact* that Stage 1 (FSE) consumes. This is the Category D boundary: the
//! EM-style fit (in `lib.rs`) stays external; this module hands its output
//! into the fabric as one interned fact.

use crate::{Baseline, FEATURE_DIM};
use fabric::Fabric;

/// Enter a per-speaker baseline into the fabric arena as a single interned
/// fact, and commit it (mark it as an unconditional, stable baseline).
///
/// Returns the fabric `FactId` of the baseline fact.
pub fn enter_baseline_fact(fabric: &mut Fabric, _baseline: &Baseline) -> fabric::FactId {
    // Intern the baseline as one fact. In the full system this fact's
    // content is the baseline payload; here we just intern a single id.
    let fact = fabric.intern();
    // Commit the baseline so it is stable (unconditional) — it cannot be
    // un-derived by premise failure. This makes it a permanent anchor for
    // Stage 1 scoring.
    fabric.assert_assumption(fact);
    fabric.commit(fact);
    fact
}

/// Verify the baseline fact is active in the fabric (invariant
/// `baseline_fact_active`).
pub fn baseline_fact_active(fabric: &Fabric, fact: fabric::FactId) -> bool {
    fabric.is_active(fact)
}

/// A compact, comparable descriptor of the baseline for the arena payload.
/// This is what Stage 1 would read to score frames against the speaker's
/// baseline rather than a generic reference.
#[derive(Debug, Clone)]
pub struct BaselineFact {
    pub fact: fabric::FactId,
    pub dim: usize,
    pub mean: Vec<f64>,
}

/// Build the arena-facing baseline descriptor (fact id + mean vector).
pub fn baseline_descriptor(
    _fabric: &Fabric,
    fact: fabric::FactId,
    baseline: &Baseline,
) -> BaselineFact {
    BaselineFact {
        fact,
        dim: FEATURE_DIM,
        mean: baseline.mean.clone(),
    }
}

#[cfg(test)]
mod arena_tests {
    use super::*;
    use crate::fit_baseline;

    fn frames(n: usize, v: f64) -> Vec<crate::Frame> {
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

    #[test]
    fn baseline_enters_as_single_active_fact() {
        let mut f = Fabric::new();
        let b = fit_baseline(&frames(50, 1.0)).unwrap();
        let fact = enter_baseline_fact(&mut f, &b);
        assert!(baseline_fact_active(&f, fact));
    }

    #[test]
    fn descriptor_exposes_mean_for_stage1() {
        let mut f = Fabric::new();
        let b = fit_baseline(&frames(50, 2.5)).unwrap();
        let fact = enter_baseline_fact(&mut f, &b);
        let desc = baseline_descriptor(&f, fact, &b);
        assert_eq!(desc.dim, FEATURE_DIM);
        assert!(desc.mean.iter().all(|&m| (m - 2.5).abs() < 1e-6));
    }
}
