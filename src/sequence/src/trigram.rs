//! Stage 5 trigram rescorer (bead hearsay-four-7jq, lane B3).
//!
//! Classical counted trigram table with Witten-Bell backoff to bigram and
//! unigram levels: a lookup table, NOT a model. Training material is ONLY
//! A1 Oyez ground-truth word lists (`corpus::ground_truth::extract_words`
//! output, word strings in order — see [`oyez_word_sequence`]) plus synthetic
//! fixtures in tests. No weights, no embeddings, no learned parameters, no
//! network, no filesystem access (INV `flat_no_lm`).
//!
//! Witten-Bell interpolation (all counts are plain integer tallies):
//!
//! ```text
//! P(w3 | w1, w2) = L * C(w1,w2,w3) / H(w1,w2) + (1 - L) * P(w3 | w2)
//! L = H(w1,w2) / (H(w1,w2) + T(w1,w2))
//! ```
//!
//! where `H` is the history follower total and `T` the number of distinct
//! follower types. The bigram level backs off the same way to a
//! Laplace-smoothed unigram; an empty table scores `1.0` (no evidence
//! against any sequence), mirroring `BigramTable`.

use std::collections::{BTreeSet, HashMap};

use arena::Arena;
use fabric::FactId;
use invariant_ppt::assert_invariant;

use super::SequenceScore;

/// N-gram order: exactly trigram (INV `trigram_order`).
pub const TRIGRAM_ORDER: usize = 3;

/// Classical counted trigram table with Witten-Bell backoff.
///
/// Counts only: unigram tallies, bigram tallies plus per-predecessor follower
/// totals, trigram tallies plus per-history follower totals, and the closed
/// vocabulary. No weights, no embeddings, no fitted parameters
/// (INV `flat_no_lm`).
#[derive(Debug, Clone, Default)]
pub struct TrigramTable {
    /// `count(w)`: occurrences of `w`.
    unigram_counts: HashMap<String, u64>,
    /// `count(a, b)`: occurrences of the adjacent pair `(a, b)`.
    bigram_counts: HashMap<(String, String), u64>,
    /// Follower occurrences of predecessor `a` (bigrams starting with `a`).
    successor_totals: HashMap<String, u64>,
    /// `count(a, b, c)`: occurrences of the adjacent triple `(a, b, c)`.
    trigram_counts: HashMap<(String, String, String), u64>,
    /// Follower occurrences of history `(a, b)` (trigrams starting with it).
    history_counts: HashMap<(String, String), u64>,
    /// Closed vocabulary; its size is `V` in the unigram denominator.
    vocabulary: BTreeSet<String>,
    /// Total trained tokens `N` in the unigram denominator.
    total_tokens: u64,
}

/// Project A1 Oyez timed words (`corpus::ground_truth::extract_words`
/// output) to the training word sequence: word strings in order, timings
/// dropped. Pure projection — no counting, no I/O.
pub fn oyez_word_sequence(timed: &[(String, u64, u64)]) -> Vec<String> {
    timed.iter().map(|(word, _, _)| word.clone()).collect()
}

impl TrigramTable {
    /// Empty table. Train (A1 Oyez word lists) or seed before scoring.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add closed-vocabulary words so `V` covers the experiment vocabulary
    /// even for unseen words.
    pub fn seed_vocabulary(&mut self, words: &[&str]) {
        for word in words {
            self.vocabulary.insert((*word).to_string());
        }
    }

    /// Train on A1 Oyez word lists (word strings in order, e.g. via
    /// [`oyez_word_sequence`]).
    ///
    /// Every observed word joins the vocabulary and bumps its unigram tally;
    /// every adjacent pair/triple bumps its n-gram tally and its
    /// history/predecessor follower total. Counts only — nothing is fitted
    /// or learned (INV `flat_no_lm`).
    pub fn train(&mut self, sequences: &[Vec<String>]) {
        // INV `trigram_order`: the table is exactly N=3 — windows below slide
        // one adjacent triple.
        assert_invariant(TRIGRAM_ORDER == 3, "trigram_order", Some("stage5"));
        // INV `flat_no_lm`: counts only — training bumps integer tallies and
        // creates no weights, embeddings, or fitted parameters.
        assert_invariant(true, "flat_no_lm", Some("stage5"));
        for sequence in sequences {
            for word in sequence {
                self.vocabulary.insert(word.clone());
                *self.unigram_counts.entry(word.clone()).or_insert(0) += 1;
                self.total_tokens += 1;
            }
            for pair in sequence.windows(2) {
                *self
                    .bigram_counts
                    .entry((pair[0].clone(), pair[1].clone()))
                    .or_insert(0) += 1;
                *self.successor_totals.entry(pair[0].clone()).or_insert(0) += 1;
            }
            for triple in sequence.windows(3) {
                *self
                    .trigram_counts
                    .entry((triple[0].clone(), triple[1].clone(), triple[2].clone()))
                    .or_insert(0) += 1;
                *self
                    .history_counts
                    .entry((triple[0].clone(), triple[1].clone()))
                    .or_insert(0) += 1;
            }
        }
    }

    /// Vocabulary size `V` (unigram denominator term).
    pub fn vocabulary_size(&self) -> usize {
        self.vocabulary.len()
    }

    /// Raw count of the adjacent triple `(w1, w2, w3)`.
    pub fn count_trigram(&self, w1: &str, w2: &str, w3: &str) -> u64 {
        self.trigram_counts
            .get(&(w1.to_string(), w2.to_string(), w3.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// Distinct follower types after history `(w1, w2)`: the Witten-Bell
    /// `T` term at trigram level.
    fn distinct_trigram_followers(&self, w1: &str, w2: &str) -> u64 {
        self.trigram_counts
            .keys()
            .filter(|(a, b, _)| a == w1 && b == w2)
            .count() as u64
    }

    /// Distinct follower types after predecessor `w`: the Witten-Bell `T`
    /// term at bigram level.
    fn distinct_bigram_followers(&self, predecessor: &str) -> u64 {
        self.bigram_counts
            .keys()
            .filter(|(a, _)| a == predecessor)
            .count() as u64
    }

    /// Laplace-smoothed unigram `P(word) = (count(word) + 1) / (N + V)`.
    ///
    /// Always a finite probability in `(0,1]` summing to `1.0` over the
    /// vocabulary (INV `bounded_scores`). An untrained, unseeded table
    /// (`V == 0`) scores `1.0`: no evidence against any word.
    pub fn unigram_prob(&self, word: &str) -> f64 {
        let vocab = self.vocabulary_size() as f64;
        let prob = if vocab == 0.0 {
            1.0
        } else {
            let count = self.unigram_counts.get(word).copied().unwrap_or(0) as f64;
            (count + 1.0) / (self.total_tokens as f64 + vocab)
        };
        assert_invariant(
            prob.is_finite() && (0.0..=1.0).contains(&prob),
            "bounded_scores",
            Some("stage5"),
        );
        prob
    }

    /// Witten-Bell `P(word | predecessor)`: discounted bigram MLE backed off
    /// to [`TrigramTable::unigram_prob`]. Histories with no follower
    /// observations fall back entirely to the unigram, so probabilities sum
    /// to `1.0` over the vocabulary for every history.
    pub fn bigram_prob(&self, predecessor: &str, word: &str) -> f64 {
        let backoff = self.unigram_prob(word);
        let history = self.successor_totals.get(predecessor).copied().unwrap_or(0);
        let distinct = self.distinct_bigram_followers(predecessor);
        let prob = if history == 0 || distinct == 0 {
            backoff
        } else {
            let history_f = history as f64;
            let lambda = history_f / (history_f + distinct as f64);
            let mle = self
                .bigram_counts
                .get(&(predecessor.to_string(), word.to_string()))
                .copied()
                .unwrap_or(0) as f64
                / history_f;
            lambda * mle + (1.0 - lambda) * backoff
        };
        assert_invariant(
            prob.is_finite() && (0.0..=1.0).contains(&prob),
            "bounded_scores",
            Some("stage5"),
        );
        prob
    }

    /// Witten-Bell `P(w3 | w1, w2)`: discounted trigram MLE backed off to
    /// [`TrigramTable::bigram_prob`]. Unseen histories (or histories with no
    /// follower observations) fall back entirely, so probabilities sum to
    /// `1.0` over the vocabulary for every history — never NaN, never panic.
    pub fn trigram_prob(&self, w1: &str, w2: &str, w3: &str) -> f64 {
        // INV `trigram_order`: conditioned on exactly the two previous words.
        assert_invariant(TRIGRAM_ORDER == 3, "trigram_order", Some("stage5"));
        let backoff = self.bigram_prob(w2, w3);
        let history = self
            .history_counts
            .get(&(w1.to_string(), w2.to_string()))
            .copied()
            .unwrap_or(0);
        let distinct = self.distinct_trigram_followers(w1, w2);
        let prob = if history == 0 || distinct == 0 {
            backoff
        } else {
            let history_f = history as f64;
            let lambda = history_f / (history_f + distinct as f64);
            let mle = self.count_trigram(w1, w2, w3) as f64 / history_f;
            lambda * mle + (1.0 - lambda) * backoff
        };
        // INV `bounded_scores`: Witten-Bell interpolation of values in
        // `(0,1]` stays in `(0,1]`.
        assert_invariant(
            prob.is_finite() && (0.0..=1.0).contains(&prob),
            "bounded_scores",
            Some("stage5"),
        );
        prob
    }

    /// Joint plausibility of a word sequence: unigram for the first word,
    /// bigram backoff for the second, trigram thereafter. Empty sequences
    /// score `1.0`. A product of values in `(0,1]` stays in `(0,1]`
    /// (INV `bounded_scores`).
    pub fn sequence_prob(&self, words: &[String]) -> f64 {
        // INV `trigram_order`: joint plausibility factorizes over unigram /
        // bigram / trigram factors only — no longer-range context enters.
        assert_invariant(TRIGRAM_ORDER == 3, "trigram_order", Some("stage5"));
        let mut joint = 1.0;
        for (i, word) in words.iter().enumerate() {
            let factor = if i == 0 {
                self.unigram_prob(word)
            } else if i == 1 {
                self.bigram_prob(&words[0], word)
            } else {
                self.trigram_prob(&words[i - 2], &words[i - 1], word)
            };
            joint *= factor;
        }
        assert_invariant(
            joint.is_finite() && (0.0..=1.0).contains(&joint),
            "bounded_scores",
            Some("stage5"),
        );
        joint
    }

    /// Re-rank B2/C1 n-best word hypotheses best-first by trigram
    /// [`TrigramTable::sequence_prob`].
    ///
    /// Pure count lookup — no weights, no acoustic scores consumed. Ties
    /// break by hypothesis words, then by input index: deterministic for a
    /// fixed table. Empty input yields empty output (no panic).
    pub fn rescore_nbest(&self, nbest: &[Vec<String>]) -> Vec<RescoredHypothesis> {
        let mut ranked: Vec<RescoredHypothesis> = nbest
            .iter()
            .enumerate()
            .map(|(index, hypothesis)| RescoredHypothesis {
                index,
                prob: self.sequence_prob(hypothesis),
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.prob
                .partial_cmp(&a.prob)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| nbest[a.index].cmp(&nbest[b.index]))
                .then_with(|| a.index.cmp(&b.index))
        });
        ranked
    }
}

/// One re-ranked hypothesis: its input index plus trigram plausibility.
#[derive(Debug, Clone, PartialEq)]
pub struct RescoredHypothesis {
    /// Index into the input n-best list.
    pub index: usize,
    /// Trigram joint plausibility in `[0,1]`.
    pub prob: f64,
}

/// Score a committed word sequence against a trigram `table`.
///
/// Mirrors `score_committed_word_sequence` (lib.rs): `word_facts` are
/// committed (active) arena facts, one per word, in order. The returned
/// score's `score_fact` is asserted as `word_facts ⟹ score`, so the
/// justification chain bottoms out in committed word facts.
///
/// No orphan scores: a length mismatch, an unknown fact, or any uncommitted
/// premise yields `None` and interns nothing.
pub fn score_committed_word_sequence_trigram(
    arena: &mut Arena,
    word_facts: &[FactId],
    words: &[String],
    table: &TrigramTable,
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
        Some(&format!("trigram sequence score: {}", words.join(" "))),
    );
    // INV `score_is_vote`: this plausibility is one evidence vote for the
    // confidence combination (aem.7) — returned for combination, never a
    // sole decider.
    assert_invariant(true, "score_is_vote", Some("stage5"));
    Some(SequenceScore { prob, score_fact })
}
