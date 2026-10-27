//! C1 gate tests (bead hearsay-four-shq): cross-level rating propagation.
//!
//! DoD (from bead acceptance):
//! 1. winner rating lands on WordRecord verbatim
//! 2. out-of-range rating fails validate red
//! 3. multi-evidence combine equals add_evidence sum on fixed fixture
//!
//! Existing competition tests are unmodified and green — see nbest_tests.rs.
//!
//! Synthetic fixtures only: fixed score maps and hand-built chains, no
//! audio, no model, no network. No scheduling layer is built here
//! (alpha-indexed-dispatch): these tests only move ratings.

use std::collections::BTreeMap;

use crate::competition::{
    NBestCandidate, PRIOR_LOG_ODDS, add_evidence, calibration_weight, emit_nbest, evidence_summary,
    propagate_winner, winner,
};
use fabric::Fabric;
use output::{
    ChainError, ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus,
    JustificationRef, Node, ProvenanceChain, WordRecord,
};

/// Minimal valid chain with monotone ratings 90/85/80
/// (frame/phoneme/lexical), ids 1-3.
fn propagation_chain() -> ProvenanceChain {
    ProvenanceChain::new(vec![
        ChainLayer {
            level: ChainLevel::Frame,
            nodes: vec![Node {
                id: 1,
                label: "FRAME:0-20ms".to_string(),
                support: vec![],
                active: true,
                rating: 90.0,
            }],
        },
        ChainLayer {
            level: ChainLevel::Phoneme,
            nodes: vec![Node {
                id: 2,
                label: "/t/".to_string(),
                support: vec![JustificationRef {
                    justification_id: "j-phon-1".to_string(),
                    premises: vec![1],
                    unconditional: false,
                }],
                active: true,
                rating: 85.0,
            }],
        },
        ChainLayer {
            level: ChainLevel::Lexical,
            nodes: vec![Node {
                id: 3,
                label: "WORD:table".to_string(),
                support: vec![JustificationRef {
                    justification_id: "j-lex-1".to_string(),
                    premises: vec![2],
                    unconditional: false,
                }],
                active: true,
                rating: 80.0,
            }],
        },
    ])
}

fn propagation_record(chain: ProvenanceChain, source_rating: f64) -> WordRecord {
    WordRecord::committed(
        "AA",
        (0, 100),
        0.9,
        Hypothesis::new("AA", 0.9, HypothesisStatus::Committed),
        vec![Hypothesis::new("AE", 0.6, HypothesisStatus::Retracted)],
        Evidence::new(EvidenceKind::Calibration, "synthetic stage-3 fixture", 0.7),
        chain,
        source_rating,
        "2 channels; log-odds 1.7346; rating 85.00",
    )
}

#[test]
fn propagation_winner_rating_lands_on_word_record_verbatim() {
    // (1) The winner's stage-3 rating reaches the committed WordRecord
    // unchanged: bundle rating == competition rating == record rating.
    let mut f = Fabric::new();
    let mut scores = BTreeMap::new();
    scores.insert("AA".to_string(), 0.9);
    scores.insert("AE".to_string(), 0.6);
    scores.insert("AH".to_string(), 0.4);
    let set = emit_nbest(&mut f, &scores, 3.0);
    let propagated = propagate_winner(&set).expect("winner propagates");
    let expected = winner(&set).expect("winner exists").rating();
    assert_eq!(propagated.rating, expected, "rating moves verbatim");

    let record = WordRecord::committed(
        "AA",
        (0, 100),
        0.9,
        Hypothesis::new("AA", 0.9, HypothesisStatus::Committed),
        vec![Hypothesis::new("AE", 0.6, HypothesisStatus::Retracted)],
        Evidence::new(EvidenceKind::Calibration, "synthetic stage-3 fixture", 0.7),
        propagation_chain(),
        propagated.rating,
        propagated.summary.clone(),
    );
    assert_eq!(record.source_rating, expected, "verbatim on the record");
    assert!(!record.evidence_summary.is_empty());
    assert!(record.validate().is_ok());

    // No scheduling: an empty frame propagates nothing (no winner invented).
    let empty = emit_nbest(&mut f, &BTreeMap::new(), 1.0);
    assert!(propagate_winner(&empty).is_none());
}

#[test]
fn propagation_out_of_range_rating_fails_validate() {
    // (2) An out-of-range propagated rating fails validate red, both on the
    // record and on a chain node.
    assert!(
        propagation_record(propagation_chain(), 85.0)
            .validate()
            .is_ok()
    );

    let mut over = propagation_record(propagation_chain(), 85.0);
    over.source_rating = 150.0;
    assert_eq!(over.validate(), Err(ChainError::SourceRatingOutOfRange));

    let mut negative = propagation_record(propagation_chain(), 85.0);
    negative.source_rating = -0.5;
    assert_eq!(negative.validate(), Err(ChainError::SourceRatingOutOfRange));

    let mut bad_node = propagation_chain();
    bad_node.layers[1].nodes[0].rating = -1.0;
    assert_eq!(
        bad_node.validate(),
        Err(ChainError::NodeRatingOutOfRange { node: 2 })
    );
}

#[test]
fn propagation_multi_evidence_combine_equals_add_evidence_sum() {
    // (3) Fixed fixture: two calibrated weights combine additively, and the
    // final rating equals 100·σ of the summed log-odds.
    let w1 = calibration_weight(0.75);
    let w2 = calibration_weight(0.8);
    let mut cand = NBestCandidate {
        fact: fabric::FactId(1),
        log_odds: PRIOR_LOG_ODDS,
        evidence_count: 0,
    };
    let r1 = add_evidence(&mut cand, w1);
    let r2 = add_evidence(&mut cand, w2);

    let expected_log_odds = PRIOR_LOG_ODDS + w1 + w2;
    assert!(
        (cand.log_odds - expected_log_odds).abs() < 1e-12,
        "additive combination: {} vs {expected_log_odds}",
        cand.log_odds
    );
    assert_eq!(cand.evidence_count, 2);
    let expected_rating = 100.0 / (1.0 + (-expected_log_odds).exp());
    assert!(
        (r2 - expected_rating).abs() < 1e-9,
        "rating equals 100·σ of the sum: {r2} vs {expected_rating}"
    );
    assert!(r2 >= r1, "supporting evidence never decreases a rating");
    assert!((0.0..=100.0).contains(&r2), "rating_in_range");
}

#[test]
fn propagation_evidence_summary_names_channels_and_rating() {
    // The per-record summary reports the channel count and the rating, and
    // the propagated bundle carries the winner's summary unchanged.
    let mut cand = NBestCandidate {
        fact: fabric::FactId(7),
        log_odds: PRIOR_LOG_ODDS,
        evidence_count: 0,
    };
    add_evidence(&mut cand, calibration_weight(0.75));
    add_evidence(&mut cand, calibration_weight(0.6));
    assert_eq!(cand.evidence_count, 2);
    let summary = evidence_summary(&cand);
    let expected = format!(
        "2 channels; log-odds {:.4}; rating {:.2}",
        cand.log_odds,
        cand.rating()
    );
    assert_eq!(summary, expected);
    assert!(summary.contains("2 channels"));

    let single = NBestCandidate {
        fact: fabric::FactId(8),
        log_odds: PRIOR_LOG_ODDS + calibration_weight(0.9),
        evidence_count: 1,
    };
    assert!(evidence_summary(&single).starts_with("1 channel;"));

    let mut f = Fabric::new();
    let mut scores = BTreeMap::new();
    scores.insert("AA".to_string(), 0.9);
    let set = emit_nbest(&mut f, &scores, 1.0);
    let propagated = propagate_winner(&set).expect("winner propagates");
    let won = winner(&set).expect("winner exists");
    assert_eq!(propagated.summary, evidence_summary(won));
    assert_eq!(propagated.evidence_count, won.evidence_count);
}

#[test]
fn propagation_monotone_chain_validates_and_rise_rejected() {
    // Ratings descend frame -> sequence: the chain validates; a rating that
    // rises above its premise is rejected on that edge.
    assert!(propagation_chain().validate().is_ok());

    let mut rising = propagation_chain();
    rising.layers[2].nodes[0].rating = 95.0;
    assert_eq!(
        rising.validate(),
        Err(ChainError::RatingIncreasesAlongChain {
            node: 3,
            premise: 2
        })
    );

    // The propagated record over a monotone chain validates end to end.
    assert!(
        propagation_record(propagation_chain(), 85.0)
            .validate()
            .is_ok()
    );
}
