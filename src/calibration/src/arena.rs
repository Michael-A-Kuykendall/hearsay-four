//! Per-speaker baseline into the arena — the Stage 0 → fabric handoff.
//!
//! Task 6lx.3: the calibrated baseline enters the arena as a *single derived
//! fact* that Stage 1 (FSE) consumes. This is the Category D boundary: the
//! EM-style fit (in `lib.rs`) stays external; this module hands its output
//! into the fabric as one interned fact.

use crate::enrollment::Enrollment;
use crate::{Baseline, FEATURE_DIM, fit_baseline};
use fabric::Fabric;

/// Enter a per-speaker baseline into the fabric arena as a single interned
/// fact, and commit it (mark it as an unconditional, stable baseline).
///
/// The entered baseline must be a valid fit payload (`Baseline::is_valid`) —
/// a degenerate or missing baseline never enters silently. Returns the
/// fabric `FactId` of the baseline fact.
pub fn enter_baseline_fact(fabric: &mut Fabric, baseline: &Baseline) -> fabric::FactId {
    // The fact carries a validated fit payload, never a bare interned id:
    // reject degenerate baselines at the arena boundary.
    invariant_ppt::assert_invariant(
        baseline.is_valid(),
        "baseline_finite_and_non_degenerate",
        Some("stage0"),
    );
    // Intern the baseline as one fact. The fabric stores justification
    // structure only, so the validated payload travels alongside the fact
    // (see `EnrolledBaseline`); here the fact is the Tier-2 anchor.
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

/// A wired enrollment (D1 enrollment-as-baseline, bead hearsay-four-cz0):
/// the validated classical-fit payload together with the committed Tier-2
/// fabric fact that carries it.
///
/// The fabric stores justification structure only, so a fact id alone is a
/// bare interned id — scoring consumers (`ConditionA`/`ConditionB`,
/// `measure_session`) need the [`Baseline`] payload alongside it. This struct
/// keeps the two together from `enroll_baseline` to the consumer.
#[derive(Debug, Clone)]
pub struct EnrolledBaseline {
    /// The fitted, validated per-speaker baseline payload.
    pub baseline: Baseline,
    /// The committed, active Tier-2 fact (usable as a `derive_and_assert` premise).
    pub fact: fabric::FactId,
}

/// Explicit enrollment-wiring failure — an empty capture is an error, never
/// a silent stub baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentError {
    /// The enrollment carried no frames (empty utterance / backend silence).
    EmptyEnrollment,
}

impl std::fmt::Display for EnrollmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnrollmentError::EmptyEnrollment => write!(f, "enrollment has no frames"),
        }
    }
}

impl std::error::Error for EnrollmentError {}

/// Wire one captured enrollment utterance into the arena as a committed
/// Tier-2 baseline fact (D1: enrollment-as-baseline).
///
/// `Enrollment.frames → fit_baseline → enter_baseline_fact` (assert + commit).
/// The returned fact is active and committed, so it serves as a premise for
/// Tier-2 derivations (`accent::derive::derive_and_assert`), while the
/// returned payload feeds scoring (`ConditionA`/`ConditionB`, `measure_session`).
///
/// An empty enrollment is an explicit [`EnrollmentError::EmptyEnrollment`] —
/// enrollment never falls back to a silent stub baseline.
pub fn enroll_baseline(
    fabric: &mut Fabric,
    enrollment: &Enrollment,
) -> Result<EnrolledBaseline, EnrollmentError> {
    if enrollment.frames.is_empty() {
        return Err(EnrollmentError::EmptyEnrollment);
    }
    // Non-empty input always fits (`fit_baseline` returns `None` only for no
    // frames); the boundary assert inside `enter_baseline_fact` re-checks
    // payload validity before anything enters the arena.
    let baseline = fit_baseline(&enrollment.frames).expect("nonempty enrollment fits");
    let fact = enter_baseline_fact(fabric, &baseline);
    invariant_ppt::assert_invariant(
        baseline_fact_active(fabric, fact),
        "enrollment_baseline_committed",
        Some("stage0"),
    );
    Ok(EnrolledBaseline { baseline, fact })
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

/// D1 enrollment-as-baseline wiring tests (bead hearsay-four-cz0): captured
/// enrollment utterances flow through `fit_baseline` into a committed Tier-2
/// fact via `enter_baseline_fact`, consumable by `derive_and_assert` and
/// scoring. Synthetic frames only — no audio, no network.
#[cfg(test)]
mod enrollment_wiring_tests {
    use super::*;
    use crate::ENROLLMENT_PROMPT;
    use crate::enrollment::{Enrollment, StubCapture, capture_enrollment};

    #[test]
    fn stub_enrollment_enters_valid_payload_consumable_by_derive() {
        // Existing StubCapture path end to end (MicCapture stays in
        // src/capture — untouched by this wiring).
        let mut backend = StubCapture::default();
        let enrollment = capture_enrollment(&mut backend);
        assert!(enrollment.is_valid());

        let mut fabric = Fabric::new();
        let enrolled = enroll_baseline(&mut fabric, &enrollment).expect("stub enrollment wires");

        // A valid Baseline payload travels with the fact — not a bare
        // interned id: centroid matches the captured distribution exactly.
        assert!(enrolled.baseline.is_valid());
        assert_eq!(enrolled.baseline.frame_count, enrollment.frames.len());
        assert!(enrolled.baseline.variance.iter().all(|&v| v > 0.0));
        assert!(
            enrolled
                .baseline
                .mean
                .iter()
                .all(|&m| (m - backend.base_value).abs() < 1e-9),
            "payload centroid must match the enrollment frames"
        );
        // The fact is active and committed (unconditional Tier-2 anchor).
        assert!(baseline_fact_active(&fabric, enrolled.fact));
        assert!(
            fabric
                .justifications(enrolled.fact)
                .iter()
                .any(|j| j.unconditional),
            "wired fact must carry a committed (unconditional) justification"
        );

        // The payload feeds scoring — the same NLL math measure_session
        // applies per utterance through ConditionA/ConditionB.
        let probe = enrollment.frames[0];
        assert!(crate::score_frame(&enrolled.baseline, &probe).is_finite());

        // derive_and_assert accepts the wired fact as a Tier-2 premise: the
        // derived signature fact goes active with provenance on the wiring.
        let (signature, derived) =
            accent::derive::derive_and_assert(&mut fabric, &[], &[enrolled.fact]);
        assert_eq!(signature.displacements.len(), accent::derive::PHONEME_COUNT);
        assert!(fabric.is_active(derived));
        assert!(
            fabric
                .justifications(derived)
                .iter()
                .any(|j| j.premises.contains(&enrolled.fact)),
            "derived fact must cite the wired baseline fact"
        );
    }

    #[test]
    fn empty_enrollment_is_explicit_error_no_stub_baseline() {
        let mut fabric = Fabric::new();
        // Hand-built empty enrollment: explicit error, never a baseline.
        let empty = Enrollment {
            prompt: ENROLLMENT_PROMPT.to_string(),
            prompt_matched: true,
            frames: Vec::new(),
        };
        assert!(!empty.is_valid());
        assert_eq!(
            enroll_baseline(&mut fabric, &empty).unwrap_err(),
            EnrollmentError::EmptyEnrollment
        );
        // A silent stub backend stays loud too: zero captured frames is an
        // error, not a stub baseline.
        let mut silent = StubCapture {
            frame_count: 0,
            base_value: 1.0,
        };
        let captured = capture_enrollment(&mut silent);
        assert!(captured.frames.is_empty());
        assert_eq!(
            enroll_baseline(&mut fabric, &captured).unwrap_err(),
            EnrollmentError::EmptyEnrollment
        );
    }
}
