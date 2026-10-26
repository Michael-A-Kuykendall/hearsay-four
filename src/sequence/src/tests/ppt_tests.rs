//! idw.2 gate: `cd src/sequence && cargo test --all-features`
//!
//! Contract test 'stage5 sequence' over the invariants from
//! docs/design/ngram-sequence.md §5 (bounded_scores, flat_no_lm,
//! bigram_order, score_is_vote).
//!
//! Plus proptest fuzz over varied word sequences. Serialized with
//! `invariant_ppt::test_lock`.

use proptest::prelude::*;

use invariant_ppt::{
    clear_contracts, clear_invariant_log, has_invariant, register_contract, test_lock,
    verify_all_contracts,
};

use crate::{BigramTable, score_committed_word_sequence};

const CONTRACT_INVARIANTS: &[&str] = &[
    "bounded_scores",
    "flat_no_lm",
    "bigram_order",
    "score_is_vote",
];

/// Small probe alphabet: trained words plus unseen-but-closed-vocabulary
/// words, so fuzz covers seen, unseen-pair, and unseen-predecessor cases.
const PROBE_WORDS: [&str; 6] = [
    "STAKED",
    "PROCESS",
    "RIPPLE",
    "BYZANTIUM",
    "FURNACE",
    "CINDER",
];

fn word_seq(ids: Vec<usize>) -> Vec<String> {
    ids.into_iter()
        .map(|i| PROBE_WORDS[i].to_string())
        .collect()
}

fn trained_table() -> BigramTable {
    let mut table = BigramTable::new();
    table.seed_vocabulary(&lexicon::CLOSED_VOCABULARY_WORDS);
    table.train(&[
        vec!["STAKED".to_string(), "PROCESS".to_string()],
        vec!["STAKED".to_string(), "PROCESS".to_string()],
        vec!["STAKED".to_string(), "RIPPLE".to_string()],
    ]);
    table
}

fn committed_word(arena: &mut arena::Arena, word: &str) -> fabric::FactId {
    let id = arena.ingest(&arena::Candidate::new(
        word.to_string(),
        Vec::new(),
        Vec::new(),
    ));
    arena.fabric.assert_assumption(id);
    arena.fabric.commit(id);
    id
}

#[test]
fn contract_stage5_sequence_invariants_enforced() {
    let _guard = test_lock::lock();
    clear_invariant_log();
    clear_contracts();
    register_contract("stage5 sequence", CONTRACT_INVARIANTS);

    // Train a counted bigram table (fires flat_no_lm, bigram_order).
    let table = trained_table();

    // Score seen, unseen-pair, and unseen-predecessor cases
    // (fires bounded_scores).
    let probes = ["STAKED", "PROCESS", "RIPPLE", "BYZANTIUM", "NEVER-SEEN"];
    for a in &probes {
        for b in &probes {
            let p = table.bigram_prob(a, b);
            assert!(p.is_finite() && (0.0..=1.0).contains(&p));
            assert!(p > 0.0, "zero score despite smoothing: ({a}, {b})");
        }
    }

    // Joint plausibility factorizes over adjacent pairs only
    // (fires bigram_order, bounded_scores).
    let seq = vec![
        "STAKED".to_string(),
        "PROCESS".to_string(),
        "RIPPLE".to_string(),
    ];
    let joint = table.sequence_prob(&seq);
    let expected = table.bigram_prob("STAKED", "PROCESS") * table.bigram_prob("PROCESS", "RIPPLE");
    assert!((joint - expected).abs() < 1e-12);

    // The table score is returned as one justified vote over committed
    // word facts (fires score_is_vote).
    let mut arena = arena::Arena::new();
    let facts: Vec<fabric::FactId> = seq.iter().map(|w| committed_word(&mut arena, w)).collect();
    let scored = score_committed_word_sequence(&mut arena, &facts, &seq, &table)
        .expect("committed words must score");
    assert!(scored.prob.is_finite() && (0.0..=1.0).contains(&scored.prob));

    // Every invariant fired.
    for inv in CONTRACT_INVARIANTS {
        assert!(has_invariant(inv), "invariant {inv} was not recorded");
    }
    assert!(
        verify_all_contracts() >= 1,
        "'stage5 sequence' contract verified"
    );
}

// P-Test: proptest over varied word sequences — every score stays a finite
// probability in (0,1], and the joint is exactly the product of adjacent
// bigram factors (no longer-range context leaks in).
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_varied_sequences_bounded_and_markov(
        train_seqs in prop::collection::vec(prop::collection::vec(0..6usize, 0..6), 0..4),
        probe in prop::collection::vec(0..6usize, 0..6),
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("stage5 sequence", &["bounded_scores", "bigram_order"]);
        let mut table = BigramTable::new();
        table.seed_vocabulary(&lexicon::CLOSED_VOCABULARY_WORDS);
        let train_word_seqs: Vec<Vec<String>> =
            train_seqs.into_iter().map(word_seq).collect();
        table.train(&train_word_seqs);
        for a in &PROBE_WORDS {
            for b in &PROBE_WORDS {
                let p = table.bigram_prob(a, b);
                prop_assert!(p.is_finite() && (0.0..=1.0).contains(&p) && p > 0.0);
            }
        }
        let seq = word_seq(probe);
        let joint = table.sequence_prob(&seq);
        let expected: f64 = seq
            .windows(crate::NGRAM_ORDER)
            .map(|pair| table.bigram_prob(&pair[0], &pair[1]))
            .product();
        prop_assert!((joint - expected).abs() < 1e-12);
        prop_assert!(joint.is_finite() && (0.0..=1.0).contains(&joint));
        prop_assert!(verify_all_contracts() >= 1);
    }
}

// P-Test: retraining on the same material reproduces every score bit-for-bit
// (counts only, no hidden fitted state), and scoring a committed sequence
// yields a justified vote — or nothing for an empty sequence.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_retrain_deterministic_and_score_is_vote(
        train_seqs in prop::collection::vec(prop::collection::vec(0..6usize, 0..5), 0..4),
        probe in prop::collection::vec(0..6usize, 0..5),
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("stage5 sequence", &["flat_no_lm", "bounded_scores"]);
        let train_word_seqs: Vec<Vec<String>> =
            train_seqs.into_iter().map(word_seq).collect();
        let mut first = BigramTable::new();
        first.train(&train_word_seqs);
        let mut second = BigramTable::new();
        second.train(&train_word_seqs);
        for a in &PROBE_WORDS {
            for b in &PROBE_WORDS {
                prop_assert_eq!(first.bigram_prob(a, b), second.bigram_prob(a, b));
            }
        }
        let seq = word_seq(probe);
        let mut arena = arena::Arena::new();
        if seq.is_empty() {
            prop_assert!(
                score_committed_word_sequence(&mut arena, &[], &[], &first).is_none()
            );
        } else {
            let facts: Vec<fabric::FactId> =
                seq.iter().map(|w| committed_word(&mut arena, w)).collect();
            let scored = score_committed_word_sequence(&mut arena, &facts, &seq, &first);
            prop_assert!(scored.is_some());
            let scored = scored.unwrap();
            prop_assert!(scored.prob.is_finite() && (0.0..=1.0).contains(&scored.prob));
            prop_assert!(arena.fabric.support_count(scored.score_fact) >= 1);
        }
        prop_assert!(verify_all_contracts() >= 1);
    }
}
