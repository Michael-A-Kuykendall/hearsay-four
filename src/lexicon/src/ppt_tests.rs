//! mwa.3 gate: `cd src/lexicon && cargo test --all-features`
//!
//! Contract test 'stage4 lexicon' over the invariants from
//! docs/design/lexicon-scope.md §5:
//! - lookup_not_model (the dictionary is a lookup table; no learned weights)
//! - words_in_alphabet (every phone is in the ARPABET alphabet, aem.10)
//! - word_inherits_syllable_justification (every word fact is justified by
//!   the syllable facts beneath it; no orphan word)
//! - pronunciation_complete (every closed-vocabulary word has ≥1 entry)
//!
//! Plus proptest fuzz over syllable-sequence inputs. Serialized with
//! `invariant_ppt::test_lock`.

use std::collections::HashMap;

use proptest::prelude::*;

use invariant_ppt::{
    assert_invariant, clear_contracts, clear_invariant_log, has_invariant, register_contract,
    test_lock, verify_all_contracts,
};

use arena::{Arena, Candidate};

use crate::word_match::match_committed_sequence;
use crate::{
    CLOSED_VOCABULARY_CMUDICT, CLOSED_VOCABULARY_WORDS, PHONEME_ALPHABET, PronunciationEntry,
    ingest_with_arena, phones_in_alphabet,
};

const CONTRACT_INVARIANTS: &[&str] = &[
    "lookup_not_model",
    "words_in_alphabet",
    "word_inherits_syllable_justification",
    "pronunciation_complete",
];

/// Ingest the closed vocabulary into a fresh arena, firing the three
/// table-shape invariants over the real table contents.
fn ingest_closed_vocabulary(arena: &mut Arena) -> HashMap<String, Vec<PronunciationEntry>> {
    let table = ingest_with_arena(CLOSED_VOCABULARY_CMUDICT, arena);

    // INV lookup_not_model: the table carries phone strings only — no
    // scores, weights, or probabilities.
    let lookup_only = table
        .values()
        .flatten()
        .flat_map(|entry| entry.phones.iter())
        .all(|phone| phone.chars().all(|c| c.is_ascii_uppercase()));
    assert_invariant(lookup_only, "lookup_not_model", Some("lexicon ingest"));

    // INV words_in_alphabet: every dictionary phone is in the ARPABET alphabet.
    let in_alphabet = table
        .values()
        .flatten()
        .all(|entry| phones_in_alphabet(&entry.phones));
    assert_invariant(in_alphabet, "words_in_alphabet", Some("lexicon ingest"));

    // INV pronunciation_complete: every closed-vocabulary word has ≥1 entry.
    let complete = CLOSED_VOCABULARY_WORDS
        .iter()
        .all(|word| table.get(*word).is_some_and(|entries| !entries.is_empty()));
    assert_invariant(complete, "pronunciation_complete", Some("lexicon ingest"));

    table
}

#[test]
fn contract_stage4_lexicon_invariants_enforced() {
    let _guard = test_lock::lock();
    clear_invariant_log();
    clear_contracts();
    register_contract("stage4 lexicon", CONTRACT_INVARIANTS);

    // Ingest the closed vocabulary (fires lookup_not_model,
    // words_in_alphabet, pronunciation_complete over the real table).
    let mut arena = Arena::new();
    let table = ingest_closed_vocabulary(&mut arena);

    // INV word_inherits_syllable_justification: match a committed STAKED
    // sequence (fires inside `match_committed_sequence`).
    let syllables = table["STAKED"][0].phoneme_ids.clone();
    for id in &syllables {
        arena.fabric.assert_assumption(*id);
        arena.fabric.commit(*id);
    }
    let matches = match_committed_sequence(&mut arena, &syllables, &table);
    assert_eq!(matches.len(), 1, "one word must align: {matches:?}");
    assert_eq!(matches[0].word, "STAKED");

    // Every invariant fired.
    for inv in CONTRACT_INVARIANTS {
        assert!(has_invariant(inv), "invariant {inv} was not recorded");
    }
    assert!(
        verify_all_contracts() >= 1,
        "'stage4 lexicon' contract verified"
    );
}

// P-Test: proptest over syllable-sequence inputs (phone sequences drawn
// from the ARPABET alphabet, length 0..8) — matching is deterministic and
// never creates an orphan word.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_syllable_sequence_matching_never_orphans(
        phones in prop::collection::vec(0usize..PHONEME_ALPHABET.len(), 0..8),
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract(
            "stage4 lexicon",
            &["lookup_not_model", "words_in_alphabet", "pronunciation_complete"],
        );

        let mut arena = Arena::new();
        let table = ingest_closed_vocabulary(&mut arena);

        // Commit the fuzzed syllable sequence as arena facts.
        let mut syllables = Vec::with_capacity(phones.len());
        for index in &phones {
            let id = arena.ingest(&Candidate::new(
                PHONEME_ALPHABET[*index].to_string(),
                Vec::new(),
                Vec::new(),
            ));
            arena.fabric.assert_assumption(id);
            arena.fabric.commit(id);
            syllables.push(id);
        }

        let first = match_committed_sequence(&mut arena, &syllables, &table);
        // Deterministic: a second run aligns the same words.
        let second = match_committed_sequence(&mut arena, &syllables, &table);
        let words_first: Vec<_> = first.iter().map(|m| m.word.clone()).collect();
        let words_second: Vec<_> = second.iter().map(|m| m.word.clone()).collect();
        prop_assert_eq!(words_first, words_second);

        // No orphan words: every word fact inherits syllable justification.
        // Full matches carry the aligned dictionary pronunciation; partial
        // fallback hypotheses carry the committed input sequence.
        let expected_phones: Vec<String> = phones
            .iter()
            .map(|index| PHONEME_ALPHABET[*index].to_string())
            .collect();
        for matched in &first {
            prop_assert!(arena.fabric.support_count(matched.word_fact) > 0);
            prop_assert!(arena.fabric.is_active(matched.word_fact));
            if matched.partial {
                prop_assert_eq!(&matched.phones, &expected_phones);
            } else {
                let table_prons: Vec<&Vec<String>> = table[&matched.word]
                    .iter()
                    .map(|entry| &entry.phones)
                    .collect();
                prop_assert!(
                    table_prons.contains(&&matched.phones),
                    "full match must align to a dictionary pronunciation"
                );
            }
        }
        prop_assert!(verify_all_contracts() >= 1);
    }
}
