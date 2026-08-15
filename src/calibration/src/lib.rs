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
