//! Provisional vs stable streaming hypotheses (bead hearsay-four-6tc, L6
//! streaming + voice activity endpointing).
//!
//! The decoder emits provisional text that may revise as more audio arrives;
//! VAD endpoint closure promotes a word prefix to stable (committed,
//! append-only). This module is the string-level contract between the two:
//! word-prefix splitting, hypothesis assembly, and NDJSON rendering shaped
//! like the runner's `ProvisionalEvent` display stream. Pure logic, no audio
//! or decoder dependency.

use serde_json::Value;

/// One streaming hypothesis: a stable (endpoint-committed) prefix plus the
/// provisional (may still revise) suffix of the current best text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingHypothesis {
    /// Words promoted by endpoint closure. Append-only: once a word lands
    /// here it never changes.
    pub stable_text: String,
    /// Current best suffix after the stable prefix. May revise on later
    /// audio until its region closes.
    pub provisional_text: String,
    /// Whitespace-separated word count of [`Self::stable_text`].
    pub stable_words: usize,
    /// Whitespace-separated word count of stable + provisional.
    pub total_words: usize,
    /// Last frame index covered (10 ms frames, same clock as
    /// `decoder::Partial::frame_end`).
    pub frame_end: u32,
    /// True when a region closed on the update that produced this
    /// hypothesis (the stable prefix grew).
    pub region_closed: bool,
}

/// Count whitespace-separated words (empty or blank text counts zero).
pub fn word_count(text: &str) -> usize {
    if text.trim().is_empty() {
        0
    } else {
        text.split_whitespace().count()
    }
}

/// Split `full_text` into `(stable, provisional)` at `stable_word_count`
/// words: the stable prefix holds the first N words, the provisional suffix
/// holds the rest. Counts beyond the text length clamp to all-stable; empty
/// input yields two empty strings.
pub fn split_hypothesis(full_text: &str, stable_word_count: usize) -> (String, String) {
    let words: Vec<&str> = full_text.split_whitespace().collect();
    let cut = stable_word_count.min(words.len());
    (words[..cut].join(" "), words[cut..].join(" "))
}

/// Length in words of the longest common whitespace-separated prefix of `a`
/// and `b`.
pub fn common_word_prefix_len(a: &str, b: &str) -> usize {
    a.split_whitespace()
        .zip(b.split_whitespace())
        .take_while(|(x, y)| x == y)
        .count()
}

/// Build a [`StreamingHypothesis`] from the committed stable prefix and the
/// current full best text. The full text is expected to extend the stable
/// prefix (endpointed decoding constructs it that way); if a revision ever
/// crosses the stable boundary, the stable side clamps to the longest common
/// word prefix so the two halves never contradict each other.
pub fn hypothesis_from_texts(
    stable_text: &str,
    full_text: &str,
    frame_end: u32,
    region_closed: bool,
) -> StreamingHypothesis {
    let stable_words = word_count(stable_text);
    let common = common_word_prefix_len(stable_text, full_text);
    let (stable, provisional) = if common >= stable_words {
        split_hypothesis(full_text, stable_words)
    } else {
        split_hypothesis(full_text, common)
    };
    let stable_words = word_count(&stable);
    let total_words = stable_words + word_count(&provisional);
    StreamingHypothesis {
        stable_text: stable,
        provisional_text: provisional,
        stable_words,
        total_words,
        frame_end,
        region_closed,
    }
}

impl StreamingHypothesis {
    /// Stable NDJSON `kind`: `"stable"` when a region closed on this update,
    /// else `"provisional"`.
    pub fn kind_str(&self) -> &'static str {
        if self.region_closed {
            "stable"
        } else {
            "provisional"
        }
    }

    /// Serialize one NDJSON event object (plain `serde_json::Value`).
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "kind": self.kind_str(),
            "stable": self.stable_text,
            "provisional": self.provisional_text,
            "stable_words": self.stable_words,
            "total_words": self.total_words,
            "frame_end": self.frame_end,
            "region_closed": self.region_closed,
        })
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[test]
    fn split_divides_at_word_boundary() {
        let (stable, provisional) = split_hypothesis("THE QUICK BROWN FOX", 2);
        assert_eq!(stable, "THE QUICK");
        assert_eq!(provisional, "BROWN FOX");
    }

    #[test]
    fn split_clamps_beyond_length_and_empty() {
        let (stable, provisional) = split_hypothesis("ONE TWO", 99);
        assert_eq!(stable, "ONE TWO");
        assert_eq!(provisional, "");
        let (stable, provisional) = split_hypothesis("", 0);
        assert_eq!(stable, "");
        assert_eq!(provisional, "");
        let (stable, provisional) = split_hypothesis("   ", 1);
        assert_eq!(stable, "");
        assert_eq!(provisional, "");
    }

    #[test]
    fn hypothesis_keeps_stable_prefix_intact() {
        let h = hypothesis_from_texts("THE QUICK", "THE QUICK BROWN FOX", 42, false);
        assert_eq!(h.stable_text, "THE QUICK");
        assert_eq!(h.provisional_text, "BROWN FOX");
        assert_eq!(h.stable_words, 2);
        assert_eq!(h.total_words, 4);
        assert_eq!(h.frame_end, 42);
        assert!(!h.region_closed);
        assert_eq!(h.kind_str(), "provisional");
    }

    #[test]
    fn hypothesis_clamps_when_revision_crosses_stable() {
        // A revision rewrote the second stable word: stable clamps to the
        // common prefix ("THE") instead of contradicting the full text.
        let h = hypothesis_from_texts("THE QUICK", "THE SLOW BROWN FOX", 7, false);
        assert_eq!(h.stable_text, "THE");
        assert_eq!(h.provisional_text, "SLOW BROWN FOX");
        assert_eq!(h.stable_words, 1);
        assert_eq!(h.total_words, 4);
    }

    #[test]
    fn json_carries_kind_and_counts() {
        let h = hypothesis_from_texts("A B", "A B C", 3, true);
        let v = h.to_json();
        assert_eq!(v["kind"], Value::String("stable".to_string()));
        assert_eq!(v["stable"], Value::String("A B".to_string()));
        assert_eq!(v["provisional"], Value::String("C".to_string()));
        assert_eq!(v["stable_words"], Value::Number(2.into()));
        assert_eq!(v["total_words"], Value::Number(3.into()));
        assert_eq!(v["region_closed"], Value::Bool(true));
    }
}
