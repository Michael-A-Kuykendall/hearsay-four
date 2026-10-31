//! Energy/spectral voice activity detection with hangover (bead
//! hearsay-four-6tc, L6 streaming + voice activity endpointing).
//!
//! Untrained rule-based detector over canonical 16 kHz mono PCM: each 10 ms
//! frame passes a log-energy gate plus a zero-crossing gate (the spectral
//! half — voiced and unvoiced speech both cross regularly, digital silence
//! never does, flat DC rarely does). A hangover bridges micro-pauses so one
//! utterance stays one region; endpoint closure fires when silence outlasts
//! the hangover, and a maximum region length force-splits runaway speech at
//! its quietest recent frame. SEG-1 `fse::boundaries` output is accepted as
//! an optional refinement pass (snap/split within tolerance); segmentation
//! itself is not rebuilt here.

use std::collections::VecDeque;

use fse::boundaries::{Boundary, BoundaryConfig, detect_boundaries};

/// Canonical sample rate this VAD accepts (16 kHz mono PCM).
pub const SAMPLE_RATE_HZ: u32 = 16_000;
/// VAD frame length in milliseconds (matches the 10 ms decoder frame clock).
pub const FRAME_MS: u64 = 10;
/// Samples per VAD frame at [`SAMPLE_RATE_HZ`].
pub const SAMPLES_PER_FRAME: usize = 160;
/// Recent-frame window (in frames) searched for the quietest split point on
/// a forced max-length split (50 frames = 500 ms).
const SPLIT_SEARCH_FRAMES: usize = 50;

/// Tuning for [`Segmenter`]. All defaults are documented on the fields.
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Per-frame log-energy gate in dBFS (20*log10 RMS). Frames at or above
    /// this are speech candidates. Default -40.0.
    pub energy_threshold_db: f64,
    /// Zero-crossing-rate floor (fraction of adjacent sample pairs that
    /// change sign). Rejects digital silence (0.0) and flat DC.
    /// Default 0.01.
    pub zcr_min: f64,
    /// Zero-crossing-rate ceiling. Rejects harsh wideband noise while
    /// admitting voiced (~0.05) and unvoiced (~0.3) speech. Default 0.6.
    pub zcr_max: f64,
    /// Frames of trailing silence tolerated before a region closes
    /// (300 ms default = 30 frames). Gaps at or under this merge.
    pub hangover_frames: usize,
    /// Minimum raw-speech span of a kept region (100 ms default = 10
    /// frames). Shorter blips are dropped as noise.
    pub min_speech_frames: usize,
    /// Maximum region length in seconds before a forced split
    /// (default 4.0). Bounds the decoder's per-region streaming buffer,
    /// which bounds the v0 re-decode cost and the worst per-chunk latency.
    pub max_region_secs: f64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            energy_threshold_db: -40.0,
            zcr_min: 0.01,
            zcr_max: 0.6,
            hangover_frames: 30,
            min_speech_frames: 10,
            max_region_secs: 4.0,
        }
    }
}

impl VadConfig {
    /// Maximum region length in frames (rounded up, minimum 1).
    pub fn max_region_frames(&self) -> usize {
        ((self.max_region_secs * 1000.0 / FRAME_MS as f64).ceil() as usize).max(1)
    }
}

/// One closed speech region, in frames, samples, and milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechSegment {
    /// First frame of the region (inclusive).
    pub frame_start: usize,
    /// One past the last frame of the region (exclusive).
    pub frame_end: usize,
    /// First sample (inclusive) at 16 kHz.
    pub start_sample: usize,
    /// One past the last sample (exclusive).
    pub end_sample: usize,
    /// Start timestamp in milliseconds.
    pub start_ms: u64,
    /// End timestamp in milliseconds.
    pub end_ms: u64,
}

impl SpeechSegment {
    fn from_frame_span(frame_start: usize, frame_end: usize) -> Self {
        Self {
            frame_start,
            frame_end,
            start_sample: frame_start * SAMPLES_PER_FRAME,
            end_sample: frame_end * SAMPLES_PER_FRAME,
            start_ms: frame_start as u64 * FRAME_MS,
            end_ms: frame_end as u64 * FRAME_MS,
        }
    }

    /// Span length in frames.
    pub fn len_frames(&self) -> usize {
        self.frame_end.saturating_sub(self.frame_start)
    }

    /// True when the span is empty.
    pub fn is_empty(&self) -> bool {
        self.frame_end <= self.frame_start
    }
}

/// Log-energy of one frame in dBFS (20*log10 of the RMS over full scale).
/// An all-zero (or empty) frame yields `f64::NEG_INFINITY`.
pub fn frame_energy_db(frame: &[i16]) -> f64 {
    if frame.is_empty() {
        return f64::NEG_INFINITY;
    }
    let sum_sq: f64 = frame.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_sq / frame.len() as f64).sqrt();
    if rms <= 0.0 {
        return f64::NEG_INFINITY;
    }
    20.0 * (rms / f64::from(i16::MAX)).log10()
}

/// Zero-crossing rate of one frame: fraction of adjacent sample pairs that
/// change sign (zero counts as non-negative). Empty or single-sample frames
/// yield 0.0.
pub fn zero_crossing_rate(frame: &[i16]) -> f64 {
    if frame.len() < 2 {
        return 0.0;
    }
    let mut crossings = 0usize;
    for pair in frame.windows(2) {
        let a = pair[0] >= 0;
        let b = pair[1] >= 0;
        if a != b {
            crossings += 1;
        }
    }
    crossings as f64 / (frame.len() - 1) as f64
}

/// Raw (pre-hangover) speech gate for one frame's features.
fn raw_speech(energy_db: f64, zcr: f64, config: &VadConfig) -> bool {
    energy_db >= config.energy_threshold_db && zcr >= config.zcr_min && zcr <= config.zcr_max
}

/// Stateful frame-level VAD with hangover. Feed 160-sample frames in stream
/// order; frame boundaries stay aligned to stream start so file and live
/// chunkings decide identically.
#[derive(Debug)]
pub struct Segmenter {
    config: VadConfig,
    frame_index: usize,
    hangover_remaining: usize,
    open_start: Option<usize>,
    last_speech_frame: usize,
    /// Recent per-frame energies (oldest first) for quietest-point
    /// max-length splits.
    recent_energy: VecDeque<(usize, f64)>,
}

impl Segmenter {
    /// New segmenter over the given tuning.
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            frame_index: 0,
            hangover_remaining: 0,
            open_start: None,
            last_speech_frame: 0,
            recent_energy: VecDeque::new(),
        }
    }

    /// True while a region is open (speech or hangover silence).
    pub fn is_open(&self) -> bool {
        self.open_start.is_some()
    }

    /// Number of frames consumed so far.
    pub fn frames_seen(&self) -> usize {
        self.frame_index
    }

    /// Feed one [`SAMPLES_PER_FRAME`]-sample frame. Returns regions closed
    /// by this frame (at most one). Any other frame length is an error —
    /// split into 160-sample frames first so decisions stay aligned.
    pub fn feed_frame(&mut self, frame: &[i16]) -> Result<Vec<SpeechSegment>, String> {
        if frame.len() != SAMPLES_PER_FRAME {
            return Err(format!(
                "VAD frame must hold {SAMPLES_PER_FRAME} samples, got {}",
                frame.len()
            ));
        }
        let i = self.frame_index;
        self.frame_index += 1;
        let energy = frame_energy_db(frame);
        let zcr = zero_crossing_rate(frame);
        let speech = raw_speech(energy, zcr, &self.config);
        self.recent_energy.push_back((i, energy));
        while self.recent_energy.len() > SPLIT_SEARCH_FRAMES {
            self.recent_energy.pop_front();
        }

        // Endpoint bookkeeping: speech re-arms the hangover; silence burns
        // one frame of it; silence with nothing left exhausts the region.
        let mut exhausted = false;
        if speech {
            if self.open_start.is_none() {
                self.open_start = Some(i);
            }
            self.last_speech_frame = i;
            self.hangover_remaining = self.config.hangover_frames;
        } else if self.hangover_remaining > 0 {
            self.hangover_remaining -= 1;
        } else {
            exhausted = true;
        }

        let mut closed = Vec::new();
        // Endpoint: silence outlasted the hangover. Gaps at or under the
        // hangover merged above (exhaustion fires on the frame after the
        // hangover hits zero); idle silence has no open region to close.
        if exhausted && let Some(start) = self.open_start.take() {
            let end = self.last_speech_frame + 1;
            if end > start && end - start >= self.config.min_speech_frames {
                closed.push(SpeechSegment::from_frame_span(start, end));
            }
        }
        // Forced split: the open region hit the maximum length.
        if let Some(start) = self.open_start
            && i + 1 - start >= self.config.max_region_frames()
        {
            let split = self.quietest_split_point(start, i + 1);
            if split > start {
                closed.push(SpeechSegment::from_frame_span(start, split));
                // The tail stays open from the split point.
                self.open_start = Some(split);
                // Keep the hangover alive so the tail does not close on
                // the next silence frame before fresh speech lands.
                self.hangover_remaining = self.config.hangover_frames.max(1);
            }
        }
        Ok(closed)
    }

    /// End of stream: close any open region (kept when it meets the minimum
    /// length) and return it.
    pub fn finish(&mut self) -> Vec<SpeechSegment> {
        let mut closed = Vec::new();
        if let Some(start) = self.open_start.take() {
            let end = (self.last_speech_frame + 1).max(start);
            if end > start && end - start >= self.config.min_speech_frames {
                closed.push(SpeechSegment::from_frame_span(start, end));
            }
        }
        closed
    }

    /// Quietest frame strictly inside (`start`, `end`) from the
    /// recent-energy ring (the split point for a forced max-length cut).
    /// Falls back to `end` when the window holds nothing usable.
    fn quietest_split_point(&self, start: usize, end: usize) -> usize {
        let mut best: Option<(usize, f64)> = None;
        for &(frame, energy) in &self.recent_energy {
            if frame <= start || frame >= end {
                continue;
            }
            let replace = match best {
                None => true,
                Some((_, best_energy)) => energy < best_energy,
            };
            if replace {
                best = Some((frame, energy));
            }
        }
        best.map(|(frame, _)| frame).unwrap_or(end)
    }
}

/// Segment whole-utterance PCM (16 kHz mono) into closed speech regions.
/// The trailing partial frame (< 10 ms), if any, is dropped. Returns an
/// error unless `sample_rate` is 16 000.
pub fn segment_pcm16k(
    pcm: &[i16],
    sample_rate: u32,
    config: &VadConfig,
) -> Result<Vec<SpeechSegment>, String> {
    if sample_rate != SAMPLE_RATE_HZ {
        return Err(format!(
            "VAD takes {SAMPLE_RATE_HZ} Hz PCM, got {sample_rate} Hz"
        ));
    }
    let mut segmenter = Segmenter::new(config.clone());
    let mut closed = Vec::new();
    for frame in pcm.chunks_exact(SAMPLES_PER_FRAME) {
        closed.extend(segmenter.feed_frame(frame)?);
    }
    closed.extend(segmenter.finish());
    Ok(closed)
}

/// Refine VAD regions against SEG-1 boundaries: endpoints snap to the
/// nearest boundary within `tolerance_ms`, and boundaries strictly inside a
/// region (beyond tolerance of both edges) split it. Degenerate results
/// (empty spans) keep the original region. Boundaries are advisory — an
/// empty boundary list returns the input unchanged.
pub fn refine_with_boundaries(
    segments: &[SpeechSegment],
    boundaries: &[Boundary],
    tolerance_ms: f64,
) -> Vec<SpeechSegment> {
    if boundaries.is_empty() {
        return segments.to_vec();
    }
    let mut out = Vec::with_capacity(segments.len());
    for seg in segments {
        let start_ms = seg.start_ms as f64;
        let end_ms = seg.end_ms as f64;
        let mut snapped_start = start_ms;
        let mut snapped_end = end_ms;
        let mut best_start = tolerance_ms;
        let mut best_end = tolerance_ms;
        let mut interior: Vec<f64> = Vec::new();
        for b in boundaries {
            let t = b.time_ms;
            let d_start = (t - start_ms).abs();
            let d_end = (t - end_ms).abs();
            if d_start <= tolerance_ms && d_start < best_start {
                best_start = d_start;
                snapped_start = t;
            }
            if d_end <= tolerance_ms && d_end < best_end {
                best_end = d_end;
                snapped_end = t;
            }
            if t > start_ms + tolerance_ms && t < end_ms - tolerance_ms {
                interior.push(t);
            }
        }
        interior.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut cuts = Vec::with_capacity(interior.len() + 2);
        cuts.push(snapped_start);
        cuts.extend(interior);
        cuts.push(snapped_end);
        let mut kept = false;
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if b - a < FRAME_MS as f64 {
                continue;
            }
            let frame_start = (a / FRAME_MS as f64).round() as usize;
            let frame_end = (b / FRAME_MS as f64).round() as usize;
            if frame_end > frame_start {
                out.push(SpeechSegment::from_frame_span(frame_start, frame_end));
                kept = true;
            }
        }
        if !kept {
            out.push(seg.clone());
        }
    }
    out
}

/// Run the canonical frontend plus the SEG-1 boundary detector over 16 kHz
/// mono PCM. Advisory only: any frontend failure yields an empty vec, never
/// an error.
pub fn boundaries_for_pcm16k(pcm: &[i16]) -> Vec<Boundary> {
    let windows = fse::window_audio(pcm, SAMPLE_RATE_HZ);
    if windows.is_empty() {
        return Vec::new();
    }
    let frames = match fse::extract_features(&windows, SAMPLE_RATE_HZ) {
        Ok(frames) => frames,
        Err(_) => return Vec::new(),
    };
    detect_boundaries(&frames, &BoundaryConfig::default())
}

#[cfg(test)]
mod vad_tests {
    use super::*;

    /// 440 Hz sine at 16 kHz, amplitude 8000, global phase (no frame-edge
    /// clicks): the synthetic "speech" fixture.
    fn sine_script(frames: usize, start_sample: usize) -> Vec<i16> {
        (0..frames * SAMPLES_PER_FRAME)
            .map(|k| {
                let n = (start_sample + k) as f64;
                (8000.0 * (2.0 * std::f64::consts::PI * 440.0 * n / 16_000.0).sin()).round() as i16
            })
            .collect()
    }

    fn silence_script(frames: usize) -> Vec<i16> {
        vec![0i16; frames * SAMPLES_PER_FRAME]
    }

    #[test]
    fn frame_features_separate_sine_from_silence() {
        let sine = sine_script(1, 0);
        let energy = frame_energy_db(&sine);
        let zcr = zero_crossing_rate(&sine);
        // Sine RMS ~ amplitude/sqrt(2): about -15 dBFS, far above gate.
        assert!(energy > -20.0, "sine energy {energy}");
        // 440 Hz => ~8.8 crossings per 160-sample frame.
        assert!((0.03..0.09).contains(&zcr), "sine zcr {zcr} near 0.055");
        let silence = silence_script(1);
        assert_eq!(frame_energy_db(&silence), f64::NEG_INFINITY);
        assert_eq!(zero_crossing_rate(&silence), 0.0);
        // Empty and degenerate inputs never panic.
        assert_eq!(frame_energy_db(&[]), f64::NEG_INFINITY);
        assert_eq!(zero_crossing_rate(&[]), 0.0);
        assert_eq!(zero_crossing_rate(&[7]), 0.0);
    }

    /// Migrated DIA-1 gate evidence (bead hearsay-four-6if, via 6tc notes):
    /// speech-silence-speech yields two regions with edges within tolerance.
    #[test]
    fn speech_silence_speech_yields_two_regions_within_tolerance() {
        let mut pcm = sine_script(100, 0);
        pcm.extend_from_slice(&silence_script(60));
        pcm.extend_from_slice(&sine_script(100, 100 * SAMPLES_PER_FRAME));
        let segments = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default())
            .expect("segment speech-silence-speech");
        assert_eq!(segments.len(), 2, "segments: {segments:?}");
        // Tolerance: +/-2 frames (+/-20 ms) on every edge.
        assert!(segments[0].frame_start <= 2);
        assert!((segments[0].frame_end as i64 - 100).abs() <= 2);
        assert!((segments[1].frame_start as i64 - 160).abs() <= 2);
        assert!((segments[1].frame_end as i64 - 260).abs() <= 2);
        assert_eq!(
            segments[0].start_ms,
            segments[0].frame_start as u64 * FRAME_MS
        );
        assert_eq!(
            segments[0].end_sample,
            segments[0].frame_end * SAMPLES_PER_FRAME
        );
    }

    #[test]
    fn all_silence_yields_no_regions() {
        let pcm = silence_script(200);
        let segments = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default())
            .expect("segment all-silence");
        assert!(segments.is_empty(), "segments: {segments:?}");
    }

    #[test]
    fn all_speech_yields_single_covering_region() {
        let pcm = sine_script(200, 0);
        let segments = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default())
            .expect("segment all-speech");
        assert_eq!(segments.len(), 1, "segments: {segments:?}");
        assert_eq!(segments[0].frame_start, 0);
        assert_eq!(segments[0].frame_end, 200);
    }

    #[test]
    fn hangover_bridges_short_pause() {
        // 10-frame gap, under the 30-frame hangover: one region.
        let mut pcm = sine_script(50, 0);
        pcm.extend_from_slice(&silence_script(10));
        pcm.extend_from_slice(&sine_script(50, 50 * SAMPLES_PER_FRAME));
        let segments = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default())
            .expect("segment bridged pause");
        assert_eq!(segments.len(), 1, "segments: {segments:?}");
        assert_eq!(segments[0].frame_start, 0);
        assert_eq!(segments[0].frame_end, 110);
    }

    #[test]
    fn short_blip_dropped_below_minimum() {
        // 5 speech frames, under the 10-frame minimum: no region.
        let mut pcm = sine_script(5, 0);
        pcm.extend_from_slice(&silence_script(50));
        let segments =
            segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default()).expect("segment blip");
        assert!(segments.is_empty(), "segments: {segments:?}");
    }

    #[test]
    fn max_length_force_splits_long_speech() {
        let config = VadConfig {
            max_region_secs: 1.0,
            ..VadConfig::default()
        };
        assert_eq!(config.max_region_frames(), 100);
        let pcm = sine_script(250, 0);
        let segments = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &config).expect("segment long speech");
        assert!(segments.len() >= 2, "segments: {segments:?}");
        // Coverage is contiguous and complete: no frame lost or doubled.
        assert_eq!(segments[0].frame_start, 0);
        assert_eq!(segments.last().expect("last").frame_end, 250);
        for pair in segments.windows(2) {
            assert_eq!(pair[0].frame_end, pair[1].frame_start);
        }
    }

    #[test]
    fn incremental_feeding_matches_whole_utterance() {
        // One Segmenter fed in uneven halves decides exactly like the
        // whole-utterance helper: file/live chunkings agree by construction.
        let mut pcm = sine_script(80, 0);
        pcm.extend_from_slice(&silence_script(50));
        pcm.extend_from_slice(&sine_script(80, 80 * SAMPLES_PER_FRAME));
        let expected = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default())
            .expect("whole-utterance segments");
        let mut segmenter = Segmenter::new(VadConfig::default());
        let mut got = Vec::new();
        let frames: Vec<&[i16]> = pcm.chunks_exact(SAMPLES_PER_FRAME).collect();
        let (first, second) = frames.split_at(frames.len() / 2 + 7);
        for frame in first.iter().chain(second.iter()) {
            got.extend(segmenter.feed_frame(frame).expect("feed frame"));
        }
        got.extend(segmenter.finish());
        assert_eq!(got, expected);
    }

    #[test]
    fn segment_rejects_non_16k_and_bad_frames() {
        let pcm = sine_script(4, 0);
        assert!(segment_pcm16k(&pcm, 8000, &VadConfig::default()).is_err());
        let mut segmenter = Segmenter::new(VadConfig::default());
        assert!(segmenter.feed_frame(&pcm[..100]).is_err());
        assert_eq!(segmenter.frames_seen(), 0);
        assert!(!segmenter.is_open());
    }

    #[test]
    fn trailing_partial_frame_is_dropped() {
        // 12 speech frames (above the 10-frame minimum) plus 100 stray
        // samples: the region covers the 12 whole frames only.
        let mut pcm = sine_script(12, 0);
        pcm.extend_from_slice(&[8000i16; 100]);
        let segments = segment_pcm16k(&pcm, SAMPLE_RATE_HZ, &VadConfig::default())
            .expect("segment partial tail");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].frame_start, 0);
        assert_eq!(segments[0].frame_end, 12);
    }

    #[test]
    fn refine_snaps_edges_and_splits_interior() {
        let seg = SpeechSegment::from_frame_span(100, 200);
        let boundaries = vec![
            Boundary {
                frame: 100,
                time_ms: 1005.0,
            },
            Boundary {
                frame: 150,
                time_ms: 1500.0,
            },
            Boundary {
                frame: 199,
                time_ms: 1990.0,
            },
        ];
        let refined = refine_with_boundaries(&[seg], &boundaries, 50.0);
        assert_eq!(refined.len(), 2, "refined: {refined:?}");
        // 1005 ms snaps to frame 101 (100.5 rounds away from zero), 1990 ms
        // to frame 199, interior 1500 ms splits.
        assert_eq!(refined[0].frame_start, 101);
        assert_eq!(refined[0].frame_end, 150);
        assert_eq!(refined[1].frame_start, 150);
        assert_eq!(refined[1].frame_end, 199);
    }

    #[test]
    fn refine_without_boundaries_returns_input() {
        let segs = vec![
            SpeechSegment::from_frame_span(0, 50),
            SpeechSegment::from_frame_span(80, 120),
        ];
        assert_eq!(refine_with_boundaries(&segs, &[], 50.0), segs);
    }

    #[test]
    fn boundaries_helper_never_fails() {
        // Advisory helper: empty input yields empty vec, real input yields
        // whatever SEG-1 finds (possibly empty) without erroring.
        assert!(boundaries_for_pcm16k(&[]).is_empty());
        let pcm = sine_script(100, 0);
        let _ = boundaries_for_pcm16k(&pcm);
    }
}
