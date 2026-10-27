//! B1 (hearsay-four-24i): full-CMUdict loader tests over a synthetic fixture.
//!
//! The fixture (`data/full_cmudict_synthetic.txt`) is generated, CMUdict-shaped
//! data (>10k entries) — no download, no real CMUdict file needed for green.
//! Gate: `cd src/lexicon && cargo test --offline cmudict`.

use std::collections::HashMap;
use std::path::PathBuf;

use lexicon::{
    CLOSED_VOCABULARY_CMUDICT, CLOSED_VOCABULARY_WORDS, ingest_full_cmudict, ingest_sample,
    phones_in_alphabet,
};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("full_cmudict_synthetic.txt")
}

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lexicon_full_dict_{name}_{}.txt",
        std::process::id()
    ))
}

fn phone_vec(phones: &[&str]) -> Vec<String> {
    phones.iter().map(|p| p.to_string()).collect()
}

#[test]
fn cmudict_subset_still_loads_byte_identical() {
    // The 30-word subset loads byte-identical through the full file loader:
    // same keys, same phone strings, same order as the in-code default.
    let path = tmp_path("subset");
    std::fs::write(&path, CLOSED_VOCABULARY_CMUDICT).unwrap();
    let via_file = ingest_full_cmudict(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let via_sample = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
    assert_eq!(via_file, via_sample);
    assert_eq!(via_file.len(), CLOSED_VOCABULARY_WORDS.len());
    assert_eq!(via_file.len(), 30);
}

#[test]
fn cmudict_fixture_loads_over_10k_entries() {
    let table = ingest_full_cmudict(fixture_path()).unwrap();
    let entries: usize = table.values().map(Vec::len).sum();
    assert!(
        entries > 10_000,
        "fixture must carry >10k entries, got {entries}"
    );
    // Every fixture phone strips into the 39-phone alphabet.
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
fn cmudict_fixture_spot_checks_pronunciations() {
    let table: HashMap<String, Vec<Vec<String>>> = ingest_full_cmudict(fixture_path()).unwrap();
    // Hand-written subset lines embedded at the top of the fixture.
    assert_eq!(
        table["STAKED"],
        vec![phone_vec(&["S", "T", "EY", "K", "T"])]
    );
    assert_eq!(
        table["IMPEACHMENT"],
        vec![phone_vec(&[
            "IH", "M", "P", "IY", "CH", "M", "AH", "N", "T"
        ])]
    );
    // Deterministic synthetic rows (stress digits stripped on ingest).
    assert_eq!(
        table["WORDF00001"],
        vec![phone_vec(&["IH", "K", "S", "AE"])]
    );
    assert_eq!(
        table["WORDF00002"],
        vec![phone_vec(&["K", "S", "AE", "T", "M"])]
    );
}

#[test]
fn cmudict_malformed_line_returns_error() {
    // A word with no phones is malformed: loud error, never panic or skip.
    let path = tmp_path("malformed");
    std::fs::write(&path, "QUICK  K W IH1 K\nBROKENLINE\nFOX  F AA1 K S\n").unwrap();
    let result = ingest_full_cmudict(&path);
    std::fs::remove_file(&path).ok();
    let err = result.expect_err("malformed line must error, not skip");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn cmudict_default_remains_30_word_subset() {
    // The default vocabulary is untouched: still exactly the 30-word subset.
    assert_eq!(CLOSED_VOCABULARY_WORDS.len(), 30);
    let table = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
    assert_eq!(table.len(), 30);
    for word in CLOSED_VOCABULARY_WORDS {
        assert!(
            table.get(word).is_some_and(|prons| !prons.is_empty()),
            "{word} missing from default subset"
        );
    }
}
