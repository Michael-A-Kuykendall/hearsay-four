//! Stage 4 lexicon scaffold (bead hearsay-four-o21).
//!
//! CMUdict-shaped pronunciation dictionary: a lookup table, NOT a model.
//! Full ingestion, arena `FactId` resolution (aem.5), and the mwa.3 PPT
//! contract tests land in mwa.1/mwa.2/mwa.3. This crate only establishes
//! the package, the entry API, and the gate path `cd src/lexicon && cargo
//! test ingest`.
//!
//! Reference: docs/design/lexicon-scope.md, docs/design/phoneme-alphabet.md
//! (aem.10: 39-phone ARPABET; aem.14: CMUdict subset, stress stripped).

use std::collections::HashMap;

/// Standard 39-phone ARPABET set (aem.10): 15 vowels + 24 consonants.
pub const PHONEME_ALPHABET: [&str; 39] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW", "B",
    "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T",
    "TH", "V", "W", "Y", "Z", "ZH",
];

/// One dictionary entry: orthographic word + stress-stripped ARPABET phones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PronunciationEntry {
    pub word: String,
    pub phones: Vec<String>,
}

/// Strip CMUdict stress marks: `IH1` -> `IH`, `AO` -> `AO`.
pub fn strip_stress(phone: &str) -> String {
    phone.trim_end_matches(['0', '1', '2']).to_string()
}

/// Parse one CMUdict-shaped line: `QUICK  K W IH1 K`.
/// Returns `None` for blank/comment lines or phone-less lines.
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
    Some(PronunciationEntry { word, phones })
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
}
