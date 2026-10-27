//! Real-time-factor (RTF) measurement (bead hearsay-four-fe3, RTF-1).
//!
//! Nothing records processing-time vs audio-time, so live-transcription
//! claims are fiction without this. The harness (`hearsay-rtf` bin) runs the
//! C3 streaming pipeline over single-speaker Oyez clips and reports
//! wall-clock processing seconds per audio second (mean + p95 across
//! chunks). This module is the pure math core: per-chunk ratio, mean, and
//! nearest-rank p95 over fixed chunk timings, so the unit tests below use a
//! stubbed clock (fixed `ChunkTiming`s — no wall clock, no audio, no
//! flakiness).
//!
//! Model-free, Oyez + synthetic only, offline: counting and arithmetic
//! only — no network, no STT text, no embeddings, no learned weights.

/// One chunk's wall-clock cost against the audio it consumed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkTiming {
    /// Audio seconds in the chunk (must be finite and > 0).
    pub audio_secs: f64,
    /// Wall-clock processing seconds for the chunk (must be finite, >= 0).
    pub processing_secs: f64,
}

impl ChunkTiming {
    /// Fixed-chunk fixture constructor (the stubbed-clock entry point).
    pub fn new(audio_secs: f64, processing_secs: f64) -> Self {
        Self {
            audio_secs,
            processing_secs,
        }
    }
}

/// Processing seconds per audio second for one chunk.
pub fn chunk_rtf(timing: &ChunkTiming) -> Result<f64, String> {
    if !timing.audio_secs.is_finite() || timing.audio_secs <= 0.0 {
        return Err(format!(
            "chunk audio_secs must be finite and > 0 (got {})",
            timing.audio_secs
        ));
    }
    if !timing.processing_secs.is_finite() || timing.processing_secs < 0.0 {
        return Err(format!(
            "chunk processing_secs must be finite and >= 0 (got {})",
            timing.processing_secs
        ));
    }
    Ok(timing.processing_secs / timing.audio_secs)
}

/// Mean RTF across chunks (unweighted mean of per-chunk ratios).
pub fn mean_rtf(timings: &[ChunkTiming]) -> Result<f64, String> {
    if timings.is_empty() {
        return Err("no chunk timings".to_string());
    }
    let mut sum = 0.0;
    for t in timings {
        sum += chunk_rtf(t)?;
    }
    Ok(sum / timings.len() as f64)
}

/// p95 RTF across chunks (nearest-rank: sort ascending, take rank
/// `ceil(0.95 * n)`, 1-based; a single chunk reports its own RTF).
pub fn p95_rtf(timings: &[ChunkTiming]) -> Result<f64, String> {
    if timings.is_empty() {
        return Err("no chunk timings".to_string());
    }
    let mut rtfs: Vec<f64> = timings.iter().map(chunk_rtf).collect::<Result<_, _>>()?;
    rtfs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = rtfs.len();
    let rank = ((0.95 * n as f64).ceil() as usize).max(1).min(n);
    Ok(rtfs[rank - 1])
}

/// Full RTF report over one harness run.
#[derive(Debug, Clone)]
pub struct RtfReport {
    /// Chunks timed.
    pub chunk_count: usize,
    /// Total audio seconds consumed.
    pub audio_secs: f64,
    /// Total wall-clock processing seconds across chunks.
    pub processing_secs: f64,
    /// `processing_secs / audio_secs` (audio-weighted overall factor).
    pub overall_rtf: f64,
    /// Unweighted mean of per-chunk RTFs.
    pub mean_rtf: f64,
    /// Nearest-rank p95 of per-chunk RTFs.
    pub p95_rtf: f64,
    /// Per-chunk RTFs in run order.
    pub chunk_rtfs: Vec<f64>,
}

/// Summarize fixed chunk timings into an [`RtfReport`].
pub fn summarize(timings: &[ChunkTiming]) -> Result<RtfReport, String> {
    if timings.is_empty() {
        return Err("no chunk timings".to_string());
    }
    let chunk_rtfs: Vec<f64> = timings.iter().map(chunk_rtf).collect::<Result<_, _>>()?;
    let audio_secs: f64 = timings.iter().map(|t| t.audio_secs).sum();
    let processing_secs: f64 = timings.iter().map(|t| t.processing_secs).sum();
    if !audio_secs.is_finite() || audio_secs <= 0.0 {
        return Err("total audio_secs must be finite and > 0".to_string());
    }
    Ok(RtfReport {
        chunk_count: timings.len(),
        audio_secs,
        processing_secs,
        overall_rtf: processing_secs / audio_secs,
        mean_rtf: mean_rtf(timings)?,
        p95_rtf: p95_rtf(timings)?,
        chunk_rtfs,
    })
}

#[cfg(test)]
mod rtf_tests {
    //! RTF-1 math gates: stubbed clock (fixed `ChunkTiming`s) asserting the
    //! ratio computation and the p95 selection. No wall clock, no audio.
    use super::*;

    /// Ratio computation: processing seconds per audio second, per chunk
    /// and summarized (mean + overall over fixed chunks).
    #[test]
    fn rtf_ratio_is_processing_over_audio() {
        let timings = vec![
            ChunkTiming::new(2.0, 0.5), // RTF 0.25
            ChunkTiming::new(2.0, 1.0), // RTF 0.50
        ];
        assert!((chunk_rtf(&timings[0]).unwrap() - 0.25).abs() < 1e-12);
        assert!((chunk_rtf(&timings[1]).unwrap() - 0.50).abs() < 1e-12);
        let report = summarize(&timings).unwrap();
        assert_eq!(report.chunk_count, 2);
        assert!((report.audio_secs - 4.0).abs() < 1e-12);
        assert!((report.processing_secs - 1.5).abs() < 1e-12);
        assert!((report.mean_rtf - 0.375).abs() < 1e-12);
        assert!((report.overall_rtf - 0.375).abs() < 1e-12);
    }

    /// p95 selection: nearest-rank over 20 fixed chunks picks the 19th
    /// smallest (rank `ceil(0.95 * 20) = 19`), not the max or the mean.
    #[test]
    fn rtf_p95_selects_nearest_rank() {
        // RTFs 0.05..=1.00 in steps of 0.05 (20 chunks, 2 s audio each).
        let timings: Vec<ChunkTiming> = (1..=20)
            .map(|i| ChunkTiming::new(2.0, 2.0 * i as f64 * 0.05))
            .collect();
        let p95 = p95_rtf(&timings).unwrap();
        assert!(
            (p95 - 0.95).abs() < 1e-12,
            "p95 over 1..=20 must be the 19th smallest (got {p95})"
        );
        let report = summarize(&timings).unwrap();
        assert!(
            (report.p95_rtf - 0.95).abs() < 1e-12,
            "report carries the same p95 (got {})",
            report.p95_rtf
        );
        // Mean (0.525) and max (1.00) differ: p95 is neither.
        assert!((report.mean_rtf - 0.525).abs() < 1e-12);
        assert_ne!(
            report.p95_rtf,
            report.chunk_rtfs.iter().cloned().fold(0.0, f64::max)
        );
    }

    /// Single chunk: mean, p95, and overall all equal its own ratio.
    #[test]
    fn rtf_single_chunk_reports_its_own_ratio() {
        let timings = vec![ChunkTiming::new(2.0, 0.4)];
        let report = summarize(&timings).unwrap();
        assert!((report.mean_rtf - 0.2).abs() < 1e-12);
        assert!((report.p95_rtf - 0.2).abs() < 1e-12);
        assert!((report.overall_rtf - 0.2).abs() < 1e-12);
    }

    /// Error paths: empty timings, non-positive audio, negative or
    /// non-finite processing are all `Err` (never silent NaN output).
    #[test]
    fn rtf_rejects_degenerate_timings() {
        assert!(summarize(&[]).is_err(), "empty timings is an error");
        assert!(mean_rtf(&[]).is_err(), "empty mean is an error");
        assert!(p95_rtf(&[]).is_err(), "empty p95 is an error");
        assert!(
            chunk_rtf(&ChunkTiming::new(0.0, 0.5)).is_err(),
            "zero audio_secs is an error"
        );
        assert!(
            chunk_rtf(&ChunkTiming::new(-2.0, 0.5)).is_err(),
            "negative audio_secs is an error"
        );
        assert!(
            chunk_rtf(&ChunkTiming::new(2.0, -0.1)).is_err(),
            "negative processing_secs is an error"
        );
        assert!(
            chunk_rtf(&ChunkTiming::new(2.0, f64::NAN)).is_err(),
            "NaN processing_secs is an error"
        );
        assert!(
            chunk_rtf(&ChunkTiming::new(f64::INFINITY, 0.5)).is_err(),
            "infinite audio_secs is an error"
        );
    }
}
