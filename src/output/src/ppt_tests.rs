//! nak.3 gate: `cd src/output && cargo test --all-features`
//!
//! Contract test 'stage11 output' over the invariants from
//! docs/design/provenance-chain-schema.md §6:
//! - provenance_terminates (every chain reaches frame-level acoustic
//!   evidence; no cycles, no dangling refs)
//! - records_loser_and_evidence (every record has >=1 losing hypothesis
//!   and a non-empty decided_by)
//! - score_in_range (all scores finite and in [0,1])
//! - chain_acyclic (layers strictly ascend; no self/higher refs)
//!
//! Plus proptest fuzz over transcripts. Serialized with
//! `invariant_ppt::test_lock`.

use proptest::prelude::*;

use invariant_ppt::{
    assert_invariant, clear_contracts, clear_invariant_log, has_invariant, register_contract,
    test_lock, verify_all_contracts,
};

use crate::{
    ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus, JustificationRef,
    Node, ProvenanceChain, SessionMeta, Transcript, WordRecord,
};

const CONTRACT_NAME: &str = "stage11 output";

const CONTRACT_INVARIANTS: &[&str] = &[
    "provenance_terminates",
    "records_loser_and_evidence",
    "score_in_range",
    "chain_acyclic",
];

/// A minimal valid chain: frame -> phoneme -> lexical, ids 1-3. Ratings
/// descend (90/85/80): in range and monotone non-increasing
/// (bead hearsay-four-shq).
fn sample_chain() -> ProvenanceChain {
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

fn sample_record(chain: ProvenanceChain) -> WordRecord {
    WordRecord::committed(
        "table",
        (0, 420),
        0.91,
        Hypothesis::new("table", 0.91, HypothesisStatus::Committed),
        vec![Hypothesis::new("cable", 0.62, HypothesisStatus::Retracted)],
        Evidence::new(
            EvidenceKind::Acoustic,
            "onset burst energy favors /t/ over /k/",
            0.78,
        ),
        chain,
        85.0,
        "2 channels; log-odds 1.7346; rating 85.00",
    )
}

/// Build a valid chain of the requested shape: `frame_nodes` acoustic nodes,
/// then `upper_layers` strictly ascending layers. Ids are unique and every
/// premise points at a strictly lower layer, so the result validates.
fn valid_chain(frame_nodes: usize, upper_layers: usize, seed: u64) -> ProvenanceChain {
    const LEVELS: [ChainLevel; 5] = [
        ChainLevel::Frame,
        ChainLevel::Phoneme,
        ChainLevel::Syllable,
        ChainLevel::Lexical,
        ChainLevel::Sequence,
    ];
    fn next_rand(state: &mut u64) -> u64 {
        let mut x = *state | 1;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }
    let depth = (1 + upper_layers).min(LEVELS.len());
    let mut layers = Vec::with_capacity(depth);
    let mut next_id: u64 = 1;
    let mut lower_ids: Vec<u64> = Vec::new();
    let mut state = seed;
    // Ratings descend 5.0 per layer from 90.0 at the frame layer: every
    // fuzzed chain is monotone non-increasing by construction
    // (bead hearsay-four-shq).
    let layer_rating = |index: usize| 90.0 - 5.0 * index as f64;
    for (index, level) in LEVELS.iter().enumerate().take(depth) {
        let node_count = if index == 0 {
            frame_nodes.max(1)
        } else {
            1 + (next_rand(&mut state) % 3) as usize
        };
        // Only strictly lower layers are eligible premises (INV chain_acyclic).
        let eligible: Vec<u64> = lower_ids.clone();
        let mut nodes = Vec::with_capacity(node_count);
        for n in 0..node_count {
            let id = next_id;
            next_id += 1;
            let support = if index == 0 {
                vec![]
            } else {
                let premise_count = 1 + (next_rand(&mut state) % 2) as usize;
                let mut premises = Vec::with_capacity(premise_count);
                for _ in 0..premise_count {
                    premises.push(eligible[(next_rand(&mut state) as usize) % eligible.len()]);
                }
                vec![JustificationRef {
                    justification_id: format!("j-{index}-{n}"),
                    premises,
                    unconditional: false,
                }]
            };
            nodes.push(Node {
                id,
                label: format!("node-{id}"),
                support,
                active: true,
                rating: layer_rating(index),
            });
            lower_ids.push(id);
        }
        layers.push(ChainLayer {
            level: *level,
            nodes,
        });
    }
    ProvenanceChain::new(layers)
}

fn valid_record(word: &str, confidence: f32, chain: ProvenanceChain) -> WordRecord {
    WordRecord::committed(
        word,
        (0, 100),
        confidence,
        Hypothesis::new(word, confidence, HypothesisStatus::Committed),
        vec![Hypothesis::new(
            "other",
            (confidence * 0.5).clamp(0.0, 1.0),
            HypothesisStatus::Retracted,
        )],
        Evidence::new(EvidenceKind::Acoustic, "fuzz deciding evidence", 0.5),
        chain,
        // Propagated stage-3 rating on the 0–100 scale mirrors the
        // confidence fixture (bead hearsay-four-shq); the summary is fixed
        // synthetic text, never empty.
        f64::from(confidence) * 100.0,
        "fuzz evidence summary",
    )
}

fn scores_in_range(record: &WordRecord) -> bool {
    [
        record.confidence,
        record.winner.score,
        record.decided_by.strength,
    ]
    .iter()
    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        && record
            .losing
            .iter()
            .all(|h| h.score.is_finite() && (0.0..=1.0).contains(&h.score))
}

fn layers_acyclic(chain: &ProvenanceChain) -> bool {
    fn rank(level: ChainLevel) -> u8 {
        match level {
            ChainLevel::Frame => 0,
            ChainLevel::Phoneme => 1,
            ChainLevel::Syllable => 2,
            ChainLevel::Lexical => 3,
            ChainLevel::Sequence => 4,
        }
    }
    let mut id_layer: std::collections::BTreeMap<u64, usize> = std::collections::BTreeMap::new();
    for (index, layer) in chain.layers.iter().enumerate() {
        if index > 0 && rank(layer.level) <= rank(chain.layers[index - 1].level) {
            return false;
        }
        for node in &layer.nodes {
            if id_layer.insert(node.id, index).is_some() {
                return false;
            }
        }
    }
    for (index, layer) in chain.layers.iter().enumerate() {
        for node in &layer.nodes {
            for justification in &node.support {
                for premise in &justification.premises {
                    match id_layer.get(premise) {
                        Some(&premise_layer) if premise_layer < index => {}
                        _ => return false,
                    }
                }
            }
        }
    }
    true
}

#[test]
fn contract_stage11_output_invariants_enforced() {
    let _guard = test_lock::lock();
    clear_invariant_log();
    clear_contracts();
    register_contract(CONTRACT_NAME, CONTRACT_INVARIANTS);

    // INV provenance_terminates: a grounded chain validates; a chain with
    // the frame layer removed and one with a dangling premise do not.
    let terminates = sample_record(sample_chain()).validate().is_ok();
    let live_ok = WordRecord::committed_from_live(
        "table",
        (0, 420),
        0.91,
        Hypothesis::new("table", 0.91, HypothesisStatus::Committed),
        vec![Hypothesis::new("cable", 0.62, HypothesisStatus::Retracted)],
        Evidence::new(
            EvidenceKind::Acoustic,
            "onset burst energy favors /t/ over /k/",
            0.78,
        ),
        sample_chain(),
        85.0,
        "2 channels; log-odds 1.7346; rating 85.00",
    )
    .is_ok();
    let mut no_base = sample_chain();
    no_base.layers.remove(0);
    let no_base_rejected = sample_record(no_base).validate().is_err();
    let mut dangling = sample_chain();
    dangling.layers[2].nodes[0].support[0].premises = vec![999];
    let dangling_rejected = sample_record(dangling).validate().is_err();
    assert_invariant(
        terminates && live_ok && no_base_rejected && dangling_rejected,
        "provenance_terminates",
        Some("stage11 output"),
    );

    // INV records_loser_and_evidence: the sample record carries both; an
    // emptied loser list and an emptied reason are both rejected.
    let mut no_loser = sample_record(sample_chain());
    no_loser.losing.clear();
    let no_loser_rejected = no_loser.validate().is_err();
    let mut no_evidence = sample_record(sample_chain());
    no_evidence.decided_by.description.clear();
    let no_evidence_rejected = no_evidence.validate().is_err();
    let has_both = !sample_record(sample_chain()).losing.is_empty()
        && !sample_record(sample_chain())
            .decided_by
            .description
            .trim()
            .is_empty();
    assert_invariant(
        has_both && no_loser_rejected && no_evidence_rejected,
        "records_loser_and_evidence",
        Some("stage11 output"),
    );

    // INV score_in_range: the sample record validates; NaN/inf/out-of-range
    // scores are rejected.
    let scores_ok = scores_in_range(&sample_record(sample_chain()));
    let mut bad = sample_record(sample_chain());
    bad.confidence = f32::NAN;
    let nan_rejected = bad.validate().is_err();
    let mut out = sample_record(sample_chain());
    out.winner.score = 1.5;
    let range_rejected = out.validate().is_err();
    assert_invariant(
        scores_ok && nan_rejected && range_rejected,
        "score_in_range",
        Some("stage11 output"),
    );

    // INV chain_acyclic: the sample chain ascends; swapped layers and a
    // self-premise are rejected.
    let acyclic = layers_acyclic(&sample_chain());
    let mut descending = sample_chain();
    descending.layers.swap(0, 2);
    let descending_rejected = sample_record(descending).validate().is_err();
    let mut cyclic = sample_chain();
    cyclic.layers[2].nodes[0].support[0].premises = vec![3];
    let cyclic_rejected = cyclic.validate().is_err();
    assert_invariant(
        acyclic && descending_rejected && cyclic_rejected,
        "chain_acyclic",
        Some("stage11 output"),
    );

    // Every invariant fired.
    for inv in CONTRACT_INVARIANTS {
        assert!(has_invariant(inv), "invariant {inv} was not recorded");
    }
    assert!(
        verify_all_contracts() >= 1,
        "'stage11 output' contract verified"
    );
}

// P-Test: proptest over transcripts (word counts, confidences, chain
// shapes) — every generated record validates, serializes, and round-trips.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_transcripts_validate_and_round_trip(
        n_words in 1usize..=4,
        confidence in 0.0f32..=1.0,
        frame_nodes in 1usize..=3,
        upper_layers in 1usize..=4,
        seed in 0u64..u64::MAX,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract(CONTRACT_NAME, CONTRACT_INVARIANTS);

        let mut words = Vec::with_capacity(n_words);
        for w in 0..n_words {
            let chain = valid_chain(frame_nodes, upper_layers, seed.wrapping_add(w as u64));
            let record = valid_record(&format!("w{w}"), confidence, chain);
            prop_assert!(record.validate().is_ok());
            // Deterministic: validation agrees with itself.
            prop_assert_eq!(record.validate().is_ok(), record.validate().is_ok());
            // JSON round-trip preserves the record and stays valid.
            let json = record.to_json().expect("valid record serializes");
            let back = WordRecord::from_json(&json).expect("round-trip parses");
            prop_assert_eq!(&back, &record);
            prop_assert!(back.validate().is_ok());
            // Each contract invariant holds over this fuzzed record.
            assert_invariant(
                record.provenance.layers.first().is_some_and(|l| l.level == ChainLevel::Frame)
                    && record.validate().is_ok(),
                "provenance_terminates",
                Some("stage11 output"),
            );
            assert_invariant(
                !record.losing.is_empty() && !record.decided_by.description.trim().is_empty(),
                "records_loser_and_evidence",
                Some("stage11 output"),
            );
            assert_invariant(scores_in_range(&record), "score_in_range", Some("stage11 output"));
            assert_invariant(layers_acyclic(&record.provenance), "chain_acyclic", Some("stage11 output"));
            words.push(record);
        }
        let transcript = Transcript::new(
            words,
            SessionMeta {
                speaker_id: "spk-fuzz".to_string(),
                recording_ref: "rec-fuzz".to_string(),
                started_ms: 0,
            },
            None,
        );
        prop_assert_eq!(transcript.words.len(), n_words);
        prop_assert!(verify_all_contracts() >= 1);
    }
}

// P-Test: proptest over score values — validation agrees with the [0,1]
// finite range check on every fuzzed score, in every score field.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_score_range_matches_validation(
        field in 0usize..5,
        value in proptest::num::f32::ANY,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract(CONTRACT_NAME, &["score_in_range"]);

        let mut record = valid_record("w", 0.7, sample_chain());
        match field {
            0 => record.confidence = value,
            1 => record.winner.score = value,
            2 => record.decided_by.strength = value,
            3 => record.losing[0].score = value,
            _ => record.losing.push(Hypothesis::new(
                "extra",
                value,
                HypothesisStatus::Retracted,
            )),
        }
        let in_range = value.is_finite() && (0.0..=1.0).contains(&value);
        prop_assert_eq!(record.validate().is_ok(), in_range);
        assert_invariant(
            record.validate().is_ok() == in_range,
            "score_in_range",
            Some("stage11 output"),
        );
        prop_assert!(verify_all_contracts() >= 1);
    }
}
