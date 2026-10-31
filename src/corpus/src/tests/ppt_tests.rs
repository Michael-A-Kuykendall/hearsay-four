//! ul2.4 gate: `cd src/corpus && cargo test --all-features`
//!
//! PPT/invariant tests for the corpus pipeline: registers the contract
//! `corpus pipeline`, exercises every invariant-holding code path, runs
//! proptest over corpus manifest samples, then verifies all contracts.
//! Serialized with `invariant_ppt::test_lock::lock()`.

use crate::audio::decode_to_pcm_16k_mono;
use crate::courtlistener::api_token;
use crate::invariants::{
    AUDIO_TRANSCRIPT_PAIRED, DECODE_PRODUCES_PCM_16K, MANIFEST_WELLFORMED, TOKEN_NEVER_HARDCODED,
};
use crate::manifest::{CorpusManifest, CorpusSource, Split};
use invariant_ppt::test_lock;
use proptest::prelude::*;

/// C-Test: the corpus pipeline enforces all four invariants end-to-end.
#[test]
fn contract_corpus_pipeline_invariants_enforced() {
    let _guard = test_lock::lock();
    invariant_ppt::clear_invariant_log();
    invariant_ppt::clear_contracts();

    invariant_ppt::register_contract(
        "corpus pipeline",
        &[
            TOKEN_NEVER_HARDCODED,
            DECODE_PRODUCES_PCM_16K,
            MANIFEST_WELLFORMED,
            AUDIO_TRANSCRIPT_PAIRED,
        ],
    );

    // Exercise every invariant-holding path. Both the better-voice checkout
    // and the CourtListener token are external inputs: skip when absent.
    if !CorpusManifest::is_available() {
        eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
        return;
    }
    if api_token().is_err() {
        eprintln!("SKIP: $COURTLISTENER_API_TOKEN unset (env or repo-root .env)");
        return;
    }
    let m = CorpusManifest::load().expect("manifest load"); // fires wellformed + paired
    let phrase = m.rows_from(CorpusSource::PhraseTraining, None);
    let wav = phrase.first().expect("phrase sample");
    decode_to_pcm_16k_mono(&wav.audio_path).expect("decode fires pcm_16k");
    api_token().expect("auth fires token_never_hardcoded");

    assert!(
        invariant_ppt::verify_all_contracts() >= 1,
        "at least the 'corpus pipeline' contract verified"
    );
}

// P-Test (proptest): every manifest row from every source is well-formed.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn ppt_manifest_rows_wellformed(_index in 0usize..4) {
        let _guard = test_lock::lock();
        if !CorpusManifest::is_available() {
            eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
            return Ok(());
        }
        let m = CorpusManifest::load().expect("manifest load");
        prop_assert!(!m.rows.is_empty());
        for row in &m.rows {
            prop_assert!(!row.text.trim().is_empty(), "text present for {:?}", row.audio_path);
            prop_assert!(row.audio_path.is_absolute());
            prop_assert!(!row.audio_path.as_os_str().to_string_lossy().is_empty());
            if let Some(d) = row.duration {
                prop_assert!(d > 0.0, "duration positive for {:?}", row.audio_path);
            }
        }
        // Source/split distribution sanity: exact counts from the inventory.
        prop_assert_eq!(m.rows_from(CorpusSource::PhraseTraining, None).len(), 3500);
        prop_assert_eq!(m.rows_from(CorpusSource::QualityChecked, Some(Split::Train)).len(), 900);
        prop_assert_eq!(m.rows_from(CorpusSource::QualityChecked, Some(Split::Val)).len(), 100);
    }
}

/// P-Test: decode of every sampled clip yields non-empty canonical frames.
#[test]
fn ppt_decode_samples_produce_pcm() {
    let _guard = test_lock::lock();
    if !CorpusManifest::is_available() {
        eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
        return;
    }
    let m = CorpusManifest::load().expect("manifest load");
    let mut samples: Vec<_> = m
        .rows_from(CorpusSource::PhraseTraining, None)
        .iter()
        .take(3)
        .cloned()
        .collect();
    samples.extend(
        m.rows_from(CorpusSource::QualityChecked, Some(Split::Train))
            .iter()
            .take(3)
            .cloned(),
    );
    samples.extend(
        m.rows_from(CorpusSource::BulkTts, Some(Split::Train))
            .iter()
            .take(3)
            .cloned(),
    );
    for row in samples {
        let frames = decode_to_pcm_16k_mono(&row.audio_path)
            .unwrap_or_else(|e| panic!("{}: {e}", row.audio_path.display()));
        assert!(
            !frames.is_empty(),
            "{}: non-empty 16k frames",
            row.audio_path.display()
        );
    }
}
