//! Syllable-sequence -> word matching with justification inheritance (mwa.2,
//! B2 hearsay-four-r4g).
//!
//! Committed syllable/phone sequences (Stage 3) are matched against the
//! dictionary's phone sequences (lexicon-scope.md section 4). A word fact is
//! created when a committed sequence aligns to a dictionary
//! pronunciation, and it **inherits justification** from the syllable facts
//! beneath it (`premises ⟹ word` in the fabric), so provenance extends down
//! to acoustic evidence. INV `word_inherits_syllable_justification`: every
//! word fact has `support_count > 0`; no orphan word is ever created.
//!
//! B2 fuzzy alignment (model-free, synthetic fixtures only): the committed
//! sequence aligns to a pronunciation under phone-confusion tolerant edit
//! distance. Substitution cost follows articulatory class (same class 1.0,
//! consonant cross-manner 1.5, vowel/consonant cross-class 2.0); insertions
//! and deletions cost 1.0. A pronunciation is a full-word candidate when its
//! alignment uses at most [`MAX_WORD_EDITS`] edits (2). When no pronunciation
//! aligns, the matcher emits best-prefix partial-word hypotheses over the
//! committed syllable facts instead of zero matches — still justified by the
//! syllable facts, so still never orphans.
//!
//! Guards are unchanged: an empty, unknown, or uncommitted sequence yields
//! zero matches and interns nothing. The `word_fact` assertion shape
//! (intern + assert with the syllable premises + invariant) is unchanged for
//! both full and partial hypotheses.

use std::collections::{BTreeSet, HashMap};

use arena::Arena;
use fabric::FactId;
use invariant_ppt::assert_invariant;

use crate::PronunciationEntry;

/// Maximum edit count for a full-word alignment (B2: bounded distance <= 2).
pub const MAX_WORD_EDITS: usize = 2;

/// Insertion / deletion cost in the fuzzy alignment.
pub const INDEL_COST: f32 = 1.0;

/// A dictionary word matched to a committed syllable/phone sequence.
///
/// `word_fact` is a fresh fabric fact asserted with the matched syllable
/// facts as its justification set; `phones` is the aligned pronunciation
/// (for a partial hypothesis, the committed input phones). `cost` is the
/// articulatory-class alignment penalty (0.0 for an exact match); `partial`
/// marks syllable-level fallback hypotheses emitted when no full-word
/// alignment exists.
#[derive(Debug, Clone, PartialEq)]
pub struct WordMatch {
    pub word: String,
    pub word_fact: FactId,
    pub phones: Vec<String>,
    pub cost: f32,
    pub partial: bool,
}

/// Articulatory manner class driving substitution costs (model-free table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhoneClass {
    Vowel,
    Stop,
    Fricative,
    Affricate,
    Nasal,
    LiquidGlide,
    Other,
}

/// Classify one ARPABET phone by articulatory manner; anything outside the
/// 39-phone alphabet is `Other`.
fn phone_class(phone: &str) -> PhoneClass {
    use PhoneClass::*;
    match phone {
        "AA" | "AE" | "AH" | "AO" | "AW" | "AY" | "EH" | "ER" | "EY" | "IH" | "IY" | "OW"
        | "OY" | "UH" | "UW" => Vowel,
        "B" | "D" | "G" | "K" | "P" | "T" => Stop,
        "DH" | "F" | "HH" | "S" | "SH" | "TH" | "V" | "Z" | "ZH" => Fricative,
        "CH" | "JH" => Affricate,
        "M" | "N" | "NG" => Nasal,
        "L" | "R" | "W" | "Y" => LiquidGlide,
        _ => Other,
    }
}

/// Substitution cost between two phones by articulatory class: 0.0 for
/// identical phones, 1.0 within one class, 1.5 across consonant manners,
/// 2.0 across the vowel/consonant boundary (or involving `Other`).
pub fn substitution_cost(a: &str, b: &str) -> f32 {
    if a == b {
        return 0.0;
    }
    let (ca, cb) = (phone_class(a), phone_class(b));
    if ca == cb {
        return 1.0;
    }
    let is_consonant = |c: PhoneClass| {
        matches!(
            c,
            PhoneClass::Stop
                | PhoneClass::Fricative
                | PhoneClass::Affricate
                | PhoneClass::Nasal
                | PhoneClass::LiquidGlide
        )
    };
    if is_consonant(ca) && is_consonant(cb) {
        1.5
    } else {
        2.0
    }
}

/// Align `observed` to a dictionary pronunciation, minimizing (edits, cost)
/// lexicographically: fewest edits first, then the articulatory-class
/// penalty. Returns the edit count and the total penalty.
fn align_counts(observed: &[String], pron: &[String]) -> (usize, f32) {
    let (m, n) = (observed.len(), pron.len());
    let mut prev: Vec<(usize, f32)> = (0..=n).map(|j| (j, j as f32 * INDEL_COST)).collect();
    for i in 1..=m {
        let mut cur = vec![(0usize, 0.0f32); n + 1];
        cur[0] = (i, i as f32 * INDEL_COST);
        for j in 1..=n {
            let sub_cost = substitution_cost(&observed[i - 1], &pron[j - 1]);
            let (pe, pc) = prev[j - 1];
            let sub = if sub_cost == 0.0 {
                (pe, pc)
            } else {
                (pe + 1, pc + sub_cost)
            };
            let del = (prev[j].0 + 1, prev[j].1 + INDEL_COST);
            let ins = (cur[j - 1].0 + 1, cur[j - 1].1 + INDEL_COST);
            cur[j] = [sub, del, ins]
                .into_iter()
                .min_by(|a, b| {
                    a.0.cmp(&b.0)
                        .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                })
                .expect("alignment candidates never empty");
        }
        prev = cur;
    }
    prev[n]
}

/// Length of the common phone prefix between `observed` and a pronunciation.
fn prefix_overlap(observed: &[String], pron: &[String]) -> usize {
    observed
        .iter()
        .zip(pron.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

/// Assert one match's `word_fact` with the syllable premises (INV
/// `word_inherits_syllable_justification`). Shared by full and partial
/// hypotheses so the assertion shape never diverges.
fn assert_match_fact(arena: &mut Arena, word: &str, premises: &BTreeSet<FactId>) -> FactId {
    let word_fact = arena.fabric.intern();
    arena.fabric.assert(word_fact, premises.clone(), false);
    // INV: word_inherits_syllable_justification.
    assert_invariant(
        arena.fabric.support_count(word_fact) >= 1,
        "word_inherits_syllable_justification",
        Some(&format!("lexicon match: {word}")),
    );
    word_fact
}

/// Match a committed syllable/phone sequence to dictionary words.
///
/// `syllable_facts` are committed (active) arena facts, one per phone, in
/// order. Their phone strings are read from the arena, so the word's
/// justification chain bottoms out in real facts. Each returned word's
/// `word_fact` is asserted as `syllable_facts ⟹ word` (INV
/// `word_inherits_syllable_justification`).
///
/// Full-word alignment is phone-confusion tolerant: a pronunciation whose
/// alignment costs at most [`MAX_WORD_EDITS`] edits is a full (`partial:
/// false`) match scored by its articulatory-class penalty. When no
/// pronunciation aligns, the syllable-level fallback emits the best-prefix
/// partial-word hypothesis (`partial: true`) instead of zero matches.
///
/// No orphan words: an empty, unknown, or uncommitted sequence yields zero
/// matches and interns nothing.
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

    // Score every pronunciation once: (edit count, class penalty, overlap).
    let mut scored: Vec<(&str, &PronunciationEntry, usize, f32, usize)> = Vec::new();
    for (word, entries) in dictionary {
        for entry in entries {
            let (edits, cost) = align_counts(&phones, &entry.phones);
            let overlap = prefix_overlap(&phones, &entry.phones);
            scored.push((word.as_str(), entry, edits, cost, overlap));
        }
    }

    // Full-word hypotheses: bounded edit distance (<= MAX_WORD_EDITS).
    let full: Vec<(&str, &PronunciationEntry, f32)> = scored
        .iter()
        .filter(|(_, _, edits, _, _)| *edits <= MAX_WORD_EDITS)
        .map(|(word, entry, _, cost, _)| (*word, *entry, *cost))
        .collect();
    if !full.is_empty() {
        let mut matches = Vec::with_capacity(full.len());
        for (word, entry, cost) in full {
            let word_fact = assert_match_fact(arena, word, &premises);
            matches.push(WordMatch {
                word: word.to_string(),
                word_fact,
                phones: entry.phones.clone(),
                cost,
                partial: false,
            });
        }
        matches.sort_by(|a, b| a.cost.total_cmp(&b.cost).then_with(|| a.word.cmp(&b.word)));
        return matches;
    }

    // Syllable-level partial fallback: no full-word align exists, so emit
    // the best-prefix hypothesis (or hypotheses, on ties) rather than zero
    // matches. Still justified by the syllable facts: never an orphan.
    if scored.is_empty() {
        return Vec::new();
    }
    let best_overlap = scored.iter().map(|(_, _, _, _, o)| *o).max().unwrap_or(0);
    let mut partials = Vec::new();
    for (word, _, _, cost, overlap) in &scored {
        if *overlap == best_overlap {
            let word_fact = assert_match_fact(arena, word, &premises);
            partials.push(WordMatch {
                word: (*word).to_string(),
                word_fact,
                phones: phones.clone(),
                cost: *cost,
                partial: true,
            });
        }
    }
    partials.sort_by(|a, b| a.cost.total_cmp(&b.cost).then_with(|| a.word.cmp(&b.word)));
    partials
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

        // Committed but aligning to no pronunciation: B2 partial fallback —
        // a partial hypothesis, never zero matches, never an orphan.
        let zh = arena::Candidate::new("ZH".to_string(), Vec::new(), Vec::new());
        let committed_absent = [arena.ingest(&zh), arena.ingest(&zh)];
        for id in &committed_absent {
            arena.fabric.assert_assumption(*id);
            arena.fabric.commit(*id);
        }
        let partials = match_committed_sequence(&mut arena, &committed_absent, &table);
        assert!(
            !partials.is_empty(),
            "unmatchable frames must emit a partial hypothesis"
        );
        assert!(
            partials.iter().all(|m| m.partial),
            "no full-word align exists: every hypothesis is partial"
        );
        for matched in &partials {
            assert!(
                arena.fabric.support_count(matched.word_fact) > 0,
                "partial hypothesis must inherit syllable justification"
            );
            assert!(
                arena.fabric.is_active(matched.word_fact),
                "justified partial hypothesis must be active"
            );
        }
    }
}

/// B2 fuzzy phone-sequence matcher tests (bead hearsay-four-r4g).
///
/// (1) exact sequences match with identical costs (parity); (2) 1-phone
/// substitution matches with penalty > 0; (3) unmatchable frames emit a
/// partial hypothesis — never zero matches, never orphan; (4) distance-3
/// corruption is rejected (no full-word match); (5) no-orphan guards hold
/// under fuzz (proptest over random 1-edit corruptions).
///
/// Synthetic fixtures only: every sequence is built from the in-code closed
/// vocabulary or the 39-phone alphabet. Model-free: no learned weights.
#[cfg(test)]
mod fuzzy_matcher_tests {
    use super::*;
    use crate::{CLOSED_VOCABULARY_CMUDICT, PHONEME_ALPHABET, ingest_with_arena};

    fn phone_vec(phones: &[&str]) -> Vec<String> {
        phones.iter().map(|p| p.to_string()).collect()
    }

    /// Commit `phones` as arena facts (assumption + commit) for matching.
    fn commit_phones(arena: &mut Arena, phones: &[String]) -> Vec<FactId> {
        let ids = crate::resolve_phones(arena, phones);
        for id in &ids {
            arena.fabric.assert_assumption(*id);
            arena.fabric.commit(*id);
        }
        ids
    }

    fn closed_vocab(arena: &mut Arena) -> HashMap<String, Vec<PronunciationEntry>> {
        ingest_with_arena(CLOSED_VOCABULARY_CMUDICT, arena)
    }

    fn staked_phones() -> Vec<String> {
        phone_vec(&["S", "T", "EY", "K", "T"])
    }

    /// (1) Exact sequences match with identical costs (parity): STAKED aligns
    /// once, at zero penalty, deterministically across runs.
    #[test]
    fn matcher_exact_sequences_match_with_identical_costs() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);
        let syllables = commit_phones(&mut arena, &staked_phones());

        let first = match_committed_sequence(&mut arena, &syllables, &table);
        let second = match_committed_sequence(&mut arena, &syllables, &table);
        assert_eq!(first.len(), 1, "exact STAKED must align: {first:?}");
        assert_eq!(first[0].word, "STAKED");
        assert!(!first[0].partial, "exact align is a full-word match");
        assert_eq!(first[0].cost, 0.0, "exact align carries zero penalty");
        // Parity: repeated runs report identical costs.
        assert_eq!(
            first[0].cost, second[0].cost,
            "exact costs must be identical across runs"
        );
        assert_eq!(first[0].phones, second[0].phones);
    }

    /// (1b) Exact match shape: word plus the aligned pronunciation phones.
    #[test]
    fn matcher_exact_match_reports_word_and_pronunciation() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);
        let syllables = commit_phones(&mut arena, &staked_phones());

        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].word, "STAKED");
        assert_eq!(matches[0].phones, staked_phones());
    }

    /// (2) A 1-phone substitution (EY -> IY, same vowel class) still matches
    /// STAKED as a full word, with penalty > 0.
    #[test]
    fn matcher_single_substitution_matches_with_penalty() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);
        let corrupted = phone_vec(&["S", "T", "IY", "K", "T"]);
        let syllables = commit_phones(&mut arena, &corrupted);

        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        let staked = matches
            .iter()
            .find(|m| m.word == "STAKED")
            .expect("1-edit corruption must still match STAKED");
        assert!(!staked.partial, "distance-1 align is a full-word match");
        assert!(
            staked.cost > 0.0,
            "substitution must carry positive penalty, got {}",
            staked.cost
        );
        assert_eq!(
            staked.cost, 1.0,
            "same-class substitution costs exactly 1.0"
        );
        assert_eq!(staked.phones, staked_phones());
    }

    /// Substitution costs follow articulatory class: same class < consonant
    /// cross-manner < vowel/consonant cross-class.
    #[test]
    fn matcher_articulatory_class_orders_substitution_costs() {
        // Identical phones are free.
        assert_eq!(substitution_cost("EY", "EY"), 0.0);
        // Same class (vowel/vowel, stop/stop): 1.0.
        assert_eq!(substitution_cost("EY", "IY"), 1.0);
        assert_eq!(substitution_cost("S", "Z"), 1.0);
        // Consonant cross-manner (fricative/stop): 1.5.
        assert_eq!(substitution_cost("S", "T"), 1.5);
        // Vowel/consonant cross-class: 2.0 — strictly the most expensive.
        assert_eq!(substitution_cost("EY", "S"), 2.0);
        assert!(
            substitution_cost("EY", "IY") < substitution_cost("S", "T")
                && substitution_cost("S", "T") < substitution_cost("EY", "S"),
            "costs must order same-class < cross-manner < cross-class"
        );
    }

    /// (3) Unmatchable frames emit a partial hypothesis: never zero matches,
    /// never an orphan.
    #[test]
    fn matcher_unmatchable_frames_emit_partial_hypothesis() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);
        let alien = phone_vec(&["ZH", "ZH"]);
        let syllables = commit_phones(&mut arena, &alien);

        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        assert!(
            !matches.is_empty(),
            "unmatchable frames must emit a partial hypothesis, never zero matches"
        );
        assert!(
            matches.iter().all(|m| m.partial),
            "nothing aligns: every hypothesis is partial: {matches:?}"
        );
        for matched in &matches {
            assert!(
                arena.fabric.support_count(matched.word_fact) > 0,
                "partial hypothesis must never be an orphan"
            );
            assert!(
                arena.fabric.is_active(matched.word_fact),
                "justified partial hypothesis must be active"
            );
        }
    }

    /// (3b) The partial hypothesis is justified by exactly the committed
    /// syllable facts and carries the observed phones.
    #[test]
    fn matcher_partial_hypothesis_inherits_justification() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);
        let alien = phone_vec(&["ZH", "ZH"]);
        let syllables = commit_phones(&mut arena, &alien);

        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        assert!(!matches.is_empty());
        let expected: BTreeSet<FactId> = syllables.into_iter().collect();
        for matched in &matches {
            assert!(matched.partial);
            assert_eq!(
                matched.phones, alien,
                "partial hypothesis carries the observed phones"
            );
            assert!(
                arena
                    .fabric
                    .justifications(matched.word_fact)
                    .iter()
                    .any(|j| j.premises == expected),
                "partial must inherit justification from the syllable facts"
            );
        }
    }

    /// (4) Distance-3 corruption is rejected: no full-word match survives
    /// three edits (only the partial fallback remains).
    #[test]
    fn matcher_distance_three_corruption_rejected() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);
        // Three substitutions away from STAKED (S T EY K T -> ZH ZH ZH K T).
        let corrupted = phone_vec(&["ZH", "ZH", "ZH", "K", "T"]);
        let syllables = commit_phones(&mut arena, &corrupted);

        let matches = match_committed_sequence(&mut arena, &syllables, &table);
        assert!(
            matches.iter().all(|m| m.partial),
            "distance-3 corruption must have no full-word match: {matches:?}"
        );
        assert!(
            !matches.is_empty(),
            "rejected corruption still yields a partial hypothesis"
        );
    }

    /// (5a) No-orphan guards hold under fuzz: empty, unknown, and
    /// uncommitted sequences yield zero matches and intern nothing.
    #[test]
    fn matcher_guards_reject_empty_unknown_uncommitted() {
        let mut arena = Arena::new();
        let table = closed_vocab(&mut arena);

        assert!(match_committed_sequence(&mut arena, &[], &table).is_empty());

        let unknown = vec![FactId(u64::MAX), FactId(u64::MAX - 1)];
        assert!(match_committed_sequence(&mut arena, &unknown, &table).is_empty());

        let uncommitted = crate::resolve_phones(&mut arena, &staked_phones());
        assert!(!uncommitted.is_empty());
        assert!(match_committed_sequence(&mut arena, &uncommitted, &table).is_empty());
    }

    /// (5b) Proptest over random 1-edit corruptions of STAKED: every run
    /// yields a justified STAKED full match, deterministically, never zero
    /// matches, never an orphan.
    #[test]
    fn matcher_no_orphan_under_random_single_edit_corruptions() {
        use proptest::prelude::*;
        proptest!(ProptestConfig::with_cases(32), |(pos in 0usize..5,
         phone_idx in 0usize..PHONEME_ALPHABET.len(),
         op in 0u8..3u8)| {
            let mut corrupted = staked_phones();
            match op {
                // Substitute one phone (possibly a no-op: still exact).
                0 => corrupted[pos] = PHONEME_ALPHABET[phone_idx].to_string(),
                // Insert one phone.
                1 => corrupted.insert(pos, PHONEME_ALPHABET[phone_idx].to_string()),
                // Delete one phone.
                _ => {
                    corrupted.remove(pos % corrupted.len());
                }
            }

            let mut arena = Arena::new();
            let table = closed_vocab(&mut arena);
            let syllables = commit_phones(&mut arena, &corrupted);

            let first = match_committed_sequence(&mut arena, &syllables, &table);
            prop_assert!(
                !first.is_empty(),
                "1-edit corruption must never yield zero matches"
            );
            let staked = first.iter().find(|m| m.word == "STAKED");
            prop_assert!(
                staked.is_some_and(|m| !m.partial),
                "STAKED is within 1 edit: full match required: {first:?}"
            );
            for matched in &first {
                prop_assert!(arena.fabric.support_count(matched.word_fact) > 0);
                prop_assert!(arena.fabric.is_active(matched.word_fact));
            }
            // Deterministic: a second run aligns identically.
            let second = match_committed_sequence(&mut arena, &syllables, &table);
            let key = |m: &WordMatch| {
                (m.word.clone(), m.partial, m.cost.to_bits(), m.phones.clone())
            };
            prop_assert_eq!(
                first.iter().map(key).collect::<Vec<_>>(),
                second.iter().map(key).collect::<Vec<_>>()
            );
        });
    }
}
