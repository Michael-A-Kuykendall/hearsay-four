//! Fail-fast experiment conditions (39c): Condition A control.
//!
//! Condition A (control): every utterance scored **fresh** against the
//! generic Stage 0 baseline with NO cross-utterance memory — stateless, no
//! accent-signature carry-forward (failfast-separation-criterion.md §2).
//!
//! [`ConditionA`] holds only the immutable baseline; scoring is a pure
//! function of `(baseline, frames)` so no per-utterance state can leak.

pub mod derive;
pub mod drift;
pub mod feedback;
pub mod measure;
pub mod window_feedback;

use invariant_ppt::assert_invariant;

pub use calibration::{Baseline, FEATURE_DIM, Frame};
pub use measure::SessionRow;

/// Session data root: `SESSION_ROOT` env override, else the repo default.
pub const SESSION_ROOT_ENV: &str = "SESSION_ROOT";
pub const DEFAULT_SESSION_ROOT: &str = "/home/michael/repos/hearsay-four/data/session";
/// Session audio file, relative to the session root.
pub const SESSION_AUDIO: &str = "audio/24-889_20260429-argument.mp3";

/// Condition-A scoring errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreError {
    /// No frames to score.
    EmptyUtterance,
    /// Stop before start, or out of range.
    InvalidSlice,
}

impl std::fmt::Display for ScoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreError::EmptyUtterance => write!(f, "utterance has no frames"),
            ScoreError::InvalidSlice => write!(f, "invalid slice bounds"),
        }
    }
}

impl std::error::Error for ScoreError {}

/// Condition A: fresh, stateless per-utterance scoring.
///
/// The only state is the generic reference baseline (immutable). Scoring an
/// utterance cannot affect scoring any other utterance.
#[derive(Debug, Clone)]
pub struct ConditionA {
    baseline: Baseline,
}

impl ConditionA {
    pub fn new(baseline: Baseline) -> Self {
        ConditionA { baseline }
    }

    pub fn baseline(&self) -> &Baseline {
        &self.baseline
    }

    /// Score one utterance: mean NLL of its frames under the generic
    /// baseline. Pure — no memory of any other utterance (INV:
    /// `condition_a_stateless`).
    pub fn score_utterance(&self, frames: &[Frame]) -> Result<f64, ScoreError> {
        if frames.is_empty() {
            return Err(ScoreError::EmptyUtterance);
        }
        let total: f64 = frames
            .iter()
            .map(|f| fse::score_frame(&self.baseline, f))
            .sum();
        let mean = total / frames.len() as f64;
        assert_invariant(
            mean.is_finite(),
            "condition_a_stateless",
            Some("39c condition A"),
        );
        Ok(mean)
    }
}
/// Condition B: treatment with accent-signature carry-forward.
///
/// Contrasts with [`ConditionA`] (fresh, stateless) by persisting a
/// per-feature accent signature that is updated utterance-over-utterance via
/// a bounded exponential moving average (accent-feedback-semantics.md §3)
/// and fed back into scoring as a bias toward the speaker's realized
/// displacement (accent-feedback-semantics.md §4).
///
/// `adjusted_mean_i = baseline_mean_i + β · Δ_i`, and
/// `Δ_i ← (1 − α)·Δ_i_prev + α·δ_i(new)`, with α = 0.2, β = 1.0 preregistered.
#[derive(Debug, Clone)]
pub struct ConditionB {
    /// The immutable canonical reference this treatment adapts away from.
    baseline: Baseline,
    /// Per-feature-displacement accent signature (Δ), length `FEATURE_DIM`.
    signature: Vec<f64>,
    /// EMA learning rate (preregistered; default 0.2).
    alpha: f64,
    /// Feedback gain into scoring (preregistered; default 1.0).
    beta: f64,
    /// Number of utterances consumed so far (carry-forward evidence).
    utterance_count: usize,
}

/// Preregistered EMA learning rate (`accent-feedback-semantics.md` §3).
pub const ACCENT_EMA_ALPHA: f64 = 0.2;
/// Preregistered feedback gain (`accent-feedback-semantics.md` §4).
pub const ACCENT_FEEDBACK_BETA: f64 = 1.0;

impl ConditionB {
    /// A fresh treatment scorer: zero signature, default preregistered
    /// learning rate and feedback gain.
    pub fn new(baseline: Baseline) -> Self {
        ConditionB {
            baseline,
            signature: vec![0.0; FEATURE_DIM],
            alpha: ACCENT_EMA_ALPHA,
            beta: ACCENT_FEEDBACK_BETA,
            utterance_count: 0,
        }
    }

    /// Construct with explicit learning rate and feedback gain (for sweeps).
    pub fn with_rates(baseline: Baseline, alpha: f64, beta: f64) -> Result<Self, ScoreError> {
        if alpha <= 0.0 || alpha > 1.0 || beta <= 0.0 {
            return Err(ScoreError::InvalidSlice);
        }
        Ok(ConditionB {
            baseline,
            signature: vec![0.0; FEATURE_DIM],
            alpha,
            beta,
            utterance_count: 0,
        })
    }

    pub fn baseline(&self) -> &Baseline {
        &self.baseline
    }

    /// The current accent-signature displacement, `Δ` (length `FEATURE_DIM`).
    pub fn signature(&self) -> &[f64] {
        &self.signature
    }

    /// Number of utterances scored so far.
    pub fn utterance_count(&self) -> usize {
        self.utterance_count
    }

    /// Score one utterance under B.
    ///
    /// Steps (INV `condition_b_carry_forward`):
    /// 1. Build the adjusted baseline: `mean_i + beta·signature_i`.
    /// 2. Score the utterance against the adjusted baseline (this is the
    ///    feedback — the signature biases Stage 1 toward the speaker).
    /// 3. Measure this utterance's displacement from the canonical mean.
    /// 4. EMA-update the signature (bounded: α∈(0,1], finite inputs).
    pub fn score_utterance(&mut self, frames: &[Frame]) -> Result<f64, ScoreError> {
        if frames.is_empty() {
            return Err(ScoreError::EmptyUtterance);
        }
        let adjusted = self.adjusted_baseline();
        let total: f64 = frames.iter().map(|f| fse::score_frame(&adjusted, f)).sum();
        let score = total / frames.len() as f64;
        assert_invariant(score.is_finite(), "condition_b_score_finite", Some("39c"));
        assert_invariant(
            self.signature.len() == FEATURE_DIM,
            "condition_b_signature_dim",
            Some("39B"),
        );

        let delta = self.derive_displacement(frames);
        // Bounded EMA update (accent-feedback-semantics.md §3).
        let alpha = self.alpha;
        for (sig, &d) in self.signature.iter_mut().zip(delta.iter()) {
            *sig = (1.0 - alpha) * *sig + alpha * d;
        }
        self.utterance_count += 1;
        assert_invariant(
            self.signature.iter().all(|v| v.is_finite()),
            "condition_b_signature_bounded",
            Some("39B"),
        );
        Ok(score)
    }

    /// The canonical baseline with the accent-signature feedback bias applied.
    fn adjusted_baseline(&self) -> Baseline {
        let mut adjusted = self.baseline.clone();
        for (m, &sig) in adjusted.mean.iter_mut().zip(self.signature.iter()) {
            *m += self.beta * sig;
        }
        adjusted
    }

    /// Measured per-feature displacement of this utterance from the canonical
    /// reference mean (before any bias): `mean(frames) − baseline.mean`.
    fn derive_displacement(&self, frames: &[Frame]) -> Vec<f64> {
        let n = frames.len() as f64;
        let mut mean = vec![0.0; FEATURE_DIM];
        for f in frames {
            for (acc, &x) in mean.iter_mut().zip(f.iter()) {
                *acc += x;
            }
        }
        for (acc, &m) in mean.iter_mut().zip(self.baseline.mean.iter()) {
            *acc = *acc / n - m;
        }
        mean
    }
}

/// Slice a decoded PCM buffer to `[start_sec, stop_sec)` at its sample rate.
pub fn slice_pcm(
    pcm: &[i16],
    sample_rate: u32,
    start_sec: f64,
    stop_sec: f64,
) -> Result<Vec<i16>, ScoreError> {
    if stop_sec <= start_sec || start_sec < 0.0 {
        return Err(ScoreError::InvalidSlice);
    }
    let start = (start_sec * sample_rate as f64) as usize;
    let stop = (stop_sec * sample_rate as f64) as usize;
    if start >= pcm.len() {
        return Err(ScoreError::InvalidSlice);
    }
    Ok(pcm[start..stop.min(pcm.len())].to_vec())
}

/// Build a speaker baseline from enrollment PCM via the same front-end path
/// as scoring (window → extract → fit). Powers the `--enroll` runs (bead
/// fo7); the generic reference stays the default everywhere else.
pub fn baseline_from_frames(
    pcm: &[i16],
    sample_rate: u32,
) -> Result<calibration::Baseline, String> {
    if pcm.is_empty() {
        return Err("no enrollment samples".to_string());
    }
    let windows = fse::window_audio(pcm, sample_rate);
    let frames = fse::extract_features(&windows, sample_rate)
        .map_err(|e| format!("enrollment feature extraction failed: {e:?}"))?;
    calibration::fit_baseline(&frames).ok_or_else(|| "enrollment produced no frames".to_string())
}

/// Decode the session audio (full clip) into 16 kHz mono PCM.
pub fn decode_session_audio() -> Result<corpus::audio::Pcm16kAudio, String> {
    let root = std::env::var(SESSION_ROOT_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(DEFAULT_SESSION_ROOT));
    corpus::audio::decode_to_pcm16k(&root.join(SESSION_AUDIO))
}

/// Turn a session row's PCM slice into scored Condition-A result.
pub fn score_session_utterance(
    condition: &ConditionA,
    pcm: &[i16],
    sample_rate: u32,
    start_sec: f64,
    stop_sec: f64,
) -> Result<(Vec<Frame>, f64), ScoreError> {
    let sliced = slice_pcm(pcm, sample_rate, start_sec, stop_sec)?;
    let windows = fse::window_audio(&sliced, sample_rate);
    let frames =
        fse::extract_features(&windows, sample_rate).map_err(|_| ScoreError::EmptyUtterance)?;
    let score = condition.score_utterance(&frames)?;
    Ok((frames, score))
}

/// Turn a session row's PCM slice into scored Condition-B result (same audio
/// slicing as A; only the scorer differs — INV `measurement_symmetry`, the
/// same 16 kHz decode + window + feature extraction is shared).
pub fn score_session_utterance_b(
    condition: &mut ConditionB,
    pcm: &[i16],
    sample_rate: u32,
    start_sec: f64,
    stop_sec: f64,
) -> Result<(Vec<Frame>, f64), ScoreError> {
    let sliced = slice_pcm(pcm, sample_rate, start_sec, stop_sec)?;
    let windows = fse::window_audio(&sliced, sample_rate);
    let frames =
        fse::extract_features(&windows, sample_rate).map_err(|_| ScoreError::EmptyUtterance)?;
    let score = condition.score_utterance(&frames)?;
    Ok((frames, score))
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
