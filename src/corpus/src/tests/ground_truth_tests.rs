//! A1 gate (bead hearsay-four-e24): Oyez ground-truth word-list extractor.
//!
//! Offline, synthetic fixtures shaped like `session.tsv` manifest rows
//! (`start_sec` / `stop_sec` / `text`); no network, no new data. The
//! number-word text in `ground_truth_number_word_passthrough` mirrors the
//! committed `data/oyez-pilot/session.tsv` row 0
//! ("We will hear argument next today in Case 10-218, PPL Montana v.
//! Montana.").

use crate::ground_truth::{TimedSegment, extract_words};

#[test]
fn ground_truth_two_segments_yield_exact_word_timing_list() {
    let rows = vec![
        TimedSegment::new(0.0, 2.0, "Hello world"),
        TimedSegment::new(2.0, 3.0, "Case 10-218"),
    ];
    assert_eq!(
        extract_words(&rows),
        vec![
            ("hello".to_string(), 0, 1000),
            ("world".to_string(), 1000, 2000),
            ("case".to_string(), 2000, 2500),
            ("10-218".to_string(), 2500, 3000),
        ]
    );
}

#[test]
fn ground_truth_empty_rows_yield_empty_no_panic() {
    assert!(extract_words(&[]).is_empty(), "no rows -> no words");
    assert!(
        extract_words(&[TimedSegment::new(0.0, 1.0, "")]).is_empty(),
        "empty text -> no words"
    );
    assert!(
        extract_words(&[TimedSegment::new(0.0, 1.0, "   ,. -- ")]).is_empty(),
        "punctuation-only text -> no words"
    );
}

#[test]
fn ground_truth_case_punctuation_normalization() {
    let rows = vec![TimedSegment::new(
        7.245,
        9.761,
        "Mr. Chief Justice and may it PLEASE the Court:",
    )];
    let words = extract_words(&rows);
    let texts: Vec<&str> = words.iter().map(|(w, _, _)| w.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "mr", "chief", "justice", "and", "may", "it", "please", "the", "court"
        ]
    );
    // Timings stay inside the segment span and remain contiguous.
    let (start_ms, end_ms) = (7245u64, 9761u64);
    assert_eq!(words.first().unwrap().1, start_ms);
    assert_eq!(words.last().unwrap().2, end_ms);
    for pair in words.windows(2) {
        assert_eq!(pair[0].2, pair[1].1, "word timings contiguous");
        assert!(pair[0].1 < pair[0].2, "each word advances");
    }
}

#[test]
fn ground_truth_number_word_passthrough() {
    // Text mirrors committed data/oyez-pilot/session.tsv row 0.
    let rows = vec![TimedSegment::new(
        0.0,
        5.95,
        "We will hear argument next today in Case 10-218, PPL Montana v. Montana.",
    )];
    let words = extract_words(&rows);
    let texts: Vec<&str> = words.iter().map(|(w, _, _)| w.as_str()).collect();
    assert!(texts.contains(&"10-218"), "number-word passes through");
    assert!(texts.contains(&"ppl"), "acronym lowercased, not dropped");
    assert_eq!(texts.len(), 13, "exact token count: {:?}", texts);
    assert_eq!(words.first().unwrap().1, 0);
    assert_eq!(words.last().unwrap().2, 5950);
}
