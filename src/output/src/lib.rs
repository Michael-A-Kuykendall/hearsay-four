//! Stage 11 output scaffold (bead hearsay-four-zm9).
//!
//! Per-word provenance records shaped by
//! `docs/design/provenance-chain-schema.md`: every transcribed word maps to
//! exactly one [`WordRecord`] carrying the winning hypothesis, the losing
//! candidates, the deciding evidence, and the derivation chain down to raw
//! acoustic evidence.
//!
//! This crate only establishes the data shape, a constructor from committed
//! stage data, and the gate path `cd src/output && cargo test
//! provenance_chain`. The full chain-terminates-at-acoustic-evidence logic
//! (invariants `provenance_terminates`, `chain_acyclic`, contract tests)
//! stays in nak.1 — it is deliberately NOT built here.

/// One candidate word or phoneme string with its competition score.
#[derive(Debug, Clone, PartialEq)]
pub struct Hypothesis {
    /// Candidate word or phoneme string.
    pub text: String,
    /// Confidence/competition score, expected in [0,1] (INV `score_in_range`).
    pub score: f32,
    /// Survival status of this hypothesis.
    pub status: HypothesisStatus,
}

/// Survival status of a hypothesis (schema section 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypothesisStatus {
    Committed,
    Retracted,
    Survived,
}

/// The specific evidence that decided winner over loser (schema 3.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// Human-readable why.
    pub description: String,
    /// Magnitude of the deciding evidence, expected in [0,1].
    pub strength: f32,
}

/// Evidence source kinds (schema section 3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    Lexical,
    Phonological,
    Sequence,
    Acoustic,
    Calibration,
}

/// One derivation layer, strictly ascending frame -> sequence (schema 3.5).
#[derive(Debug, Clone, PartialEq)]
pub struct ChainLayer {
    pub level: ChainLevel,
    pub nodes: Vec<Node>,
}

/// Layer levels, in strictly ascending derivation order (schema section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainLevel {
    Frame,
    Phoneme,
    Syllable,
    Lexical,
    Sequence,
}

/// One derivation node.
///
/// `id` stands in for the hash-consed arena `NodeId`; the mapping to the
/// real arena ids is nak.1's job, not this scaffold's.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u64,
    /// E.g. "/t/", "SYL:ta", "WORD:table".
    pub label: String,
    /// Justification sets that support this node.
    pub support: Vec<JustificationRef>,
    /// Current survival status.
    pub active: bool,
}

/// Reference to an arena justification set (survival semantics, aem.13).
#[derive(Debug, Clone, PartialEq)]
pub struct JustificationRef {
    pub justification_id: String,
    /// Premise node ids (strictly lower layers only, INV `chain_acyclic`).
    pub premises: Vec<u64>,
    /// Unconditional vs defeasible justification.
    pub unconditional: bool,
}

/// The derivation, layer by layer, down to acoustic evidence (schema 3.5).
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceChain {
    /// Acoustic -> phone -> syllable -> lexical -> sequence.
    pub layers: Vec<ChainLayer>,
}

/// The per-word root record (schema section 3.2).
#[derive(Debug, Clone, PartialEq)]
pub struct WordRecord {
    /// The transcribed word.
    pub text: String,
    /// Start of the acoustic span.
    pub start_ms: u64,
    /// End of the acoustic span.
    pub end_ms: u64,
    /// Committed confidence in [0,1].
    pub confidence: f32,
    /// The chosen word hypothesis.
    pub winner: Hypothesis,
    /// Competing candidates that lost (INV `records_loser_and_evidence`).
    pub losing: Vec<Hypothesis>,
    /// The deciding evidence (INV `records_loser_and_evidence`).
    pub decided_by: Evidence,
    /// Full derivation to acoustic evidence.
    pub provenance: ProvenanceChain,
}

/// Session metadata for a [`Transcript`] (schema section 3.1).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMeta {
    pub speaker_id: String,
    pub recording_ref: String,
    pub started_ms: u64,
}

/// Transcript output: words plus session context (schema section 3.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub words: Vec<WordRecord>,
    pub session: SessionMeta,
    /// "A" | "B" (fail-fast), else `None`.
    pub conditions: Option<String>,
}

impl Hypothesis {
    pub fn new(text: impl Into<String>, score: f32, status: HypothesisStatus) -> Self {
        Self {
            text: text.into(),
            score,
            status,
        }
    }
}

impl Evidence {
    pub fn new(kind: EvidenceKind, description: impl Into<String>, strength: f32) -> Self {
        Self {
            kind,
            description: description.into(),
            strength,
        }
    }
}

impl ProvenanceChain {
    pub fn new(layers: Vec<ChainLayer>) -> Self {
        Self { layers }
    }
}

impl WordRecord {
    /// Assemble a record from committed stage data: the winning hypothesis,
    /// the hypotheses it beat, the evidence that decided the competition,
    /// and the derivation chain. Validation of the chain (termination at
    /// acoustic evidence, acyclicity) belongs to nak.1, not this constructor.
    pub fn committed(
        text: impl Into<String>,
        span_ms: (u64, u64),
        confidence: f32,
        winner: Hypothesis,
        losing: Vec<Hypothesis>,
        decided_by: Evidence,
        provenance: ProvenanceChain,
    ) -> Self {
        Self {
            text: text.into(),
            start_ms: span_ms.0,
            end_ms: span_ms.1,
            confidence,
            winner,
            losing,
            decided_by,
            provenance,
        }
    }
}

impl Transcript {
    pub fn new(words: Vec<WordRecord>, session: SessionMeta, conditions: Option<String>) -> Self {
        Self {
            words,
            session,
            conditions,
        }
    }
}

/// A provenance/record validation failure (contract invariants, nak.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    /// No layers at all; nothing grounds the word.
    EmptyChain,
    /// The bottom layer is not frame-level acoustic evidence.
    BaseNotFrame,
    /// Layer levels do not strictly ascend (index of offending layer).
    LevelsNotAscending(usize),
    /// Two nodes share one id (would allow cyclic reads).
    DuplicateNodeId(u64),
    /// A frame node carries support refs instead of acoustic grounding.
    FrameNodeHasSupport(u64),
    /// A non-frame node has no justification (dangling mid-chain root).
    NodeWithoutSupport(u64),
    /// A premise id resolves to no node in any layer.
    DanglingPremise { node: u64, premise: u64 },
    /// A premise points at the same or a higher layer (cycle risk).
    NonAscendingPremise { node: u64, premise: u64 },
    /// No premise path from this node down to a frame node.
    UngroundedNode(u64),
    /// `losing` is empty (INV `records_loser_and_evidence`).
    NoLosingHypothesis,
    /// `decided_by` carries no human-readable reason.
    EmptyDecidingEvidence,
    /// A score/strength field is NaN, infinite, or outside [0,1].
    ScoreOutOfRange(&'static str),
}

impl ChainLevel {
    fn rank(self) -> u8 {
        match self {
            ChainLevel::Frame => 0,
            ChainLevel::Phoneme => 1,
            ChainLevel::Syllable => 2,
            ChainLevel::Lexical => 3,
            ChainLevel::Sequence => 4,
        }
    }
}

impl ProvenanceChain {
    /// Check INV `provenance_terminates` + INV `chain_acyclic`: layers start
    /// at frame-level acoustic evidence, strictly ascend, every premise
    /// resolves to a strictly lower layer, and every node reaches a frame
    /// node by following premises.
    pub fn validate(&self) -> Result<(), ChainError> {
        if self.layers.is_empty() {
            return Err(ChainError::EmptyChain);
        }
        if self.layers[0].level != ChainLevel::Frame {
            return Err(ChainError::BaseNotFrame);
        }
        for (index, window) in self.layers.windows(2).enumerate() {
            if window[1].level.rank() <= window[0].level.rank() {
                return Err(ChainError::LevelsNotAscending(index + 1));
            }
        }
        let mut layer_of: std::collections::BTreeMap<u64, usize> =
            std::collections::BTreeMap::new();
        for (index, layer) in self.layers.iter().enumerate() {
            for node in &layer.nodes {
                if layer_of.insert(node.id, index).is_some() {
                    return Err(ChainError::DuplicateNodeId(node.id));
                }
            }
        }
        for (index, layer) in self.layers.iter().enumerate() {
            for node in &layer.nodes {
                if index == 0 {
                    if !node.support.is_empty() {
                        return Err(ChainError::FrameNodeHasSupport(node.id));
                    }
                } else if node.support.is_empty() {
                    return Err(ChainError::NodeWithoutSupport(node.id));
                }
                for justification in &node.support {
                    for &premise in &justification.premises {
                        match layer_of.get(&premise) {
                            None => {
                                return Err(ChainError::DanglingPremise {
                                    node: node.id,
                                    premise,
                                });
                            }
                            Some(&premise_layer) if premise_layer >= index => {
                                return Err(ChainError::NonAscendingPremise {
                                    node: node.id,
                                    premise,
                                });
                            }
                            Some(_) => {}
                        }
                    }
                }
            }
        }
        // Every node must reach a frame node by descending premises.
        // Frame nodes are grounded by construction; memoize per node.
        let mut grounded: std::collections::BTreeMap<u64, bool> = std::collections::BTreeMap::new();
        for layer in &self.layers {
            for node in &layer.nodes {
                if !Self::reaches_frame(node.id, &self.layers, &layer_of, &mut grounded) {
                    return Err(ChainError::UngroundedNode(node.id));
                }
            }
        }
        Ok(())
    }

    fn reaches_frame(
        id: u64,
        layers: &[ChainLayer],
        layer_of: &std::collections::BTreeMap<u64, usize>,
        grounded: &mut std::collections::BTreeMap<u64, bool>,
    ) -> bool {
        if let Some(&done) = grounded.get(&id) {
            return done;
        }
        // Cycle guard: mark in-progress as ungrounded while descending.
        grounded.insert(id, false);
        let index = match layer_of.get(&id) {
            None => return false,
            Some(&index) => index,
        };
        if index == 0 {
            grounded.insert(id, true);
            return true;
        }
        let node = layers[index]
            .nodes
            .iter()
            .find(|node| node.id == id)
            .expect("id mapped to this layer");
        let mut ok = false;
        'outer: for justification in &node.support {
            // Vacuous (premise-free) support above the frame layer grounds
            // nothing; only frame nodes bottom out the chain.
            if justification.premises.is_empty() {
                continue;
            }
            for &premise in &justification.premises {
                if !Self::reaches_frame(premise, layers, layer_of, grounded) {
                    continue 'outer;
                }
            }
            ok = true;
            break;
        }
        grounded.insert(id, ok);
        ok
    }
}

impl WordRecord {
    /// Build a record from live justification sets, then enforce the
    /// contract before returning it.
    ///
    /// Live-read contract (fabric `proof_tree` pattern, see
    /// `src/fabric/src/provenance.rs`): the caller walks the live
    /// `Fabric::justifications` graph for the committed word — the same
    /// always-on sets bead 6za.2 keeps, never a query-time reconstruction —
    /// maps each fact to a [`Node`] (hash-consed arena `NodeId` as `id`) and
    /// each live justification set to a [`JustificationRef`], groups nodes
    /// into strictly ascending [`ChainLayer`]s bottoming out at frame-level
    /// acoustic evidence, and passes the result here with the committed
    /// winner, the losing hypotheses it beat (stage3/stage4 N-best outputs),
    /// and the deciding evidence. Validation (INVs `provenance_terminates`,
    /// `records_loser_and_evidence`, `score_in_range`, `chain_acyclic`)
    /// runs before the record is returned.
    pub fn committed_from_live(
        text: impl Into<String>,
        span_ms: (u64, u64),
        confidence: f32,
        winner: Hypothesis,
        losing: Vec<Hypothesis>,
        decided_by: Evidence,
        provenance: ProvenanceChain,
    ) -> Result<Self, ChainError> {
        let record = Self::committed(
            text, span_ms, confidence, winner, losing, decided_by, provenance,
        );
        record.validate()?;
        Ok(record)
    }

    /// Check INVs `records_loser_and_evidence`, `score_in_range`, plus the
    /// full chain contract (`provenance_terminates`, `chain_acyclic`).
    pub fn validate(&self) -> Result<(), ChainError> {
        if self.losing.is_empty() {
            return Err(ChainError::NoLosingHypothesis);
        }
        if self.decided_by.description.trim().is_empty() {
            return Err(ChainError::EmptyDecidingEvidence);
        }
        for (field, value) in [
            ("confidence", self.confidence),
            ("winner.score", self.winner.score),
            ("decided_by.strength", self.decided_by.strength),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(ChainError::ScoreOutOfRange(field));
            }
        }
        for hypothesis in &self.losing {
            if !hypothesis.score.is_finite() || !(0.0..=1.0).contains(&hypothesis.score) {
                return Err(ChainError::ScoreOutOfRange("losing.score"));
            }
        }
        self.provenance.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chain() -> ProvenanceChain {
        ProvenanceChain::new(vec![
            ChainLayer {
                level: ChainLevel::Frame,
                nodes: vec![Node {
                    id: 1,
                    label: "FRAME:0-20ms".to_string(),
                    support: vec![],
                    active: true,
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
                }],
            },
            ChainLayer {
                level: ChainLevel::Syllable,
                nodes: vec![Node {
                    id: 3,
                    label: "SYL:ta".to_string(),
                    support: vec![JustificationRef {
                        justification_id: "j-syl-1".to_string(),
                        premises: vec![2],
                        unconditional: false,
                    }],
                    active: true,
                }],
            },
            ChainLayer {
                level: ChainLevel::Lexical,
                nodes: vec![Node {
                    id: 4,
                    label: "WORD:table".to_string(),
                    support: vec![JustificationRef {
                        justification_id: "j-lex-1".to_string(),
                        premises: vec![3],
                        unconditional: false,
                    }],
                    active: true,
                }],
            },
            ChainLayer {
                level: ChainLevel::Sequence,
                nodes: vec![Node {
                    id: 5,
                    label: "SEQ:table".to_string(),
                    support: vec![JustificationRef {
                        justification_id: "j-seq-1".to_string(),
                        premises: vec![4],
                        unconditional: false,
                    }],
                    active: true,
                }],
            },
        ])
    }

    #[test]
    fn provenance_chain_scaffold_records_winner_loser_and_evidence() {
        let record = WordRecord::committed(
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
        );

        // Winner, loser, and deciding evidence share one record: the
        // forensic property the whole output exists for.
        assert_eq!(record.text, "table");
        assert_eq!(record.winner.text, "table");
        assert!(
            record.losing.iter().any(|h| h.text == "cable"),
            "losing candidate missing"
        );
        assert!(!record.decided_by.description.is_empty());

        // Scores in range (INV `score_in_range`).
        for score in [
            record.confidence,
            record.winner.score,
            record.decided_by.strength,
        ] {
            assert!(score.is_finite() && (0.0..=1.0).contains(&score));
        }
        for hypothesis in &record.losing {
            assert!(hypothesis.score.is_finite());
            assert!((0.0..=1.0).contains(&hypothesis.score));
        }

        // Layers strictly ascend frame -> sequence (schema section 4).
        let levels: Vec<ChainLevel> = record.provenance.layers.iter().map(|l| l.level).collect();
        assert_eq!(
            levels,
            vec![
                ChainLevel::Frame,
                ChainLevel::Phoneme,
                ChainLevel::Syllable,
                ChainLevel::Lexical,
                ChainLevel::Sequence,
            ]
        );

        // Transcript assembly from committed stage data.
        let transcript = Transcript::new(
            vec![record],
            SessionMeta {
                speaker_id: "spk-01".to_string(),
                recording_ref: "rec-001".to_string(),
                started_ms: 0,
            },
            None,
        );
        assert_eq!(transcript.words.len(), 1);
    }

    fn live_record(chain: ProvenanceChain) -> WordRecord {
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
        )
    }

    #[test]
    fn provenance_chain_terminates_at_acoustic_evidence() {
        // A committed word validates: the chain bottoms out at frame-level
        // acoustic evidence (INV `provenance_terminates`).
        let record = live_record(sample_chain());
        assert!(record.validate().is_ok());
        assert!(
            WordRecord::committed_from_live(
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
            )
            .is_ok()
        );

        // A premise pointing at no node is a dangling ref: rejected.
        let mut dangling = sample_chain();
        dangling.layers[4].nodes[0].support[0].premises = vec![999];
        assert_eq!(
            live_record(dangling).validate(),
            Err(ChainError::DanglingPremise {
                node: 5,
                premise: 999
            })
        );

        // The chain must bottom out at frame evidence, not above it.
        let mut no_base = sample_chain();
        no_base.layers.remove(0);
        assert_eq!(
            live_record(no_base).validate(),
            Err(ChainError::BaseNotFrame)
        );

        // Frame nodes are acoustic ground: they carry no support refs.
        let mut frame_support = sample_chain();
        frame_support.layers[0].nodes[0]
            .support
            .push(JustificationRef {
                justification_id: "j-frame-bogus".to_string(),
                premises: vec![],
                unconditional: true,
            });
        assert_eq!(
            live_record(frame_support).validate(),
            Err(ChainError::FrameNodeHasSupport(1))
        );
    }

    #[test]
    fn provenance_chain_records_loser_and_evidence() {
        // INV `records_loser_and_evidence`: >=1 losing hypothesis plus the
        // deciding evidence, on every record.
        let record = live_record(sample_chain());
        assert!(!record.losing.is_empty());
        assert!(!record.decided_by.description.is_empty());

        let mut no_loser = live_record(sample_chain());
        no_loser.losing.clear();
        assert_eq!(no_loser.validate(), Err(ChainError::NoLosingHypothesis));

        let mut no_evidence = live_record(sample_chain());
        no_evidence.decided_by.description.clear();
        assert_eq!(
            no_evidence.validate(),
            Err(ChainError::EmptyDecidingEvidence)
        );

        let mut bad_score = live_record(sample_chain());
        bad_score.confidence = f32::NAN;
        assert_eq!(
            bad_score.validate(),
            Err(ChainError::ScoreOutOfRange("confidence"))
        );
    }

    #[test]
    fn provenance_chain_strictly_ascends_acyclic() {
        // INV `chain_acyclic`: layers strictly ascend frame -> sequence and
        // no premise points at the same or a higher layer.
        assert!(live_record(sample_chain()).validate().is_ok());

        let mut descending = sample_chain();
        descending.layers.swap(1, 3);
        assert_eq!(
            live_record(descending).validate(),
            Err(ChainError::LevelsNotAscending(2))
        );

        // Self-reference is a cycle: rejected.
        let mut cyclic = sample_chain();
        cyclic.layers[2].nodes[0].support[0].premises = vec![3];
        assert_eq!(
            cyclic.validate(),
            Err(ChainError::NonAscendingPremise {
                node: 3,
                premise: 3
            })
        );

        // A premise skipping down past the frame layer dangles: rejected.
        let mut upward = sample_chain();
        upward.layers[1].nodes[0].support[0].premises = vec![4];
        assert_eq!(
            upward.validate(),
            Err(ChainError::NonAscendingPremise {
                node: 2,
                premise: 4
            })
        );

        // Shared ids would let a read loop back on itself: rejected.
        let mut duplicated = sample_chain();
        duplicated.layers[1].nodes[0].id = 1;
        assert_eq!(
            live_record(duplicated).validate(),
            Err(ChainError::DuplicateNodeId(1))
        );
    }
}
