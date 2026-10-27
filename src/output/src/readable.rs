//! Readable-text restoration + TXT/SRT/VTT exporters (bead hearsay-four-kpz, TXT-1).
//!
//! Words leave the pipeline lowercase with no punctuation and only JSON
//! leaves the binaries; a transcript nobody can read or import is a demo
//! artifact. This module restores classical casing/punctuation and exports
//! plain TXT, SRT, and VTT from [`Transcript`]/[`WordRecord`].
//!
//! Technique (classical only, model-free, offline, synthetic fixtures
//! only):
//! - Sentence boundaries come from two signals: silence gaps (the pause
//!   between the previous word's end and the next word's start reaches
//!   [`ReadableConfig::gap_ms`]) and verification plausibility (a word
//!   whose confidence falls below [`ReadableConfig::min_confidence`] is an
//!   implausible continuation, so it ends the sentence — the classical
//!   repair-point heuristic).
//! - Casing: the first word of each sentence is capitalized. A fixed table
//!   of unambiguous initialisms ([`is_acronym`]) is rendered all-caps
//!   wherever it appears (the acronym guard: sentence-initial position
//!   must not corrupt "fbi" into "Fbi"), and the pronoun "i" renders "I".
//!   Common words that collide with initialisms ("us", "am") are
//!   deliberately NOT in the table, so "tell us" never becomes "tell US".
//! - Punctuation: one period ends each sentence. Commas, question marks,
//!   and exclamation marks are a documented non-goal — gap-only heuristics
//!   cannot detect them without a model, so this module never guesses them.
//!
//! Exporters are total functions returning `String` (no `Result`): the
//! serve path re-validates every provenance chain before anything is
//! printed, so export takes a pre-validated [`Transcript`] as its
//! precondition and never fails on its own.

use crate::{Transcript, WordRecord};

/// Silence gap that ends a sentence, in milliseconds.
///
/// The default (700 ms) sits well above ordinary inter-word pauses inside a
/// read utterance and well below deliberate sentence breaks, so the golden
/// fixture's 1400 ms pause splits while its 0–200 ms word gaps do not.
pub const DEFAULT_GAP_MS: u64 = 700;

/// Verification-plausibility floor: a word with confidence strictly below
/// this ends the sentence even when the following pause is short.
pub const DEFAULT_MIN_CONFIDENCE: f32 = 0.5;

/// Knobs for sentence-boundary detection ([`is_sentence_boundary`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReadableConfig {
    /// Pause in ms between words that ends a sentence.
    pub gap_ms: u64,
    /// A word with confidence below this ends the sentence.
    pub min_confidence: f32,
}

impl ReadableConfig {
    /// Build an explicit configuration.
    pub fn new(gap_ms: u64, min_confidence: f32) -> Self {
        Self {
            gap_ms,
            min_confidence,
        }
    }
}

impl Default for ReadableConfig {
    fn default() -> Self {
        Self {
            gap_ms: DEFAULT_GAP_MS,
            min_confidence: DEFAULT_MIN_CONFIDENCE,
        }
    }
}

/// Fixed classical table of unambiguous initialisms, lowercase.
///
/// Every entry must NOT be a common English word: "us" (pronoun) and "am"
/// (verb) are deliberately absent, so the guard never corrupts ordinary
/// text. No learning, no weights — a lookup table.
const ACRONYMS: &[&str] = &[
    "fbi", "cia", "nsa", "nasa", "nato", "scotus", "potus", "dhs", "doj", "irs", "epa", "fda",
    "cdc", "nypd", "lapd", "swat", "usa", "uk", "eu", "dna", "gps", "faq", "tv", "sos",
];

/// Acronym guard: true when the word (case-insensitive) is a tabled
/// initialism and must render all-caps.
pub fn is_acronym(word: &str) -> bool {
    ACRONYMS.contains(&word.to_lowercase().as_str())
}

/// Decide whether `prev` ends a sentence: the pause before `next` reaches
/// [`ReadableConfig::gap_ms`], or `prev` is below the verification/
/// plausibility floor ([`ReadableConfig::min_confidence`]). Pure rule —
/// counts and thresholds only, no model.
pub fn is_sentence_boundary(prev: &WordRecord, next: &WordRecord, config: &ReadableConfig) -> bool {
    let gap = next.start_ms.saturating_sub(prev.end_ms);
    gap >= config.gap_ms || prev.confidence < config.min_confidence
}

/// Render one raw (lowercase, unpunctuated) pipeline word with restored
/// casing. `sentence_initial` capitalizes the first word of a sentence;
/// the acronym guard and the pronoun "i" apply in any position.
fn render_word(raw: &str, sentence_initial: bool) -> String {
    let lowered = raw.to_lowercase();
    if is_acronym(&lowered) {
        return lowered.to_uppercase();
    }
    if lowered == "i" {
        return "I".to_string();
    }
    if sentence_initial {
        let mut chars = lowered.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        }
    } else {
        lowered
    }
}

/// One restored sentence: display text with its acoustic span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredSentence {
    /// Cased text with a terminal period (e.g. "The FBI agent spoke.").
    pub text: String,
    /// Start of the first word's acoustic span.
    pub start_ms: u64,
    /// End of the last word's acoustic span.
    pub end_ms: u64,
}

/// Restore cased, punctuated sentences from a transcript's word stream.
///
/// Words are grouped at [`is_sentence_boundary`] splits; each group renders
/// its words with [`render_word`] casing plus one terminal period. An empty
/// transcript yields no sentences.
pub fn restore_sentences(
    transcript: &Transcript,
    config: &ReadableConfig,
) -> Vec<RestoredSentence> {
    let words = &transcript.words;
    if words.is_empty() {
        return Vec::new();
    }
    // Split points: index of the first word of each sentence after the first.
    let mut starts = vec![0usize];
    for i in 0..words.len().saturating_sub(1) {
        if is_sentence_boundary(&words[i], &words[i + 1], config) {
            starts.push(i + 1);
        }
    }
    starts.push(words.len());

    starts
        .windows(2)
        .map(|pair| {
            let group = &words[pair[0]..pair[1]];
            let rendered: Vec<String> = group
                .iter()
                .enumerate()
                .map(|(k, w)| render_word(&w.text, k == 0))
                .collect();
            RestoredSentence {
                text: format!("{}.", rendered.join(" ")),
                start_ms: group.first().map(|w| w.start_ms).unwrap_or(0),
                end_ms: group.last().map(|w| w.end_ms).unwrap_or(0),
            }
        })
        .collect()
}

/// Plain-text export: restored sentences joined by spaces, one trailing
/// newline. Empty transcript yields the empty string.
pub fn to_txt(transcript: &Transcript, config: &ReadableConfig) -> String {
    let sentences = restore_sentences(transcript, config);
    if sentences.is_empty() {
        return String::new();
    }
    let mut out: String = sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    out.push('\n');
    out
}

/// Format a millisecond timestamp as `HH:MM:SS,mmm` (SRT cue timing).
pub fn format_srt_timestamp(ms: u64) -> String {
    let (hours, rem) = (ms / 3_600_000, ms % 3_600_000);
    let (minutes, rem) = (rem / 60_000, rem % 60_000);
    let (seconds, millis) = (rem / 1_000, rem % 1_000);
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// Format a millisecond timestamp as `HH:MM:SS.mmm` (VTT cue timing).
pub fn format_vtt_timestamp(ms: u64) -> String {
    let (hours, rem) = (ms / 3_600_000, ms % 3_600_000);
    let (minutes, rem) = (rem / 60_000, rem % 60_000);
    let (seconds, millis) = (rem / 1_000, rem % 1_000);
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// SRT subtitle export: one cue per restored sentence, numbered from 1,
/// timed by the sentence's acoustic span. Empty transcript yields "".
pub fn to_srt(transcript: &Transcript, config: &ReadableConfig) -> String {
    let sentences = restore_sentences(transcript, config);
    let cues: Vec<String> = sentences
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{}\n{} --> {}\n{}",
                i + 1,
                format_srt_timestamp(s.start_ms),
                format_srt_timestamp(s.end_ms),
                s.text
            )
        })
        .collect();
    if cues.is_empty() {
        return String::new();
    }
    let mut out = cues.join("\n\n");
    out.push('\n');
    out
}

/// WebVTT subtitle export: `WEBVTT` header, one cue per restored sentence.
/// An empty transcript still carries the mandatory header (`"WEBVTT\n\n"`).
pub fn to_vtt(transcript: &Transcript, config: &ReadableConfig) -> String {
    let sentences = restore_sentences(transcript, config);
    let mut out = String::from("WEBVTT\n\n");
    if sentences.is_empty() {
        return out;
    }
    let cues: Vec<String> = sentences
        .iter()
        .map(|s| {
            format!(
                "{} --> {}\n{}",
                format_vtt_timestamp(s.start_ms),
                format_vtt_timestamp(s.end_ms),
                s.text
            )
        })
        .collect();
    out.push_str(&cues.join("\n\n"));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    //! TXT-1 gates (bead hearsay-four-kpz): golden-fixture exports are
    //! byte-exact in all three formats; restoration rules (boundary
    //! detection, capitalization, acronym guard) are unit-tested.
    //!
    //! Gate: `cd src/output && cargo test --offline readable` -> green.
    //! Synthetic fixtures only (hand-built records, no audio, no network).

    use super::*;
    use crate::{
        ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus,
        JustificationRef, Node, ProvenanceChain, SessionMeta,
    };

    /// Synthetic record with an explicit span and confidence: a minimal
    /// valid chain (frame -> phoneme -> lexical) so the fixture mirrors
    /// committed pipeline records.
    fn fixture_record(
        word: &str,
        start_ms: u64,
        end_ms: u64,
        confidence: f32,
        id: u64,
    ) -> WordRecord {
        let chain = ProvenanceChain::new(vec![
            ChainLayer {
                level: ChainLevel::Frame,
                nodes: vec![Node {
                    id,
                    label: format!("FRAME:{id}"),
                    support: vec![],
                    active: true,
                    rating: 90.0,
                }],
            },
            ChainLayer {
                level: ChainLevel::Phoneme,
                nodes: vec![Node {
                    id: id + 1,
                    label: "/t/".to_string(),
                    support: vec![JustificationRef {
                        justification_id: format!("j-phon-{id}"),
                        premises: vec![id],
                        unconditional: false,
                    }],
                    active: true,
                    rating: 85.0,
                }],
            },
            ChainLayer {
                level: ChainLevel::Lexical,
                nodes: vec![Node {
                    id: id + 2,
                    label: format!("WORD:{word}"),
                    support: vec![JustificationRef {
                        justification_id: format!("j-lex-{id}"),
                        premises: vec![id + 1],
                        unconditional: false,
                    }],
                    active: true,
                    rating: 80.0,
                }],
            },
        ]);
        WordRecord::committed(
            word,
            (start_ms, end_ms),
            confidence,
            Hypothesis::new(word, confidence, HypothesisStatus::Committed),
            vec![Hypothesis::new("other", 0.4, HypothesisStatus::Retracted)],
            Evidence::new(EvidenceKind::Acoustic, "synthetic fixture", 0.7),
            chain,
            85.0,
            "2 channels; log-odds 1.7346; rating 85.00",
        )
    }

    /// Golden fixture: two sentences split by a 1400 ms silence gap,
    /// exercising capitalization, the pronoun "i", and the acronym guard
    /// ("nasa") inside ordinary words.
    fn golden_transcript() -> Transcript {
        Transcript::new(
            vec![
                fixture_record("the", 0, 200, 0.9, 1),
                fixture_record("court", 200, 400, 0.9, 10),
                fixture_record("is", 400, 600, 0.9, 20),
                fixture_record("in", 600, 800, 0.9, 30),
                fixture_record("session", 800, 1100, 0.9, 40),
                fixture_record("i", 2500, 2600, 0.9, 50),
                fixture_record("met", 2600, 2800, 0.9, 60),
                fixture_record("the", 2800, 2950, 0.9, 70),
                fixture_record("nasa", 2950, 3150, 0.9, 80),
                fixture_record("agent", 3150, 3400, 0.9, 90),
            ],
            SessionMeta {
                speaker_id: "synthetic".to_string(),
                recording_ref: "fixture".to_string(),
                started_ms: 0,
            },
            None,
        )
    }

    #[test]
    fn golden_fixture_exports_txt_byte_exact() {
        let transcript = golden_transcript();
        assert!(transcript.words.iter().all(|w| w.validate().is_ok()));
        let txt = to_txt(&transcript, &ReadableConfig::default());
        assert_eq!(txt, "The court is in session. I met the NASA agent.\n");
    }

    #[test]
    fn golden_fixture_exports_srt_byte_exact() {
        let srt = to_srt(&golden_transcript(), &ReadableConfig::default());
        assert_eq!(
            srt,
            "1\n\
             00:00:00,000 --> 00:00:01,100\n\
             The court is in session.\n\
             \n\
             2\n\
             00:00:02,500 --> 00:00:03,400\n\
             I met the NASA agent.\n"
        );
    }

    #[test]
    fn golden_fixture_exports_vtt_byte_exact() {
        let vtt = to_vtt(&golden_transcript(), &ReadableConfig::default());
        assert_eq!(
            vtt,
            "WEBVTT\n\
             \n\
             00:00:00.000 --> 00:00:01.100\n\
             The court is in session.\n\
             \n\
             00:00:02.500 --> 00:00:03.400\n\
             I met the NASA agent.\n"
        );
    }

    #[test]
    fn boundary_gap_splits_at_threshold() {
        let config = ReadableConfig::new(700, 0.5);
        let first = fixture_record("hello", 0, 500, 0.9, 1);
        // Gap exactly at the threshold splits (>=).
        let at = fixture_record("world", 1200, 1400, 0.9, 10);
        assert!(is_sentence_boundary(&first, &at, &config));
        // One millisecond under does not.
        let under = fixture_record("world", 1199, 1400, 0.9, 10);
        assert!(!is_sentence_boundary(&first, &under, &config));
    }

    #[test]
    fn boundary_low_confidence_splits_without_gap() {
        let config = ReadableConfig::new(700, 0.5);
        let weak = fixture_record("um", 0, 200, 0.2, 1);
        let next = fixture_record("world", 250, 450, 0.9, 10);
        assert!(
            is_sentence_boundary(&weak, &next, &config),
            "implausible word ends the sentence despite a 50 ms gap"
        );
        // Confidence exactly at the floor is still plausible (>= keeps).
        let edge = fixture_record("um", 0, 200, 0.5, 1);
        assert!(!is_sentence_boundary(&edge, &next, &config));
    }

    #[test]
    fn capitalization_first_word_only_plus_pronoun() {
        let transcript = Transcript::new(
            vec![
                fixture_record("hello", 0, 200, 0.9, 1),
                fixture_record("i", 200, 300, 0.9, 10),
                fixture_record("am", 300, 500, 0.9, 20),
                fixture_record("here", 500, 700, 0.9, 30),
            ],
            SessionMeta {
                speaker_id: "synthetic".to_string(),
                recording_ref: "fixture".to_string(),
                started_ms: 0,
            },
            None,
        );
        let sentences = restore_sentences(&transcript, &ReadableConfig::default());
        assert_eq!(sentences.len(), 1);
        // "am" is NOT an acronym: the verb stays lowercase; "i" -> "I".
        assert_eq!(sentences[0].text, "Hello I am here.");
    }

    #[test]
    fn acronym_guard_keeps_initialisms_uppercase() {
        assert!(is_acronym("fbi"));
        assert!(is_acronym("FBI"));
        assert!(is_acronym("nasa"));
        // Common words that collide with initial spellings stay lowercase.
        assert!(!is_acronym("us"), "\"us\" pronoun is not an acronym");
        assert!(!is_acronym("am"), "\"am\" verb is not an acronym");
        let transcript = Transcript::new(
            vec![
                fixture_record("fbi", 0, 300, 0.9, 1),
                fixture_record("agent", 300, 600, 0.9, 10),
                fixture_record("told", 600, 800, 0.9, 20),
                fixture_record("us", 800, 1000, 0.9, 30),
            ],
            SessionMeta {
                speaker_id: "synthetic".to_string(),
                recording_ref: "fixture".to_string(),
                started_ms: 0,
            },
            None,
        );
        let sentences = restore_sentences(&transcript, &ReadableConfig::default());
        assert_eq!(sentences.len(), 1);
        // Sentence-initial "fbi" renders "FBI" (not "Fbi"); "us" is untouched.
        assert_eq!(sentences[0].text, "FBI agent told us.");
    }

    #[test]
    fn sentence_spans_cover_word_spans() {
        let sentences = restore_sentences(&golden_transcript(), &ReadableConfig::default());
        assert_eq!(sentences.len(), 2);
        assert_eq!((sentences[0].start_ms, sentences[0].end_ms), (0, 1100));
        assert_eq!((sentences[1].start_ms, sentences[1].end_ms), (2500, 3400));
    }

    #[test]
    fn timestamp_formatting_pads_fields() {
        assert_eq!(format_srt_timestamp(0), "00:00:00,000");
        assert_eq!(format_srt_timestamp(3_723_001), "01:02:03,001");
        assert_eq!(format_vtt_timestamp(0), "00:00:00.000");
        assert_eq!(format_vtt_timestamp(3_723_001), "01:02:03.001");
    }

    #[test]
    fn empty_transcript_exports_stay_well_formed() {
        let empty = Transcript::new(
            vec![],
            SessionMeta {
                speaker_id: "synthetic".to_string(),
                recording_ref: "fixture".to_string(),
                started_ms: 0,
            },
            None,
        );
        let config = ReadableConfig::default();
        assert_eq!(to_txt(&empty, &config), "");
        assert_eq!(to_srt(&empty, &config), "");
        assert_eq!(to_vtt(&empty, &config), "WEBVTT\n\n");
    }
}
