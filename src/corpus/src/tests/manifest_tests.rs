//! ul2.3 gate: `cd src/corpus && cargo test manifest`

use crate::manifest::{CorpusManifest, CorpusSource, Split};

#[test]
fn manifest_phrase_training_parses_3500_rows() {
    let m = CorpusManifest::load().expect("manifest load");
    let rows = m.rows_from(CorpusSource::PhraseTraining, None);
    assert_eq!(rows.len(), 3500, "phrase_training must have 3500 rows");
    let first = &rows[0];
    assert!(first.audio_path.is_absolute(), "paths resolve to absolute");
    assert!(first.audio_path.exists(), "referenced audio exists on disk");
    assert!(!first.text.trim().is_empty(), "transcript non-empty");
    assert_eq!(first.duration, None, "phrase TSV has no duration column");
}

#[test]
fn manifest_quality_checked_train_val_counts() {
    let m = CorpusManifest::load().expect("manifest load");
    let train = m.rows_from(CorpusSource::QualityChecked, Some(Split::Train));
    let val = m.rows_from(CorpusSource::QualityChecked, Some(Split::Val));
    assert_eq!(train.len(), 900, "qc train.tsv must have 900 rows");
    assert_eq!(val.len(), 100, "qc val.tsv must have 100 rows");
    for row in train.iter().chain(val.iter()) {
        assert!(
            row.duration.is_some_and(|d| d > 0.0),
            "qc rows carry positive duration: {:?}",
            row.audio_path
        );
        assert!(!row.text.trim().is_empty());
        assert!(row.audio_path.exists(), "qc audio referenced on disk");
    }
}

#[test]
fn manifest_bulk_tts_counts_and_durations() {
    let m = CorpusManifest::load().expect("manifest load");
    let train = m.rows_from(CorpusSource::BulkTts, Some(Split::Train));
    let val = m.rows_from(CorpusSource::BulkTts, Some(Split::Val));
    assert_eq!(train.len(), 900, "bulk train_1k.tsv (after header) = 900");
    assert_eq!(val.len(), 100, "bulk val_1k.tsv (after header) = 100");
    for row in train.iter().chain(val.iter()) {
        assert!(
            row.duration.is_some_and(|d| d > 0.0),
            "bulk rows carry positive duration: {:?}",
            row.audio_path
        );
        assert!(row.audio_path.exists());
    }
}

#[test]
fn manifest_typed_row_struct_fields() {
    let m = CorpusManifest::load().expect("manifest load");
    let total: usize = m.rows_from(CorpusSource::PhraseTraining, None).len()
        + m.rows_from(CorpusSource::BulkTts, None).len()
        + m.rows_from(CorpusSource::QualityChecked, None).len();
    assert_eq!(total, m.rows.len(), "all rows accounted for by source");
    assert!(m.rows.iter().all(|r| r.audio_path.is_absolute()));
    assert!(m.rows.iter().all(|r| !r.text.trim().is_empty()));
}

#[test]
fn manifest_wellformed_invariant_fires_during_load() {
    use crate::invariants::MANIFEST_WELLFORMED;
    // Loading exercises assert_invariant(MANIFEST_WELLFORMED) per row; the
    // log must show it was enforced.
    let _ = CorpusManifest::load().expect("manifest load");
    assert!(invariant_ppt::has_invariant(MANIFEST_WELLFORMED));
}
