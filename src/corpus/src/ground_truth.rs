//! Oyez ground-truth word-list extractor (bead hearsay-four-e24, lane A1).
//!
//! WER scoring needs reference word lists with timings. The timed segments
//! already committed under `data/oyez-pilot/session.tsv` and
//! `data/oyez-bulk/*/session.tsv` (columns `bucket, idx, start_sec,
//! stop_sec, speaker, drift_tags, text`, emitted by `scripts/oyez_ingest.py`)
//! are the ONLY data source here.
//!
//! Note: [`crate::courtlistener`] carries only a flat `stt_transcript` string
//! with no per-segment timings, so this extractor reads the `session.tsv`
//! manifest-row shape (`start_sec` / `stop_sec` / `text`) — not courtlistener.
//!
//! Pure, model-free, offline: no network, no new data, no filesystem access.
//! Word timing within a segment is an even split of the segment's `[start_ms,
//! end_ms)` span across its normalized words (contiguous, gapless, covering
//! the full span). Normalization: Unicode lowercasing, strip surrounding
//! (leading/trailing) punctuation, drop tokens that normalize to empty.
//! Number-words (tokens containing digits, e.g. `"10-218"`) pass through like
//! any other word — digits are never a reason to drop a token.

/// One timed transcript segment, mirroring a `session.tsv` manifest row's
/// timing + text columns (`start_sec`, `stop_sec`, `text`).
#[derive(Debug, Clone, PartialEq)]
pub struct TimedSegment {
    /// Segment start in seconds (TSV `start_sec`).
    pub start_sec: f64,
    /// Segment end in seconds (TSV `stop_sec`).
    pub stop_sec: f64,
    /// Segment transcript text (TSV `text`).
    pub text: String,
}

impl TimedSegment {
    /// Build a segment from seconds + text.
    pub fn new(start_sec: f64, stop_sec: f64, text: impl Into<String>) -> Self {
        Self {
            start_sec,
            stop_sec,
            text: text.into(),
        }
    }
}

/// Normalize one raw whitespace-separated token: lowercase, strip surrounding
/// punctuation, keep internal characters (hyphens, apostrophes) intact.
/// Returns `None` when nothing remains (pure punctuation / empty input).
/// Number-words pass through: `"10-218,"` -> `Some("10-218")`.
pub fn normalize_word(raw: &str) -> Option<String> {
    let lowered = raw.to_lowercase();
    let stripped = lowered.trim_matches(|c: char| !c.is_alphanumeric());
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

/// Split a segment's text into normalized words, dropping empties.
fn segment_words(text: &str) -> Vec<String> {
    text.split_whitespace().filter_map(normalize_word).collect()
}

/// Extract ground-truth words with millisecond timings from timed rows.
///
/// Each segment's `[start_ms, end_ms)` span (seconds scaled by 1000 and
/// rounded) is split evenly across that segment's normalized words; word `i`
/// of `n` spans
/// `[start + round(dur*i/n), start + round(dur*(i+1)/n))`, so words are
/// contiguous and the last word ends exactly at `end_ms`. Segments with no
/// words contribute nothing. Zero/negative-duration segments collapse their
/// words to zero-width at `start_ms` rather than panicking.
pub fn extract_words(rows: &[TimedSegment]) -> Vec<(String, u64, u64)> {
    let mut out = Vec::new();
    for seg in rows {
        let words = segment_words(&seg.text);
        if words.is_empty() {
            continue;
        }
        let start_ms = (seg.start_sec * 1000.0).round().max(0.0) as u64;
        let end_ms = (seg.stop_sec * 1000.0).round().max(0.0) as u64;
        let dur_ms = end_ms.saturating_sub(start_ms);
        let n = words.len() as f64;
        for (i, word) in words.into_iter().enumerate() {
            let s = start_ms + ((dur_ms as f64 * i as f64 / n).round() as u64);
            let e = start_ms + ((dur_ms as f64 * (i as f64 + 1.0) / n).round() as u64);
            out.push((word, s, e));
        }
    }
    out
}
