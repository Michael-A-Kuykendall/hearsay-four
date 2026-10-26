//! Stage 4 lexicon ingestion (beads hearsay-four-o21, hearsay-four-mwa.1).
//!
//! CMUdict-shaped pronunciation dictionary: a lookup table, NOT a model.
//! Closed-vocabulary ingestion plus arena `FactId` resolution (aem.5) land
//! here; syllable matching (mwa.2) and the mwa.3 PPT contract tests build on
//! this table. Gate path: `cd src/lexicon && cargo test ingest`.
//!
//! Reference: docs/design/lexicon-scope.md, docs/design/phoneme-alphabet.md
//! (aem.10: 39-phone ARPABET; aem.14: CMUdict subset, stress stripped).

use std::collections::HashMap;

use arena::{Arena, Candidate};
use fabric::FactId;

/// Standard 39-phone ARPABET set (aem.10): 15 vowels + 24 consonants.
pub const PHONEME_ALPHABET: [&str; 39] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW", "B",
    "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T",
    "TH", "V", "W", "Y", "Z", "ZH",
];

/// One dictionary entry: orthographic word + stress-stripped ARPABET phones.
///
/// `phoneme_ids` holds the arena `NodeId`s (aem.5) for `phones`, in order —
/// empty until [`PronunciationEntry::resolve`] (or [`ingest_with_arena`])
/// interns each phone. The table itself stays phone strings only: a lookup
/// table, NOT a model (INV `lookup_not_model`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PronunciationEntry {
    pub word: String,
    pub phones: Vec<String>,
    pub phoneme_ids: Vec<FactId>,
}

impl PronunciationEntry {
    /// Resolve every phone to its arena `NodeId`, filling `phoneme_ids`.
    ///
    /// Each phone interns as a context-free candidate; identical phones
    /// hash-cons to the same `NodeId` (aem.5).
    pub fn resolve(&mut self, arena: &mut Arena) {
        self.phoneme_ids = resolve_phones(arena, &self.phones);
    }
}

/// Strip CMUdict stress marks: `IH1` -> `IH`, `AO` -> `AO`.
pub fn strip_stress(phone: &str) -> String {
    phone.trim_end_matches(['0', '1', '2']).to_string()
}

/// Parse one CMUdict-shaped line: `QUICK  K W IH1 K`.
/// Returns `None` for blank/comment lines or phone-less lines.
/// The entry's `phoneme_ids` start empty; call [`PronunciationEntry::resolve`]
/// (or use [`ingest_with_arena`]) to intern phones to arena `NodeId`s.
pub fn parse_cmudict_line(line: &str) -> Option<PronunciationEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(";;;") {
        return None;
    }
    let mut parts = line.split_whitespace();
    let word = parts.next()?.to_string();
    let phones: Vec<String> = parts.map(strip_stress).collect();
    if phones.is_empty() {
        return None;
    }
    Some(PronunciationEntry {
        word,
        phones,
        phoneme_ids: Vec::new(),
    })
}

/// Ingest CMUdict-shaped text into a word -> pronunciations lookup table.
pub fn ingest_sample(text: &str) -> HashMap<String, Vec<Vec<String>>> {
    let mut table: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for line in text.lines() {
        if let Some(entry) = parse_cmudict_line(line) {
            table.entry(entry.word).or_default().push(entry.phones);
        }
    }
    table
}

/// The closed ~30-word experiment vocabulary (uppercased), mirroring
/// `CLOSED_VOCABULARY` in `src/fse` (`fse/src/lib.rs`, 39c.1).
///
/// Membership source of truth is the fse list; this copy keeps the lexicon
/// crate dependency-light (lookup table only, no scoring code).
pub const CLOSED_VOCABULARY_WORDS: [&str; 30] = [
    "IMPEACHMENT",
    "METHODICAL",
    "SURVIVAL",
    "PROCESS",
    "INTENSE",
    "REFLECTION",
    "KINSHIP",
    "RESENTED",
    "CONGEALED",
    "UNCORROBORATED",
    "HIGHBOY",
    "HOTBED",
    "RIPPLE",
    "PREDESTINE",
    "BYZANTIUM",
    "GENETIC",
    "CONSTITUTION",
    "INTERPERSONAL",
    "ENCHANTMENT",
    "PERFUME",
    "SUBMISSIONS",
    "OVERBOARD",
    "KNOCKDOWN",
    "SLEEPWALKING",
    "UNREALIZED",
    "PANHANDLING",
    "HORRIBLY",
    "SALIVATING",
    "RHUBARB",
    "STAKED",
];

/// CMUdict-shaped sample covering the full closed vocabulary
/// (lexicon-scope.md sections 2-3): `WORD  PHONES` with CMUdict stress
/// digits, stripped on ingest for the fail-fast.
pub const CLOSED_VOCABULARY_CMUDICT: &str = "\
IMPEACHMENT  IH2 M P IY1 CH M AH0 N T
METHODICAL  M EH2 TH AA1 D IH0 K AH0 L
SURVIVAL  S ER0 V AY1 V AH0 L
PROCESS  P R AA1 S EH2 S
INTENSE  IH2 N T EH1 N S
REFLECTION  R IH0 F L EH1 K SH AH0 N
KINSHIP  K IH1 N SH IH2 P
RESENTED  R IY0 Z EH1 N T IH0 D
CONGEALED  K AH0 N JH IY1 L D
UNCORROBORATED  AH2 N K AO0 R AA1 B ER0 EY2 T IH0 D
HIGHBOY  HH AY1 B OY2
HOTBED  HH AA1 T B EH2 D
RIPPLE  R IH1 P AH0 L
PREDESTINE  P R IY0 D EH1 S T IH0 N
BYZANTIUM  B IH0 Z AE1 N T IY0 AH0 M
GENETIC  JH AH0 N EH1 T IH0 K
CONSTITUTION  K AA2 N S T IH0 T UW1 SH AH0 N
INTERPERSONAL  IH2 N T ER0 P ER1 S AH0 N AH0 L
ENCHANTMENT  EH0 N CH AE1 N T M AH0 N T
PERFUME  P ER1 F Y UW2 M
SUBMISSIONS  S AH0 B M IH1 SH AH0 N Z
OVERBOARD  OW1 V ER0 B AO2 R D
KNOCKDOWN  N AA1 K D AW2 N
SLEEPWALKING  S L IY1 P W AO2 K IH0 NG
UNREALIZED  AH0 N R IY1 L AY2 Z D
PANHANDLING  P AE1 N HH AE2 N D AH0 L IH0 NG
HORRIBLY  HH AO1 R AH0 B L IY0
SALIVATING  S AE1 L AH0 V EY2 T IH0 NG
RHUBARB  R UW1 B AA2 R B
STAKED  S T EY1 K T
";

/// Resolve a phone sequence to arena `NodeId`s, in order (aem.5).
///
/// Each phone interns as a context-free [`Candidate`]; identical phones
/// hash-cons to the same id.
pub fn resolve_phones(arena: &mut Arena, phones: &[String]) -> Vec<FactId> {
    phones
        .iter()
        .map(|phone| arena.ingest(&Candidate::new(phone.clone(), Vec::new(), Vec::new())))
        .collect()
}

/// Ingest CMUdict-shaped text, resolving every entry's phones to arena
/// `NodeId`s (lexicon-scope.md section 3).
///
/// Returns the word -> pronunciations lookup table; each entry carries both
/// the stress-stripped `phones` and the parallel `phoneme_ids`.
pub fn ingest_with_arena(
    text: &str,
    arena: &mut Arena,
) -> HashMap<String, Vec<PronunciationEntry>> {
    let mut table: HashMap<String, Vec<PronunciationEntry>> = HashMap::new();
    for line in text.lines() {
        if let Some(mut entry) = parse_cmudict_line(line) {
            entry.resolve(arena);
            table.entry(entry.word.clone()).or_default().push(entry);
        }
    }
    table
}

/// Every phone in the slice is in the ARPABET alphabet
/// (INV `words_in_alphabet`, lexicon-scope.md section 5).
pub fn phones_in_alphabet(phones: &[String]) -> bool {
    phones
        .iter()
        .all(|p| PHONEME_ALPHABET.contains(&p.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
QUICK  K W IH1 K
BROWN  B R AW1 N
FOX  F AA1 K S
FOX  F AO1 K S
DOG  D AO1 G
;;; comment lines are ignored
";

    #[test]
    fn ingest_sample_loads_words_with_stress_stripped() {
        let table = ingest_sample(SAMPLE);
        assert_eq!(table.len(), 4);
        assert_eq!(
            table["QUICK"],
            vec![vec![
                "K".to_string(),
                "W".to_string(),
                "IH".to_string(),
                "K".to_string()
            ]]
        );
        // Two pronunciations for FOX (alternate pronunciations preserved).
        assert_eq!(table["FOX"].len(), 2);
    }

    #[test]
    fn ingest_words_in_alphabet() {
        let table = ingest_sample(SAMPLE);
        for (word, prons) in &table {
            for phones in prons {
                assert!(
                    phones_in_alphabet(phones),
                    "{word} has out-of-alphabet phones"
                );
            }
        }
    }

    #[test]
    fn ingest_pronunciation_complete() {
        let table = ingest_sample(SAMPLE);
        for word in ["QUICK", "BROWN", "FOX", "DOG"] {
            assert!(
                table.get(word).is_some_and(|prons| !prons.is_empty()),
                "{word} has no pronunciation"
            );
        }
    }

    #[test]
    fn ingest_lookup_not_model() {
        // The table carries phone strings only: no scores, weights, or
        // probabilities (INV `lookup_not_model`, lexicon-scope.md section 5).
        let table = ingest_sample(SAMPLE);
        for phones in table.values().flatten() {
            for phone in phones {
                assert!(
                    phone.chars().all(|c| c.is_ascii_uppercase()),
                    "non-phone payload in lookup table: {phone}"
                );
            }
        }
    }

    #[test]
    fn ingest_closed_vocabulary_sample_loads_all_words() {
        let table = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
        assert_eq!(table.len(), CLOSED_VOCABULARY_WORDS.len());
        // Stress marks are stripped on ingest (fail-fast: phones, not stress).
        assert_eq!(
            table["STAKED"],
            vec![vec![
                "S".to_string(),
                "T".to_string(),
                "EY".to_string(),
                "K".to_string(),
                "T".to_string()
            ]]
        );
    }

    #[test]
    fn ingest_closed_vocabulary_words_in_alphabet() {
        let table = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
        for (word, prons) in &table {
            for phones in prons {
                assert!(
                    phones_in_alphabet(phones),
                    "{word} has out-of-alphabet phones"
                );
            }
        }
    }

    #[test]
    fn ingest_closed_vocabulary_pronunciation_complete() {
        let table = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
        for word in CLOSED_VOCABULARY_WORDS {
            assert!(
                table.get(word).is_some_and(|prons| !prons.is_empty()),
                "{word} has no pronunciation"
            );
        }
    }

    #[test]
    fn ingest_closed_vocabulary_lookup_not_model() {
        let table = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
        for phones in table.values().flatten() {
            for phone in phones {
                assert!(
                    phone.chars().all(|c| c.is_ascii_uppercase()),
                    "non-phone payload in lookup table: {phone}"
                );
            }
        }
    }

    #[test]
    fn ingest_phoneme_ids_resolve_to_arena_nodes() {
        let mut arena = Arena::new();
        let table = ingest_with_arena(CLOSED_VOCABULARY_CMUDICT, &mut arena);
        assert_eq!(table.len(), CLOSED_VOCABULARY_WORDS.len());
        for (word, entries) in &table {
            for entry in entries {
                // One NodeId per phone, in order (lexicon-scope.md section 3).
                assert_eq!(
                    entry.phoneme_ids.len(),
                    entry.phones.len(),
                    "{word}: phoneme_ids out of step with phones"
                );
                for (phone, id) in entry.phones.iter().zip(entry.phoneme_ids.iter()) {
                    let content = arena
                        .content(*id)
                        .unwrap_or_else(|| panic!("{word}: dangling NodeId"));
                    assert_eq!(
                        &content.phoneme, phone,
                        "{word}: NodeId resolves wrong phone"
                    );
                }
            }
        }
        // Hash-consing: identical phones share one NodeId (aem.5), distinct
        // phones never alias.
        let staked = &table["STAKED"][0].phoneme_ids;
        let constitution_t = table["CONSTITUTION"][0].phoneme_ids[4];
        assert_eq!(staked[1], staked[4], "same phone must share one NodeId");
        assert_eq!(
            staked[1], constitution_t,
            "same phone must share one NodeId across words"
        );
        assert_ne!(staked[0], staked[1], "distinct phones must not alias");
    }
}
