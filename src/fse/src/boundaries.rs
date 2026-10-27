//! SEG-1 — classical word-boundary detection from acoustic dips (bead `hearsay-four-uth`).
//!
//! Mechanism (no model, counting only): per-frame log-energy valleys (the
//! `logE` slot of the fse 39-dim `Frame`: `[c1..c12, logE, Δ(13), ΔΔ(13)]`)
//! measured against a sliding local median, UNION spectral-flux onset peaks
//! (Euclidean distance between consecutive static-cepstra slices
//! `Frame[0..13]`) measured against a multiple of the clip median flux.
//! Candidates inside one silence-hangover window collapse to a single
//! boundary at the cluster centroid, so a flat silence floor yields one
//! boundary near the gap center rather than one per floor frame; edge frames
//! are excluded. Empty input yields no boundaries.
//!
//! Time base: one frame per [`crate::SHIFT_MS`] (10 ms), so
//! `time_ms = frame as f64 * SHIFT_MS`.

use calibration::Frame;

use crate::STATIC_DIM;

/// One detected boundary: a frame index plus its millisecond timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    /// Index of the centroid frame in the input slice.
    pub frame: usize,
    /// Centroid timestamp in milliseconds (`frame as f64 * SHIFT_MS`).
    pub time_ms: f64,
}

/// Detector tuning. All thresholds are relative (medians, ratios), so no
/// absolute level calibration is needed; see `Default` for the values the
/// SEG-1 gate was recorded against.
#[derive(Debug, Clone)]
pub struct BoundaryConfig {
    /// Natural-log energy drop below the local median that counts as a
    /// valley (`3.0` ≈ 20x energy dip).
    pub dip_depth: f64,
    /// Frames each side for the sliding local-energy median.
    pub median_radius: usize,
    /// Spectral-flux multiple of the clip median flux that counts as an
    /// onset peak.
    pub onset_ratio: f64,
    /// Cluster window in frames: candidates this close merge into one
    /// boundary (silence hangover for micro-pauses).
    pub hangover_frames: usize,
    /// Frames excluded at both edges (onset/offset ramps live here).
    pub edge_margin_frames: usize,
}

impl Default for BoundaryConfig {
    fn default() -> Self {
        Self {
            dip_depth: 3.0,
            median_radius: 30,
            onset_ratio: 3.0,
            hangover_frames: 15,
            edge_margin_frames: 10,
        }
    }
}

/// Median of a scratch buffer (sorts in place; upper median for even len).
fn median_of(sorted: &mut [f64]) -> f64 {
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted[sorted.len() / 2]
}

/// Detect word-boundary candidates in per-frame features.
///
/// Pure function of `frames`: deterministic, no I/O, no model. Returns
/// boundaries in increasing frame order.
pub fn detect_boundaries(frames: &[Frame], config: &BoundaryConfig) -> Vec<Boundary> {
    let n = frames.len();
    let edge = config.edge_margin_frames.max(1);
    if n < 2 * edge + 3 {
        return Vec::new();
    }

    // Per-frame log energy (the logE slot) and static-cepstra spectral flux.
    let energy: Vec<f64> = frames.iter().map(|f| f[STATIC_DIM - 1]).collect();
    let mut flux = vec![0.0f64; n];
    for (i, pair) in frames.windows(2).enumerate() {
        let sum: f64 = pair[0]
            .iter()
            .zip(pair[1].iter())
            .take(STATIC_DIM)
            .map(|(a, b)| (b - a) * (b - a))
            .sum();
        flux[i + 1] = sum.sqrt();
    }

    // Sliding local-energy median; global flux median.
    let mut local_med = vec![0.0f64; n];
    let mut scratch = Vec::with_capacity(2 * config.median_radius + 1);
    for (i, slot) in local_med.iter_mut().enumerate() {
        let lo = i.saturating_sub(config.median_radius);
        let hi = (i + config.median_radius + 1).min(n);
        scratch.clear();
        scratch.extend_from_slice(&energy[lo..hi]);
        *slot = median_of(&mut scratch);
    }
    let mut flux_scratch = flux.clone();
    let flux_med = median_of(&mut flux_scratch).max(f64::MIN_POSITIVE);

    // Candidate frames: energy valley deep enough, or flux onset peak high
    // enough. Non-strict local extrema so flat silence floors still seed
    // candidates (the depth gate rejects flat non-silence).
    let mut candidates: Vec<usize> = Vec::new();
    for i in edge..(n - edge) {
        let valley = energy[i] <= energy[i - 1]
            && energy[i] <= energy[i + 1]
            && local_med[i] - energy[i] >= config.dip_depth;
        let peak = flux[i] >= flux[i - 1]
            && flux[i] >= flux[i + 1]
            && flux[i] >= config.onset_ratio * flux_med;
        if valley || peak {
            candidates.push(i);
        }
    }

    // Hangover clustering: maximal runs linked by gaps <= hangover_frames
    // collapse to one boundary at the run centroid (rounded to a frame).
    let mut boundaries = Vec::new();
    let mut run_start = 0usize;
    while run_start < candidates.len() {
        let mut run_end = run_start;
        while run_end + 1 < candidates.len()
            && candidates[run_end + 1] - candidates[run_end] <= config.hangover_frames
        {
            run_end += 1;
        }
        let run = &candidates[run_start..=run_end];
        let centroid = run.iter().sum::<usize>() as f64 / run.len() as f64;
        let frame = centroid.round() as usize;
        boundaries.push(Boundary {
            frame,
            time_ms: frame as f64 * crate::SHIFT_MS,
        });
        run_start = run_end + 1;
    }
    boundaries
}
