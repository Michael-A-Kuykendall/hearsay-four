//! Large-vocab trie source: LM top-N by frequency plus book words.
//!
//! [`build_entries`] unions the LM's top-`top_n` words by unigram count with
//! the book word types (the personal LM's prediction vocabulary, or
//! normalized `--book` text), then resolves pronunciations through the
//! merged [`lexicon::TieredLexicon`]. Words without a lexicon entry are OOV:
//! dropped from the trie and counted loudly in [`VocabReport`] (no G2P
//! model: no-model constraint).

use std::collections::{BTreeMap, BTreeSet};

use lm::KnTrigram;

use crate::trie::PronEntry;

/// How the book word list was sourced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSource {
    /// Personal (book) component of an interpolated LM file.
    PersonalLm,
    /// Normalized `--book` text file.
    BookText,
}

/// Counts describing one built vocabulary (counts only, no word lists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabReport {
    /// Requested top-N (kept words can exceed it via the book union).
    pub top_n_requested: usize,
    /// LM top-N candidates (lowercased types).
    pub lm_candidates: usize,
    /// Book word types seen.
    pub book_types: usize,
    /// Where the book word list came from.
    pub book_source: BookSource,
    /// Lowercased union types before the lexicon filter.
    pub union_types: usize,
    /// Distinct UPPER-CASE words with >= 1 pronunciation.
    pub kept_words: usize,
    /// Total pronunciation entries (multi-pron words expand).
    pub kept_prons: usize,
    /// Kept words supplied by tier-1 (CMUdict).
    pub tier1_words: usize,
    /// Kept words supplied by tier-2 (librispeech-g2p).
    pub tier2_words: usize,
    /// Top-N words without a lexicon entry (LM members take precedence).
    pub oov_lm: usize,
    /// Book-only words without a lexicon entry.
    pub oov_book_only: usize,
}

/// True for LM special tokens (never trie words).
fn is_special(word: &str) -> bool {
    word == lm::EOS_TOKEN || word == lm::UNK_TOKEN || word == lm::NUM_TOKEN
}

/// Top-`n` LM words by unigram count, lowercased, special tokens excluded.
///
/// `personal` (the book component of an interpolated LM) contributes its
/// counts and its personal-only words, so the ranking reflects the
/// interpolated training distribution. Deterministic: count desc, then
/// alphabetical.
pub fn top_lm_words(general: &KnTrigram, personal: Option<&KnTrigram>, n: usize) -> Vec<String> {
    let mut union: BTreeSet<String> = general
        .prediction_vocabulary()
        .into_iter()
        .map(str::to_string)
        .collect();
    if let Some(book) = personal {
        union.extend(book.prediction_vocabulary().into_iter().map(str::to_string));
    }
    let mut ranked: Vec<(String, u64)> = union
        .into_iter()
        .filter(|w| !is_special(w))
        .map(|w| {
            let count =
                general.count_unigram(&w) + personal.map(|b| b.count_unigram(&w)).unwrap_or(0);
            (w, count)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(n);
    ranked.into_iter().map(|(w, _)| w).collect()
}

/// Book word types from the personal LM's prediction vocabulary.
pub fn book_types_from_personal(personal: &KnTrigram) -> BTreeSet<String> {
    personal
        .prediction_vocabulary()
        .into_iter()
        .map(str::to_string)
        .filter(|w| !is_special(w))
        .collect()
}

/// Book word types from raw book text (fallback when no personal LM).
pub fn book_types_from_text(book_text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in book_text.lines() {
        for sent in lm::normalize_text(line) {
            out.extend(sent);
        }
    }
    out.retain(|w| !is_special(w));
    out
}

/// Resolve the union vocab to pronunciations through the tiered lexicon.
///
/// Lexicon lookup is UPPER-CASE; each kept word emits one [`PronEntry`] per
/// pronunciation (separate trie paths, per the 71g pronunciation pin).
/// OOV words are dropped and counted in the report, never guessed.
pub fn build_entries(
    lm_top: &[String],
    book: &BTreeSet<String>,
    top_n_requested: usize,
    book_source: BookSource,
    lexicon: &lexicon::TieredLexicon,
) -> (Vec<PronEntry>, VocabReport) {
    let lm_set: BTreeSet<&str> = lm_top.iter().map(String::as_str).collect();
    let mut union: BTreeSet<&str> = lm_set.clone();
    union.extend(book.iter().map(String::as_str));
    // Distinct UPPER-CASE words (lowercase collisions merge onto one form).
    let mut by_upper: BTreeMap<String, &str> = BTreeMap::new();
    for word in &union {
        by_upper.entry(word.to_uppercase()).or_insert(word);
    }
    let mut entries: Vec<PronEntry> = Vec::new();
    let mut kept_words = 0usize;
    let mut tier1_words = 0usize;
    let mut tier2_words = 0usize;
    let mut oov_lm = 0usize;
    let mut oov_book_only = 0usize;
    for (upper, lower) in &by_upper {
        match lexicon.table.get(upper) {
            Some(prons) => {
                kept_words += 1;
                match lexicon.sources.get(upper) {
                    Some(lexicon::LexiconSource::Cmudict) => tier1_words += 1,
                    Some(lexicon::LexiconSource::LibrispeechG2p) => tier2_words += 1,
                    None => {}
                }
                for pron in prons {
                    entries.push(PronEntry {
                        word: upper.clone(),
                        phones: pron.clone(),
                    });
                }
            }
            None => {
                if lm_set.contains(lower) {
                    oov_lm += 1;
                } else {
                    oov_book_only += 1;
                }
            }
        }
    }
    let report = VocabReport {
        top_n_requested,
        lm_candidates: lm_top.len(),
        book_types: book.len(),
        book_source,
        union_types: union.len(),
        kept_words,
        kept_prons: entries.len(),
        tier1_words,
        tier2_words,
        oov_lm,
        oov_book_only,
    };
    (entries, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sents(texts: &[&str]) -> Vec<Vec<String>> {
        texts
            .iter()
            .map(|t| t.split(' ').map(str::to_string).collect())
            .collect()
    }

    #[test]
    fn top_n_orders_by_combined_count() {
        let general = KnTrigram::train(&sents(&["b b b", "a a", "c"]));
        assert_eq!(
            top_lm_words(&general, None, 2),
            vec!["b".to_string(), "a".to_string()]
        );
        let personal = KnTrigram::train(&sents(&["c c c c", "d"]));
        // Combined: c=5, b=3, a=2, d=1; personal-only d included.
        assert_eq!(
            top_lm_words(&general, Some(&personal), 4),
            vec![
                "c".to_string(),
                "b".to_string(),
                "a".to_string(),
                "d".to_string()
            ]
        );
        // Special tokens never surface.
        for w in top_lm_words(&general, Some(&personal), 100) {
            assert!(!is_special(&w), "special token leaked: {w}");
        }
    }

    #[test]
    fn top_n_ties_break_alphabetically() {
        let general = KnTrigram::train(&sents(&["b", "a"]));
        assert_eq!(
            top_lm_words(&general, None, 2),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn book_types_from_text_normalizes() {
        let types = book_types_from_text("Hello, HELLO! Quilter's 42.\n\n");
        assert_eq!(
            types,
            BTreeSet::from(["hello".to_string(), "quilter's".to_string()])
        );
    }

    #[test]
    fn build_entries_resolves_tiers_and_counts_oov() {
        let tier1 = "HOTBED HH AA T B EH D\nRIPPLE R IH P AH L\nRIPPLE R IH P UL\n";
        let tier2 = "QUILTER K W IH L T ER\n";
        let lex = lexicon::ingest_tiered(tier1, tier2);
        let lm_top = vec!["hotbed".to_string(), "missing".to_string()];
        let book: BTreeSet<String> = BTreeSet::from([
            "ripple".to_string(),
            "quilter".to_string(),
            "ghost".to_string(),
        ]);
        let (entries, report) = build_entries(&lm_top, &book, 2, BookSource::BookText, &lex);
        // hotbed(t1) + ripple(t1, 2 prons) + quilter(t2); missing/ghost OOV.
        assert_eq!(report.kept_words, 3);
        assert_eq!(report.kept_prons, 4);
        assert_eq!(report.tier1_words, 2);
        assert_eq!(report.tier2_words, 1);
        assert_eq!(report.oov_lm, 1);
        assert_eq!(report.oov_book_only, 1);
        assert_eq!(report.union_types, 5);
        assert_eq!(entries.len(), 4);
        assert!(entries.iter().all(|e| e.word == e.word.to_uppercase()));
    }
}
