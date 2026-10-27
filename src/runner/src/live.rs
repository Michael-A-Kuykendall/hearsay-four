//! Live mic ingest into the C3 streaming pipeline (bead hearsay-four-4zx, MIC-1).
//!
//! `hearsay-serve` took complete WAV files/stdin only; no live stream
//! reached the C3 pipeline. This module is the live ingest path: a
//! [`LiveSource`] yields microphone-sized PCM frames, [`run_live_session`]
//! feeds them into [`StreamingPipeline`](crate::StreamingPipeline)
//! incrementally (one committed word per `chunk_secs` of audio), and the
//! caller prints provisional words as they commit plus the final validated
//! transcript (see `hearsay-serve --live`).
//!
//! Hermetic by construction (no hardware, no network, no models):
//! - [`PcmFrameStream`] replays a decoded fixture file in mic-sized frames
//!   — the StubCapture/test-utils role for live PCM (synthetic live input,
//!   never a physical mic). The `CaptureBackend` stub itself
//!   (`calibration::enrollment::StubCapture`, `test-utils` feature) is
//!   exercised in the serve gate alongside the live run.
//! - [`FailAfter`] simulates a device failure (immediate or mid-stream) so
//!   the gate proves device errors surface instead of stalling.
//!
//! Exit-code contract (enforced by `serve_live_from_source` in
//! `hearsay-serve`): [`LiveError::Capture`] (no frames, device failure) is
//! an input error (exit 2, empty stdout); [`LiveError::Pipeline`] (the
//! pipeline itself failed on valid input) is a pipeline failure (exit 1,
//! like the batch path). A dead source never yields an empty success —
//! silence is an error, never a stall.
//!
//! Model-free, fixture audio + synthetic only, offline.

use calibration::Baseline;
use corpus::audio::Pcm16kAudio;
use fse::PhoneTemplate;
use output::SessionMeta;

use crate::{StreamingConfig, StreamingOutput, StreamingPipeline};

/// One live audio frame source: mic-sized PCM slices, a device error, or
/// end-of-stream. Production swaps in a mic-driven impl behind the `mic`
/// feature; the hermetic gate uses [`PcmFrameStream`] (fixture replay) and
/// [`FailAfter`] (device failure).
pub trait LiveSource {
    /// Device sample rate in Hz (fixes the pipeline clock).
    fn sample_rate(&self) -> u32;
    /// Next live frame: `Some(Ok(pcm))` feeds the pipeline,
    /// `Some(Err(msg))` is a device failure (surfaces as exit 2, never a
    /// silent stall), `None` is a clean end of stream.
    fn next_chunk(&mut self) -> Option<Result<Vec<i16>, String>>;
}

/// Synthetic live input: a decoded fixture file replayed in mic-sized
/// frames (`frame_ms` of audio per [`LiveSource::next_chunk`]).
///
/// Chunk boundaries stay position-aligned from stream start (the pipeline
/// commits per `chunk_secs` regardless of feed sizes), so replaying one
/// file in 50 ms, 100 ms, or 250 ms frames commits identical words.
pub struct PcmFrameStream {
    frames: Vec<i16>,
    pos: usize,
    frame_samples: usize,
    sample_rate: u32,
}

impl PcmFrameStream {
    /// Slice `pcm` into `frame_ms`-millisecond live frames.
    pub fn new(pcm: &Pcm16kAudio, frame_ms: u64) -> Result<Self, String> {
        if frame_ms == 0 {
            return Err("live frame duration must be positive".to_string());
        }
        if pcm.sample_rate < 8_000 {
            return Err(format!("unsupported sample rate {}", pcm.sample_rate));
        }
        let frame_samples = ((pcm.sample_rate as u64 * frame_ms) / 1000).max(1) as usize;
        Ok(Self {
            frames: pcm.frames.clone(),
            pos: 0,
            frame_samples,
            sample_rate: pcm.sample_rate,
        })
    }
}

impl LiveSource for PcmFrameStream {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn next_chunk(&mut self) -> Option<Result<Vec<i16>, String>> {
        if self.pos >= self.frames.len() {
            return None;
        }
        let end = (self.pos + self.frame_samples).min(self.frames.len());
        let chunk = self.frames[self.pos..end].to_vec();
        self.pos = end;
        Some(Ok(chunk))
    }
}

/// Device-failure injector: delegates to an inner source for `ok_budget`
/// chunks, then reports a device error. `ok_budget = 0` fails immediately
/// (dead device); larger budgets fail mid-stream (device drops during a
/// live session that already committed words).
pub struct FailAfter<S> {
    inner: S,
    ok_budget: usize,
    served: usize,
    message: String,
}

impl<S> FailAfter<S> {
    /// Fail with `message` after `ok_budget` successful chunks.
    pub fn new(inner: S, ok_budget: usize, message: &str) -> Self {
        Self {
            inner,
            ok_budget,
            served: 0,
            message: message.to_string(),
        }
    }
}

impl<S: LiveSource> LiveSource for FailAfter<S> {
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn next_chunk(&mut self) -> Option<Result<Vec<i16>, String>> {
        if self.served >= self.ok_budget {
            return Some(Err(self.message.clone()));
        }
        match self.inner.next_chunk() {
            Some(Ok(chunk)) => {
                self.served += 1;
                Some(Ok(chunk))
            }
            // A clean end of the inner stream stays clean: the failure
            // injector only injects failures, never converts EOF to error.
            other => other,
        }
    }
}

/// How a live session failed. The variants map to the serve exit codes
/// directly: `Capture` → 2 (input/device error, empty stdout),
/// `Pipeline` → 1 (pipeline failure on valid input, like the batch path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveError {
    /// No frames ever arrived, or the device failed (immediately or
    /// mid-stream). Never silent: the message names the failure.
    Capture(String),
    /// Valid input reached the pipeline but it failed (construct, feed, or
    /// finish — e.g. input too short to commit any word).
    Pipeline(String),
}

impl std::fmt::Display for LiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LiveError::Capture(msg) => write!(f, "live capture failed: {msg}"),
            LiveError::Pipeline(msg) => write!(f, "live pipeline failed: {msg}"),
        }
    }
}

impl std::error::Error for LiveError {}

/// One finished live session: the validated C3 output plus the provisional
/// timeline (committed words after each feed that committed something, in
/// stream order; the last snapshot equals the final transcript words).
#[derive(Debug, Clone)]
pub struct LiveSessionReport {
    /// Validated transcript + stats + repairs + audit (same as batch).
    pub output: StreamingOutput,
    /// Provisional word snapshots, one per committing feed, in order.
    pub provisional: Vec<Vec<String>>,
    /// Raw PCM frames fed in total.
    pub frames_fed: usize,
    /// Live source chunks consumed (excludes the failing chunk, if any).
    pub source_chunks: usize,
}

/// Words committed so far, in stream order.
fn committed_words(pipe: &StreamingPipeline) -> Vec<String> {
    pipe.records().iter().map(|r| r.text.clone()).collect()
}

/// Feed a live source into the C3 pipeline incrementally and finish.
///
/// Returns the validated output with the provisional timeline, or a typed
/// [`LiveError`]. A source that yields zero frames, a device failure at any
/// point, and a stream too short to commit any word are all `Err` — a live
/// session never succeeds silently with nothing to show.
pub fn run_live_session<S: LiveSource>(
    mut source: S,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
    session: SessionMeta,
    config: StreamingConfig,
) -> Result<LiveSessionReport, LiveError> {
    let mut pipe =
        StreamingPipeline::new(source.sample_rate(), baseline, codebook, session, config)
            .map_err(LiveError::Pipeline)?;
    let mut provisional: Vec<Vec<String>> = Vec::new();
    let mut frames_fed = 0usize;
    let mut source_chunks = 0usize;
    let mut saw_chunk = false;
    while let Some(next) = source.next_chunk() {
        let chunk = next.map_err(LiveError::Capture)?;
        saw_chunk = true;
        source_chunks += 1;
        frames_fed += chunk.len();
        let before = pipe.records().len();
        pipe.feed(&chunk).map_err(LiveError::Pipeline)?;
        if pipe.records().len() > before {
            provisional.push(committed_words(&pipe));
        }
    }
    if !saw_chunk {
        return Err(LiveError::Capture(
            "device delivered zero frames (no live input)".to_string(),
        ));
    }
    let output = pipe.finish().map_err(LiveError::Pipeline)?;
    let final_words: Vec<String> = output
        .transcript
        .words
        .iter()
        .map(|w| w.text.clone())
        .collect();
    if provisional.last() != Some(&final_words) {
        provisional.push(final_words);
    }
    Ok(LiveSessionReport {
        output,
        provisional,
        frames_fed,
        source_chunks,
    })
}

#[cfg(test)]
mod live_tests {
    //! MIC-1 hermetic gates (bead hearsay-four-4zx).
    //!
    //! Model-free, fixture audio + synthetic only, offline. The fixture is
    //! the committed `data/demo/sample.wav` courtroom clip (8 s mono
    //! 16 kHz), replayed in mic-sized frames — no physical mic in the gate.
    use super::*;
    use crate::generic_reference_baseline;

    /// First 4 s of the courtroom fixture (2 chunks at 2 s/chunk), exactly
    /// like the serve gates: enough to prove end-to-end transcription.
    fn fixture_4s() -> Pcm16kAudio {
        let full =
            corpus::audio::decode_to_pcm16k(std::path::Path::new("../../data/demo/sample.wav"))
                .expect("fixture decodes");
        let take = ((full.sample_rate as f64 * 4.0) as usize).min(full.frames.len());
        Pcm16kAudio {
            sample_rate: full.sample_rate,
            channels: full.channels,
            frames: full.frames[..take].to_vec(),
        }
    }

    fn live_config() -> StreamingConfig {
        StreamingConfig {
            chunk_secs: 2.0,
            max_secs: 4.0,
            window_width: 0,
        }
    }

    fn live_session() -> SessionMeta {
        SessionMeta {
            speaker_id: "live-test".to_string(),
            recording_ref: "stub-live".to_string(),
            started_ms: 0,
        }
    }

    fn words_of(output: &StreamingOutput) -> Vec<String> {
        output
            .transcript
            .words
            .iter()
            .map(|w| w.text.clone())
            .collect()
    }

    /// Stub-fed live session commits every fixture word in order: the live
    /// run (100 ms synthetic-live frames) matches the batch
    /// `run_streaming_pipeline` words exactly, every word validates, and
    /// the provisional timeline ends at the final transcript.
    #[test]
    fn stub_fed_live_session_commits_all_fixture_words_in_order() {
        let pcm = fixture_4s();
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let expected = crate::run_streaming_pipeline(
            &pcm,
            &baseline,
            &codebook,
            live_session(),
            &live_config(),
        )
        .expect("batch pipeline on fixture");
        assert!(
            !expected.transcript.words.is_empty(),
            "fixture must commit words"
        );

        let source = PcmFrameStream::new(&pcm, 100).expect("100 ms synthetic-live frames");
        let report = run_live_session(source, &baseline, &codebook, live_session(), live_config())
            .expect("stub-fed live session");
        let got = words_of(&report.output);
        assert_eq!(
            got,
            words_of(&expected),
            "live commits all fixture words in order"
        );
        for word in &report.output.transcript.words {
            word.validate()
                .expect("live chain terminates at acoustic evidence");
        }
        assert!(!report.provisional.is_empty(), "provisional timeline kept");
        assert_eq!(
            report.provisional.last().expect("last snapshot"),
            &got,
            "provisional timeline ends at the final transcript"
        );
        // Provisional snapshots grow monotonically toward the final words.
        for snapshot in &report.provisional {
            assert!(
                got.starts_with(snapshot),
                "provisional {snapshot:?} is a prefix of final {got:?}"
            );
        }
    }

    /// Feed granularity never changes the commit: 50 ms, 100 ms, and
    /// 250 ms synthetic-live frames commit identical words (chunk
    /// boundaries stay position-aligned from stream start).
    #[test]
    fn live_frame_granularity_does_not_change_committed_words() {
        let pcm = fixture_4s();
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let mut runs = Vec::new();
        for frame_ms in [50u64, 100, 250] {
            let source = PcmFrameStream::new(&pcm, frame_ms).expect("frame stream");
            let report =
                run_live_session(source, &baseline, &codebook, live_session(), live_config())
                    .expect("live session");
            runs.push(words_of(&report.output));
        }
        assert_eq!(runs[0], runs[1], "50 ms vs 100 ms frames agree");
        assert_eq!(runs[1], runs[2], "100 ms vs 250 ms frames agree");
    }

    /// A dead device is an error, never a silent stall: immediate failure
    /// and mid-stream failure (after real chunks) both return `Capture`
    /// with a message naming the failure.
    #[test]
    fn live_device_error_is_capture_error_never_silent_stall() {
        let pcm = fixture_4s();
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();

        // Immediate: the device fails on the first frame.
        let dead = FailAfter::new(
            PcmFrameStream::new(&pcm, 100).expect("frame stream"),
            0,
            "no default input device",
        );
        let err = run_live_session(dead, &baseline, &codebook, live_session(), live_config())
            .expect_err("dead device must fail");
        assert!(
            matches!(err, LiveError::Capture(_)),
            "dead device is a Capture error (got {err})"
        );
        assert!(
            err.to_string().contains("no default input device"),
            "message names the failure (got {err})"
        );

        // Mid-stream: one good frame lands, then the device drops.
        let dying = FailAfter::new(
            PcmFrameStream::new(&pcm, 100).expect("frame stream"),
            1,
            "capture stream error: device disconnected",
        );
        let err = run_live_session(dying, &baseline, &codebook, live_session(), live_config())
            .expect_err("mid-stream failure must fail");
        assert!(
            matches!(err, LiveError::Capture(_)),
            "mid-stream failure is a Capture error (got {err})"
        );
        assert!(
            err.to_string().contains("device disconnected"),
            "message names the failure (got {err})"
        );
    }

    /// Zero frames is an error, not an empty success: a source that ends
    /// immediately never yields an `Ok` report with nothing in it.
    #[test]
    fn live_empty_stream_is_capture_error() {
        let pcm = Pcm16kAudio {
            sample_rate: 16_000,
            channels: 1,
            frames: Vec::new(),
        };
        let source = PcmFrameStream::new(&pcm, 100).expect("frame stream");
        let err = run_live_session(
            source,
            &generic_reference_baseline(),
            &fse::default_codebook(),
            live_session(),
            live_config(),
        )
        .expect_err("empty stream must fail");
        assert!(
            matches!(err, LiveError::Capture(_)),
            "empty stream is a Capture error (got {err})"
        );
    }
}
