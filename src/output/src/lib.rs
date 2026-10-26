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
}
