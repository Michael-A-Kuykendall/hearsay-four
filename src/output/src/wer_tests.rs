//! A2 gate tests (bead hearsay-four-chd): word-level WER scorer.
//!
//! Gate: `cd src/output && cargo test --offline wer` -> ok. 6 passed.
//! Synthetic fixtures only (A1 types + hand-built word lists, no audio).

use crate::{
    AlignOp, ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus,
    JustificationRef, Node, ProvenanceChain, SessionMeta, Transcript, WordRecord, transcript_wer,
    wer,
};

fn w(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}

fn sample_record(word: &str, id_base: u64) -> WordRecord {
    let chain = ProvenanceChain::new(vec![
        ChainLayer {
            level: ChainLevel::Frame,
            nodes: vec![Node {
                id: id_base,
                label: format!("FRAME:{id_base}"),
                support: vec![],
                active: true,
                rating: 90.0,
            }],
        },
        ChainLayer {
            level: ChainLevel::Phoneme,
            nodes: vec![Node {
                id: id_base + 1,
                label: "/t/".to_string(),
                support: vec![JustificationRef {
                    justification_id: format!("j-phon-{id_base}"),
                    premises: vec![id_base],
                    unconditional: false,
                }],
                active: true,
                rating: 85.0,
            }],
        },
        ChainLayer {
            level: ChainLevel::Lexical,
            nodes: vec![Node {
                id: id_base + 2,
                label: format!("WORD:{word}"),
                support: vec![JustificationRef {
                    justification_id: format!("j-lex-{id_base}"),
                    premises: vec![id_base + 1],
                    unconditional: false,
                }],
                active: true,
                rating: 80.0,
            }],
        },
    ]);
    WordRecord::committed(
        word,
        (0, 100),
        0.9,
        Hypothesis::new(word, 0.9, HypothesisStatus::Committed),
        vec![Hypothesis::new("other", 0.4, HypothesisStatus::Retracted)],
        Evidence::new(EvidenceKind::Acoustic, "synthetic fixture", 0.7),
        chain,
        85.0,
        "2 channels; log-odds 1.7346; rating 85.00",
    )
}

#[test]
fn wer_identical_sequences_score_zero() {
    let reference = w(&["the", "cat", "sat"]);
    let score = wer(&reference, &reference.clone());
    assert_eq!(score.wer, 0.0);
    assert_eq!(score.substitutions, 0);
    assert_eq!(score.deletions, 0);
    assert_eq!(score.insertions, 0);
    assert_eq!(score.reference_len, 3);
    assert_eq!(score.alignment.len(), 3);
    assert!(
        score.alignment.iter().all(|s| s.op == AlignOp::Correct),
        "identical sequences align all-correct: {:?}",
        score.alignment
    );
}

#[test]
fn wer_single_substitution_exact_fraction() {
    let score = wer(&w(&["the", "cat", "sat"]), &w(&["the", "dog", "sat"]));
    assert_eq!(score.substitutions, 1);
    assert_eq!(score.deletions, 0);
    assert_eq!(score.insertions, 0);
    assert!(
        (score.wer - 1.0 / 3.0).abs() < 1e-12,
        "one substitution over three reference words: {}",
        score.wer
    );
    let ops: Vec<AlignOp> = score.alignment.iter().map(|s| s.op).collect();
    assert_eq!(
        ops,
        vec![AlignOp::Correct, AlignOp::Substitution, AlignOp::Correct]
    );
}

#[test]
fn wer_mixed_substitution_deletion_insertion_fixture() {
    // S: cat->dog, D: mat, I: extra => 3 errors over 7 reference words.
    // Exact-match anchors (sat/on before the deletion, red/up after it)
    // pin the optimal alignment so the S/D/I mix is unambiguous.
    let score = wer(
        &w(&["the", "cat", "sat", "on", "mat", "red", "up"]),
        &w(&["the", "dog", "sat", "on", "red", "up", "extra"]),
    );
    assert_eq!(score.substitutions, 1);
    assert_eq!(score.deletions, 1);
    assert_eq!(score.insertions, 1);
    assert!(
        (score.wer - 3.0 / 7.0).abs() < 1e-12,
        "S+D+I over reference length: {}",
        score.wer
    );
    let ops: Vec<AlignOp> = score.alignment.iter().map(|s| s.op).collect();
    assert_eq!(
        ops,
        vec![
            AlignOp::Correct,
            AlignOp::Substitution,
            AlignOp::Correct,
            AlignOp::Correct,
            AlignOp::Deletion,
            AlignOp::Correct,
            AlignOp::Correct,
            AlignOp::Insertion,
        ]
    );
    assert_eq!(
        score.alignment[4].reference.as_deref(),
        Some("mat"),
        "deletion carries the reference word"
    );
    assert_eq!(
        score.alignment[7].hypothesis.as_deref(),
        Some("extra"),
        "insertion carries the hypothesis word"
    );
}

#[test]
fn wer_empty_hypothesis_is_all_deletions() {
    let reference = w(&["the", "cat", "sat", "mat"]);
    let score = wer(&reference, &[]);
    assert_eq!(score.wer, 1.0);
    assert_eq!(score.substitutions, 0);
    assert_eq!(score.deletions, 4);
    assert_eq!(score.insertions, 0);
    assert_eq!(score.alignment.len(), 4);
    assert!(
        score.alignment.iter().all(|s| s.op == AlignOp::Deletion),
        "empty hypothesis aligns all-deletion: {:?}",
        score.alignment
    );
}

#[test]
fn wer_normalizes_case_and_punctuation() {
    // Case and surrounding punctuation normalize away; filler punctuation
    // tokens drop out entirely.
    let score = wer(&w(&["Hello,", "WORLD"]), &w(&["hello", "world"]));
    assert_eq!(score.wer, 0.0);
    let score = wer(&w(&["hello", "world"]), &w(&["hello", "...", "world"]));
    assert_eq!(score.wer, 0.0);
    assert_eq!(score.reference_len, 2);
}

#[test]
fn wer_scored_transcript_still_validates() {
    let transcript = Transcript::new(
        vec![
            sample_record("the", 1),
            sample_record("cat", 10),
            sample_record("sat", 20),
        ],
        SessionMeta {
            speaker_id: "synthetic".to_string(),
            recording_ref: "fixture".to_string(),
            started_ms: 0,
        },
        None,
    );
    let score = transcript_wer(&w(&["the", "cat", "sat"]), &transcript);
    assert_eq!(score.wer, 0.0);
    for record in &transcript.words {
        record
            .validate()
            .expect("scored transcript records still validate");
    }
}
