//! Stage 0 calibration — classical per-speaker baseline fit.
//!
//! Implements the design in `docs/design/calibration-utterance.md` and
//! `docs/design/phoneme-alphabet.md` (aem.11, aem.10). This is the
//! **Category D** component: a classical formant/cepstral baseline fit
//! (Reynolds & Rose GMM-style) that stays *external* to the fabric and
//! simply hands its output in as a single baseline fact.
//!
//! Per the conversion inventory §2.5, this is the reference "don't invert
//! this" example — the EM fitting procedure is ordinary numerical code that
//! feeds a fact, not a target for fabric inversion.
//!
//! This is a functional baseline over synthetic/mock feature data for the
//! prototype. The real acoustic front end (Stage 1 FSE) will feed actual
//! 39-dim MFCC frames; this module provides the classical fit machinery and
//! a deterministic test harness.

use std::collections::BTreeMap;

/// Number of MFCC coefficients per frame (aem.10: 12 MFCC + energy + deltas
/// = 39 features). This is the feature-space dimensionality.
pub const FEATURE_DIM: usize = 39;

pub mod arena;
pub mod enrollment;

#[cfg(test)]
mod frontend_feed_tests;

/// A single feature frame (39-dim vector).
pub type Frame = [f64; FEATURE_DIM];

/// A per-speaker baseline signature: the classical fit's summary statistics.
///
/// This is the fact that enters the arena (task 6lx.3) and is consumed by
/// Stage 1 scoring.
#[derive(Debug, Clone)]
pub struct Baseline {
    /// Mean feature vector across the enrollment frames (the centroid).
    pub mean: Vec<f64>,
    /// Per-dimension variance (diagonal-covariance GMM-style fit).
    pub variance: Vec<f64>,
    /// Number of enrollment frames used.
    pub frame_count: usize,
}

/// The fixed enrollment utterance (aem.11): the English pangram.
pub const ENROLLMENT_PROMPT: &str = "The quick brown fox jumps over the lazy dog";

impl Baseline {
    /// Verify the baseline is finite and non-degenerate (invariant
    /// `baseline_finite`).
    pub fn is_valid(&self) -> bool {
        self.mean.len() == FEATURE_DIM
            && self.variance.len() == FEATURE_DIM
            && self.mean.iter().all(|x| x.is_finite())
            && self.variance.iter().all(|x| x.is_finite() && *x > 0.0)
    }

    /// The per-feature standard deviation.
    pub fn std(&self) -> Vec<f64> {
        self.variance.iter().map(|v| v.sqrt()).collect()
    }
}

/// A classical Gaussian baseline fit (diagonal covariance) over enrollment
/// frames. This is the Category D "ordinary numerical code" — EM-style, but
/// for a single Gaussian the closed-form MLE is just mean/variance.
///
/// Returns `None` if no frames are provided.
pub fn fit_baseline(frames: &[Frame]) -> Option<Baseline> {
    if frames.is_empty() {
        return None;
    }
    let n = frames.len() as f64;
    let mut mean = vec![0.0; FEATURE_DIM];
    for f in frames {
        for (m, &x) in mean.iter_mut().zip(f.iter()) {
            *m += x;
        }
    }
    for m in mean.iter_mut() {
        *m /= n;
    }
    let mut variance = vec![0.0; FEATURE_DIM];
    for f in frames {
        for (v, (&x, &m)) in variance.iter_mut().zip(f.iter().zip(mean.iter())) {
            *v += (x - m) * (x - m);
        }
    }
    // Population variance; add tiny epsilon to avoid zero variance (degenerate).
    let epsilon = 1e-9;
    for v in variance.iter_mut() {
        *v = *v / n + epsilon;
    }
    let baseline = Baseline {
        mean,
        variance,
        frame_count: frames.len(),
    };
    // Invariants enforced at the fit boundary (for the PPT/C-Test system,
    // task 6lx.4). These are the contract invariants for Stage 0.
    invariant_ppt::assert_invariant(
        baseline.is_valid(),
        "baseline_finite_and_non_degenerate",
        Some("stage0"),
    );
    Some(baseline)
}

/// Ceiling for a degenerate per-dimension variance (D3 front-end feed,
/// bead hearsay-four-7eh).
///
/// [`fit_baseline`] adds `epsilon = 1e-9` to every variance, so constant
/// frames (all-silence FSE output, or synthesised stub frames) fit to a
/// variance of exactly `1e-9` in every dimension. Anything at or below this
/// ceiling carries no usable speaker information and is rejected by
/// [`fit_baseline_checked`]; real speech fits orders of magnitude above it
/// (locked by `frontend_oyez_golden_variance_matches_fixture`).
pub const DEGENERATE_VARIANCE_CEIL: f64 = 1e-6;

/// Explicit failure for the strict front-end fit entry point
/// ([`fit_baseline_checked`]) — an invalid baseline is an error, never a
/// silent degenerate fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineError {
    /// No frames provided.
    EmptyInput,
    /// Frames carry no usable speech information: a non-finite component, a
    /// non-finite/non-degenerate fit, or a zero-variance (degenerate) fit —
    /// e.g. all-silence input or constant synthesised frames.
    DegenerateFrames,
}

impl std::fmt::Display for BaselineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaselineError::EmptyInput => write!(f, "baseline fit has no frames"),
            BaselineError::DegenerateFrames => {
                write!(f, "baseline fit is degenerate (silence or constant frames)")
            }
        }
    }
}

impl std::error::Error for BaselineError {}

/// Strict front-end fit (D3, bead hearsay-four-7eh): the default enrollment
/// entry point over real Stage-1 FSE frames (`frames in -> Gaussian baseline
/// out`).
///
/// Unlike [`fit_baseline`], which is the raw numerical core kept for
/// back-compat (fse/accent scoring paths), this entry rejects an invalid
/// baseline explicitly instead of fitting it silently:
/// - empty input → [`BaselineError::EmptyInput`]
/// - non-finite components, or a degenerate (zero-variance) fit as produced
///   by all-silence input or constant synthesised frames →
///   [`BaselineError::DegenerateFrames`]
///
/// The finiteness pre-check runs before [`fit_baseline`] because the core's
/// boundary invariant panics on non-finite payloads.
pub fn fit_baseline_checked(frames: &[Frame]) -> Result<Baseline, BaselineError> {
    if frames.is_empty() {
        return Err(BaselineError::EmptyInput);
    }
    if frames.iter().any(|f| f.iter().any(|x| !x.is_finite())) {
        return Err(BaselineError::DegenerateFrames);
    }
    let baseline = fit_baseline(frames).expect("nonempty finite frames fit");
    if !baseline.is_valid() {
        return Err(BaselineError::DegenerateFrames);
    }
    if baseline
        .variance
        .iter()
        .all(|&v| v <= DEGENERATE_VARIANCE_CEIL)
    {
        return Err(BaselineError::DegenerateFrames);
    }
    Ok(baseline)
}

/// Score a frame against a baseline: negative log-likelihood under the
/// diagonal Gaussian (the classical fit). Lower = closer to baseline.
pub fn score_frame(baseline: &Baseline, frame: &Frame) -> f64 {
    let std: Vec<f64> = baseline.std();
    let mut nll = 0.0;
    for ((&x, &m), &s) in frame.iter().zip(baseline.mean.iter()).zip(std.iter()) {
        let z = (x - m) / s;
        nll += 0.5 * z * z + s.ln(); // constant ln(2π) term omitted (equal for all)
    }
    nll
}

/// Produce a compact baseline "fact" payload (aem.16-compatible: just the
/// values; the arena wraps it in a FactId). Returns a named-vector map for
/// clarity.
pub fn baseline_fact(baseline: &Baseline) -> BTreeMap<String, Vec<f64>> {
    let mut m = BTreeMap::new();
    m.insert("mean".to_string(), baseline.mean.clone());
    m.insert("variance".to_string(), baseline.variance.clone());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn fit_produces_valid_baseline() {
        let frames = synthetic_frames(100, 1.5);
        let b = fit_baseline(&frames).expect("baseline");
        assert!(b.is_valid());
        assert_eq!(b.frame_count, 100);
        assert!(b.mean.iter().all(|&m| (m - 1.5).abs() < 1e-6));
    }

    #[test]
    fn empty_frames_no_baseline() {
        assert!(fit_baseline(&[]).is_none());
    }

    #[test]
    fn same_distribution_scores_low() {
        let frames = synthetic_frames(100, 2.0);
        let b = fit_baseline(&frames).unwrap();
        // A frame from the same distribution (same value) scores near the
        // baseline minimum; a far frame scores higher.
        let same = synthetic_frames(1, 2.0)[0];
        let far = synthetic_frames(1, 2.0 + 5.0)[0];
        assert!(score_frame(&b, &same) < score_frame(&b, &far));
    }

    #[test]
    fn variance_is_positive() {
        let frames = synthetic_frames(10, 0.0);
        let b = fit_baseline(&frames).unwrap();
        assert!(b.variance.iter().all(|&v| v > 0.0));
    }

    #[test]
    fn baseline_fact_is_named() {
        let frames = synthetic_frames(5, 3.0);
        let b = fit_baseline(&frames).unwrap();
        let fact = baseline_fact(&b);
        assert!(fact.contains_key("mean"));
        assert!(fact.contains_key("variance"));
        assert_eq!(fact["mean"].len(), FEATURE_DIM);
    }
}
