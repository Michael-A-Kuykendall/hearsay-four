//! Plain-English why sentences (bead hearsay-four-v8fa--plain-why-sentences).
//!
//! Template-based, deterministic, NO AI: [`render_why_text`] turns a
//! [`WhyThisWord`](crate::why::WhyThisWord) provenance record into 1-2
//! sentences, e.g. `Heard 'family' over 'fly': the sounds scored -812 vs
//! -815, and 'the family' is 40x more likely than 'the fly' in the word
//! model.`
//!
//! Record shapes discovered (bodies read, not guessed):
//! - `output::why::WhyThisWord` (in-crate, the rich path): embeds the
//!   [`WordRecord`](crate::WordRecord) substrate verbatim and adds acoustic
//!   LLR + frames, LM history / sequence LLR / probability, per-class terms,
//!   [`alternatives`](crate::why::WhyThisWord::alternatives) with acoustic +
//!   sequence LLRs, defeat margin, and revivals. This is the primary input.
//! - `runner::demo_transcribe::why_json` (out-of-crate demo panel): a JSON
//!   object with `acoustic_score`, empty `alternatives`, `lm_probability`,
//!   `lm_history`, `lexicon_source`, and TBD fabric events. The
//!   decoder-direct demo path has no fabric tiling, so it cannot build a
//!   `WhyThisWord`; [`render_demo_why_text`] renders its scalar inputs
//!   instead (same templates, same determinism).
//!
//! One-line hookups (not applied here: runner files belong to other lanes):
//! - Fabric path, wherever a `WhyThisWord` is served:
//!   `let text = output::why_text::render_why_text(&why);`
//! - Demo path, in `Transcriber::transcript_json_with_why`
//!   (`src/runner/src/demo_transcribe.rs`, next to the `word["why"]`
//!   assignment):
//!   `word["why_text"] = serde_json::json!(output::why_text::render_demo_why_text(&token.word, token.acoustic_llr, history, prob, self.lexicon_source(&token.word)));`
//!
//! Contract: both renderers are pure total functions (`&` input, `String`
//! output, no `Result`, no panics on `None` fields). Every number rendered
//! comes straight from the record: raw values print verbatim (shortest
//! round-trip), and the LM likelihood ratio is `exp(winner_seq - alt_seq)`
//! from the two recorded sequence LLRs. Sign comparisons pick the template
//! branch; no thresholds, no learned weights.
//!
//! Documented non-goals (as in [`WhyThisWord::render`](crate::why::WhyThisWord::render)
//! the full detail stays available there): the speaker / lexical / verify /
//! user class terms and revival events get no sentence of their own — a 1-2
//! sentence summary covers the head-to-head (acoustic + word model) only.

use crate::why::{AlternativeWhy, WhyThisWord};

/// Shortest round-trip rendering of a record number: integral floats print
/// without decimals (`-812`, not `-812.0000`), matching the panel example.
fn fmt_num(v: f64) -> String {
    format!("{v}")
}

/// Render an LM likelihood ratio: whole when within float noise of an
/// integer (`40x`, not `40.0x`), else one decimal.
fn fmt_ratio(r: f64) -> String {
    let rounded = r.round();
    if (rounded - r).abs() < 1e-9 {
        format!("{rounded}")
    } else {
        format!("{r:.1}")
    }
}

/// LM phrase: history words (skipping `<s>` boundaries) plus the word.
fn phrase(history: &[String], word: &str) -> String {
    let mut parts: Vec<&str> = history
        .iter()
        .filter(|w| w.as_str() != "<s>")
        .map(String::as_str)
        .collect();
    parts.push(word);
    parts.join(" ")
}

/// Plain-English why for one committed word: contested words render one
/// sentence (head-to-head acoustic + word-model clauses), unopposed words
/// render two (verdict + evidence). Pure, total, deterministic.
pub fn render_why_text(why: &WhyThisWord) -> String {
    match why.alternatives.first() {
        Some(alt) => render_contested(why, alt),
        None => render_unopposed(why),
    }
}

/// One sentence: `Heard 'w' over 'a' [margins]: the sounds ..., and ....`
/// Numbers stay winner-first so the two clauses read in the same order.
fn render_contested(why: &WhyThisWord, alt: &AlternativeWhy) -> String {
    let winner = why.record.text.as_str();
    let runner = alt.text.as_str();
    let more = if why.alternatives.len() > 1 {
        format!(" (plus {} more)", why.alternatives.len() - 1)
    } else {
        String::new()
    };
    let margin = match why.defeat.margin {
        Some(m) => format!(" by a margin of {}", fmt_num(m)),
        None => String::new(),
    };
    let acoustic = match (why.acoustic.llr, alt.acoustic_llr) {
        (Some(x), Some(y)) => format!("scored {} vs {}", fmt_num(x), fmt_num(y)),
        (Some(x), None) => format!(
            "scored {} (no sound score recorded for '{runner}')",
            fmt_num(x)
        ),
        (None, Some(y)) => format!(
            "had no sound score recorded for '{winner}' ('{runner}' scored {})",
            fmt_num(y)
        ),
        (None, None) => "had no sound score recorded".to_string(),
    };
    let lm = render_lm_clause(why, alt);
    format!("Heard '{winner}' over '{runner}'{more}{margin}: the sounds {acoustic}, and {lm}.")
}

/// Word-model clause: a likelihood ratio when both sequence LLRs are
/// recorded (larger-first so the ratio is always >= 1x), else the winner's
/// probability or raw score, else an honest absence note.
fn render_lm_clause(why: &WhyThisWord, alt: &AlternativeWhy) -> String {
    let winner = why.record.text.as_str();
    let runner = alt.text.as_str();
    let winner_phrase = phrase(&why.lm.history, winner);
    let runner_phrase = phrase(&why.lm.history, runner);
    match (why.lm.sequence_llr, alt.sequence_llr) {
        (Some(s), Some(t)) if s > t => {
            let ratio = fmt_ratio((s - t).exp());
            format!(
                "'{winner_phrase}' is {ratio}x more likely than '{runner_phrase}' in the word model"
            )
        }
        (Some(s), Some(t)) if s < t => {
            let ratio = fmt_ratio((t - s).exp());
            format!(
                "'{runner_phrase}' is {ratio}x more likely than '{winner_phrase}' in the word model, but '{winner}' still won"
            )
        }
        (Some(s), Some(_)) => {
            format!("the word model scored both equally ({})", fmt_num(s))
        }
        _ => match (why.lm.lm_probability, why.lm.sequence_llr) {
            (Some(p), _) => format!(
                "the word model gives '{winner_phrase}' probability {}",
                fmt_num(p)
            ),
            (None, Some(s)) => format!("the word-model score was {}", fmt_num(s)),
            (None, None) => match alt.sequence_llr {
                Some(t) => format!(
                    "the word-model score was {} for '{runner}' (none recorded for '{winner}')",
                    fmt_num(t)
                ),
                None => "no word-model score was recorded".to_string(),
            },
        },
    }
}

/// Two sentences: the unopposed verdict (with the record's own note), then
/// the acoustic span and word-model probability.
fn render_unopposed(why: &WhyThisWord) -> String {
    let winner = why.record.text.as_str();
    let note = if why.alternatives_note.is_empty() {
        "no runner-up was recorded".to_string()
    } else {
        why.alternatives_note.clone()
    };
    let first = format!("Heard '{winner}' with no challenger ({note}).");
    let acoustic = match why.acoustic.llr {
        Some(a) => format!(
            "The sounds scored {} over frames {}-{}",
            fmt_num(a),
            why.acoustic.start_frame,
            why.acoustic.end_frame
        ),
        None => "No sound score was recorded".to_string(),
    };
    let lm = match (why.lm.lm_probability, why.lm.sequence_llr) {
        (Some(p), _) => {
            let phrase_text = phrase(&why.lm.history, winner);
            format!(
                "the word model gives '{phrase_text}' probability {}",
                fmt_num(p)
            )
        }
        (None, Some(s)) => format!("the word-model score was {}", fmt_num(s)),
        (None, None) => "no word-model score was recorded".to_string(),
    };
    format!("{first} {acoustic}, and {lm}.")
}

/// Plain-English why for the runner demo panel (`why_json` scalars): two
/// sentences covering the acoustic score, the word-model probability, and
/// the lexicon tag. The demo panel records no alternatives and no fabric
/// events, so neither gets a clause. The LM phrase is lowercased: the demo
/// word model operates on lowercase words (see
/// `runner::demo_transcribe::Transcriber::lm_probability`) while lattice
/// words are upper-case.
pub fn render_demo_why_text(
    word: &str,
    acoustic_score: f64,
    history: &[String],
    lm_probability: f64,
    lexicon_source: Option<&str>,
) -> String {
    let phrase_text = phrase(history, word).to_lowercase();
    let second = match lexicon_source {
        Some(src) => format!("It came from the {src} word list."),
        None => "No word-list tag was recorded.".to_string(),
    };
    format!(
        "Heard '{word}': the sounds scored {}, and the word model gives '{phrase_text}' probability {}. {second}",
        fmt_num(acoustic_score),
        fmt_num(lm_probability),
    )
}

#[cfg(test)]
mod tests {
    //! Lane H gates (bead hearsay-four-v8fa--plain-why-sentences): fixture
    //! sentences are byte-exact; every branch (contested / unopposed /
    //! missing scores / LM-tie / multi-alternative / demo panel) is covered.
    //!
    //! Gate: `cd src/output && cargo test -j4 why_text` -> green.
    //! Synthetic fixtures only (hand-built records, no audio, no network).

    use super::*;
    use crate::why::{AcousticWhy, ClassTerm, DefeatWhy, LmWhy};
    use crate::{
        ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus, Node,
        ProvenanceChain, WordRecord,
    };

    /// Minimal substrate record; [`render_why_text`] reads only `text`, so
    /// the chain is a single frame node (no validation needed).
    fn fixture_record(word: &str) -> WordRecord {
        let chain = ProvenanceChain::new(vec![ChainLayer {
            level: ChainLevel::Frame,
            nodes: vec![Node {
                id: 1,
                label: "FRAME:0".to_string(),
                support: vec![],
                active: true,
                rating: 90.0,
            }],
        }]);
        WordRecord::committed(
            word,
            (0, 100),
            0.9,
            Hypothesis::new(word, 0.9, HypothesisStatus::Committed),
            vec![Hypothesis::new("other", 0.4, HypothesisStatus::Retracted)],
            Evidence::new(EvidenceKind::Acoustic, "fixture", 0.8),
            chain,
            80.0,
            "fixture evidence summary",
        )
    }

    fn absent_term(class: &str) -> ClassTerm {
        ClassTerm {
            class: class.to_string(),
            llr: None,
            weight: 1.0,
            weighted: 0.0,
            null: "absent".to_string(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fixture_why(
        word: &str,
        acoustic_llr: Option<f64>,
        history: Vec<String>,
        sequence_llr: Option<f64>,
        lm_probability: Option<f64>,
        alternatives: Vec<AlternativeWhy>,
        alternatives_note: &str,
        margin: Option<f64>,
    ) -> WhyThisWord {
        WhyThisWord {
            record: fixture_record(word),
            token_id: 7,
            acoustic: AcousticWhy {
                llr: acoustic_llr,
                start_frame: 0,
                end_frame: 10,
                start_ms: 0,
                end_ms: 100,
                null: "acoustic-null".to_string(),
                source: None,
            },
            lm: LmWhy {
                history,
                predecessor: None,
                sequence_llr,
                sequence_weighted: 0.0,
                lm_probability,
                null: "sequence-null".to_string(),
            },
            speaker: absent_term("speaker"),
            lexical: absent_term("lexical"),
            verify: absent_term("verify"),
            user: absent_term("user"),
            user_facts: vec![],
            alternatives,
            alternatives_note: alternatives_note.to_string(),
            defeat: DefeatWhy {
                defeating_tiling: vec![word.to_string()],
                defeating_tiling_ids: vec![7],
                defeating_tiling_score: 1.0,
                margin,
                defeated_tokens: vec![],
            },
            revivals: vec![],
            acoustic_weighted: 0.0,
            tiling_token_total: 0.0,
            tiling_total: 1.0,
            lexicon_source: None,
        }
    }

    fn fixture_alt(
        text: &str,
        acoustic_llr: Option<f64>,
        sequence_llr: Option<f64>,
    ) -> AlternativeWhy {
        AlternativeWhy {
            text: text.to_string(),
            token_id: 9,
            tiling_component_total: 0.5,
            acoustic_llr,
            sequence_llr,
        }
    }

    fn the_history() -> Vec<String> {
        vec!["<s>".to_string(), "the".to_string()]
    }

    #[test]
    fn headline_family_over_fly_matches_panel_example() {
        let why = fixture_why(
            "family",
            Some(-812.0),
            the_history(),
            Some(40f64.ln()),
            Some(0.02),
            vec![fixture_alt("fly", Some(-815.0), Some(0.0))],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'family' over 'fly': the sounds scored -812 vs -815, and 'the family' is 40x more likely than 'the fly' in the word model."
        );
    }

    #[test]
    fn sounds_favored_loser_numbers_stay_winner_first() {
        let why = fixture_why(
            "reef",
            Some(2.8),
            vec!["<s>".to_string()],
            Some(0.3),
            Some(0.42),
            vec![fixture_alt("wreath", Some(3.0), Some(0.1))],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'reef' over 'wreath': the sounds scored 2.8 vs 3, and 'reef' is 1.2x more likely than 'wreath' in the word model."
        );
    }

    #[test]
    fn lm_favored_loser_names_winner_anyway() {
        let why = fixture_why(
            "now",
            Some(2.0),
            vec!["<s>".to_string()],
            Some(0.0),
            Some(0.35),
            vec![fixture_alt("how", Some(1.9), Some(3f64.ln()))],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'now' over 'how': the sounds scored 2 vs 1.9, and 'how' is 3x more likely than 'now' in the word model, but 'now' still won."
        );
    }

    #[test]
    fn margin_renders_when_present() {
        let why = fixture_why(
            "family",
            Some(-812.0),
            the_history(),
            Some(40f64.ln()),
            Some(0.02),
            vec![fixture_alt("fly", Some(-815.0), Some(0.0))],
            "",
            Some(3.2),
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'family' over 'fly' by a margin of 3.2: the sounds scored -812 vs -815, and 'the family' is 40x more likely than 'the fly' in the word model."
        );
    }

    #[test]
    fn unopposed_renders_two_sentences_with_record_note() {
        let why = fixture_why(
            "reef",
            Some(2.8),
            vec!["<s>".to_string()],
            Some(0.3),
            Some(0.42),
            vec![],
            "no runner-up tiling: winner was unopposed",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'reef' with no challenger (no runner-up tiling: winner was unopposed). The sounds scored 2.8 over frames 0-10, and the word model gives 'reef' probability 0.42."
        );
    }

    #[test]
    fn alt_missing_acoustic_names_the_gap() {
        let why = fixture_why(
            "family",
            Some(-812.0),
            the_history(),
            Some(40f64.ln()),
            Some(0.02),
            vec![fixture_alt("fly", None, Some(0.0))],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'family' over 'fly': the sounds scored -812 (no sound score recorded for 'fly'), and 'the family' is 40x more likely than 'the fly' in the word model."
        );
    }

    #[test]
    fn alt_missing_sequence_falls_back_to_probability() {
        let why = fixture_why(
            "family",
            Some(-812.0),
            the_history(),
            Some(1.5),
            Some(0.02),
            vec![fixture_alt("fly", Some(-815.0), None)],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'family' over 'fly': the sounds scored -812 vs -815, and the word model gives 'the family' probability 0.02."
        );
    }

    #[test]
    fn lm_tie_scores_both_equally() {
        let why = fixture_why(
            "reef",
            Some(2.8),
            vec!["<s>".to_string()],
            Some(0.3),
            Some(0.42),
            vec![fixture_alt("wreath", Some(3.0), Some(0.3))],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'reef' over 'wreath': the sounds scored 2.8 vs 3, and the word model scored both equally (0.3)."
        );
    }

    #[test]
    fn multiple_alternatives_count_the_rest() {
        let why = fixture_why(
            "reef",
            Some(2.8),
            vec!["<s>".to_string()],
            Some(0.3),
            Some(0.42),
            vec![
                fixture_alt("wreath", Some(3.0), Some(0.1)),
                fixture_alt("reaf", Some(2.0), Some(0.0)),
                fixture_alt("rief", Some(1.0), Some(-0.5)),
            ],
            "",
            None,
        );
        assert_eq!(
            render_why_text(&why),
            "Heard 'reef' over 'wreath' (plus 2 more): the sounds scored 2.8 vs 3, and 'reef' is 1.2x more likely than 'wreath' in the word model."
        );
    }

    #[test]
    fn render_is_deterministic_and_clean() {
        let why = fixture_why(
            "family",
            Some(-812.0),
            the_history(),
            Some(40f64.ln()),
            Some(0.02),
            vec![fixture_alt("fly", Some(-815.0), Some(0.0))],
            "",
            None,
        );
        let first = render_why_text(&why);
        let second = render_why_text(&why);
        assert_eq!(first, second, "template render must be deterministic");
        assert!(first.starts_with("Heard 'family'"));
        assert!(first.ends_with('.'));
        assert!(!first.contains(".."), "no doubled periods");
        assert!(!first.contains("{}"), "no unfilled template braces");
    }

    #[test]
    fn demo_panel_with_lexicon_renders_two_sentences() {
        let text = render_demo_why_text("HELLO", -12.5, &[], 0.0012, Some("cmudict"));
        assert_eq!(
            text,
            "Heard 'HELLO': the sounds scored -12.5, and the word model gives 'hello' probability 0.0012. It came from the cmudict word list."
        );
    }

    #[test]
    fn demo_panel_without_lexicon_names_the_gap() {
        let text = render_demo_why_text("WORLD", -9.75, &["hello".to_string()], 0.5, None);
        assert_eq!(
            text,
            "Heard 'WORLD': the sounds scored -9.75, and the word model gives 'hello world' probability 0.5. No word-list tag was recorded."
        );
    }

    #[test]
    fn demo_panel_skips_sentence_boundaries_in_phrase() {
        let text = render_demo_why_text(
            "WORLD",
            -9.75,
            &["<s>".to_string(), "<s>".to_string()],
            0.5,
            Some("cmudict"),
        );
        assert_eq!(
            text,
            "Heard 'WORLD': the sounds scored -9.75, and the word model gives 'world' probability 0.5. It came from the cmudict word list."
        );
    }
}
