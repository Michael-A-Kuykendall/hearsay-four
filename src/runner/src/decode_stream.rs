//! Decoder-driven streaming sessions over the frozen `decoder::Recognizer`
//! trait (bead hearsay-four-6tc, L6 streaming + voice activity endpointing).
//!
//! File and live paths share ONE session implementation: PCM chunks in,
//! `decoder::Partial` hypotheses out, per-chunk timing (RTF through
//! `crate::rtf`, latency in milliseconds), and VAD endpoint closure that
//! promotes provisional text to stable. The trait is used exactly as frozen
//! (`decode`/`stream` plus a caller-injected region reset); the v0
//! re-decode-per-chunk cost is measured per chunk and reported, never hidden.

use std::collections::VecDeque;
use std::time::Instant;

use decoder::{Partial, Recognizer};
use lattice::Lattice;
use output::streaming::{StreamingHypothesis, hypothesis_from_texts, word_count};

use crate::rtf::{ChunkTiming, RtfReport, summarize};
use crate::vad::{SAMPLE_RATE_HZ, SAMPLES_PER_FRAME, Segmenter, SpeechSegment, VadConfig};

/// Sample history kept for forced-split tail re-streaming: 64 frames of
/// samples (covers the 50-frame VAD split search plus margin).
const HISTORY_SAMPLES: usize = 64 * SAMPLES_PER_FRAME;

/// One timed `stream` call: what audio went in, what it cost, what came out.
#[derive(Debug, Clone)]
pub struct ChunkObservation {
    /// Zero-based index of this `stream` call in the session.
    pub chunk_idx: usize,
    /// Audio seconds in the fed chunk.
    pub audio_secs: f64,
    /// Wall seconds the `stream` call took.
    pub processing_secs: f64,
    /// `processing_secs * 1000.0`: end-of-chunk to partial availability.
    pub latency_ms: f64,
    /// Best-hypothesis text returned (`""` when no partial).
    pub partial_text: String,
    /// Word count of `partial_text`.
    pub partial_words: usize,
    /// `frame_end` of the first partial (0 when no partial).
    pub frame_end: u32,
}

/// Best-hypothesis text of a `stream` response: the first partial's text, or
/// `""` when the response holds none. The v0 decoder returns exactly one
/// partial; later shapes stay compatible by ranking first.
pub fn partial_text(partials: &[Partial]) -> String {
    partials.first().map(|p| p.text.clone()).unwrap_or_default()
}

/// Batch-decode text: lattice token words joined with single spaces.
pub fn lattice_text(lattice: &Lattice) -> String {
    lattice
        .tokens
        .iter()
        .map(|t| t.word.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A `Recognizer` wrapped with per-chunk timing. Feed 16 kHz mono PCM in
/// any chunking; every `stream` call is timed for RTF/latency evidence.
pub struct RecognizerSession<R: Recognizer> {
    recognizer: R,
    sample_rate: u32,
    observations: Vec<ChunkObservation>,
    streamed_samples: usize,
}

impl<R: Recognizer> RecognizerSession<R> {
    /// Wrap `recognizer` for 16 kHz mono PCM. Any other rate is an error —
    /// the decoder contract is 16 kHz in.
    pub fn new(recognizer: R, sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SAMPLE_RATE_HZ {
            return Err(format!(
                "decoder session takes {SAMPLE_RATE_HZ} Hz PCM, got {sample_rate} Hz"
            ));
        }
        Ok(Self {
            recognizer,
            sample_rate,
            observations: Vec::new(),
            streamed_samples: 0,
        })
    }

    /// Feed one chunk; returns the decoder's partials verbatim and records
    /// the call's timing. Empty chunks are passed through (the decoder
    /// defines their meaning) and still timed.
    pub fn feed(&mut self, chunk: &[i16]) -> Vec<Partial> {
        let start = Instant::now();
        let partials = self.recognizer.stream(chunk);
        let processing_secs = start.elapsed().as_secs_f64();
        let text = partial_text(&partials);
        let frame_end = partials.first().map(|p| p.frame_end).unwrap_or(0);
        self.observations.push(ChunkObservation {
            chunk_idx: self.observations.len(),
            audio_secs: chunk.len() as f64 / f64::from(self.sample_rate),
            processing_secs,
            latency_ms: processing_secs * 1000.0,
            partial_words: word_count(&text),
            partial_text: text,
            frame_end,
        });
        self.streamed_samples += chunk.len();
        partials
    }

    /// Per-call observations in feed order.
    pub fn observations(&self) -> &[ChunkObservation] {
        &self.observations
    }

    /// Borrow the wrapped recognizer (e.g. for a batch `decode` comparison).
    pub fn recognizer(&self) -> &R {
        &self.recognizer
    }

    /// Mutable borrow of the wrapped recognizer.
    pub fn recognizer_mut(&mut self) -> &mut R {
        &mut self.recognizer
    }

    /// Total PCM samples streamed through `feed`.
    pub fn streamed_samples(&self) -> usize {
        self.streamed_samples
    }

    /// Total audio seconds streamed.
    pub fn streamed_secs(&self) -> f64 {
        self.streamed_samples as f64 / f64::from(self.sample_rate)
    }

    /// Total `stream` wall seconds.
    pub fn processing_secs(&self) -> f64 {
        self.observations.iter().map(|o| o.processing_secs).sum()
    }

    /// Per-call timings for the shared RTF helpers. Zero-audio observations
    /// (empty-chunk probes) are excluded — the RTF helpers reject them.
    pub fn chunk_timings(&self) -> Vec<ChunkTiming> {
        self.observations
            .iter()
            .filter(|o| o.audio_secs > 0.0)
            .map(|o| ChunkTiming::new(o.audio_secs, o.processing_secs))
            .collect()
    }

    /// RTF summary over streamed audio (processing / streamed).
    pub fn rtf_report(&self) -> Result<RtfReport, String> {
        summarize(&self.chunk_timings())
    }

    /// Worst per-chunk latency in milliseconds (0.0 with no observations).
    pub fn max_latency_ms(&self) -> f64 {
        self.observations
            .iter()
            .map(|o| o.latency_ms)
            .fold(0.0, f64::max)
    }

    /// Nearest-rank 95th percentile of per-chunk latency in milliseconds
    /// (0.0 with no observations).
    pub fn p95_latency_ms(&self) -> f64 {
        percentile(
            &self
                .observations
                .iter()
                .map(|o| o.latency_ms)
                .collect::<Vec<_>>(),
            0.95,
        )
    }
}

/// Nearest-rank percentile over a snapshot (0.0 when empty).
fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// Stream whole-utterance PCM through `session` in `chunk_samples` pieces;
/// returns the last `stream` response (empty vec when `pcm` is empty).
/// The continuous (no-VAD) path: the final partial covers the whole buffer,
/// directly comparable to a batch `decode` of the same PCM.
pub fn feed_all<R: Recognizer>(
    session: &mut RecognizerSession<R>,
    pcm: &[i16],
    chunk_samples: usize,
) -> Result<Vec<Partial>, String> {
    if chunk_samples == 0 {
        return Err("chunk_samples must be >= 1".to_string());
    }
    let mut last = Vec::new();
    for chunk in pcm.chunks(chunk_samples) {
        last = session.feed(chunk);
    }
    Ok(last)
}

/// One endpointed-session output: a provisional display update or a region
/// closure promoting text to stable.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Current best hypothesis (stable prefix + revisable suffix).
    Provisional(StreamingHypothesis),
    /// A VAD region closed: its text is stable from here on.
    Stable {
        /// The closed VAD region.
        segment: SpeechSegment,
        /// Final best text of the region (decoder buffer at closure).
        text: String,
    },
}

/// Incremental VAD-endpointed session over one `Recognizer`: VAD frames stay
/// aligned to stream start (any feed chunking decides identically), decoder
/// `stream` calls carry only region-active audio, and `reset` clears the
/// decoder buffer at each region bound so the v0 re-decode cost stays
/// region-bounded. For `MooreDecoder` pass `MooreDecoder::reset_stream`.
pub struct EndpointedSession<R: Recognizer> {
    session: RecognizerSession<R>,
    segmenter: Segmenter,
    reset: fn(&mut R),
    pending: Vec<i16>,
    due: Vec<i16>,
    history: VecDeque<i16>,
    /// Most recent streamed partial (text + frame end) since the last
    /// region reset. A region close flushes only still-due audio — usually
    /// nothing, since feeds stream eagerly — so the close reads the
    /// region-final text from here instead of an empty flush.
    last_partial: String,
    last_frame_end: u32,
    consumed_samples: usize,
    stable_prefix: String,
    stable_words: usize,
    segments: Vec<SpeechSegment>,
    total_samples: usize,
}

impl<R: Recognizer> EndpointedSession<R> {
    /// New endpointed session over `recognizer` with `reset` clearing the
    /// decoder buffer at each region bound.
    pub fn new(
        recognizer: R,
        reset: fn(&mut R),
        sample_rate: u32,
        vad_config: VadConfig,
    ) -> Result<Self, String> {
        Ok(Self {
            session: RecognizerSession::new(recognizer, sample_rate)?,
            segmenter: Segmenter::new(vad_config),
            reset,
            pending: Vec::new(),
            due: Vec::new(),
            history: VecDeque::new(),
            last_partial: String::new(),
            last_frame_end: 0,
            consumed_samples: 0,
            stable_prefix: String::new(),
            stable_words: 0,
            segments: Vec::new(),
            total_samples: 0,
        })
    }

    /// Feed PCM samples in any chunking. Returns display events in order:
    /// at most one provisional per call plus one provisional-then-stable
    /// pair per region closed.
    pub fn feed(&mut self, chunk: &[i16]) -> Result<Vec<StreamEvent>, String> {
        self.total_samples += chunk.len();
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        while self.pending.len() >= SAMPLES_PER_FRAME {
            let frame: Vec<i16> = self.pending.drain(..SAMPLES_PER_FRAME).collect();
            self.push_history(&frame);
            self.consumed_samples += SAMPLES_PER_FRAME;
            let active_before = self.segmenter.is_open();
            let closed = self.segmenter.feed_frame(&frame)?;
            let active_after = self.segmenter.is_open();
            if active_before || active_after || !closed.is_empty() {
                self.due.extend_from_slice(&frame);
            }
            if active_before && !active_after && closed.is_empty() {
                // Silent blip drop (under the minimum length): its audio
                // never reaches the decoder.
                self.due.clear();
            }
            for segment in closed {
                self.close_segment(segment, &mut events);
            }
        }
        if !self.due.is_empty() && self.segmenter.is_open() {
            let partials = self.flush_due();
            let frame_end = partials.first().map(|p| p.frame_end).unwrap_or(0);
            let full = join_prefix(&self.stable_prefix, &partial_text(&partials));
            events.push(StreamEvent::Provisional(hypothesis_from_texts(
                &self.stable_prefix,
                &full,
                frame_end,
                false,
            )));
        }
        Ok(events)
    }

    /// End of stream: pad the trailing partial frame with digital silence
    /// for the VAD, close any open region, and return the closing events.
    /// Padded zeros never reach the decoder — only real samples stream.
    pub fn finish(&mut self) -> Result<Vec<StreamEvent>, String> {
        let mut events = Vec::new();
        let tail: Vec<i16> = std::mem::take(&mut self.pending);
        if !tail.is_empty() {
            let mut frame = tail.clone();
            frame.resize(SAMPLES_PER_FRAME, 0);
            let active_before = self.segmenter.is_open();
            let closed = self.segmenter.feed_frame(&frame)?;
            self.push_history(&frame);
            self.consumed_samples += SAMPLES_PER_FRAME;
            let active_after = self.segmenter.is_open();
            if active_before || active_after || !closed.is_empty() {
                // Stream the real tail only, never the zero padding.
                self.due.extend_from_slice(&tail);
            }
            if active_before && !active_after && closed.is_empty() {
                self.due.clear();
            }
            for segment in closed {
                self.close_segment(segment, &mut events);
            }
        }
        let trailing = self.segmenter.finish();
        for segment in trailing {
            self.close_segment(segment, &mut events);
        }
        // A dropped tail (under-minimum blip at end of stream) leaves due
        // audio that must never leak into a later region; the session is
        // over but the recognizer is left clean regardless.
        self.due.clear();
        (self.reset)(self.session.recognizer_mut());
        self.last_partial.clear();
        self.last_frame_end = 0;
        Ok(events)
    }

    /// The timed decoder session underneath (observations, RTF, latency).
    pub fn session(&self) -> &RecognizerSession<R> {
        &self.session
    }

    /// Closed VAD regions in order.
    pub fn segments(&self) -> &[SpeechSegment] {
        &self.segments
    }

    /// Stable (endpoint-committed) transcript: closed-region finals joined
    /// with single spaces. Append-only.
    pub fn stable_text(&self) -> &str {
        &self.stable_prefix
    }

    /// Word count of [`Self::stable_text`].
    pub fn stable_words(&self) -> usize {
        self.stable_words
    }

    /// Total input PCM samples fed (all audio, streamed or VAD-skipped).
    pub fn total_samples(&self) -> usize {
        self.total_samples
    }

    /// Total input audio seconds (all audio, streamed or VAD-skipped).
    pub fn total_secs(&self) -> f64 {
        self.total_samples as f64 / f64::from(SAMPLE_RATE_HZ)
    }

    /// Session RTF: total `stream` wall seconds over total input audio
    /// seconds (0.0 with no input). This is the user-facing number — the
    /// VAD-skipped silence counts as audio the decoder never paid for.
    pub fn session_rtf(&self) -> f64 {
        if self.total_samples == 0 {
            0.0
        } else {
            self.session.processing_secs() / self.total_secs()
        }
    }

    fn close_segment(&mut self, segment: SpeechSegment, events: &mut Vec<StreamEvent>) {
        // Flush any still-due audio first (the close record, then read the
        // region-final text: the flush updates `last_partial` when it
        // streams, otherwise the final text is the last feed's partial.
        let _ = self.flush_due();
        let text = self.last_partial.clone();
        let frame_end = self.last_frame_end;
        self.push_stable(&text);
        self.segments.push(segment.clone());
        // The closure update doubles as the settled provisional state, then
        // the stable commit.
        events.push(StreamEvent::Provisional(hypothesis_from_texts(
            &self.stable_prefix,
            &self.stable_prefix,
            frame_end,
            true,
        )));
        events.push(StreamEvent::Stable { segment, text });
        (self.reset)(self.session.recognizer_mut());
        // The buffer is fresh: stale region-final text must not leak into
        // the next region's close.
        self.last_partial.clear();
        self.last_frame_end = 0;
        // Forced max-length splits leave the tail open: its leading samples
        // were already consumed into the old buffer, so re-stream them from
        // history into the fresh buffer. Endpoint closes need nothing —
        // their hangover tail belongs to the old region.
        if self.segmenter.is_open() {
            let end = self.segments.last().map(|s| s.end_sample).unwrap_or(0);
            self.restream_tail(end);
        }
    }

    fn push_stable(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if self.stable_prefix.is_empty() {
            self.stable_prefix = text.to_string();
        } else {
            self.stable_prefix.push(' ');
            self.stable_prefix.push_str(text);
        }
        self.stable_words = word_count(&self.stable_prefix);
    }

    fn flush_due(&mut self) -> Vec<Partial> {
        if self.due.is_empty() {
            return Vec::new();
        }
        let due = std::mem::take(&mut self.due);
        let partials = self.session.feed(&due);
        self.last_partial = partial_text(&partials);
        self.last_frame_end = partials.first().map(|p| p.frame_end).unwrap_or(0);
        partials
    }

    fn push_history(&mut self, frame: &[i16]) {
        self.history.extend(frame.iter().copied());
        while self.history.len() > HISTORY_SAMPLES {
            self.history.pop_front();
        }
    }

    fn restream_tail(&mut self, region_end_sample: usize) {
        if region_end_sample >= self.consumed_samples {
            return;
        }
        let tail_len = self.consumed_samples - region_end_sample;
        let available = self.history.len().min(tail_len);
        let skip = self.history.len() - available;
        let tail: Vec<i16> = self.history.iter().skip(skip).copied().collect();
        self.due.extend_from_slice(&tail);
    }
}

/// Join a stable prefix with new text, skipping empties cleanly.
fn join_prefix(prefix: &str, text: &str) -> String {
    match (prefix.trim().is_empty(), text.trim().is_empty()) {
        (true, true) => String::new(),
        (true, false) => text.to_string(),
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix} {text}"),
    }
}

/// Result of [`run_endpointed`]: the finished session plus every display
/// event in feed order.
pub struct EndpointedOutcome<R: Recognizer> {
    /// The finished session (timings, segments, stable transcript).
    pub session: EndpointedSession<R>,
    /// Display events in feed order.
    pub events: Vec<StreamEvent>,
}

/// Run whole-utterance PCM through an endpointed session in
/// `chunk_samples` pieces (the file path; the live path feeds the same
/// session type frame by frame).
pub fn run_endpointed<R: Recognizer>(
    recognizer: R,
    reset: fn(&mut R),
    pcm: &[i16],
    sample_rate: u32,
    vad_config: &VadConfig,
    chunk_samples: usize,
) -> Result<EndpointedOutcome<R>, String> {
    if chunk_samples == 0 {
        return Err("chunk_samples must be >= 1".to_string());
    }
    let mut session = EndpointedSession::new(recognizer, reset, sample_rate, vad_config.clone())?;
    let mut events = Vec::new();
    for chunk in pcm.chunks(chunk_samples) {
        events.extend(session.feed(chunk)?);
    }
    events.extend(session.finish()?);
    Ok(EndpointedOutcome { session, events })
}

#[cfg(test)]
mod decode_stream_tests {
    use super::*;
    use decoder::{DecoderConfig, MooreDecoder};
    use std::path::PathBuf;

    /// Counting no-op recognizer: proves VAD gating (what audio reaches
    /// `stream`) without a model.
    struct NullRecognizer {
        calls: usize,
        samples: usize,
    }

    impl Recognizer for NullRecognizer {
        fn decode(&self, _pcm16k: &[i16]) -> Lattice {
            Lattice {
                utterance: String::new(),
                frame_shift_ms: 10.0,
                producer: "null".to_string(),
                tokens: Vec::new(),
                arcs: Vec::new(),
            }
        }

        fn stream(&mut self, chunk: &[i16]) -> Vec<Partial> {
            self.calls += 1;
            self.samples += chunk.len();
            Vec::new()
        }
    }

    fn null_recognizer() -> NullRecognizer {
        NullRecognizer {
            calls: 0,
            samples: 0,
        }
    }

    fn null_reset(_recognizer: &mut NullRecognizer) {}

    fn sine_script(frames: usize, start_sample: usize) -> Vec<i16> {
        (0..frames * SAMPLES_PER_FRAME)
            .map(|k| {
                let n = (start_sample + k) as f64;
                (8000.0 * (2.0 * std::f64::consts::PI * 440.0 * n / 16_000.0).sin()).round() as i16
            })
            .collect()
    }

    #[test]
    fn silence_never_reaches_decoder() {
        let pcm = vec![0i16; 2 * SAMPLE_RATE_HZ as usize];
        let outcome = run_endpointed(
            null_recognizer(),
            null_reset,
            &pcm,
            SAMPLE_RATE_HZ,
            &VadConfig::default(),
            8000,
        )
        .expect("endpointed silence");
        assert_eq!(outcome.session.session().observations().len(), 0);
        assert!(outcome.session.segments().is_empty());
        assert_eq!(outcome.session.stable_text(), "");
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.session.session_rtf(), 0.0);
    }

    #[test]
    fn blip_audio_never_reaches_decoder() {
        // 5 speech frames then silence: under the 10-frame minimum, so the
        // blip is dropped and `stream` is never called.
        let mut pcm = sine_script(5, 0);
        pcm.extend_from_slice(&vec![0i16; 50 * SAMPLES_PER_FRAME]);
        let outcome = run_endpointed(
            null_recognizer(),
            null_reset,
            &pcm,
            SAMPLE_RATE_HZ,
            &VadConfig::default(),
            8000,
        )
        .expect("endpointed blip");
        assert_eq!(outcome.session.session().observations().len(), 0);
        assert!(outcome.session.segments().is_empty());
    }

    #[test]
    fn speech_region_streams_once_per_feed_then_closes() {
        // 100 speech frames + 60 silence, fed in 50-frame (8000-sample)
        // chunks: two provisional flushes while open, one close flush at
        // the hangover-exhausted frame 130, then idle.
        let mut pcm = sine_script(100, 0);
        pcm.extend_from_slice(&vec![0i16; 60 * SAMPLES_PER_FRAME]);
        let outcome = run_endpointed(
            null_recognizer(),
            null_reset,
            &pcm,
            SAMPLE_RATE_HZ,
            &VadConfig::default(),
            8000,
        )
        .expect("endpointed region");
        let session = &outcome.session;
        assert_eq!(session.session().observations().len(), 3);
        // Frames 0..=130 streamed (speech plus hangover tail through the
        // exhausting frame): 131 frames.
        assert_eq!(
            session.session().streamed_samples(),
            131 * SAMPLES_PER_FRAME
        );
        assert_eq!(session.segments().len(), 1);
        assert_eq!(session.segments()[0].frame_start, 0);
        assert_eq!(session.segments()[0].frame_end, 100);
        // Two provisionals while open, then the settled provisional plus
        // the stable commit.
        assert_eq!(outcome.events.len(), 4);
        assert!(matches!(outcome.events[3], StreamEvent::Stable { .. }));
        // Session RTF over a null recognizer is ~0 (nothing decoded).
        assert!(
            session.session_rtf() < 0.01,
            "rtf {}",
            session.session_rtf()
        );
    }

    #[test]
    fn percentile_ranks_nearest() {
        assert_eq!(percentile(&[], 0.95), 0.0);
        assert_eq!(percentile(&[5.0], 0.5), 5.0);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.95), 4.0);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.25), 1.0);
    }

    #[test]
    fn join_prefix_skips_empties() {
        assert_eq!(join_prefix("", ""), "");
        assert_eq!(join_prefix("A", ""), "A");
        assert_eq!(join_prefix("", "B"), "B");
        assert_eq!(join_prefix("A", "B"), "A B");
    }

    #[test]
    fn rejects_non_16k_and_empty_chunks() {
        assert!(RecognizerSession::new(null_recognizer(), 8000).is_err());
        assert!(
            EndpointedSession::new(null_recognizer(), null_reset, 8000, VadConfig::default())
                .is_err()
        );
        assert!(
            run_endpointed(
                null_recognizer(),
                null_reset,
                &[0i16; 160],
                8000,
                &VadConfig::default(),
                160,
            )
            .is_err()
        );
        assert!(
            run_endpointed(
                null_recognizer(),
                null_reset,
                &[0i16; 160],
                SAMPLE_RATE_HZ,
                &VadConfig::default(),
                0,
            )
            .is_err()
        );
        let mut session =
            RecognizerSession::new(null_recognizer(), SAMPLE_RATE_HZ).expect("16k session");
        assert!(feed_all(&mut session, &[0i16; 160], 0).is_err());
    }

    #[test]
    fn empty_input_yields_no_stable() {
        let outcome = run_endpointed(
            null_recognizer(),
            null_reset,
            &[],
            SAMPLE_RATE_HZ,
            &VadConfig::default(),
            8000,
        )
        .expect("endpointed empty");
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.session.stable_text(), "");
        assert!(outcome.session.segments().is_empty());
        assert_eq!(outcome.session.session_rtf(), 0.0);
        let mut session =
            RecognizerSession::new(null_recognizer(), SAMPLE_RATE_HZ).expect("16k session");
        let last = feed_all(&mut session, &[], 8000).expect("feed empty");
        assert!(last.is_empty());
        assert!(session.observations().is_empty());
    }

    // --- Model-backed gates (si_base + closed vocab + demo fixture) ---

    fn workspace_file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    fn am_path() -> PathBuf {
        workspace_file("../../models/si_base/model.json")
    }

    fn sample_wav() -> PathBuf {
        workspace_file("../../data/demo/sample.wav")
    }

    fn load_decoder() -> MooreDecoder {
        MooreDecoder::load(
            &am_path(),
            None,
            lexicon::CLOSED_VOCABULARY_CMUDICT,
            DecoderConfig::default(),
        )
        .expect("decoder load on si_base + closed vocab")
    }

    fn sample_pcm16k() -> Vec<i16> {
        let audio = corpus::audio::decode_file(&sample_wav()).expect("decode demo wav");
        let mono = corpus::audio::to_mono(&audio);
        corpus::audio::resample_linear(&mono, audio.sample_rate, 16_000)
    }

    fn clip_secs(pcm: &[i16], secs: f64) -> &[i16] {
        let take = ((16_000.0 * secs) as usize).min(pcm.len());
        &pcm[..take]
    }

    /// Live/batch word parity on a REGRESSION clip: continuous streaming
    /// (chunked `stream` calls, no VAD) ends exactly where batch `decode`
    /// lands — the v0 re-decode makes the final partial cover the whole
    /// buffer, so equality is structural, not statistical.
    #[test]
    fn continuous_streaming_matches_batch_on_2s_clip() {
        let pcm = sample_pcm16k();
        let clip = clip_secs(&pcm, 2.0).to_vec();
        let batch_text = lattice_text(&load_decoder().decode(&clip));
        let mut session =
            RecognizerSession::new(load_decoder(), SAMPLE_RATE_HZ).expect("16k session");
        let last = feed_all(&mut session, &clip, 8000).expect("stream 2s clip");
        let stream_text = partial_text(&last);
        eprintln!("batch   : {batch_text}");
        eprintln!("streamed: {stream_text}");
        assert!(!batch_text.is_empty(), "2s clip must decode words");
        assert_eq!(
            stream_text, batch_text,
            "streamed final partial equals batch decode"
        );
    }

    /// One-decoder parity: file-sized (500 ms) and live-sized (50/100 ms)
    /// chunkings through fresh endpointed sessions produce identical
    /// segments and identical stable text. Per-region exact-match diffs
    /// feed a paired bootstrap CI (10k resamples, pinned seed).
    #[test]
    fn endpointed_chunkings_agree_with_zero_ci() {
        // Debug is ~14x slower than release on this decoder; the parity
        // mechanism (frame-aligned VAD + deterministic decode) is
        // profile-independent, so debug proves it on 2 s and release on 4 s.
        let secs = if cfg!(debug_assertions) { 2.0 } else { 4.0 };
        let pcm = sample_pcm16k();
        let clip = clip_secs(&pcm, secs).to_vec();
        let chunkings = [800usize, 1600, 8000];
        let mut stables = Vec::new();
        let mut region_runs: Vec<Vec<String>> = Vec::new();
        let mut first_segments: Vec<SpeechSegment> = Vec::new();
        for (run, chunk) in chunkings.iter().enumerate() {
            let outcome = run_endpointed(
                load_decoder(),
                MooreDecoder::reset_stream,
                &clip,
                SAMPLE_RATE_HZ,
                &VadConfig::default(),
                *chunk,
            )
            .expect("endpointed run");
            let stable = outcome.session.stable_text().to_string();
            eprintln!(
                "chunk {chunk}: stable=[{stable}] segments={:?}",
                outcome.session.segments()
            );
            assert!(
                !stable.is_empty(),
                "chunking {chunk} must commit stable words"
            );
            if run == 0 {
                first_segments = outcome.session.segments().to_vec();
            } else {
                assert_eq!(
                    outcome.session.segments(),
                    first_segments.as_slice(),
                    "chunking {chunk} segments match"
                );
            }
            let regions: Vec<String> = outcome
                .events
                .iter()
                .filter_map(|e| match e {
                    StreamEvent::Stable { text, .. } => Some(text.clone()),
                    StreamEvent::Provisional(_) => None,
                })
                .collect();
            region_runs.push(regions);
            stables.push(stable);
        }
        assert_eq!(stables[1], stables[0], "100 ms vs 50 ms agree");
        assert_eq!(stables[2], stables[0], "500 ms vs 50 ms agree");
        assert_eq!(region_runs[1], region_runs[0], "per-region texts agree");
        assert_eq!(region_runs[2], region_runs[0], "per-region texts agree");
        // Paired bootstrap CI over per-region exact-match diffs (0.0 match,
        // 1.0 mismatch) across the chunking pairs.
        let mut diffs: Vec<f64> = Vec::new();
        for other in [1usize, 2] {
            for (a, b) in region_runs[0].iter().zip(region_runs[other].iter()) {
                diffs.push(if a == b { 0.0 } else { 1.0 });
            }
        }
        assert!(!diffs.is_empty(), "need regions for a CI");
        let ci = eval::paired_bootstrap_ci(
            &diffs,
            eval::BootstrapConfig {
                resamples: 10_000,
                seed: 20261031,
                confidence: 0.95,
            },
        );
        eprintln!(
            "parity CI: n={} estimate={} lo={} hi={}",
            diffs.len(),
            ci.estimate,
            ci.lo,
            ci.hi
        );
        assert_eq!(ci.estimate, 0.0);
        assert_eq!(ci.lo, 0.0);
        assert_eq!(ci.hi, 0.0);
        assert!(!ci.excludes(0.0));
    }

    /// Measured RTF/latency gate. Training size: VAD untrained (0 tuned
    /// params on audio); decoder is file-swapped si_base + 30-word closed
    /// vocab, no retraining. Test population: the REGRESSION courtroom clip
    /// (8 s release, 4 s debug) in 200 ms stream chunks. Release asserts the
    /// bead thresholds (session RTF < 1, worst chunk < 500 ms); debug
    /// reports the same numbers without thresholds (the debug decoder is
    /// ~14x slower than release, so debug thresholds would gate the
    /// optimizer, not the design — gate evidence is the release run).
    #[test]
    fn rtf_and_latency_gate() {
        let secs = if cfg!(debug_assertions) { 4.0 } else { 8.0 };
        let pcm = sample_pcm16k();
        let clip = clip_secs(&pcm, secs).to_vec();
        let decoder = load_decoder();
        let vocab = decoder.vocab_size();
        let outcome = run_endpointed(
            decoder,
            MooreDecoder::reset_stream,
            &clip,
            SAMPLE_RATE_HZ,
            &VadConfig::default(),
            3200,
        )
        .expect("endpointed gate run");
        let session = &outcome.session;
        let decoder_rtf = session.session().rtf_report().expect("decoder RTF report");
        let session_rtf = session.session_rtf();
        let max_latency = session.session().max_latency_ms();
        let p95_latency = session.session().p95_latency_ms();
        eprintln!(
            "model=models/si_base/model.json vocab={vocab} \
             audio={:.2}s chunks={} regions={} stable_words={}",
            session.total_secs(),
            decoder_rtf.chunk_count,
            session.segments().len(),
            session.stable_words(),
        );
        eprintln!(
            "session_rtf={session_rtf:.3} decoder_overall_rtf={:.3} \
             decoder_mean_rtf={:.3} decoder_p95_rtf={:.3}",
            decoder_rtf.overall_rtf, decoder_rtf.mean_rtf, decoder_rtf.p95_rtf,
        );
        eprintln!("max_latency_ms={max_latency:.1} p95_latency_ms={p95_latency:.1}");
        eprintln!("stable=[{}]", session.stable_text());
        assert!(!session.stable_text().is_empty(), "gate clip decodes words");
        assert!(session_rtf.is_finite());
        assert!(max_latency.is_finite());
        if cfg!(debug_assertions) {
            eprintln!("debug report-only: thresholds assert in the release run");
        } else {
            assert!(session_rtf < 1.0, "session RTF {session_rtf} < 1");
            assert!(max_latency < 500.0, "worst chunk {max_latency} ms < 500");
        }
    }
}
