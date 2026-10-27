//! B3 gate: `cd src/sequence && cargo test --offline trigram`.
//!
//! Trigram rescorer contract over Oyez-shaped synthetic fixtures (lowercase
//! normalized words, the shape `corpus::ground_truth::extract_words`
//! yields): per-history probabilities sum to 1, unseen histories back off
//! without NaN/panic, the rescorer prefers the trigram-supported candidate,
//! and empty counts fall back to a uniform deterministic default.

use arena::{Arena, Candidate};

use crate::trigram::{TrigramTable, oyez_word_sequence, score_committed_word_sequence_trigram};

/// Fixed Oyez-counts fixture: two repetitions of the session-opening line
/// plus one adjournment line (synthetic, Oyez-shaped lowercase words).
fn oyez_fixture_table() -> TrigramTable {
    let mut table = TrigramTable::new();
    table.train(&[
        oyez_word_sequence(&[
            ("the".to_string(), 0, 1000),
            ("court".to_string(), 1000, 2000),
            ("is".to_string(), 2000, 3000),
            ("now".to_string(), 3000, 4000),
        ]),
        oyez_word_sequence(&[
            ("the".to_string(), 0, 1000),
            ("court".to_string(), 1000, 2000),
            ("is".to_string(), 2000, 3000),
            ("now".to_string(), 3000, 4000),
        ]),
        oyez_word_sequence(&[
            ("the".to_string(), 0, 1000),
            ("court".to_string(), 1000, 2000),
            ("adjourned".to_string(), 2000, 3000),
        ]),
    ]);
    table
}

fn words(seq: &[&str]) -> Vec<String> {
    seq.iter().map(|w| (*w).to_string()).collect()
}

fn committed_word(arena: &mut Arena, word: &str) -> fabric::FactId {
    let id = arena.ingest(&Candidate::new(word.to_string(), Vec::new(), Vec::new()));
    arena.fabric.assert_assumption(id);
    arena.fabric.commit(id);
    id
}

#[test]
fn trigram_probabilities_sum_to_one_per_history() {
    let table = oyez_fixture_table();
    // Vocabulary: the, court, is, now, adjourned.
    assert_eq!(table.vocabulary_size(), 5);
    let vocab = ["the", "court", "is", "now", "adjourned"];
    // Seen trigram history, seen bigram-only history, partially seen
    // history, and a fully unseen history must all normalize.
    for (w1, w2) in [
        ("the", "court"),
        ("court", "is"),
        ("is", "now"),
        ("court", "now"),
        ("never", "seen"),
    ] {
        let total: f64 = vocab.iter().map(|w3| table.trigram_prob(w1, w2, w3)).sum();
        assert!(
            (total - 1.0).abs() < 1e-9,
            "history ({w1}, {w2}) sums to {total}, want 1.0"
        );
    }
    // Bigram and unigram levels normalize too.
    for predecessor in ["the", "court", "never-seen"] {
        let total: f64 = vocab
            .iter()
            .map(|w| table.bigram_prob(predecessor, w))
            .sum();
        assert!((total - 1.0).abs() < 1e-9, "bigram history sums to {total}");
    }
    let total: f64 = vocab.iter().map(|w| table.unigram_prob(w)).sum();
    assert!((total - 1.0).abs() < 1e-9, "unigram sums to {total}");
}

#[test]
fn trigram_unseen_history_backs_off_without_nan_or_panic() {
    let table = oyez_fixture_table();
    // Fully unseen history: falls back through bigram to unigram.
    let p = table.trigram_prob("never", "seen", "court");
    assert!(p.is_finite(), "non-finite backoff score: {p}");
    assert!((0.0..=1.0).contains(&p), "out-of-range backoff score: {p}");
    assert_eq!(p, table.bigram_prob("seen", "court"));
    assert_eq!(p, table.unigram_prob("court"));
    // Seen bigram pair with no trigram follower (`is now` ends the fixture
    // lines): backs off to the bigram level, still finite and in range.
    let q = table.trigram_prob("is", "now", "the");
    assert!(q.is_finite(), "non-finite edge-history score: {q}");
    assert!((0.0..=1.0).contains(&q));
    assert_eq!(q, table.bigram_prob("now", "the"));
    // Partially seen history (unseen w1, seen w2) backs off the same way.
    let r = table.trigram_prob("never", "court", "is");
    assert!(r.is_finite() && (0.0..=1.0).contains(&r));
    assert_eq!(r, table.bigram_prob("court", "is"));
}

#[test]
fn trigram_witten_bell_discounted_estimate_matches_hand_count() {
    let table = oyez_fixture_table();
    // Fixture tallies: H(the,court) = 3 trigram followers, T = {is, adjourned}
    // so L = 3/5; S(court) = 3 bigram followers, T = {is, adjourned} so
    // L2 = 3/5; unigram P(is) = (2 + 1) / (11 + 5) = 3/16.
    assert_eq!(table.count_trigram("the", "court", "is"), 2);
    let p_bigram_is = 0.6 * (2.0 / 3.0) + 0.4 * (3.0 / 16.0);
    let expected = 0.6 * (2.0 / 3.0) + 0.4 * p_bigram_is;
    let eps = 1e-12;
    assert!((table.bigram_prob("court", "is") - p_bigram_is).abs() < eps);
    assert!((table.trigram_prob("the", "court", "is") - expected).abs() < eps);
    assert!((expected - 0.59).abs() < 1e-9);
    // Discounting is real: the estimate sits strictly below raw MLE 2/3.
    assert!(table.trigram_prob("the", "court", "is") < 2.0 / 3.0);
}

#[test]
fn trigram_rescorer_prefers_trigram_supported_candidate() {
    let table = oyez_fixture_table();
    // B2/C1-style n-best hypotheses over the opening line: the trigram
    // (the, court, is) is seen twice, (the, court, now) never starts a
    // trigram, so the faithful order must win.
    let nbest = vec![
        words(&["the", "court", "is", "now"]),
        words(&["the", "court", "now", "is"]),
    ];
    let ranked = table.rescore_nbest(&nbest);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].index, 0, "supported candidate must rank first");
    assert_eq!(ranked[1].index, 1);
    assert!(ranked[0].prob > ranked[1].prob);
    for r in &ranked {
        assert!(r.prob.is_finite() && (0.0..=1.0).contains(&r.prob));
    }
    // Deterministic: rescoring the same list reproduces the same ranking.
    let again = table.rescore_nbest(&nbest);
    assert_eq!(ranked, again);
    // Empty n-best yields empty output, never a panic.
    assert!(table.rescore_nbest(&[]).is_empty());
}

#[test]
fn trigram_empty_counts_uniform_deterministic_fallback() {
    // No counts, no vocabulary: no evidence against anything.
    let empty = TrigramTable::new();
    assert_eq!(empty.trigram_prob("a", "b", "c"), 1.0);
    assert_eq!(empty.sequence_prob(&words(&["a", "b", "c"])), 1.0);
    // Seeded vocabulary, no counts: uniform 1/V everywhere, bit-for-bit
    // reproducible across fresh tables.
    let mut seeded = TrigramTable::new();
    seeded.seed_vocabulary(&["a", "b", "c"]);
    let again = seeded.clone();
    for (w1, w2, w3) in [("a", "b", "c"), ("c", "c", "c"), ("b", "a", "z")] {
        assert!((seeded.trigram_prob(w1, w2, w3) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(
            seeded.trigram_prob(w1, w2, w3),
            again.trigram_prob(w1, w2, w3)
        );
    }
    // Tied candidates still rank deterministically (words, then index).
    let nbest = vec![words(&["b"]), words(&["a"])];
    let ranked = seeded.rescore_nbest(&nbest);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].prob, ranked[1].prob);
    assert_eq!(ranked[0].index, 1, "lexicographically smaller wins ties");
    assert_eq!(ranked, seeded.rescore_nbest(&nbest));
}

#[test]
fn trigram_sequence_prob_factorizes_over_trigram_factors() {
    let table = oyez_fixture_table();
    let seq = words(&["the", "court", "is", "now"]);
    let expected = table.unigram_prob("the")
        * table.bigram_prob("the", "court")
        * table.trigram_prob("the", "court", "is")
        * table.trigram_prob("court", "is", "now");
    assert!((table.sequence_prob(&seq) - expected).abs() < 1e-12);
    // Prefix behavior: empty scores 1.0, single word scores its unigram.
    assert_eq!(table.sequence_prob(&[]), 1.0);
    assert_eq!(
        table.sequence_prob(&words(&["court"])),
        table.unigram_prob("court")
    );
    assert_eq!(
        table.sequence_prob(&words(&["the", "court"])),
        table.unigram_prob("the") * table.bigram_prob("the", "court")
    );
}

#[test]
fn trigram_score_inherits_word_justification() {
    let mut arena = Arena::new();
    let table = oyez_fixture_table();
    let facts = vec![
        committed_word(&mut arena, "the"),
        committed_word(&mut arena, "court"),
        committed_word(&mut arena, "is"),
    ];
    let seq = words(&["the", "court", "is"]);
    let scored = score_committed_word_sequence_trigram(&mut arena, &facts, &seq, &table)
        .expect("committed words must score");
    assert_eq!(scored.prob, table.sequence_prob(&seq));
    assert!(scored.prob.is_finite() && (0.0..=1.0).contains(&scored.prob));
    assert!(arena.fabric.support_count(scored.score_fact) >= 1);
    // Rescoring mints a fresh fact justified by the same premises.
    let rescored = score_committed_word_sequence_trigram(&mut arena, &facts, &seq, &table)
        .expect("rescoring must succeed");
    assert_ne!(scored.score_fact, rescored.score_fact);
    assert!(arena.fabric.support_count(rescored.score_fact) >= 1);
    // Uncommitted premises justify nothing: no justification is added.
    let mut arena2 = Arena::new();
    let pending = arena2.ingest(&Candidate::new("the".to_string(), Vec::new(), Vec::new()));
    assert!(
        score_committed_word_sequence_trigram(
            &mut arena2,
            &[pending],
            &["the".to_string()],
            &table
        )
        .is_none()
    );
    assert_eq!(
        arena2.fabric.support_count(pending),
        0,
        "no orphan score fact"
    );
    // Length mismatch scores nothing.
    assert!(
        score_committed_word_sequence_trigram(&mut arena2, &[], &["the".to_string()], &table)
            .is_none()
    );
}
