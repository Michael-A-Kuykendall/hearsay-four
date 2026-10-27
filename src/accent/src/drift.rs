//! Mid-session drift as live retraction-and-recascade (91x.3).
//!
//! accent-feedback-semantics.md §5: drift is not rectified by a single
//! outlier. A drift trigger fires only when the per-utterance displacement
//! deviates from the current signature by more than a pre-registered
//! threshold `τ` on **3 of the last 5** consecutive utterances (INV
//! `drift_trigger_guarded`). On trigger: the old signature fact is retracted
//! (fabric survival semantics cascade to its dependents), the signature is
//! re-derived from recent committed facts, and the fabric automatically
//! recascades every dependent off the new baseline (INV
//! `recascade_automatic` — no orphaned baseline-dependent facts).

use invariant_ppt::assert_invariant;

use crate::derive::{CommittedPhoneme, derive_and_assert};
use fabric::{Fabric, FactId};

/// Number of utterances in the drift watch window.
pub const WINDOW: usize = 5;
/// Utterances that must deviate to fire the trigger.
pub const TRIGGER_COUNT: usize = 3;
/// Pre-registered deviation threshold (feature units), calibrated dial
/// (bead hearsay-four-dib; Stage-10 empirical dial, aem.6 §7).
///
/// Derivation (model-free guard band from the D1 enrolled baseline
/// distribution — no fitted model, no distributional assumption):
/// 1. Fit the D1 enrollment baseline over the synthetic enrollment fixture
///    (the same `tone` PCM generator D1's `enroll_baseline_tests` uses:
///    `(i % 97) as i16 * 173`, 48_000 samples @ 48 kHz, through the real
///    front-end path `baseline_from_frames` = window → extract → fit).
/// 2. Read off the worst-case per-dimension frame spread of that enrolled
///    distribution: σ_max = max per-dim std = 0.029158 (98 frames; per-dim
///    std min 0.000041, median 0.004304, mean 0.007195).
/// 3. Freeze τ = 3 × σ_max = 0.087473 → 0.0875 (rounded up: the conservative
///    side, fewer false triggers). The multiplier k = 3 is a fixed
///    guard-band constant, not a fitted parameter: by Chebyshev, any
///    distribution keeps ≥8/9 of its mass within 3σ, so enrollment-like
///    utterances (≈1σ-scale displacements) sit an order of magnitude inside
///    τ while a genuine distribution shift (≫ σ) exceeds it on 3 of 5.
///
///   TOLERANCE provenance note: τ = 0.0875 frozen 2026-10-27 (bead
///   hearsay-four-dib, branch bead/dib-drift-dial). Measured with a throwaway
///   probe bin over the synthetic `tone` fixture (probe deleted after
///   measurement; fixtures only, no network, no real audio). Re-derive by
///   re-running `baseline_from_frames` on the enrollment fixture and taking
///   3 × max per-dim std. Replaces the 2.0 placeholder; `retrain_after_drift`
///   retract-and-recascade semantics below are unchanged.
pub const DEFAULT_TOLERANCE: f64 = 0.0875;

/// A per-utterance displacement measurement (which phoneme drifted how far).
#[derive(Debug, Clone, PartialEq)]
pub struct UtteranceSample {
    /// Per-phoneme displacement vector for this utterance (length
    /// `derive::PHONEME_COUNT`).
    pub displacement: Vec<f64>,
}

/// The drift detector: guards against single-outlier false triggers.
#[derive(Debug, Clone)]
pub struct DriftDetector {
    /// Tolerance `τ` on the per-phoneme deviation.
    tau: f64,
    /// Rolling history of recent samples (max `WINDOW`), newest last.
    pub history: Vec<UtteranceSample>,
}

impl DriftDetector {
    pub fn new(tau: f64) -> DriftDetector {
        assert_invariant(tau > 0.0, "drift_tau_positive", Some("91x.3"));
        DriftDetector {
            tau,
            history: Vec::new(),
        }
    }

    /// Record one utterance's displacement. Returns `true` only when the
    /// pre-registered trigger condition is met: ≥3 of the last 5 utterances
    /// deviate by more than `τ` from the current signature (INV
    /// `drift_trigger_guarded` — no single-outlier trigger).
    pub fn observe(&mut self, sample: UtteranceSample, signature: &[f64]) -> bool {
        assert_invariant(
            sample.displacement.iter().all(|v| v.is_finite()),
            "signature_bounded",
            Some("91x.3"),
        );
        self.history.push(sample);
        if self.history.len() > WINDOW {
            self.history.remove(0);
        }
        let recent_deviating = self
            .history
            .iter()
            .filter(|s| self.is_deviating(s, signature))
            .count();
        let trigger = recent_deviating >= TRIGGER_COUNT;
        // INV `drift_trigger_guarded`: the guard is structural — the trigger
        // cannot fire without at least `TRIGGER_COUNT` observed utterances in
        // the window (a single outlier, or two, can never fire it).
        assert_invariant(
            !trigger || self.history.len() >= TRIGGER_COUNT,
            "drift_trigger_guarded",
            Some("91x.3"),
        );
        trigger
    }

    fn is_deviating(&self, sample: &UtteranceSample, signature: &[f64]) -> bool {
        signature
            .iter()
            .zip(&sample.displacement)
            .any(|(&sig, &d)| (d - sig).abs() > self.tau)
    }
}

/// Handle detected drift: retract the old signature and its source facts, then
/// re-derive from the recent committed phoneme facts.
///
/// The old signature was derived from `old_source_facts` (the calibration /
/// baseline state that has now drifted). Retracting those *sources* — not just
/// the signature — is what makes the cascade work: the signature loses its
/// last active justification, so the fabric retracts it, and every dependent
/// of the signature is recascaled off the old baseline automatically (INV
/// `recascade_automatic`, no orphaned baseline-dependent facts). The new
/// signature is derived from `recent_committed` and asserted into the fabric.
pub fn retrain_after_drift(
    fabric: &mut Fabric,
    old_source_facts: &[FactId],
    recent_committed: &[CommittedPhoneme],
    recent_premises: &[FactId],
) -> FactId {
    // Retract the drifted sources: the fabric cascades to the old signature
    // and its dependents automatically (survival semantics, 6za.1).
    for source in old_source_facts {
        fabric.retract(*source);
    }

    // Re-derive from recent committed phonetic facts.
    let (_, new_fact) = derive_and_assert(fabric, recent_committed, recent_premises);
    assert_invariant(
        old_source_facts.iter().all(|f| !fabric.is_active(*f)),
        "recascade_automatic",
        Some("91x.3"),
    );
    assert_invariant(
        fabric.is_active(new_fact),
        "recascade_automatic",
        Some("91x.3"),
    );
    new_fact
}
