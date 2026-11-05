//! Public-API triphone coverage (fixture-only, no data/ needed).
//!
//! A fixture tied model loads through `load_with_vocab` with
//! `triphone: true`: centers decide coverage (a missing unit falls back,
//! only a missing center skips a word), and decode runs end to end.

use decoder::{DecoderConfig, MooreDecoder, Recognizer};
use std::path::PathBuf;

fn fixture_phone(name: &str, mean0: f64) -> hmm::PhoneHmm {
    let dim = fse::FEATURE_DIM;
    let mut mean = vec![0.0; dim];
    mean[0] = mean0;
    hmm::PhoneHmm {
        name: name.to_string(),
        n_states: 1,
        emissions: vec![hmm::Mixture::single(
            hmm::Gaussian {
                mean,
                var: vec![1.0; dim],
            },
            hmm::ParamStats::zeros(dim),
        )],
        trans: vec![
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.5, 0.5],
            vec![0.0, 0.0, 0.0],
        ],
        tied_to: None,
    }
}

/// Tied fixture: centers AH/B/D plus SOME units (AH-B deliberately absent
/// to prove units are optional at load — centers decide coverage).
fn fixture_tied() -> hmm::HmmSet {
    let dim = fse::FEATURE_DIM;
    let mut phones: std::collections::BTreeMap<String, hmm::PhoneHmm> =
        [("AH", 0.0), ("B", 5.0), ("D", -5.0)]
            .into_iter()
            .map(|(p, m)| (p.to_string(), fixture_phone(p, m)))
            .collect();
    for (unit, mean0) in [("AH+B", -5.0), ("AH+D", 0.0), ("AH-D", -5.0)] {
        phones.insert(unit.to_string(), fixture_phone(unit, mean0));
    }
    hmm::HmmSet {
        dim,
        phones,
        global_mean: vec![0.0; dim],
        global_var: vec![1.0; dim],
        var_floor_ratio: 0.01,
        frontend_id: Some(fse::HMM_FRONTEND_ID.to_string()),
    }
}

fn entries_ab_ad() -> Vec<decoder::PronEntry> {
    vec![
        decoder::PronEntry {
            word: "AB".to_string(),
            phones: ["AH".to_string(), "B".to_string()].to_vec(),
        },
        decoder::PronEntry {
            word: "AD".to_string(),
            phones: ["AH".to_string(), "D".to_string()].to_vec(),
        },
    ]
}

fn save_fixture(model: &hmm::HmmSet, tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "decoder-triphone-{tag}-{}.json",
        std::process::id()
    ));
    model.save_text(&path).expect("save fixture tied am");
    path
}

#[test]
fn triphone_load_with_vocab_accepts_tied_model() {
    let path = save_fixture(&fixture_tied(), "tied");
    let cfg = DecoderConfig {
        triphone: true,
        ..DecoderConfig::default()
    };
    let dec = MooreDecoder::load_with_vocab(&path, None, &entries_ab_ad(), cfg).expect("load tied");
    std::fs::remove_file(&path).ok();
    assert_eq!(dec.vocab_size(), 2);
    assert!(
        dec.skipped_words().is_empty(),
        "missing AH-B unit must not skip AB: {:?}",
        dec.skipped_words()
    );
    // Decode runs end to end on synthetic audio (1 s of silence → frames).
    let pcm = vec![0i16; 16_000];
    let lat = dec.decode(&pcm);
    assert!(!lat.tokens.is_empty(), "decode must produce tokens");
    for t in &lat.tokens {
        assert!(
            t.word == "AB" || t.word == "AD",
            "unexpected word {}",
            t.word
        );
    }
}

#[test]
fn triphone_load_skips_word_with_missing_center() {
    // Centers map before identity: a word whose CENTER is absent from the
    // AM is skipped even in triphone mode (fallback needs the center).
    let path = save_fixture(&fixture_tied(), "missing-center");
    let mut entries = entries_ab_ad();
    entries.push(decoder::PronEntry {
        word: "AZ".to_string(),
        phones: ["AH".to_string(), "Z".to_string()].to_vec(),
    });
    let cfg = DecoderConfig {
        triphone: true,
        ..DecoderConfig::default()
    };
    let dec = MooreDecoder::load_with_vocab(&path, None, &entries, cfg).expect("load tied");
    std::fs::remove_file(&path).ok();
    assert_eq!(dec.vocab_size(), 2);
    assert_eq!(dec.skipped_words().len(), 1);
    assert!(
        dec.skipped_words()[0].starts_with("AZ"),
        "only AZ must skip: {:?}",
        dec.skipped_words()
    );
}
