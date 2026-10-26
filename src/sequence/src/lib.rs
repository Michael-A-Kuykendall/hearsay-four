//! Stage 5 sequence plausibility (bead hearsay-four-idw.1).
//!
//! Classical counted bigram table over word sequences: a lookup table, NOT
//! a model. Word sequences come from mwa.2 matching (`WordMatch.word`);
//! training material is the speaker's own enrollment text. Laplace (add-1)
//! smoothing keeps every score a finite probability in `[0,1]` (INV
//! `bounded_scores`). Exactly bigram order (INV `bigram_order`); no neural
//! LM / long-range disambiguation by construction (INV `flat_no_lm`). A
//! table score enters confidence combination (aem.7) as one evidence vote,
//! never a sole decider (INV `score_is_vote`).
//!
//! Reference: docs/design/ngram-sequence.md.
//! Gate path: `cd src/sequence && cargo test ngram`.

use std::collections::{BTreeSet, HashMap};

use arena::Arena;
use fabric::FactId;
use invariant_ppt::assert_invariant;

/// N-gram order: exactly bigram (INV `bigram_order`, ngram-sequence.md §1).
pub const NGRAM_ORDER: usize = 2;

/// Laplace (add-1) smoothing increment (ngram-sequence.md §2).
const LAPLACE_ADD: u64 = 1;

/// Classical counted bigram table over word sequences.
///
/// Counts only: per-predecessor totals plus adjacent-pair counts. No
/// weights, no embeddings, no learned parameters (INV `flat_no_lm`).
#[derive(Debug, Clone, Default)]
pub struct BigramTable {
    /// `count(a)`: bigrams starting with predecessor `a`.
    predecessor_counts: HashMap<String, u64>,
    /// `count(a, b)`: occurrences of the adjacent pair `(a, b)`.
    bigram_counts: HashMap<(String, String), u64>,
    /// Closed vocabulary; its size is `V` in the Laplace denominator.
    vocabulary: BTreeSet<String>,
}

impl BigramTable {
    /// Empty table. Seed or train before scoring (ngram-sequence.md §1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add closed-vocabulary words (e.g. `lexicon::CLOSED_VOCABULARY_WORDS`)
    /// so `V` covers the experiment vocabulary even for unseen words.
    pub fn seed_vocabulary(&mut self, words: &[&str]) {
        for word in words {
            self.vocabulary.insert((*word).to_string());
        }
    }

    /// Train on committed word sequences (mwa.2 `WordMatch.word` values).
    ///
    /// Every observed word joins the vocabulary; every adjacent pair bumps
    /// its bigram count and its predecessor total. Counts only — nothing is
    /// fitted or learned (INV `flat_no_lm`).
    pub fn train(&mut self, sequences: &[Vec<String>]) {
        for sequence in sequences {
            for word in sequence {
                self.vocabulary.insert(word.clone());
            }
            for pair in sequence.windows(NGRAM_ORDER) {
                let (a, b) = (pair[0].clone(), pair[1].clone());
                *self.predecessor_counts.entry(a.clone()).or_insert(0) += 1;
                *self.bigram_counts.entry((a, b)).or_insert(0) += 1;
            }
        }
    }

    /// Vocabulary size `V` (Laplace denominator term).
    pub fn vocabulary_size(&self) -> usize {
        self.vocabulary.len()
    }

    /// Raw count of the adjacent pair `(predecessor, word)`.
    pub fn count(&self, predecessor: &str, word: &str) -> u64 {
        self.bigram_counts
            .get(&(predecessor.to_string(), word.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// Laplace-smoothed `P(word | predecessor)` (ngram-sequence.md §2):
    ///
    /// ```text
    /// P_smoothed(w_b | w_a) = (count(a,b) + 1) / (count(a) + V)
    /// ```
    ///
    /// Always a finite probability in `[0,1]` (INV `bounded_scores`). An
    /// untrained, unseeded table (`V == 0`) scores `1.0`: no evidence
    /// against any pair.
    pub fn bigram_prob(&self, predecessor: &str, word: &str) -> f64 {
        let vocab = self.vocabulary_size() as f64;
        if vocab == 0.0 {
            return 1.0;
        }
        let pair = self.count(predecessor, word) as f64;
        let total = self
            .predecessor_counts
            .get(predecessor)
            .copied()
            .unwrap_or(0) as f64;
        (pair + LAPLACE_ADD as f64) / (total + vocab)
    }

    /// Joint plausibility of a committed word sequence: the product of its
    /// adjacent bigram probabilities. Empty and single-word sequences score
    /// `1.0` (no pair to judge). A product of values in `(0,1]` stays in
    /// `(0,1]` (INV `bounded_scores`).
    pub fn sequence_prob(&self, words: &[String]) -> f64 {
        if words.len() < NGRAM_ORDER {
            return 1.0;
        }
        words
            .windows(NGRAM_ORDER)
            .map(|pair| self.bigram_prob(&pair[0], &pair[1]))
            .product()
    }
}

/// A scored word sequence: plausibility plus its fabric score fact.
///
/// `score_fact` is asserted as `word_facts ⟹ score`, mirroring the mwa.2
/// `syllable_facts ⟹ word` pattern, so the score's justification chain
/// bottoms out in committed word facts (INV analogue:
/// `sequence_inherits_word_justification`).
#[derive(Debug, Clone)]
pub struct SequenceScore {
    /// Joint bigram plausibility in `[0,1]`.
    pub prob: f64,
    /// Fabric fact carrying this score, justified by `word_facts`.
    pub score_fact: FactId,
}

/// Score a committed word sequence against `table`.
///
/// `word_facts` are committed (active) arena facts, one per word, in order.
/// Each returned score's `score_fact` is asserted as
/// `word_facts ⟹ score`.
///
/// No orphan scores: a length mismatch, an unknown fact, or any
/// uncommitted premise yields `None` and interns nothing.
pub fn score_committed_word_sequence(
    arena: &mut Arena,
    word_facts: &[FactId],
    words: &[String],
    table: &BigramTable,
) -> Option<SequenceScore> {
    if words.is_empty() || word_facts.len() != words.len() {
        return None;
    }
    for id in word_facts {
        if arena.content(*id).is_none() || !arena.fabric.is_active(*id) {
            return None;
        }
    }
    let premises: BTreeSet<FactId> = word_facts.iter().copied().collect();
    let prob = table.sequence_prob(words);
    let score_fact = arena.fabric.intern();
    arena.fabric.assert(score_fact, premises, false);
    // INV: sequence_inherits_word_justification.
    assert_invariant(
        arena.fabric.support_count(score_fact) >= 1,
        "sequence_inherits_word_justification",
        Some(&format!("sequence score: {}", words.join(" "))),
    );
    Some(SequenceScore { prob, score_fact })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena::Candidate;
    use lexicon::CLOSED_VOCABULARY_WORDS;

    fn trained_table() -> BigramTable {
        let mut table = BigramTable::new();
        table.train(&[
            vec!["STAKED".to_string(), "PROCESS".to_string()],
            vec!["STAKED".to_string(), "PROCESS".to_string()],
            vec!["STAKED".to_string(), "RIPPLE".to_string()],
        ]);
        table
    }

    fn committed_word(arena: &mut Arena, word: &str) -> FactId {
        let id = arena.ingest(&Candidate::new(word.to_string(), Vec::new(), Vec::new()));
        arena.fabric.assert_assumption(id);
        arena.fabric.commit(id);
        id
    }

    #[test]
    fn ngram_bigram_table_laplace_smoothing() {
        let table = trained_table();
        // c(STAKED)=3, c(STAKED,PROCESS)=2, c(STAKED,RIPPLE)=1, V=3.
        assert_eq!(table.vocabulary_size(), 3);
        assert_eq!(table.count("STAKED", "PROCESS"), 2);
        assert_eq!(table.count("STAKED", "RIPPLE"), 1);
        let eps = 1e-12;
        assert!((table.bigram_prob("STAKED", "PROCESS") - 0.5).abs() < eps);
        assert!((table.bigram_prob("STAKED", "RIPPLE") - 1.0 / 3.0).abs() < eps);
        // Unseen pair: (0 + 1) / (3 + 3) — smoothing, never zero.
        assert!((table.bigram_prob("STAKED", "STAKED") - 1.0 / 6.0).abs() < eps);
        // Unseen predecessor: (0 + 1) / (0 + 3).
        assert!((table.bigram_prob("RIPPLE", "PROCESS") - 1.0 / 3.0).abs() < eps);
    }

    #[test]
    fn ngram_bounded_scores() {
        let mut table = trained_table();
        table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);
        assert_eq!(table.vocabulary_size(), CLOSED_VOCABULARY_WORDS.len());
        let words = ["STAKED", "PROCESS", "RIPPLE", "BYZANTIUM", "NEVER-SEEN"];
        for a in &words {
            for b in &words {
                let p = table.bigram_prob(a, b);
                assert!(p.is_finite(), "non-finite score for ({a}, {b}): {p}");
                assert!((0.0..=1.0).contains(&p), "out-of-range score: {p}");
                assert!(p > 0.0, "zero score despite smoothing: ({a}, {b})");
            }
        }
        let seq = vec!["STAKED".to_string(), "PROCESS".to_string()];
        let p = table.sequence_prob(&seq);
        assert!(p.is_finite() && (0.0..=1.0).contains(&p));
        assert_eq!(table.sequence_prob(&[]), 1.0);
        assert_eq!(table.sequence_prob(&["STAKED".to_string()]), 1.0);
    }

    #[test]
    fn ngram_bigram_order() {
        assert_eq!(NGRAM_ORDER, 2, "INV bigram_order: table is exactly N=2");
        let table = trained_table();
        // Conditioned on the immediate predecessor only: stripping the
        // first pair of a 3-word sequence leaves exactly the shared final
        // factor P(PROCESS|STAKED), whatever the earlier context was.
        let with_ripple = vec![
            "RIPPLE".to_string(),
            "STAKED".to_string(),
            "PROCESS".to_string(),
        ];
        let with_process = vec![
            "PROCESS".to_string(),
            "STAKED".to_string(),
            "PROCESS".to_string(),
        ];
        let eps = 1e-12;
        let shared = table.bigram_prob("STAKED", "PROCESS");
        assert!(
            (table.sequence_prob(&with_ripple) / table.bigram_prob("RIPPLE", "STAKED") - shared)
                .abs()
                < eps
        );
        assert!(
            (table.sequence_prob(&with_process) / table.bigram_prob("PROCESS", "STAKED") - shared)
                .abs()
                < eps
        );
        // And the predecessor matters: P(PROCESS|STAKED) != P(PROCESS|RIPPLE).
        assert!(
            (table.bigram_prob("STAKED", "PROCESS") - table.bigram_prob("RIPPLE", "PROCESS")).abs()
                > eps
        );
    }

    #[test]
    fn ngram_flat_no_lm() {
        // Structural: the manifest must declare no ML crates or weight
        // loading — the table is counts only (INV flat_no_lm).
        let manifest =
            std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
                .expect("crate manifest must be readable");
        for banned in [
            "candle",
            "torch",
            "tch",
            "burn",
            "onnx",
            "tract",
            "tensorflow",
            "transformers",
            "safetensors",
            "tokenizers",
            "hf-hub",
            "embedding",
            "neural",
        ] {
            assert!(
                !manifest.to_lowercase().contains(banned),
                "INV flat_no_lm violated: manifest mentions '{banned}'"
            );
        }
        // Pure count lookup: retraining on the same material reproduces
        // every score bit-for-bit — no hidden learned state.
        let again = trained_table();
        let table = trained_table();
        for a in ["STAKED", "PROCESS", "RIPPLE"] {
            for b in ["STAKED", "PROCESS", "RIPPLE"] {
                assert_eq!(table.bigram_prob(a, b), again.bigram_prob(a, b));
            }
        }
    }

    #[test]
    fn ngram_sequence_inherits_word_justification() {
        let mut arena = Arena::new();
        let table = trained_table();
        let facts = vec![
            committed_word(&mut arena, "STAKED"),
            committed_word(&mut arena, "PROCESS"),
        ];
        let words = vec!["STAKED".to_string(), "PROCESS".to_string()];
        let scored = score_committed_word_sequence(&mut arena, &facts, &words, &table)
            .expect("committed words must score");
        assert!((scored.prob - 0.5).abs() < 1e-12);
        assert!(arena.fabric.support_count(scored.score_fact) >= 1);
        // Rescoring mints a fresh fact justified by the same premises.
        let rescored = score_committed_word_sequence(&mut arena, &facts, &words, &table)
            .expect("rescoring must succeed");
        assert_ne!(scored.score_fact, rescored.score_fact);
        assert!(arena.fabric.support_count(rescored.score_fact) >= 1);
        // Uncommitted premises justify nothing: no justification is added.
        let mut arena2 = Arena::new();
        let pending = arena2.ingest(&Candidate::new(
            "STAKED".to_string(),
            Vec::new(),
            Vec::new(),
        ));
        assert!(
            score_committed_word_sequence(&mut arena2, &[pending], &["STAKED".to_string()], &table)
                .is_none()
        );
        assert_eq!(
            arena2.fabric.support_count(pending),
            0,
            "no orphan score fact"
        );
        // Length mismatch scores nothing.
        assert!(
            score_committed_word_sequence(&mut arena2, &[], &["STAKED".to_string()], &table)
                .is_none()
        );
    }
}
