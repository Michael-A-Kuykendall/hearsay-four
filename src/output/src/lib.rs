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

use serde::{Deserialize, Serialize};

/// One candidate word or phoneme string with its competition score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    /// Candidate word or phoneme string.
    pub text: String,
    /// Confidence/competition score, expected in [0,1] (INV `score_in_range`).
    pub score: f32,
    /// Survival status of this hypothesis.
    pub status: HypothesisStatus,
}

/// Survival status of a hypothesis (schema section 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HypothesisStatus {
    Committed,
    Retracted,
    Survived,
}

/// The specific evidence that decided winner over loser (schema 3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// Human-readable why.
    pub description: String,
    /// Magnitude of the deciding evidence, expected in [0,1].
    pub strength: f32,
}

/// Evidence source kinds (schema section 3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceKind {
    Lexical,
    Phonological,
    Sequence,
    Acoustic,
    Calibration,
}

/// One derivation layer, strictly ascending frame -> sequence (schema 3.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainLayer {
    pub level: ChainLevel,
    pub nodes: Vec<Node>,
}

/// Layer levels, in strictly ascending derivation order (schema section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: u64,
    /// E.g. "/t/", "SYL:ta", "WORD:table".
    pub label: String,
    /// Justification sets that support this node.
    pub support: Vec<JustificationRef>,
    /// Current survival status.
    pub active: bool,
    /// Stage-3 credibility rating on the Hearsay-II 0–100 scale
    /// (bead hearsay-four-shq): the frame layer carries the acoustic rating,
    /// and every higher node derives from strictly lower premises, so a
    /// node's rating never exceeds its premises (INV `rating_in_range` plus
    /// monotone non-increase along the chain, enforced by
    /// [`ProvenanceChain::validate`]).
    pub rating: f64,
}

/// Reference to an arena justification set (survival semantics, aem.13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JustificationRef {
    pub justification_id: String,
    /// Premise node ids (strictly lower layers only, INV `chain_acyclic`).
    pub premises: Vec<u64>,
    /// Unconditional vs defeasible justification.
    pub unconditional: bool,
}

/// The derivation, layer by layer, down to acoustic evidence (schema 3.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceChain {
    /// Acoustic -> phone -> syllable -> lexical -> sequence.
    pub layers: Vec<ChainLayer>,
}

/// The per-word root record (schema section 3.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Stage-3 winner rating on the Hearsay-II 0–100 scale, propagated
    /// verbatim from the stage-3 competition that committed this word
    /// (bead hearsay-four-shq; INV `rating_in_range`).
    pub source_rating: f64,
    /// Per-record summary of the evidence channels behind the commit
    /// (bead hearsay-four-shq; e.g. channel count, combined log-odds, and
    /// rating — never empty on a committed record).
    pub evidence_summary: String,
}

/// Session metadata for a [`Transcript`] (schema section 3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub speaker_id: String,
    pub recording_ref: String,
    pub started_ms: u64,
}

/// Transcript output: words plus session context (schema section 3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    ///
    /// The stage-3 propagation contract (bead hearsay-four-shq) additionally
    /// requires the winner's 0–100 rating verbatim (`source_rating`) plus a
    /// per-record evidence summary (`evidence_summary`): this constructor
    /// takes both, so a committed record always carries its propagated
    /// rating. No scheduling layer is involved (alpha-indexed-dispatch):
    /// the rating is moved here unchanged.
    // Nine args mirror the nine WordRecord fields one-to-one; a parameter
    // object would just re-invent the struct, so the lint is allowed here.
    #[allow(clippy::too_many_arguments)]
    pub fn committed(
        text: impl Into<String>,
        span_ms: (u64, u64),
        confidence: f32,
        winner: Hypothesis,
        losing: Vec<Hypothesis>,
        decided_by: Evidence,
        provenance: ProvenanceChain,
        source_rating: f64,
        evidence_summary: impl Into<String>,
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
            source_rating,
            evidence_summary: evidence_summary.into(),
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
    /// A chain node's rating is NaN, infinite, or outside [0,100]
    /// (INV `rating_in_range`, bead hearsay-four-shq).
    NodeRatingOutOfRange { node: u64 },
    /// A node's rating exceeds one of its premises' ratings: confidence
    /// must not increase along the derivation (monotone non-increase,
    /// bead hearsay-four-shq).
    RatingIncreasesAlongChain { node: u64, premise: u64 },
    /// The propagated stage-3 source rating is NaN, infinite, or outside
    /// [0,100] (INV `rating_in_range`, bead hearsay-four-shq).
    SourceRatingOutOfRange,
    /// The per-record evidence summary is missing (bead hearsay-four-shq:
    /// every committed record carries source rating + evidence summary).
    EmptyEvidenceSummary,
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

/// Tolerance for float noise when comparing propagated ratings: ratings
/// derived from the same log-odds arithmetic are bitwise equal, but
/// independently combined paths may differ by a rounding step.
const RATING_MONOTONE_EPS: f64 = 1e-9;

/// Check whether a 0–100 stage-3 rating value is usable (finite, in range).
fn rating_value_in_range(rating: f64) -> bool {
    rating.is_finite() && (0.0..=100.0).contains(&rating)
}

impl ProvenanceChain {
    /// Check INV `provenance_terminates` + INV `chain_acyclic`: layers start
    /// at frame-level acoustic evidence, strictly ascend, every premise
    /// resolves to a strictly lower layer, and every node reaches a frame
    /// node by following premises.
    ///
    /// Plus the stage-3 propagation contract (bead hearsay-four-shq):
    /// INV `rating_in_range` (every node rating finite and in [0,100]) and
    /// monotone non-increase along the chain (no node rating exceeds any of
    /// its premises' ratings).
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
        // Stage-3 propagation invariants (bead hearsay-four-shq): every
        // node rating is finite and in [0,100] ...
        let mut rating_of: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
        for layer in &self.layers {
            for node in &layer.nodes {
                if !rating_value_in_range(node.rating) {
                    return Err(ChainError::NodeRatingOutOfRange { node: node.id });
                }
                rating_of.insert(node.id, node.rating);
            }
        }
        // ... and ratings never increase along the derivation: a node is
        // only as credible as its weakest premise.
        for layer in self.layers.iter().skip(1) {
            for node in &layer.nodes {
                for justification in &node.support {
                    for &premise in &justification.premises {
                        let premise_rating = rating_of.get(&premise).copied().unwrap_or(0.0);
                        if node.rating > premise_rating + RATING_MONOTONE_EPS {
                            return Err(ChainError::RatingIncreasesAlongChain {
                                node: node.id,
                                premise,
                            });
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
    /// `records_loser_and_evidence`, `score_in_range`, `chain_acyclic`,
    /// plus the stage-3 propagation ratings `rating_in_range` and monotone
    /// non-increase) runs before the record is returned.
    // Nine args mirror the nine WordRecord fields one-to-one; a parameter
    // object would just re-invent the struct, so the lint is allowed here.
    #[allow(clippy::too_many_arguments)]
    pub fn committed_from_live(
        text: impl Into<String>,
        span_ms: (u64, u64),
        confidence: f32,
        winner: Hypothesis,
        losing: Vec<Hypothesis>,
        decided_by: Evidence,
        provenance: ProvenanceChain,
        source_rating: f64,
        evidence_summary: impl Into<String>,
    ) -> Result<Self, ChainError> {
        let record = Self::committed(
            text,
            span_ms,
            confidence,
            winner,
            losing,
            decided_by,
            provenance,
            source_rating,
            evidence_summary,
        );
        record.validate()?;
        Ok(record)
    }

    /// Check INVs `records_loser_and_evidence`, `score_in_range`,
    /// `rating_in_range` (propagated source rating in [0,100], non-empty
    /// evidence summary), plus the full chain contract
    /// (`provenance_terminates`, `chain_acyclic`, rating range + monotone
    /// non-increase along the chain).
    pub fn validate(&self) -> Result<(), ChainError> {
        if self.losing.is_empty() {
            return Err(ChainError::NoLosingHypothesis);
        }
        if self.decided_by.description.trim().is_empty() {
            return Err(ChainError::EmptyDecidingEvidence);
        }
        if !rating_value_in_range(self.source_rating) {
            return Err(ChainError::SourceRatingOutOfRange);
        }
        if self.evidence_summary.trim().is_empty() {
            return Err(ChainError::EmptyEvidenceSummary);
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

impl WordRecord {
    /// Serialize this record to JSON for the human reviewer (nak.2, schema
    /// section 5). `serde_json` would silently emit NaN/inf as `null`, so
    /// the contract (INV `score_in_range`) is enforced first: out-of-range
    /// scores surface as an `Err` here instead of corrupt JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        use serde::ser::Error as _;
        self.validate()
            .map_err(|failure| serde_json::Error::custom(format!("{failure:?}")))?;
        serde_json::to_string_pretty(self)
    }

    /// Parse a record back from [`WordRecord::to_json`] output. Callers
    /// re-check the chain contract with [`WordRecord::validate`].
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// One-screen human-reviewer rendering (nak.2, schema section 5):
    /// answers "why this word and not that one?" by showing `winner`,
    /// `losing`, and `decided_by` together, with the `provenance` chain
    /// listed from the acoustic (frame) layer up for expansion.
    pub fn render(&self) -> String {
        fn level_name(level: ChainLevel) -> &'static str {
            match level {
                ChainLevel::Frame => "frame",
                ChainLevel::Phoneme => "phoneme",
                ChainLevel::Syllable => "syllable",
                ChainLevel::Lexical => "lexical",
                ChainLevel::Sequence => "sequence",
            }
        }
        fn status_name(status: HypothesisStatus) -> &'static str {
            match status {
                HypothesisStatus::Committed => "committed",
                HypothesisStatus::Retracted => "retracted",
                HypothesisStatus::Survived => "survived",
            }
        }
        fn kind_name(kind: EvidenceKind) -> &'static str {
            match kind {
                EvidenceKind::Lexical => "lexical",
                EvidenceKind::Phonological => "phonological",
                EvidenceKind::Sequence => "sequence",
                EvidenceKind::Acoustic => "acoustic",
                EvidenceKind::Calibration => "calibration",
            }
        }
        let mut out = format!(
            "word {:?} [{}ms-{}ms] confidence {:.2} source-rating {:.2}\n",
            self.text, self.start_ms, self.end_ms, self.confidence, self.source_rating
        );
        out.push_str(&format!("EVIDENCE SUMMARY: {}\n", self.evidence_summary));
        out.push_str(&format!(
            "WINNER: {:?} (score {:.2}, {})\n",
            self.winner.text,
            self.winner.score,
            status_name(self.winner.status)
        ));
        out.push_str("LOSING:\n");
        for hypothesis in &self.losing {
            out.push_str(&format!(
                "  - {:?} (score {:.2}, {})\n",
                hypothesis.text,
                hypothesis.score,
                status_name(hypothesis.status)
            ));
        }
        out.push_str(&format!(
            "DECIDED BY [{} strength {:.2}]: {}\n",
            kind_name(self.decided_by.kind),
            self.decided_by.strength,
            self.decided_by.description
        ));
        out.push_str("PROVENANCE (acoustic layer first, expand downward):\n");
        for layer in &self.provenance.layers {
            out.push_str(&format!("  [{}]\n", level_name(layer.level)));
            for node in &layer.nodes {
                out.push_str(&format!(
                    "    id={} {:?} active={}\n",
                    node.id, node.label, node.active
                ));
                for support in &node.support {
                    out.push_str(&format!(
                        "      <- {} premises={:?} unconditional={}\n",
                        support.justification_id, support.premises, support.unconditional
                    ));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Word-level WER scorer (bead hearsay-four-chd, lane A2).
//
// Canonical home of the classical Levenshtein core moved out of `runner`
// (`runner::edit_distance` is now a thin wrapper so `run_full_pipeline`
// behavior is unchanged), plus [`wer`]: S+D+I over normalized word
// sequences divided by reference length, with per-utterance alignment.
//
// Normalization mirrors the A1 ground-truth extractor
// (`corpus::ground_truth::normalize_word`): Unicode lowercase, strip
// surrounding (leading/trailing) punctuation, drop tokens that normalize
// to empty. Pure, model-free, offline: counts only, no weights, no learning.
// ---------------------------------------------------------------------------

/// Classical Levenshtein distance over token strings (template matching —
/// counts only, no weights, no learning: a lookup-table alignment, NOT a
/// model).
///
/// Canonical core moved out of `runner` (bead hearsay-four-chd).
pub fn edit_distance(a: &[String], b: &[String]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let sub = usize::from(x != y);
            cur[j + 1] = (prev[j] + sub).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Normalize one raw whitespace-separated token: lowercase, strip
/// surrounding punctuation, keep internal characters (hyphens,
/// apostrophes) intact. Returns `None` when nothing remains. Mirrors the
/// A1 extractor so scored hypotheses compare like-for-like with
/// ground-truth word lists.
fn normalize_word(raw: &str) -> Option<String> {
    let lowered = raw.to_lowercase();
    let stripped = lowered.trim_matches(|c: char| !c.is_alphanumeric());
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

/// Normalize a word sequence: lowercase/strip each token, drop empties.
fn normalize_words(words: &[String]) -> Vec<String> {
    words.iter().filter_map(|w| normalize_word(w)).collect()
}

/// One word-level alignment operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignOp {
    /// Reference and hypothesis words match.
    Correct,
    /// Reference word replaced by a different hypothesis word.
    Substitution,
    /// Reference word with no hypothesis counterpart.
    Deletion,
    /// Hypothesis word with no reference counterpart.
    Insertion,
}

/// One aligned word pair in a [`WerScore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WerAlignment {
    /// The operation at this position.
    pub op: AlignOp,
    /// Reference word (`None` for insertions).
    pub reference: Option<String>,
    /// Hypothesis word (`None` for deletions).
    pub hypothesis: Option<String>,
}

/// Word error rate score with per-utterance alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct WerScore {
    /// (S+D+I) / reference length (0.0 for two empty sequences; the
    /// hypothesis length as `f64` when the reference is empty but the
    /// hypothesis is not).
    pub wer: f64,
    /// Substitution count.
    pub substitutions: usize,
    /// Deletion count.
    pub deletions: usize,
    /// Insertion count.
    pub insertions: usize,
    /// Normalized reference length (the divisor when nonzero).
    pub reference_len: usize,
    /// Per-utterance alignment, in reference order.
    pub alignment: Vec<WerAlignment>,
}

/// Score hypothesis against reference: S+D+I over normalized word
/// sequences divided by reference length, plus per-utterance alignment.
///
/// Both sequences are normalized first (lowercase, strip surrounding
/// punctuation, drop empties). The alignment backtrace prefers diagonal
/// (correct/substitution) over deletion over insertion, so the op
/// sequence is deterministic for a given input pair.
pub fn wer(reference: &[String], hypothesis: &[String]) -> WerScore {
    let r = normalize_words(reference);
    let h = normalize_words(hypothesis);
    let (m, n) = (r.len(), h.len());
    if m == 0 {
        let alignment: Vec<WerAlignment> = h
            .iter()
            .cloned()
            .map(|word| WerAlignment {
                op: AlignOp::Insertion,
                reference: None,
                hypothesis: Some(word),
            })
            .collect();
        let insertions = alignment.len();
        return WerScore {
            wer: if insertions == 0 {
                0.0
            } else {
                insertions as f64
            },
            substitutions: 0,
            deletions: 0,
            insertions,
            reference_len: 0,
            alignment,
        };
    }
    // dp[i][j]: min edits converting r[..i] into h[..j].
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(n + 1) {
        *cell = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let sub = usize::from(r[i - 1] != h[j - 1]);
            dp[i][j] = (dp[i - 1][j - 1] + sub)
                .min(dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1);
        }
    }
    // Backtrace from (m, n): diagonal first, then deletion, then insertion.
    let (mut i, mut j) = (m, n);
    let mut alignment = Vec::with_capacity(m + n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && r[i - 1] == h[j - 1] && dp[i][j] == dp[i - 1][j - 1] {
            alignment.push(WerAlignment {
                op: AlignOp::Correct,
                reference: Some(r[i - 1].clone()),
                hypothesis: Some(h[j - 1].clone()),
            });
            i -= 1;
            j -= 1;
        } else if i > 0 && j > 0 && dp[i][j] == dp[i - 1][j - 1] + 1 {
            alignment.push(WerAlignment {
                op: AlignOp::Substitution,
                reference: Some(r[i - 1].clone()),
                hypothesis: Some(h[j - 1].clone()),
            });
            i -= 1;
            j -= 1;
        } else if i > 0 && dp[i][j] == dp[i - 1][j] + 1 {
            alignment.push(WerAlignment {
                op: AlignOp::Deletion,
                reference: Some(r[i - 1].clone()),
                hypothesis: None,
            });
            i -= 1;
        } else {
            alignment.push(WerAlignment {
                op: AlignOp::Insertion,
                reference: None,
                hypothesis: Some(h[j - 1].clone()),
            });
            j -= 1;
        }
    }
    alignment.reverse();
    let mut substitutions = 0;
    let mut deletions = 0;
    let mut insertions = 0;
    for step in &alignment {
        match step.op {
            AlignOp::Correct => {}
            AlignOp::Substitution => substitutions += 1,
            AlignOp::Deletion => deletions += 1,
            AlignOp::Insertion => insertions += 1,
        }
    }
    WerScore {
        wer: (substitutions + deletions + insertions) as f64 / m as f64,
        substitutions,
        deletions,
        insertions,
        reference_len: m,
        alignment,
    }
}

/// Score a committed [`Transcript`] against a reference word list: the
/// hypothesis is the transcript's word texts in order. Scoring is
/// read-only; records still validate afterwards (see `wer_tests`).
pub fn transcript_wer(reference: &[String], transcript: &Transcript) -> WerScore {
    let hypothesis: Vec<String> = transcript.words.iter().map(|w| w.text.clone()).collect();
    wer(reference, &hypothesis)
}

#[cfg(test)]
mod wer_tests;

#[cfg(test)]
mod ppt_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Ratings descend frame -> sequence (90/85/80/75/70): in range and
    /// monotone non-increasing along the chain (bead hearsay-four-shq).
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
                    rating: 80.0,
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
                    rating: 75.0,
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
                    rating: 70.0,
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
            85.0,
            "2 channels; log-odds 1.7346; rating 85.00",
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
            85.0,
            "2 channels; log-odds 1.7346; rating 85.00",
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
                85.0,
                "2 channels; log-odds 1.7346; rating 85.00",
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

    #[test]
    fn render_json_serializes_word_record() {
        // (1) A WordRecord serializes to JSON ...
        let record = live_record(sample_chain());
        let json = record.to_json().expect("valid record serializes");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("to_json emits valid JSON");
        // (2) ... with winner + losing + decided_by present, so a reviewer
        // can ask "why this word and not that one?".
        assert_eq!(value["text"], "table");
        assert_eq!(value["winner"]["text"], "table");
        assert_eq!(value["losing"][0]["text"], "cable");
        let decided = value["decided_by"]["description"]
            .as_str()
            .expect("decided_by has a description");
        assert!(!decided.is_empty());
        assert_eq!(value["decided_by"]["kind"], "acoustic");
    }

    #[test]
    fn render_shows_winner_loser_and_decided_by() {
        // The one-screen rendering puts winner, losing, and decided_by
        // together, with the chain expandable to the acoustic layer.
        let record = live_record(sample_chain());
        let screen = record.render();
        assert!(screen.contains("WINNER:"), "winner shown");
        assert!(screen.contains("\"table\""), "winner word shown");
        assert!(screen.contains("LOSING:"), "losing section shown");
        assert!(screen.contains("\"cable\""), "losing word shown");
        assert!(screen.contains("DECIDED BY"), "deciding evidence shown");
        assert!(
            screen.contains("onset burst energy favors /t/ over /k/"),
            "deciding reason shown"
        );
        assert!(screen.contains("[frame]"), "acoustic layer expandable");
        assert!(screen.contains("FRAME:0-20ms"), "acoustic node shown");
    }

    #[test]
    fn render_scores_are_finite_and_in_range() {
        // (3) INV `score_in_range`: no NaN/inf in scores.
        let record = live_record(sample_chain());
        assert!(record.validate().is_ok());
        assert!(record.to_json().is_ok());
        let mut bad = live_record(sample_chain());
        bad.confidence = f32::NAN;
        assert_eq!(
            bad.validate(),
            Err(ChainError::ScoreOutOfRange("confidence"))
        );
        assert!(bad.to_json().is_err(), "NaN must not serialize");
        let mut inf = live_record(sample_chain());
        inf.winner.score = f32::INFINITY;
        assert_eq!(
            inf.validate(),
            Err(ChainError::ScoreOutOfRange("winner.score"))
        );
        assert!(inf.to_json().is_err(), "inf must not serialize");
    }

    #[test]
    fn render_round_trips_through_schema() {
        // (4) The JSON rendering round-trips through the schema unchanged.
        let record = live_record(sample_chain());
        let json = record.to_json().expect("valid record serializes");
        let back = WordRecord::from_json(&json).expect("round-trip parses");
        assert_eq!(back, record);
        assert!(back.validate().is_ok());
    }

    #[test]
    fn propagated_node_ratings_validate_range_and_monotone() {
        // Bead hearsay-four-shq: node ratings in [0,100], monotone
        // non-increasing along the chain.
        assert!(sample_chain().validate().is_ok());

        // Out-of-range node rating fails red.
        let mut high = sample_chain();
        high.layers[1].nodes[0].rating = 101.0;
        assert_eq!(
            high.validate(),
            Err(ChainError::NodeRatingOutOfRange { node: 2 })
        );
        let mut low = sample_chain();
        low.layers[3].nodes[0].rating = f64::NEG_INFINITY;
        assert_eq!(
            low.validate(),
            Err(ChainError::NodeRatingOutOfRange { node: 4 })
        );

        // A rating that rises along the derivation fails red, naming the
        // edge (node 4 at 75.0 sits above premise 3 at 80.0: raising it to
        // 85.0 breaks monotone non-increase).
        let mut rising = sample_chain();
        rising.layers[3].nodes[0].rating = 85.0;
        assert_eq!(
            rising.validate(),
            Err(ChainError::RatingIncreasesAlongChain {
                node: 4,
                premise: 3
            })
        );

        // Equal ratings still validate (non-increase permits equality).
        let mut flat = sample_chain();
        for layer in &mut flat.layers {
            for node in &mut layer.nodes {
                node.rating = 70.0;
            }
        }
        assert!(flat.validate().is_ok());
    }

    #[test]
    fn propagated_source_rating_and_summary_required() {
        // The propagated stage-3 rating lands verbatim and validates.
        let record = live_record(sample_chain());
        assert_eq!(record.source_rating, 85.0);
        assert!(!record.evidence_summary.is_empty());
        assert!(record.validate().is_ok());

        // An out-of-range propagated rating fails red.
        let mut bad_rating = live_record(sample_chain());
        bad_rating.source_rating = 150.0;
        assert_eq!(
            bad_rating.validate(),
            Err(ChainError::SourceRatingOutOfRange)
        );
        let mut nan_rating = live_record(sample_chain());
        nan_rating.source_rating = f64::NAN;
        assert_eq!(
            nan_rating.validate(),
            Err(ChainError::SourceRatingOutOfRange)
        );
        assert!(
            bad_rating.to_json().is_err(),
            "bad rating must not serialize"
        );

        // A missing evidence summary fails red.
        let mut no_summary = live_record(sample_chain());
        no_summary.evidence_summary.clear();
        assert_eq!(no_summary.validate(), Err(ChainError::EmptyEvidenceSummary));
    }

    #[test]
    fn render_shows_propagated_rating_and_summary() {
        let record = live_record(sample_chain());
        let screen = record.render();
        assert!(screen.contains("source-rating 85.00"), "rating shown");
        assert!(screen.contains("EVIDENCE SUMMARY:"), "summary shown");
        assert!(
            screen.contains("2 channels; log-odds 1.7346; rating 85.00"),
            "summary content shown"
        );
    }
}
