//! Stage 1 FSE — windowed audio + classical feature extraction (d5n.1).
//!
//! Implements the front end pinned in `docs/design/phoneme-alphabet.md`:
//! - 25 ms Hamming window, 10 ms frame shift
//! - 39-dim feature vector per frame: `[c1..c12, logE, Δ(13), ΔΔ(13)]`
//!   (12 MFCC + log energy + deltas + delta-deltas)
//! - single-pass, stateless, pure scoring against the Stage 0 calibration
//!   baseline (Category D: hand-rolled FFT/MFCC, no pretrained anything)
//!
//! The ARPABET 39-phone alphabet is re-exported here so Stage 1 emission
//! (d5n.2) keys candidates on it.

pub mod boundaries;
pub mod dtw;
pub mod frontend;
pub mod gate;
pub mod nbest;

use std::f64::consts::PI;

use invariant_ppt::assert_invariant;

pub use calibration::{Baseline, FEATURE_DIM, Frame};

/// The 39-phone ARPABET alphabet (phoneme-alphabet.md §2).
pub const ARPABET: [&str; 39] = [
    // Vowels (15)
    "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW",
    // Consonants (24)
    "B", "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T",
    "TH", "V", "W", "Y", "Z", "ZH",
];

/// The ~30-word closed vocabulary for the fail-fast experiment (39c.1).
///
/// Subset of `phrase_training` transcripts: the top-30 genuine words by
/// frequency across all 3500 transcripts (deterministic, reproducible from
/// `train_phrases.tsv`; local-bettervoice-inventory.md §7). Word-level
/// ground-truth clips for these words come from the hand-vetted
/// `quality_checked_1k` set in later fail-fast tasks (39c.2+).
pub const CLOSED_VOCABULARY: [&str; 30] = [
    "impeachment",
    "methodical",
    "survival",
    "process",
    "intense",
    "reflection",
    "kinship",
    "resented",
    "congealed",
    "uncorroborated",
    "highboy",
    "hotbed",
    "ripple",
    "predestine",
    "byzantium",
    "genetic",
    "constitution",
    "interpersonal",
    "enchantment",
    "perfume",
    "submissions",
    "overboard",
    "knockdown",
    "sleepwalking",
    "unrealized",
    "panhandling",
    "horribly",
    "salivating",
    "rhubarb",
    "staked",
];

/// Window length in ms (phoneme-alphabet.md §3).
pub const WINDOW_MS: f64 = 25.0;
/// Frame shift in ms.
pub const SHIFT_MS: f64 = 10.0;
/// Number of mel filterbank channels (standard 26 for 12 MFCC).
pub const MEL_FILTERS: usize = 26;
/// Static features per frame before deltas: c1..c12 + log energy = 13.
const STATIC_DIM: usize = 13;

/// One Hamming-windowed frame of samples (pre-feature).
pub type Window = Vec<f64>;

/// Feature extraction / pipeline errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    /// No windows/frames provided.
    EmptyInput,
    /// Sample rate too low for a meaningful mel scale (< 8 kHz).
    UnsupportedSampleRate(u32),
    /// A window is empty or shorter than the FFT needs.
    InvalidWindow(usize),
}

impl std::fmt::Display for FeatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeatureError::EmptyInput => write!(f, "empty input"),
            FeatureError::UnsupportedSampleRate(r) => {
                write!(f, "unsupported sample rate {r} Hz (need >= 8000)")
            }
            FeatureError::InvalidWindow(n) => write!(f, "invalid window length {n}"),
        }
    }
}

impl std::error::Error for FeatureError {}

/// One scored frame: the feature vector plus its NLL under the baseline.
#[derive(Debug, Clone)]
pub struct ScoredFrame {
    /// Index of the frame in the clip.
    pub index: usize,
    /// The 39-dim feature vector (INV: `feature_vector_length`).
    pub features: Frame,
    /// Negative log-likelihood against the Stage 0 baseline (finite).
    pub nll: f64,
}

// ---------------------------------------------------------------------------
// Windowing
// ---------------------------------------------------------------------------

/// Split a PCM buffer into Hamming-windowed 25 ms frames at a 10 ms shift.
///
/// Returns an empty vec for empty input (no panic, per gate item 4). Window
/// and shift lengths are derived from `sample_rate`.
pub fn window_audio(samples: &[i16], sample_rate: u32) -> Vec<Window> {
    if samples.is_empty() {
        return Vec::new();
    }
    let window_len = window_len(sample_rate);
    let shift = shift_len(sample_rate);
    if samples.len() < window_len {
        return Vec::new();
    }
    let n_frames = (samples.len() - window_len) / shift + 1;
    let hamming = hamming_window(window_len);
    let mut frames = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * shift;
        let frame: Window = hamming
            .iter()
            .zip(&samples[start..start + window_len])
            .map(|(&w, &s)| w * (s as f64 / i16::MAX as f64))
            .collect();
        frames.push(frame);
    }
    frames
}

/// Window length in samples for a sample rate.
pub fn window_len(sample_rate: u32) -> usize {
    (sample_rate as f64 * WINDOW_MS / 1000.0).round() as usize
}

/// Frame shift in samples for a sample rate.
pub fn shift_len(sample_rate: u32) -> usize {
    (sample_rate as f64 * SHIFT_MS / 1000.0).round() as usize
}

/// Hamming window coefficients: `0.54 - 0.46 cos(2πi/(N-1))`.
fn hamming_window(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| 0.54 - 0.46 * (2.0 * PI * i as f64 / (n as f64 - 1.0)).cos())
        .collect()
}

// ---------------------------------------------------------------------------
// Feature extraction (classical MFCC, hand-rolled)
// ---------------------------------------------------------------------------

/// Compute the 39-dim feature vector per frame:
/// `[c1..c12, logE, Δ(c1..c12, logE), ΔΔ(c1..c12, logE)]`.
///
/// Enforces INV `feature_vector_length`: every vector is exactly 39
/// components and finite. Empty input returns `Err` (no panic).
pub fn extract_features(windows: &[Window], sample_rate: u32) -> Result<Vec<Frame>, FeatureError> {
    if sample_rate < 8_000 {
        return Err(FeatureError::UnsupportedSampleRate(sample_rate));
    }
    if windows.is_empty() {
        return Err(FeatureError::EmptyInput);
    }
    let statics = static_features(windows, sample_rate)?;
    let delta = deltas(&statics);
    let delta2 = deltas(&delta);

    let mut frames = Vec::with_capacity(statics.len());
    for (i, s) in statics.iter().enumerate() {
        let mut f = [0.0; FEATURE_DIM];
        for (out, &v) in f.iter_mut().zip(s.iter()) {
            *out = v;
        }
        for (out, &v) in f[STATIC_DIM..].iter_mut().zip(delta[i].iter()) {
            *out = v;
        }
        for (out, &v) in f[2 * STATIC_DIM..].iter_mut().zip(delta2[i].iter()) {
            *out = v;
        }
        // INV: feature_vector_length — exactly 39 finite components.
        assert_invariant(
            f.len() == FEATURE_DIM && f.iter().all(|x| x.is_finite()),
            "feature_vector_length",
            Some(&format!("fse frame {i}")),
        );
        frames.push(f);
    }
    Ok(frames)
}

/// Per-frame static features: `[c1..c12, logE]` (13 values).
fn static_features(windows: &[Window], sample_rate: u32) -> Result<Vec<Vec<f64>>, FeatureError> {
    let window_len = windows[0].len();
    if window_len == 0 {
        return Err(FeatureError::InvalidWindow(0));
    }
    let fft_size = next_pow2(window_len);
    let filters = mel_filterbank(sample_rate, fft_size);
    let dct_n = MEL_FILTERS;
    Ok(windows
        .iter()
        .map(|w| {
            let spectrum = power_spectrum(w, fft_size);
            let log_mels: Vec<f64> = filters
                .iter()
                .map(|filter| {
                    let energy: f64 = filter
                        .iter()
                        .zip(spectrum.iter())
                        .map(|(&a, &b)| a * b)
                        .sum();
                    (energy.max(1e-12)).ln()
                })
                .collect();
            let cep = dct2(&log_mels, dct_n);
            let log_energy = (w.iter().map(|x| x * x).sum::<f64>().max(1e-12)).ln();
            let mut out = Vec::with_capacity(STATIC_DIM);
            out.extend_from_slice(&cep[1..13]); // c1..c12 (drop c0)
            out.push(log_energy);
            out
        })
        .collect())
}

/// Delta features with a ±2-frame regression window, edge-replicated.
fn deltas(statics: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let t = statics.len();
    let d = statics[0].len();
    let win = 2usize;
    let denom: f64 = (1..=win).map(|k| 2.0 * (k * k) as f64).sum();
    let mut out = vec![vec![0.0; d]; t];
    for (i, row) in out.iter_mut().enumerate() {
        for j in 0..d {
            let mut num = 0.0;
            for k in 1..=win {
                let prev = i as i64 - k as i64;
                let next = i + k;
                let pv = if prev < 0 {
                    statics[0][j]
                } else {
                    statics[prev as usize][j]
                };
                let nv = if next >= t {
                    statics[t - 1][j]
                } else {
                    statics[next][j]
                };
                num += k as f64 * (nv - pv);
            }
            row[j] = num / denom;
        }
    }
    out
}

/// Power spectrum magnitude-squared of a window, zero-padded to `fft_size`.
fn power_spectrum(window: &Window, fft_size: usize) -> Vec<f64> {
    let mut buf = vec![0.0; fft_size];
    buf[..window.len()].copy_from_slice(window);
    let spectrum = fft_radix2(&buf);
    spectrum[..fft_size / 2 + 1]
        .iter()
        .map(|&(re, im)| re * re + im * im)
        .collect()
}

/// Triangular mel filterbank: `MEL_FILTERS` filters over `fft_size/2 + 1`
/// bins, center frequencies linearly spaced on the mel scale.
fn mel_filterbank(sample_rate: u32, fft_size: usize) -> Vec<Vec<f64>> {
    let nyquist = sample_rate as f64 / 2.0;
    let mel = |f: f64| 2595.0 * (1.0 + f / 700.0).log10();
    let mel_inv = |m: f64| 700.0 * (10f64.powf(m / 2595.0) - 1.0);
    let mel_max = mel(nyquist);
    let centers: Vec<f64> = (0..MEL_FILTERS + 2)
        .map(|i| mel_inv(mel_max * i as f64 / (MEL_FILTERS + 1) as f64))
        .collect();
    let n_bins = fft_size / 2 + 1;
    let mut filters = vec![vec![0.0; n_bins]; MEL_FILTERS];
    for (m, filter) in filters.iter_mut().enumerate() {
        let (f0, f1, f2) = (centers[m], centers[m + 1], centers[m + 2]);
        for (k, w) in filter.iter_mut().enumerate() {
            let f = k as f64 * sample_rate as f64 / fft_size as f64;
            *w = if f < f0 || f > f2 {
                0.0
            } else if f < f1 {
                (f - f0) / (f1 - f0)
            } else {
                (f2 - f) / (f2 - f1)
            };
        }
    }
    filters
}

/// DCT-II (orthonormal normalization), first `n_coeffs` coefficients.
fn dct2(values: &[f64], n_coeffs: usize) -> Vec<f64> {
    let n = values.len() as f64;
    (0..n_coeffs)
        .map(|k| {
            let alpha = if k == 0 {
                (1.0 / n).sqrt()
            } else {
                (2.0 / n).sqrt()
            };
            let sum: f64 = values
                .iter()
                .enumerate()
                .map(|(i, &v)| v * (PI * k as f64 * (2.0 * i as f64 + 1.0) / (2.0 * n)).cos())
                .sum();
            alpha * sum
        })
        .collect()
}

/// Iterative radix-2 FFT (pure Rust, Category D).
fn fft_radix2(input: &[f64]) -> Vec<(f64, f64)> {
    let n = input.len();
    let mut a: Vec<(f64, f64)> = input.iter().map(|&r| (r, 0.0)).collect();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let wlen = (ang.cos(), ang.sin());
        let half = len >> 1;
        for start in (0..n).step_by(len) {
            let mut w = (1.0, 0.0);
            for k in 0..half {
                let u = a[start + k];
                let v = (
                    a[start + k + half].0 * w.0 - a[start + k + half].1 * w.1,
                    a[start + k + half].0 * w.1 + a[start + k + half].1 * w.0,
                );
                a[start + k] = (u.0 + v.0, u.1 + v.1);
                a[start + k + half] = (u.0 - v.0, u.1 - v.1);
                w = (w.0 * wlen.0 - w.1 * wlen.1, w.0 * wlen.1 + w.1 * wlen.0);
            }
        }
        len <<= 1;
    }
    a
}

/// Next power of two >= n (min 2).
fn next_pow2(n: usize) -> usize {
    let mut p = 1usize;
    while p < n {
        p <<= 1;
    }
    p.max(2)
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// A per-phone prototype template in the 39-dim feature space.
#[derive(Debug, Clone)]
pub struct PhoneTemplate {
    pub phoneme: &'static str,
    pub mean: Frame,
}

/// Score a frame against every phone template: whitened affinity.
///
/// Each candidate score is `exp(-d / FEATURE_DIM)` where `d` is the squared
/// Mahalanobis-ish distance in the speaker baseline's whitened space —
/// higher = closer to the phone template, always in (0, 1]. This is the
/// classical (Category D) per-phone score feeding [`nbest::emit_nbest`].
pub fn score_phones(
    frame: &Frame,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
) -> Vec<(&'static str, f64)> {
    let std: Vec<f64> = baseline.std();
    codebook
        .iter()
        .map(|t| {
            let d: f64 = frame
                .iter()
                .zip(t.mean.iter())
                .zip(std.iter())
                .map(|((&x, &m), &s)| {
                    let z = (x - m) / s;
                    z * z
                })
                .sum();
            let score = (-d / FEATURE_DIM as f64).exp();
            (t.phoneme, score)
        })
        .collect()
}

/// Default placeholder codebook: one prototype per ARPABET phone.
///
/// Deterministic, seeded from the phoneme index — a stand-in until the
/// fail-fast closed vocabulary (39c.1) supplies real per-phone templates.
pub fn default_codebook() -> Vec<PhoneTemplate> {
    ARPABET
        .iter()
        .enumerate()
        .map(|(i, &phoneme)| {
            let mut mean = [0.0; FEATURE_DIM];
            // Distinct per-phone prototype: spread across dims, deterministic.
            for (j, x) in mean.iter_mut().enumerate() {
                *x = ((i * 7 + j * 3) % 11) as f64 / 5.0 - 1.0;
            }
            PhoneTemplate { phoneme, mean }
        })
        .collect()
}

/// Score a feature frame against the Stage 0 baseline: finite NLL.
///
/// Lower = closer to baseline (delegates to `calibration::score_frame`).
/// Enforces finite output (INV `score_finite`).
pub fn score_frame(baseline: &Baseline, frame: &Frame) -> f64 {
    let nll = calibration::score_frame(baseline, frame);
    assert_invariant(nll.is_finite(), "score_finite", Some("stage1 fse"));
    nll
}

/// Full single-pass pipeline: window -> features -> per-frame NLL.
pub fn process_clip(
    samples: &[i16],
    sample_rate: u32,
    baseline: &Baseline,
) -> Result<Vec<ScoredFrame>, FeatureError> {
    let windows = window_audio(samples, sample_rate);
    let frames = extract_features(&windows, sample_rate)?;
    Ok(frames
        .into_iter()
        .enumerate()
        .map(|(index, features)| {
            let nll = score_frame(baseline, &features);
            ScoredFrame {
                index,
                features,
                nll,
            }
        })
        .collect())
}

/// Emit N-best phoneme candidates for every frame of a clip (Stage 1 output,
/// d5n.2): `Vec<Candidate>` per frame, capped at [`nbest::MAX_N`].
pub fn emit_clip_candidates(
    samples: &[i16],
    sample_rate: u32,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
) -> Result<Vec<Vec<nbest::Candidate>>, FeatureError> {
    let windows = window_audio(samples, sample_rate);
    let frames = extract_features(&windows, sample_rate)?;
    Ok(frames
        .iter()
        .map(|f| {
            let scores = score_phones(f, baseline, codebook);
            nbest::emit_nbest(&scores)
        })
        .collect())
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
