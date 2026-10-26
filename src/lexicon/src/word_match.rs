//! Syllable-sequence -> word matching with justification inheritance (mwa.2).
//!
//! Committed syllable/phone sequences (Stage 3) are matched against the
//! dictionary's phone sequences (lexicon-scope.md section 4). A word fact is
//! created only when a committed sequence aligns to a dictionary
//! pronunciation, and it **inherits justification** from the syllable facts
//! beneath it (`premises ⟹ word` in the fabric), so provenance extends down
//! to acoustic evidence. INV `word_inherits_syllable_justification`: every
//! word fact has `support_count > 0`; no orphan word is ever created.

use std::collections::{BTreeSet, HashMap};

use arena::Arena;
use fabric::FactId;
use invariant_ppt::assert_invariant;

use crate::PronunciationEntry;

/// A dictionary word matched to a committed syllable/phone sequence.
///
/// `word_fact` is a fresh fabric fact asserted with the matched syllable
/// facts as its justification set; `phones` is the aligned pronunciation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordMatch {
    pub word: String,
    pub word_fact: FactId,
    pub phones: Vec<String>,
}

/// Match a committed syllable/phone sequence to dictionary words.
///
/// `syllable_facts` are committed (active) arena facts, one per phone, in
/// order. Their phone strings are read from the arena, so the word's
/// justification chain bottoms out in real facts. Each returned word's
/// `word_fact` is asserted as `syllable_facts ⟹ word` (INV
/// `word_inherits_syllable_justification`).
///
/// No orphan words: an empty, unknown, uncommitted, or non-aligning
/// sequence yields zero matches and interns nothing.
pub fn match_committed_sequence(
    arena: &mut Arena,
    syllable_facts: &[FactId],
    dictionary: &HashMap<String, Vec<PronunciationEntry>>,
) -> Vec<WordMatch> {
    if syllable_facts.is_empty() {
        return Vec::new();
    }
    // Resolve the phone sequence from the arena itself.
    let mut phones = Vec::with_capacity(syllable_facts.len());
    for id in syllable_facts {
        match arena.content(*id) {
            Some(content) => phones.push(content.phoneme.clone()),
            None => return Vec::new(), // Unknown fact: never an orphan word.
        }
    }
    // Committed means active: uncommitted premises justify nothing.
    if !syllable_facts.iter().all(|id| arena.fabric.is_active(*id)) {
        return Vec::new();
    }
    let premises: BTreeSet<FactId> = syllable_facts.iter().copied().collect();
    let mut matches = Vec::new();
    for (word, entries) in dictionary {
        for entry in entries {
            if entry.phones == phones {
                let word_fact = arena.fabric.intern();
                arena.fabric.assert(word_fact, premises.clone(), false);
                // INV: word_inherits_syllable_justification.
                assert_invariant(
                    arena.fabric.support_count(word_fact) >= 1,
                    "word_inherits_syllable_justification",
                    Some(&format!("lexicon match: {word}")),
                );
                matches.push(WordMatch {
                    word: word.clone(),
                    word_fact,
                    phones: phones.clone(),
                });
            }
        }
    }
    matches.sort_by(|a, b| a.word.cmp(&b.word));
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CLOSED_VOCABULARY_CMUDICT, ingest_with_arena};

    /// STAKED syllable facts, committed (assumption + commit), plus the
    /// closed-vocabulary dictionary resolved into the same arena.
    fn committed_staked(
        arena: &mut Arena,
    ) -> (Vec<FactId>, HashMap<String, Vec<PronunciationEntry>>) {
        let table = ingest_with_arena(CLOSED_VOCABULARY_CMUDICT, arena);
        let syllables = table["STAKED"][0].phoneme_ids.clone();
        for id in &syllables {
            arena.fabric.assert_assumption(*id);
            arena.fabric.commit(*id);
        }
        (syllables, table)
    }

    #[test]
    fn match_committed_syllable_sequence_to_dictionary_word() {
        let mut arena = Arena::new();
        let (syllables, table) = committed_staked(&mut arena);
        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        assert_eq!(matches.len(), 1, "one word must align: {matches:?}");
        assert_eq!(matches[0].word, "STAKED");
        assert_eq!(
            matches[0].phones,
            vec![
                "S".to_string(),
                "T".to_string(),
                "EY".to_string(),
                "K".to_string(),
                "T".to_string()
            ]
        );
    }

    #[test]
    fn match_word_inherits_syllable_justification() {
        let mut arena = Arena::new();
        let (syllables, table) = committed_staked(&mut arena);
        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        assert_eq!(matches.len(), 1);
        let word_fact = matches[0].word_fact;
        // INV: word_inherits_syllable_justification — fabric support_count > 0.
        assert!(
            arena.fabric.support_count(word_fact) > 0,
            "word fact must be justified by the syllable facts"
        );
        assert!(
            arena.fabric.is_active(word_fact),
            "justified word fact must be active"
        );
        // The justification premises are exactly the syllable facts beneath.
        let expected: BTreeSet<FactId> = syllables.into_iter().collect();
        assert!(
            arena
                .fabric
                .justifications(word_fact)
                .iter()
                .any(|j| j.premises == expected),
            "word must inherit justification from the syllable facts"
        );
    }

    #[test]
    fn match_no_orphan_word_without_syllable_justification() {
        let mut arena = Arena::new();
        let table = ingest_with_arena(CLOSED_VOCABULARY_CMUDICT, &mut arena);

        // Empty sequence: no word.
        assert!(match_committed_sequence(&mut arena, &[], &table).is_empty());

        // Ingested but never committed (inactive): no word.
        let uncommitted: Vec<FactId> = table["STAKED"][0].phoneme_ids.clone();
        assert!(!uncommitted.is_empty());
        assert!(match_committed_sequence(&mut arena, &uncommitted, &table).is_empty());

        // Unknown facts (never interned): no word.
        let unknown = vec![FactId(u64::MAX), FactId(u64::MAX - 1)];
        assert!(match_committed_sequence(&mut arena, &unknown, &table).is_empty());

        // Committed but aligning to no pronunciation: no word.
        let zh = arena::Candidate::new("ZH".to_string(), Vec::new(), Vec::new());
        let committed_absent = [arena.ingest(&zh), arena.ingest(&zh)];
        for id in &committed_absent {
            arena.fabric.assert_assumption(*id);
            arena.fabric.commit(*id);
        }
        assert!(match_committed_sequence(&mut arena, &committed_absent, &table).is_empty());
    }
}
