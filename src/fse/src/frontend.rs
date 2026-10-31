//! A1 tunable MFCC front end (bead hearsay-four-b7p).
//! Knobs tuned on DEV only: pre-emphasis, liftering, CMN, deltas, band.

use crate::{FeatureError, MEL_FILTERS, shift_len, window_len};
use std::f64::consts::PI;

#[derive(Debug, Clone, PartialEq)]
pub struct FrontendConfig {
    pub pre_emphasis: f64,
    pub lifter: f64,
    pub cmn: bool,
    pub cmvn: bool,
    pub use_deltas: bool,
    pub use_delta_deltas: bool,
    pub mel_low_hz: f64,
    pub mel_high_hz: Option<f64>,
    pub num_mfcc: usize,
}
impl Default for FrontendConfig {
    fn default() -> Self {
        Self {
            pre_emphasis: 0.97,
            lifter: 22.0,
            cmn: true,
            cmvn: false,
            use_deltas: true,
            use_delta_deltas: true,
            mel_low_hz: 0.0,
            mel_high_hz: None,
            num_mfcc: 12,
        }
    }
}
impl FrontendConfig {
    pub fn dim(&self) -> usize {
        let s = self.num_mfcc + 1;
        s * (1 + self.use_deltas as usize + self.use_delta_deltas as usize)
    }
    /// Small DEV tuning grid named in the bead (front end tuned here only).
    pub fn tuning_grid() -> Vec<(String, Self)> {
        let base = Self::default();
        vec![
            ("base_cmn_d_dd_lifter22_pre097".into(), base.clone()),
            (
                "no_cmn".into(),
                Self {
                    cmn: false,
                    ..base.clone()
                },
            ),
            (
                "no_deltas".into(),
                Self {
                    use_deltas: false,
                    use_delta_deltas: false,
                    ..base.clone()
                },
            ),
            (
                "deltas_only".into(),
                Self {
                    use_delta_deltas: false,
                    ..base.clone()
                },
            ),
            (
                "no_preemphasis".into(),
                Self {
                    pre_emphasis: 0.0,
                    ..base.clone()
                },
            ),
            (
                "no_lifter".into(),
                Self {
                    lifter: 0.0,
                    ..base.clone()
                },
            ),
            (
                "band_300_3400".into(),
                Self {
                    mel_low_hz: 300.0,
                    mel_high_hz: Some(3400.0),
                    ..base.clone()
                },
            ),
            (
                "band_100_4000_no_cmn".into(),
                Self {
                    mel_low_hz: 100.0,
                    mel_high_hz: Some(4000.0),
                    cmn: false,
                    ..base
                },
            ),
        ]
    }
}

/// Endpoint-trimming constants: relative threshold factor, absolute noise
/// floor (normalized RMS units), hangover frames kept each side, and the
/// support-rule neighbor radius (frames).
const TRIM_K: f64 = 0.01;
const TRIM_FLOOR: f64 = 0.001;
const TRIM_HANGOVER: usize = 5;
const TRIM_RADIUS: usize = 2;

/// Speech span over per-frame RMS energies: inclusive-start, exclusive-end
/// frame indices covering the above-threshold region plus hangover each
/// side (clamped to bounds). `None` when no frame exceeds the threshold.
pub fn speech_span(energies: &[f64]) -> Option<(usize, usize)> {
    speech_span_with(energies, TRIM_RADIUS)
}

/// Speech span with explicit support radius: a frame counts only with an
/// above-threshold neighbor within `radius` frames, so isolated transient
/// blips in silence die while speech edges survive. `speech_span` fixes
/// `radius = TRIM_RADIUS`; the sensitivity tests pin radii 1..=3.
pub fn speech_span_with(energies: &[f64], radius: usize) -> Option<(usize, usize)> {
    if energies.is_empty() {
        return None;
    }
    let peak = energies.iter().cloned().fold(0.0f64, f64::max);
    let thr = TRIM_FLOOR.max(TRIM_K * peak);
    // Support rule: an above-threshold neighbor within `radius` frames.
    let n = energies.len();
    let supported = |i: usize| {
        if energies[i] <= thr {
            return false;
        }
        let lo = i.saturating_sub(radius);
        let hi = (i + radius).min(n - 1);
        (lo..=hi).any(|j| j != i && energies[j] > thr)
    };
    let first = (0..n).find(|&i| supported(i))?;
    let last = (0..n).rfind(|&i| supported(i))?;
    let start = first.saturating_sub(TRIM_HANGOVER);
    let end = (last + 1 + TRIM_HANGOVER).min(n);
    Some((start, end))
}
pub fn extract_with_config(
    samples: &[i16],
    sample_rate: u32,
    cfg: &FrontendConfig,
) -> Result<Vec<Vec<f64>>, FeatureError> {
    extract_inner(samples, sample_rate, cfg, true)
}

/// Untrimmed extraction: identical pipeline without endpoint trimming.
/// Test-only seam for the padding-fingerprint experiment; production
/// always trims. Zero behavior delta on the trimmed path.
pub fn extract_untrimmed_with_config(
    samples: &[i16],
    sample_rate: u32,
    cfg: &FrontendConfig,
) -> Result<Vec<Vec<f64>>, FeatureError> {
    extract_inner(samples, sample_rate, cfg, false)
}

fn extract_inner(
    samples: &[i16],
    sample_rate: u32,
    cfg: &FrontendConfig,
    trim: bool,
) -> Result<Vec<Vec<f64>>, FeatureError> {
    if sample_rate < 8_000 {
        return Err(FeatureError::UnsupportedSampleRate(sample_rate));
    }
    if samples.is_empty() {
        return Err(FeatureError::EmptyInput);
    }
    // pre-emphasis on f64 samples
    let mut xs: Vec<f64> = Vec::with_capacity(samples.len());
    let mut prev = 0.0f64;
    for &s in samples {
        let x = s as f64 / i16::MAX as f64;
        xs.push(x - cfg.pre_emphasis * prev);
        prev = x;
    }
    let wl = window_len(sample_rate);
    let sh = shift_len(sample_rate);
    if xs.len() < wl {
        return Err(FeatureError::EmptyInput);
    }
    let ham: Vec<f64> = (0..wl)
        .map(|i| 0.54 - 0.46 * (2.0 * PI * i as f64 / (wl as f64 - 1.0)).cos())
        .collect();
    let n = (xs.len() - wl) / sh + 1;
    let all_windows: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let st = i * sh;
            ham.iter()
                .zip(&xs[st..st + wl])
                .map(|(w, x)| w * x)
                .collect()
        })
        .collect();
    let windows: &[Vec<f64>] = if trim {
        // Endpoint trimming on raw-sample frame energies: config-independent
        // spans, and pre-emphasis would only boost hiss for the threshold.
        let raw: Vec<f64> = samples
            .iter()
            .map(|&s| s as f64 / i16::MAX as f64)
            .collect();
        let energies: Vec<f64> = (0..n)
            .map(|i| {
                let st = i * sh;
                let sum: f64 = raw[st..st + wl].iter().map(|x| x * x).sum::<f64>();
                (sum / wl as f64).sqrt()
            })
            .collect();
        let (ts, te) = speech_span(&energies).ok_or(FeatureError::EmptyInput)?;
        &all_windows[ts..te]
    } else {
        &all_windows[..]
    };
    let mut statics = static_features(windows, sample_rate, cfg)?;
    if cfg.cmvn && !statics.is_empty() {
        // V3: CMVN replaces CMN at the same pipeline point (population std,
        // floored; deltas below are computed from these normalized statics).
        let d = statics[0].len();
        let t = statics.len() as f64;
        for j in 0..d {
            let m: f64 = statics.iter().map(|r| r[j]).sum::<f64>() / t;
            let v: f64 = statics.iter().map(|r| (r[j] - m) * (r[j] - m)).sum::<f64>() / t;
            let s = v.sqrt().max(1e-6);
            for r in statics.iter_mut() {
                r[j] = (r[j] - m) / s;
            }
        }
    } else if cfg.cmn && !statics.is_empty() {
        let d = statics[0].len();
        let t = statics.len() as f64;
        for j in 0..d {
            let m: f64 = statics.iter().map(|r| r[j]).sum::<f64>() / t;
            for r in statics.iter_mut() {
                r[j] -= m;
            }
        }
    }
    let mut out: Vec<Vec<f64>> = statics.clone();
    if cfg.use_deltas {
        let d = deltas(&statics);
        for (o, dd) in out.iter_mut().zip(&d) {
            o.extend_from_slice(dd);
        }
    }
    if cfg.use_delta_deltas {
        let d1 = deltas(&statics);
        let d2 = deltas(&d1);
        for (o, dd) in out.iter_mut().zip(&d2) {
            o.extend_from_slice(dd);
        }
    }
    if out.iter().any(|r| r.iter().any(|x| !x.is_finite())) {
        return Err(FeatureError::InvalidWindow(0));
    }
    Ok(out)
}

fn static_features(
    windows: &[Vec<f64>],
    sample_rate: u32,
    cfg: &FrontendConfig,
) -> Result<Vec<Vec<f64>>, FeatureError> {
    let wl = windows[0].len();
    if wl == 0 {
        return Err(FeatureError::InvalidWindow(0));
    }
    let fft_size = next_pow2(wl);
    let filters = mel_filterbank(sample_rate, fft_size, cfg.mel_low_hz, cfg.mel_high_hz);
    Ok(windows
        .iter()
        .map(|w| {
            let spec = power_spectrum(w, fft_size);
            let log_mels: Vec<f64> = filters
                .iter()
                .map(|f| {
                    (f.iter()
                        .zip(&spec)
                        .map(|(a, b)| a * b)
                        .sum::<f64>()
                        .max(1e-12))
                    .ln()
                })
                .collect();
            let mut cep = dct2(&log_mels, MEL_FILTERS);
            if cfg.lifter > 0.0 {
                for (k, c) in cep.iter_mut().enumerate() {
                    *c *= 1.0 + (cfg.lifter / 2.0) * (PI * k as f64 / cfg.lifter).sin();
                }
            }
            let log_e = (w.iter().map(|x| x * x).sum::<f64>().max(1e-12)).ln();
            let mut o: Vec<f64> = cep[1..1 + cfg.num_mfcc].to_vec();
            o.push(log_e);
            o
        })
        .collect())
}
fn deltas(s: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let (t, d) = (s.len(), s[0].len());
    let denom: f64 = (1..=2).map(|k| 2.0 * (k * k) as f64).sum();
    (0..t)
        .map(|i| {
            (0..d)
                .map(|j| {
                    let mut num = 0.0;
                    for k in 1..=2isize {
                        let p = s[(i as isize - k).clamp(0, t as isize - 1) as usize][j];
                        let n = s[(i as isize + k).clamp(0, t as isize - 1) as usize][j];
                        num += k as f64 * (n - p);
                    }
                    num / denom
                })
                .collect()
        })
        .collect()
}
fn power_spectrum(w: &[f64], fft_size: usize) -> Vec<f64> {
    let mut b = vec![0.0; fft_size];
    b[..w.len()].copy_from_slice(w);
    fft(&b)
        .iter()
        .take(fft_size / 2 + 1)
        .map(|(r, i)| r * r + i * i)
        .collect()
}
fn mel_filterbank(sr: u32, fft_size: usize, low: f64, high: Option<f64>) -> Vec<Vec<f64>> {
    let hi = high.unwrap_or(sr as f64 / 2.0).min(sr as f64 / 2.0);
    let lo = low.min(hi);
    let mel = |f: f64| 2595.0 * (1.0 + f / 700.0).log10();
    let inv = |m: f64| 700.0 * (10f64.powf(m / 2595.0) - 1.0);
    let (mlo, mhi) = (mel(lo), mel(hi));
    let centers: Vec<f64> = (0..MEL_FILTERS + 2)
        .map(|i| inv(mlo + (mhi - mlo) * i as f64 / (MEL_FILTERS + 1) as f64))
        .collect();
    let nb = fft_size / 2 + 1;
    let mut fs = vec![vec![0.0; nb]; MEL_FILTERS];
    for (m, f) in fs.iter_mut().enumerate() {
        let (f0, f1, f2) = (centers[m], centers[m + 1], centers[m + 2]);
        for (k, w) in f.iter_mut().enumerate() {
            let fr = k as f64 * sr as f64 / fft_size as f64;
            *w = if fr < f0 || fr > f2 {
                0.0
            } else if fr < f1 {
                (fr - f0) / (f1 - f0)
            } else {
                (f2 - fr) / (f2 - f1)
            };
        }
    }
    fs
}
fn dct2(v: &[f64], n: usize) -> Vec<f64> {
    let nn = v.len() as f64;
    (0..n)
        .map(|k| {
            let a = if k == 0 {
                (1.0 / nn).sqrt()
            } else {
                (2.0 / nn).sqrt()
            };
            a * v
                .iter()
                .enumerate()
                .map(|(i, &x)| x * (PI * k as f64 * (2.0 * i as f64 + 1.0) / (2.0 * nn)).cos())
                .sum::<f64>()
        })
        .collect()
}
fn fft(input: &[f64]) -> Vec<(f64, f64)> {
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
        let (wr, wi) = (ang.cos(), ang.sin());
        let half = len >> 1;
        for st in (0..n).step_by(len) {
            let (mut w0, mut w1) = (1.0, 0.0);
            for k in 0..half {
                let u = a[st + k];
                let v = (
                    a[st + k + half].0 * w0 - a[st + k + half].1 * w1,
                    a[st + k + half].0 * w1 + a[st + k + half].1 * w0,
                );
                a[st + k] = (u.0 + v.0, u.1 + v.1);
                a[st + k + half] = (u.0 - v.0, u.1 - v.1);
                let t = w0 * wr - w1 * wi;
                w1 = w0 * wi + w1 * wr;
                w0 = t;
            }
        }
        len <<= 1;
    }
    a
}
fn next_pow2(n: usize) -> usize {
    let mut p = 1;
    while p < n {
        p <<= 1;
    }
    p.max(2)
}
