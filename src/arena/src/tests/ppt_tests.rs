//! a6d.3 gate: `cd src/arena && cargo test --all-features`
//!
//! Contract test 'stage2 arena' over the invariants from
//! hash-consing-equality.md §6, plus proptest over fragment orderings and
//! duplicates. Serialized via `invariant_ppt::test_lock`.

use proptest::prelude::*;

use crate::{Arena, Candidate};

/// All four contract invariants for Stage 2 arena ingestion.
const CONTRACT_INVARIANTS: &[&str] = &[
    "hash_consing_injective",
    "hash_consing_distinct",
    "arena_append_only",
    "cross_utterance_memoization",
];

fn candidate(phoneme: &str, sig: &[i32]) -> Candidate {
    Candidate::new(phoneme, vec![], sig.to_vec())
}

/// C-Test: the arena enforces all four hash-consing invariants.
#[test]
fn contract_stage2_arena_invariants_enforced() {
    let _guard = invariant_ppt::test_lock::lock();
    invariant_ppt::clear_invariant_log();
    invariant_ppt::clear_contracts();
    invariant_ppt::register_contract("stage2 arena", CONTRACT_INVARIANTS);

    let mut arena = Arena::new();
    let a = candidate("IH", &[1, 1, 1]);
    let b = candidate("K", &[2, 2, 2]);
    let c = candidate("AE", &[3, 3, 3]);

    // hash_consing_injective: identical content twice -> same NodeId.
    let id1 = arena.ingest(&a);
    let id1_again = arena.ingest(&candidate("IH", &[1, 1, 1]));
    assert_eq!(id1, id1_again);

    // hash_consing_distinct: distinct content -> distinct NodeId.
    let id2 = arena.ingest(&b);
    let id3 = arena.ingest(&c);
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);

    // arena_append_only: content immutable after intern.
    assert_eq!(arena.content(id1).unwrap(), &a.content);
    assert_eq!(arena.content(id2).unwrap(), &b.content);

    // cross_utterance_memoization: K first appears in utterance 1, recurs in
    // utterance 2 -> reuses the first-occurrence NodeId (fires the INV).
    let s1 = arena.ingest_utterance(&[a.clone(), b.clone()]);
    assert_eq!(s1.ids[0], id1, "a already ingested via plain ingest");
    assert_eq!(s1.ids[1], id2, "b already ingested via plain ingest");
    let s2 = arena.ingest_utterance(&[candidate("K", &[2, 2, 2])]);
    assert_eq!(
        s2.ids[0], id2,
        "later utterance reuses first-occurrence NodeId"
    );
    assert_eq!(arena.fact_count(), 3);

    assert!(
        invariant_ppt::verify_all_contracts() >= 1,
        "'stage2 arena' contract verified"
    );
}

// P-Test: any ordering/duplication of a fixed fragment pool keeps
// hash-consing exact — fact_count equals the number of distinct contents.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]
    #[test]
    fn ppt_fragment_permutations_keep_distinct_counts(
        order in proptest::collection::vec(0usize..8, 0..40),
        dup_freq in 1usize..6,
    ) {
        let _guard = invariant_ppt::test_lock::lock();
        let pool: Vec<Candidate> = (0..8)
            .map(|i| candidate(&format!("P{i}"), &[i, (i * 7) % 5, 1]))
            .collect();

        let mut arena = Arena::new();
        let mut expected_unique = 0usize;
        let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();

        // Split the shuffled sequence into pseudo-utterances.
        let mut idx = 0;
        while idx < order.len() {
            let end = (idx + 1 + order[idx] % 7).min(order.len());
            let utterance: Vec<Candidate> = order[idx..end]
                .iter()
                .flat_map(|&p| {
                    let count = 1 + (p + order[idx]) % dup_freq;
                    std::iter::repeat_n(pool[p].clone(), count)
                })
                .collect();
            let stats = arena.ingest_utterance(&utterance);
            for &p in &order[idx..end] {
                if seen.insert(p) {
                    expected_unique += 1;
                }
            }
            prop_assert_eq!(stats.ids.len(), utterance.len());
            // Reused count = candidates - new distinct contents in this utt.
            idx = end;
        }

        prop_assert_eq!(
            arena.fact_count(),
            expected_unique,
            "exactly one NodeId per distinct content, regardless of ordering/duplication"
        );
        // Every id resolves to the content it was ingested with, and
        // re-ingest is stable (same NodeId, no fabric growth).
        for c in &pool {
            if let Some(id) = arena.node_id(&c.content) {
                prop_assert_eq!(arena.content(id).unwrap(), &c.content);
                let before = arena.fact_count();
                let again = arena.ingest(c);
                prop_assert_eq!(again, id, "re-ingest stable");
                prop_assert_eq!(arena.fact_count(), before, "no growth on re-ingest");
            }
        }
    }
}

// P-Test: random pool sizes and signatures still produce distinct ids for
// distinct content.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]
    #[test]
    fn ppt_random_pool_distinct_content_distinct_ids(
        pool_size in 1usize..12,
    ) {
        let _guard = invariant_ppt::test_lock::lock();
        let mut arena = Arena::new();
        let pool: Vec<Candidate> = (0..pool_size)
            .map(|i| candidate(&format!("R{i}"), &[i as i32, 0, i as i32 * 3]))
            .collect();
        let ids: Vec<_> = pool.iter().map(|c| arena.ingest(c)).collect();
        let mut uniq = ids.clone();
        uniq.sort_unstable();
        uniq.dedup();
        prop_assert_eq!(uniq.len(), pool_size, "distinct content never merges");
        prop_assert_eq!(arena.fact_count(), pool_size);
    }
}
