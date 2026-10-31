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

pub mod word_match;

#[cfg(test)]
mod ppt_tests;

/// Standard 39-phone ARPABET set (aem.10): 15 vowels + 24 consonants.
pub const PHONEME_ALPHABET: [&str; 39] = [
    "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW", "B",
    "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T",
    "TH", "V", "W", "Y", "Z", "ZH",
];

/// Tier-1 lexicon identity (two-tier policy, bead
/// `hearsay-four-1c9--mfcc-viterbi-speed.1--oov-fallback-policy`).
///
/// Tier-1 is the better-voice `Master_English_Phonetics_FINAL_COMPLETE.txt`
/// phonetics table (CMUdict content) — NOT cmudict-0.7b. Tier-1 wins every
/// word it covers; tier-2 (`librispeech-lexicon.txt`, Sequitur G2P) fills
/// only words tier-1 lacks. This constant records the identity so no
/// follow-up can silently switch tier-1 to another dictionary.
pub const TIER1_IDENTITY: &str =
    "better-voice Master_English_Phonetics_FINAL_COMPLETE.txt (CMUdict content, NOT cmudict-0.7b)";

/// Which tier supplied a pronunciation entry (two-tier provenance).
///
/// Every [`PronunciationEntry`] carries one of these; tiered tables record
/// one per word. Serialized form is [`LexiconSource::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LexiconSource {
    /// Tier-1: CMUdict content ([`TIER1_IDENTITY`]). Wins where both exist.
    Cmudict,
    /// Tier-2: `librispeech-lexicon.txt` (Sequitur classical G2P). Gap fill.
    LibrispeechG2p,
}

impl LexiconSource {
    /// Stable provenance token: `cmudict` or `librispeech-g2p`.
    pub fn as_str(self) -> &'static str {
        match self {
            LexiconSource::Cmudict => "cmudict",
            LexiconSource::LibrispeechG2p => "librispeech-g2p",
        }
    }
}

impl std::fmt::Display for LexiconSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One dictionary entry: orthographic word + stress-stripped ARPABET phones.
///
/// `phoneme_ids` holds the arena `NodeId`s (aem.5) for `phones`, in order —
/// empty until [`PronunciationEntry::resolve`] (or [`ingest_with_arena`])
/// interns each phone. The table itself stays phone strings only: a lookup
/// table, NOT a model (INV `lookup_not_model`).
///
/// `source` records which tier supplied the pronunciation (two-tier
/// provenance); [`parse_cmudict_line`] defaults it to
/// [`LexiconSource::Cmudict`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PronunciationEntry {
    pub word: String,
    pub phones: Vec<String>,
    pub phoneme_ids: Vec<FactId>,
    pub source: LexiconSource,
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
/// The entry's [`LexiconSource`] is [`LexiconSource::Cmudict`]; use
/// [`parse_cmudict_line_with_source`] to tag another tier.
pub fn parse_cmudict_line(line: &str) -> Option<PronunciationEntry> {
    parse_cmudict_line_with_source(line, LexiconSource::Cmudict)
}

/// Parse one CMUdict-shaped line, tagging the entry with `source`.
///
/// Same accepted format as [`parse_cmudict_line`] (whitespace-separated
/// `WORD PHONES...`, stress digits stripped); only the provenance tag
/// differs. Tier-2 (`librispeech-lexicon.txt`) lines parse through here with
/// [`LexiconSource::LibrispeechG2p`] — both its separators (double space and
/// tab) are plain whitespace to `split_whitespace`.
pub fn parse_cmudict_line_with_source(
    line: &str,
    source: LexiconSource,
) -> Option<PronunciationEntry> {
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
        source,
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

/// Ingest a full CMUdict-shaped file into the word -> pronunciations lookup
/// table (same line format as [`parse_cmudict_line`] / [`ingest_sample`]).
///
/// Blank lines and `;;;` comment lines are skipped. Any other line that does
/// not parse (e.g. a word with no phones) is an `InvalidData` error — the
/// loader fails loudly instead of panicking or silently skipping the line.
/// The default vocabulary stays the 30-word [`CLOSED_VOCABULARY_CMUDICT`]
/// subset; call this only when a full-dictionary path is supplied.
pub fn ingest_full_cmudict(
    path: impl AsRef<std::path::Path>,
) -> std::io::Result<HashMap<String, Vec<Vec<String>>>> {
    let text = std::fs::read_to_string(path.as_ref())?;
    let mut table: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(";;;") {
            continue;
        }
        match parse_cmudict_line(line) {
            Some(entry) => {
                table.entry(entry.word).or_default().push(entry.phones);
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "malformed CMUdict line {} in {}: {line:?}",
                        index + 1,
                        path.as_ref().display()
                    ),
                ));
            }
        }
    }
    Ok(table)
}

/// Convert better-voice Master phonetics TSV text to CMUdict-shaped text.
///
/// The Master table ([`TIER1_IDENTITY`]) is lowercase `word\tPHONES\ttag`
/// rows with `#` comment lines and CRLF endings; the returned text is
/// `WORD PHONES...` lines (words uppercased) ready for [`ingest_sample`] or
/// [`ingest_tiered`]. Rows without a word and a phone column, or with an
/// empty word or phone field, are skipped — byte-identical semantics to the
/// conversion formerly inline in `r2_perf::load_lexicon`.
pub fn master_tsv_to_cmudict(text: &str) -> String {
    let mut cmudict = String::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(word), Some(phones)) = (cols.next(), cols.next()) else {
            continue;
        };
        let word = word.trim().to_uppercase();
        if word.is_empty() || phones.trim().is_empty() {
            continue;
        }
        cmudict.push_str(&word);
        cmudict.push(' ');
        cmudict.push_str(phones.trim());
        cmudict.push('\n');
    }
    cmudict
}

/// A two-tier pronunciation table with per-word provenance.
///
/// `table` is the word -> pronunciations lookup (same shape as
/// [`ingest_sample`]); `sources` records which tier supplied each word.
/// Tier-1 (CMUdict content) wins every word it covers; tier-2
/// (`librispeech-lexicon.txt`) fills only words tier-1 lacks, so every word
/// in `table` has exactly one entry in `sources`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TieredLexicon {
    pub table: HashMap<String, Vec<Vec<String>>>,
    pub sources: HashMap<String, LexiconSource>,
}

impl TieredLexicon {
    /// Words supplied by tier-1 (CMUdict content).
    pub fn tier1_words(&self) -> usize {
        self.sources
            .values()
            .filter(|s| **s == LexiconSource::Cmudict)
            .count()
    }

    /// Words supplied by tier-2 (`librispeech-lexicon.txt` gap fill).
    pub fn tier2_words(&self) -> usize {
        self.sources
            .values()
            .filter(|s| **s == LexiconSource::LibrispeechG2p)
            .count()
    }
}

/// Ingest two CMUdict-shaped texts as one tiered table with provenance.
///
/// `tier1_text` is CMUdict content ([`TIER1_IDENTITY`], already converted via
/// [`master_tsv_to_cmudict`] when it comes from the Master TSV);
/// `tier2_text` is `librispeech-lexicon.txt` content. Tier-1 wins per word:
/// a word present in tier-1 keeps exactly its tier-1 pronunciations and is
/// tagged [`LexiconSource::Cmudict`]; a word only in tier-2 is tagged
/// [`LexiconSource::LibrispeechG2p`]. Blank/`;;;` lines are skipped in both.
pub fn ingest_tiered(tier1_text: &str, tier2_text: &str) -> TieredLexicon {
    let table = ingest_sample(tier1_text);
    let mut sources: HashMap<String, LexiconSource> = HashMap::new();
    for word in table.keys() {
        sources.insert(word.clone(), LexiconSource::Cmudict);
    }
    let mut table = table;
    for line in tier2_text.lines() {
        if let Some(entry) = parse_cmudict_line_with_source(line, LexiconSource::LibrispeechG2p) {
            if table.contains_key(&entry.word) {
                continue;
            }
            sources.insert(entry.word.clone(), entry.source);
            table.entry(entry.word).or_default().push(entry.phones);
        }
    }
    TieredLexicon { table, sources }
}

/// Load the two-tier lexicon from its on-disk sources.
///
/// `master_tsv_path` is the tier-1 Master phonetics TSV
/// ([`TIER1_IDENTITY`]); `librispeech_path` is the tier-2
/// `librispeech-lexicon.txt` (CMUdict-shaped). Fails loudly on unreadable
/// files; see [`ingest_tiered`] for the merge policy.
pub fn load_tiered(
    master_tsv_path: impl AsRef<std::path::Path>,
    librispeech_path: impl AsRef<std::path::Path>,
) -> std::io::Result<TieredLexicon> {
    let master = std::fs::read_to_string(master_tsv_path.as_ref())?;
    let tier1 = master_tsv_to_cmudict(&master);
    let tier2 = std::fs::read_to_string(librispeech_path.as_ref())?;
    Ok(ingest_tiered(&tier1, &tier2))
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
/// Every entry's [`LexiconSource`] is [`LexiconSource::Cmudict`]; use
/// [`ingest_with_arena_sourced`] to tag another tier.
pub fn ingest_with_arena(
    text: &str,
    arena: &mut Arena,
) -> HashMap<String, Vec<PronunciationEntry>> {
    ingest_with_arena_sourced(text, arena, LexiconSource::Cmudict)
}

/// Ingest CMUdict-shaped text like [`ingest_with_arena`], tagging every
/// entry's [`LexiconSource`] with `source` (two-tier provenance).
pub fn ingest_with_arena_sourced(
    text: &str,
    arena: &mut Arena,
    source: LexiconSource,
) -> HashMap<String, Vec<PronunciationEntry>> {
    let mut table: HashMap<String, Vec<PronunciationEntry>> = HashMap::new();
    for line in text.lines() {
        if let Some(mut entry) = parse_cmudict_line_with_source(line, source) {
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

    #[test]
    fn tier1_identity_names_master_file_not_cmudict_point_release() {
        assert!(
            TIER1_IDENTITY.contains("Master_English_Phonetics_FINAL_COMPLETE"),
            "tier-1 identity must name the Master file: {TIER1_IDENTITY}"
        );
        assert!(
            TIER1_IDENTITY.contains("NOT cmudict-0.7b"),
            "tier-1 identity must disclaim cmudict-0.7b: {TIER1_IDENTITY}"
        );
    }

    #[test]
    fn lexicon_source_tokens_are_stable() {
        assert_eq!(LexiconSource::Cmudict.as_str(), "cmudict");
        assert_eq!(LexiconSource::LibrispeechG2p.as_str(), "librispeech-g2p");
        assert_eq!(LexiconSource::Cmudict.to_string(), "cmudict");
        assert_eq!(LexiconSource::LibrispeechG2p.to_string(), "librispeech-g2p");
    }

    #[test]
    fn parse_defaults_source_to_cmudict() {
        let entry = parse_cmudict_line("QUICK  K W IH1 K").expect("parse");
        assert_eq!(entry.source, LexiconSource::Cmudict);
        let tier2 =
            parse_cmudict_line_with_source("QUICK\tK W IH1 K", LexiconSource::LibrispeechG2p)
                .expect("parse tab-separated tier-2 line");
        assert_eq!(tier2.word, "QUICK");
        assert_eq!(tier2.source, LexiconSource::LibrispeechG2p);
    }

    #[test]
    fn master_tsv_conversion_skips_comments_and_empty_fields() {
        let tsv = "# comment line\nhello\tHH AH0 L OW1\tbase\nempty\t\tbase\n\tHH IY1\tbase\nnocol\nWORLD\tW ER1 L D\tbase\r\n";
        assert_eq!(
            master_tsv_to_cmudict(tsv),
            "HELLO HH AH0 L OW1\nWORLD W ER1 L D\n"
        );
    }

    #[test]
    fn tiered_ingest_tier1_wins_and_every_word_tagged() {
        let tier1 = "SHARED  S EH1 R D\nONLY1  OW1 N L IY1 W AH1 N\n";
        let tier2 = "SHARED\tSH AE1 R D\nONLY2\tOW1 N L IY1 T UW1\n";
        let tiered = ingest_tiered(tier1, tier2);
        assert_eq!(tiered.table.len(), 3);
        assert_eq!(tiered.sources.len(), 3);
        // Tier-1 wins the shared word: pronunciation AND tag.
        assert_eq!(
            tiered.table["SHARED"],
            vec![vec![
                "S".to_string(),
                "EH".to_string(),
                "R".to_string(),
                "D".to_string()
            ]]
        );
        assert_eq!(tiered.sources["SHARED"], LexiconSource::Cmudict);
        assert_eq!(tiered.sources["ONLY1"], LexiconSource::Cmudict);
        assert_eq!(tiered.sources["ONLY2"], LexiconSource::LibrispeechG2p);
        assert_eq!(tiered.tier1_words(), 2);
        assert_eq!(tiered.tier2_words(), 1);
        // Every table word has exactly one provenance tag.
        for word in tiered.table.keys() {
            assert!(
                tiered.sources.contains_key(word),
                "{word} lacks a provenance tag"
            );
        }
    }

    #[test]
    fn arena_ingest_tags_entries_with_source() {
        let mut arena = Arena::new();
        let tier1 = ingest_with_arena("QUICK  K W IH1 K\n", &mut arena);
        assert_eq!(
            tier1["QUICK"][0].source,
            LexiconSource::Cmudict,
            "untagged ingest defaults to tier-1"
        );
        let tier2 = ingest_with_arena_sourced(
            "QUICK\tK W IH1 K\n",
            &mut arena,
            LexiconSource::LibrispeechG2p,
        );
        assert_eq!(tier2["QUICK"][0].source, LexiconSource::LibrispeechG2p);
    }
}
