//! Fabric decision contract — reference implementation.
//!
//! Implements `docs/design/fabric-decision-contract.md` **spec version 1.0**
//! (bead hearsay-four-dg8 / A4b; spec frozen by hearsay-four-p7u, SHA-256
//! `4b7c4e7b8a603ff6ae33127298fe8687049ecf105dbb99532b6501a80b8f5521`).
//!
//! The contract layer sits *around* the existing [`Justification`] object
//! (spec §2, §13 item 1): the fabric keeps owning activation, survival,
//! deliberate retraction, and `commit()`; this module adds the decision
//! metadata (source class, `llr`, declared null) plus the tiling score
//! `S(T)` (§4), the liveness score `L(h)` (§5), dormancy/revival (§8, §9),
//! and defeat with provenance (§10). Scores are always recomputed from
//! currently active justifications — nothing is cached across a retraction
//! (§2 weakening, §9.4 cascade).

use crate::{Fabric, FactId};
use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

/// The frozen spec version implemented by this module (§14).
pub const SPEC_VERSION: &str = "1.2";

/// Label of the sentence-start pseudo-token `t0` (§4 boundary pair).
pub const SENTENCE_START: &str = "<s>";

/// The six source classes (§3). Five are unary; SEQUENCE is pairwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceClass {
    Acoustic,
    Lexical,
    Sequence,
    Speaker,
    Verify,
    User,
}

impl SourceClass {
    /// All unary classes, in the fixed order used for deterministic sums (§4).
    pub const UNARY: [SourceClass; 5] = [
        SourceClass::Acoustic,
        SourceClass::Lexical,
        SourceClass::Speaker,
        SourceClass::Verify,
        SourceClass::User,
    ];

    pub fn is_unary(self) -> bool {
        self != SourceClass::Sequence
    }

    /// The declared null this class's `llr` is computed against (§3 table).
    /// An `llr` with no named null is not admissible evidence (§3 rule 1).
    pub fn declared_null(self) -> &'static str {
        match self {
            SourceClass::Acoustic => "garbage/SIL model over the same frames",
            SourceClass::Lexical => "lexicon-free phone-unigram model over the same phone sequence",
            SourceClass::Sequence => "unigram word model",
            SourceClass::Speaker => "speaker-independent (SI) model over the same frames",
            SourceClass::Verify => "verifier run on a mismatched (shifted) span",
            SourceClass::User => "no correction present",
        }
    }
}

/// Token kinds per A3 (§2). Only Word and Sil tokens appear in tilings (§7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    Sil,
    Phone,
}

/// A token fact's decision-layer record (§2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub fact: FactId,
    pub label: String,
    pub kind: TokenKind,
    pub start_frame: u32,
    pub end_frame: u32,
}

impl Token {
    pub fn len_frames(&self) -> u32 {
        self.end_frame - self.start_frame
    }
    pub fn is_empty(&self) -> bool {
        self.end_frame <= self.start_frame
    }
    /// Only WORD/SIL tokens may join a tiling (§7).
    pub fn tiling_eligible(&self) -> bool {
        matches!(self.kind, TokenKind::Word | TokenKind::Sil)
    }
}

/// Contract violations rejected at the decision-layer boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    /// Zero-length spans are invalid tokens (§7).
    ZeroLengthSpan,
    UnknownToken(FactId),
    /// SEQUENCE is pairwise and cannot be attached as unary evidence (§3).
    SequenceIsPairwise,
    /// USER evidence is the fixed `+U0` only (§3 rule 4); use `add_user`.
    UserNeedsFixedValue,
}

type DecisionResult<T> = Result<T, DecisionError>;

/// Experiment arm (§11): what happens to a token below `theta_live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arm {
    /// Conventional beam prune: a token below `theta_live` is retracted
    /// at once and never receives later evidence. Budgets count live
    /// tokens only.
    B,
    /// Hearsay-IV: a token below `theta_live` goes dormant and stays
    /// revivable by later evidence (§8, §9). Budgets count live +
    /// dormant against the same total.
    #[default]
    C,
}

/// Frozen parameters (§12). Values are DEV-tuned defaults shipped by A4b;
/// changing one after freeze is a spec amendment, never a silent edit (§14).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionParams {
    pub w_acoustic: f64,
    pub w_lexical: f64,
    pub w_sequence: f64,
    pub w_speaker: f64,
    pub w_verify: f64,
    pub w_user: f64,
    /// Fixed USER `llr` `+U0` (§3 rule 4).
    pub u0: f64,
    /// Live/dormant threshold on `L(h)` (§8).
    pub theta_live: f64,
    pub live_budget: usize,
    pub dormant_budget: usize,
    /// Arm-C combined live+dormant total (§8, §11); `None` disables it.
    pub combined_budget: Option<usize>,
    /// Commit frontier lag `H` (frames) (§10).
    pub h: u32,
    /// Unconditional commit lag `H_max >= H` (frames) (§10).
    pub h_max: u32,
    /// Commit margin `delta >= 0` (§10).
    pub delta: f64,
    /// Dormant successor window `J` (frames), pre-registered default 3 (§9).
    pub j: u32,
    /// Experiment arm (§11): below-threshold fate (retract vs dormant).
    pub arm: Arm,
}

impl Default for DecisionParams {
    fn default() -> Self {
        DecisionParams {
            w_acoustic: 1.0,
            w_lexical: 1.0,
            w_sequence: 1.0,
            w_speaker: 1.0,
            w_verify: 1.0,
            w_user: 1.0,
            u0: 100.0,
            theta_live: 0.0,
            live_budget: usize::MAX,
            dormant_budget: usize::MAX,
            combined_budget: None,
            h: 10,
            h_max: 20,
            delta: 0.0,
            j: 3,
            arm: Arm::C,
        }
    }
}

impl DecisionParams {
    pub fn weight(&self, class: SourceClass) -> f64 {
        match class {
            SourceClass::Acoustic => self.w_acoustic,
            SourceClass::Lexical => self.w_lexical,
            SourceClass::Sequence => self.w_sequence,
            SourceClass::Speaker => self.w_speaker,
            SourceClass::Verify => self.w_verify,
            SourceClass::User => self.w_user,
        }
    }
}

/// Unary decision evidence: one justification's spec-level metadata (§2, §3).
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryEvidence {
    pub token: FactId,
    pub class: SourceClass,
    pub llr: f64,
    /// Declared null the `llr` was computed against (§3 rule 1).
    pub null: &'static str,
    /// Premise facts gating activation (weakening, §2): the evidence counts
    /// only while every premise is active in the fabric.
    pub premises: BTreeSet<FactId>,
}

/// Pairwise SEQUENCE evidence on an ordered adjacent pair (§3, §4).
/// `from == None` is the sentence-start boundary pair (`<s>` -> `to`, §4).
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceEvidence {
    pub from: Option<FactId>,
    pub to: FactId,
    pub llr: f64,
    pub null: &'static str,
    pub premises: BTreeSet<FactId>,
}

/// Which budget cap triggered an eviction (R2, docs/design/a4-budget-semantics.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKind {
    Live,
    Dormant,
    Combined,
}

/// Provenance events recorded by the decision layer (§8, §9.4, §10).
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionEvent {
    /// A dormant token crossed `theta_live` via new right/left context (§9.4).
    Revived {
        token: FactId,
        via: FactId,
        llr: f64,
    },
    /// A non-winning token in a committed region was defeated (§10).
    Defeated {
        token: FactId,
        winner: Vec<FactId>,
        /// `None` when the winner was unopposed (§10, amendment 1.2).
        margin: Option<f64>,
    },
    /// A token was evicted under budget exhaustion (§8); budget records
    /// which cap triggered (R2, docs/design/a4-budget-semantics.md).
    Evicted {
        token: FactId,
        liveness: f64,
        budget: BudgetKind,
    },
    /// An arm-B token below `theta_live` was beam-pruned (§11):
    /// deliberately retracted at once, never revivable, never receiving
    /// later evidence. Not a budget eviction.
    Pruned { token: FactId, liveness: f64 },
    /// A region committed (§10).
    Committed {
        region: (u32, u32),
        winner: Vec<FactId>,
        /// `None` when the winner was unopposed (§10, amendment 1.2).
        margin: Option<f64>,
    },
}

/// One unary term of a token's tiling-score breakdown (P2).
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryTerm {
    pub class: SourceClass,
    pub llr: Option<f64>,
    pub weight: f64,
    pub weighted: f64,
}

/// Per-token tiling-score breakdown (P2): `S(T)` components only.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenBreakdown {
    pub terms: Vec<UnaryTerm>,
    pub sequence_llr: Option<f64>,
    pub unary_total: f64,
    pub total: f64,
}

/// A scored tiling (§4, §7).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredTiling {
    pub tokens: Vec<FactId>,
    pub score: f64,
}

impl ScoredTiling {
    /// Deterministic tie key (§10): earlier start, then lexicographic label,
    /// then lower token id — compared token by token over the tiling.
    fn tie_key(&self, tokens: &BTreeMap<FactId, Token>) -> Vec<(u32, String, u64)> {
        self.tokens
            .iter()
            .map(|f| {
                let t = &tokens[f];
                (t.start_frame, t.label.clone(), f.0)
            })
            .collect()
    }
}

/// Outcome of a commit attempt (§10).
#[derive(Debug, Clone, PartialEq)]
pub enum CommitOutcome {
    Committed {
        winner: ScoredTiling,
        runner_up: Option<ScoredTiling>,
        /// `None` when the winner was unopposed (§10, amendment 1.2).
        margin: Option<f64>,
        defeated: Vec<FactId>,
    },
    /// Frontier has not passed `e + H` yet.
    NotEligible,
    /// Margin below `delta` and frontier has not reached `e + H_max`.
    MarginTooSmall {
        margin: f64,
    },
    NoTiling,
}

/// Operation counters for the decision layer (PERF nlvs proof data).
/// `Cell` because the hot query paths (`liveness`, pair lookups) take
/// `&self`; single-threaded use only.
#[derive(Debug, Default)]
struct FabricStats {
    liveness_calls: Cell<u64>,
    unary_lookups: Cell<u64>,
    seq_pair_lookups: Cell<u64>,
    heap_pushes: Cell<u64>,
    heap_pops: Cell<u64>,
    evicted: Cell<u64>,
    pruned: Cell<u64>,
    defeated: Cell<u64>,
    committed: Cell<u64>,
}

impl FabricStats {
    fn bump(which: &Cell<u64>) {
        which.set(which.get() + 1);
    }
}

/// Plain-data snapshot of a [`DecisionFabric`]'s operation counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FabricStatsSnapshot {
    pub liveness_calls: u64,
    pub unary_lookups: u64,
    pub seq_pair_lookups: u64,
    pub heap_pushes: u64,
    pub heap_pops: u64,
    pub evicted: u64,
    pub pruned: u64,
    pub defeated: u64,
    pub committed: u64,
}

/// A premise-index entry: evidence gated on one premise fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceRef {
    Unary { idx: usize },
    Seq { idx: usize },
}

/// Budget-heap entry: min-first by (`liveness`, §10 tie rank), with a
/// version tag for lazy deletion. Total order via `total_cmp` (NaN sorts
/// last, deterministically) plus the unique tie rank.
#[derive(Debug, Clone, Copy)]
struct HeapEntry {
    liveness: f64,
    rank: usize,
    version: u64,
    token: FactId,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.liveness
            .total_cmp(&other.liveness)
            .then_with(|| self.rank.cmp(&other.rank))
    }
}

/// The decision contract layer over a [`Fabric`] (spec version 1.0).
/// Stability classification of one evidence justification (saturation
/// port, bead nlvs rewrite §2). Unconditional justifications carry no
/// premises and are never traversed by propagation; defeasible ones gate
/// on premises and deactivate when any premise retracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stability {
    Unconditional,
    Defeasible,
}

/// Interning key of one evidence atom: the (token, class, llr) identity
/// without premises. The same atom re-asserted with different premises
/// shares the key; each assert appends a justification (row) to the key's
/// list, never rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EvidenceKey {
    Unary {
        token: FactId,
        class: SourceClass,
        llr_bits: u64,
    },
    Seq {
        from: Option<FactId>,
        to: FactId,
        llr_bits: u64,
    },
}

/// Identity of one evidence row (one justification) on the propagation
/// frontier. Row indexes are append-only, hence stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PendingRow {
    Unary { idx: usize },
    Seq { idx: usize },
}

/// Memoized scoring components. Every entry is a pure function of the
/// evidence rows plus premise truth: eager propagation refreshes entries
/// on change and compute-on-miss covers the rest, so a cached value always
/// equals a fresh fold. `RefCell` because the hot queries take `&self`
/// (same single-threaded contract as [`FabricStats`]).
#[derive(Debug, Default)]
struct LivenessCache {
    unary_max: HashMap<(FactId, SourceClass), Option<f64>>,
    seq_pair_max: HashMap<(Option<FactId>, FactId), Option<f64>>,
    seq_to_max: HashMap<FactId, Option<f64>>,
    seq_from_max: HashMap<FactId, Option<f64>>,
    liveness: HashMap<FactId, f64>,
    /// Derived live/dormant pools over `open` (saturation port §4):
    /// refreshed incrementally on every liveness recompute, rebuilt from
    /// scratch when external mutation dirties them. Iterating a pool in
    /// id order reproduces the exact `live_tokens`/`dormant_tokens` scan.
    live_set: BTreeSet<FactId>,
    dormant_set: BTreeSet<FactId>,
    pools_dirty: bool,
}

impl LivenessCache {
    fn clear(&mut self) {
        self.unary_max.clear();
        self.seq_pair_max.clear();
        self.seq_to_max.clear();
        self.seq_from_max.clear();
        self.liveness.clear();
        self.live_set.clear();
        self.dormant_set.clear();
        self.pools_dirty = true;
    }
}

pub struct DecisionFabric {
    fabric: Fabric,
    params: DecisionParams,
    tokens: BTreeMap<FactId, Token>,
    unary: Vec<UnaryEvidence>,
    sequence: Vec<SequenceEvidence>,
    /// Dormant-predecessor index (§9.2): end frame -> token facts.
    by_end: BTreeMap<u32, BTreeSet<FactId>>,
    /// Successor index (§9.1): start frame -> token facts. Beside `by_end`;
    /// range queries replace full token scans (PERF nlvs).
    by_start: BTreeMap<u32, BTreeSet<FactId>>,
    /// Unary index: (token, class) -> positions in `unary`, in push order.
    unary_idx: HashMap<(FactId, SourceClass), Vec<usize>>,
    /// Sequence pair index: (from, to) -> positions in `sequence`, in push
    /// order. `from == None` is the `<s>` boundary pair (§4).
    seq_pair_idx: HashMap<(Option<FactId>, FactId), Vec<usize>>,
    /// Sequence index by `to` endpoint (boundary pairs included).
    seq_by_to: HashMap<FactId, Vec<usize>>,
    /// Sequence index by `from` endpoint (boundary pairs never appear here).
    seq_by_from: HashMap<FactId, Vec<usize>>,
    /// Premise index: premise fact -> evidence mentioning it. Powers both
    /// `dependents_of` (§9.4) and the budget-loop neighbor recompute.
    premise_refs: HashMap<FactId, Vec<EvidenceRef>>,
    /// Active-token working set, uncommitted regions only: every active
    /// token outside a committed region is a member. Committed-region
    /// members leave at commit time, so the set stays bounded by the
    /// commit window (item 8) instead of growing with winners. Pool
    /// QUERIES (`live_tokens`, `dormant_tokens`, counts) iterate this set.
    open: BTreeSet<FactId>,
    /// Every active token, committed regions included. Pool MUTATIONS
    /// (`prune_below_threshold`, `enforce_budgets`) iterate this set, so
    /// prune/evict outcomes stay bit-identical to the pre-index code.
    active_all: BTreeSet<FactId>,
    /// Tokens whose span lies in a committed region (O(1) region test).
    committed_tokens: BTreeSet<FactId>,
    committed_regions: Vec<(u32, u32)>,
    log: Vec<DecisionEvent>,
    stats: FabricStats,
    peak_open: usize,
    /// Evidence interning (saturation port §1): atom key -> row indexes,
    /// the atom's justification list in assert order.
    intern: HashMap<EvidenceKey, Vec<usize>>,
    /// Propagation frontier (saturation port §4): rows whose support may
    /// have changed, drained eagerly at the end of every mutation.
    pending: VecDeque<PendingRow>,
    /// Frontier membership: each row is enqueued at most once per drain.
    pending_set: HashSet<PendingRow>,
    /// Memoized scoring components (saturation port §4).
    cache: RefCell<LivenessCache>,
    /// External-mutation flag: set by `fabric_mut` (the only path that
    /// bypasses propagation); queries clear the caches when it is set.
    external_dirty: Cell<bool>,
}

impl DecisionFabric {
            pub fn new(params: DecisionParams) -> Self {
        DecisionFabric {
            fabric: Fabric::new(),
            params,
            tokens: BTreeMap::new(),
            unary: Vec::new(),
            sequence: Vec::new(),
            by_end: BTreeMap::new(),
            by_start: BTreeMap::new(),
            unary_idx: HashMap::new(),
            seq_pair_idx: HashMap::new(),
            seq_by_to: HashMap::new(),
            seq_by_from: HashMap::new(),
            premise_refs: HashMap::new(),
            open: BTreeSet::new(),
            active_all: BTreeSet::new(),
            committed_tokens: BTreeSet::new(),
            committed_regions: Vec::new(),
            log: Vec::new(),
            stats: FabricStats::default(),
            peak_open: 0,
            intern: HashMap::new(),
            pending: VecDeque::new(),
            pending_set: HashSet::new(),
            cache: RefCell::new(LivenessCache::default()),
            external_dirty: Cell::new(false),
        }
    }

    pub fn params(&self) -> &DecisionParams {
        &self.params
    }
    pub fn fabric(&self) -> &Fabric {
        &self.fabric
    }
    pub fn fabric_mut(&mut self) -> &mut Fabric {
        // External-mutation flag: the caller may retract/assert behind
        // propagation's back, so the next query re-syncs (clears caches;
        // membership re-syncs in `sync_open` on the `&mut` paths).
        self.external_dirty.set(true);
        &mut self.fabric
    }
    pub fn token(&self, fact: FactId) -> Option<&Token> {
        self.tokens.get(&fact)
    }
    pub fn events(&self) -> &[DecisionEvent] {
        &self.log
    }

    /// Register a token fact (§2). Zero-length spans are rejected (§7).
            pub fn register_token(
        &mut self,
        fact: FactId,
        label: impl Into<String>,
        kind: TokenKind,
        start_frame: u32,
        end_frame: u32,
    ) -> DecisionResult<FactId> {
        if end_frame <= start_frame {
            return Err(DecisionError::ZeroLengthSpan);
        }
        let token = Token {
            fact,
            label: label.into(),
            kind,
            start_frame,
            end_frame,
        };
        self.by_end.entry(end_frame).or_default().insert(fact);
        self.by_start.entry(start_frame).or_default().insert(fact);
        self.tokens.insert(fact, token);
        let in_region = self
            .committed_regions
            .iter()
            .any(|(s, e)| start_frame >= *s && end_frame <= *e);
        if in_region {
            self.committed_tokens.insert(fact);
        }
        if self.fabric.is_active(fact) {
            self.active_all.insert(fact);
            if !in_region {
                self.open.insert(fact);
                self.track_open();
            } else {
                self.open.remove(&fact);
            }
        } else {
            self.open.remove(&fact);
            self.active_all.remove(&fact);
        }
        // Sync the derived pools to the new membership (idempotent; also
        // refreshes a re-registered token's liveness).
        self.refresh_liveness(fact);
        Ok(fact)
    }

    /// Mint a fresh token fact in the fabric, assert it as an assumption
    /// (tokens enter active; evidence and defeat move them from there),
    /// and register it.
    pub fn mint_token(
        &mut self,
        label: impl Into<String>,
        kind: TokenKind,
        start_frame: u32,
        end_frame: u32,
    ) -> DecisionResult<FactId> {
        let fact = self.fabric.intern();
        self.fabric.assert_assumption(fact);
        self.register_token(fact, label, kind, start_frame, end_frame)
    }

    fn known(&self, fact: FactId) -> DecisionResult<()> {
        if self.tokens.contains_key(&fact) {
            Ok(())
        } else {
            Err(DecisionError::UnknownToken(fact))
        }
    }

    /// Attach unary decision evidence (§3). Absent class = absent term
    /// (rule 2); within a class only the max active `llr` counts (rule 3).
        pub fn add_unary(
        &mut self,
        token: FactId,
        class: SourceClass,
        llr: f64,
        premises: BTreeSet<FactId>,
    ) -> DecisionResult<()> {
        self.known(token)?;
        if class == SourceClass::Sequence {
            return Err(DecisionError::SequenceIsPairwise);
        }
        if class == SourceClass::User {
            return Err(DecisionError::UserNeedsFixedValue);
        }
        // Stability gate: unconditional rows carry no premises and are
        // never entered in the PT index; defeasible rows are.
        let stability = Self::stability_of(class, &premises);
        let idx = self.unary.len();
        if stability == Stability::Defeasible {
            for p in &premises {
                self.premise_refs
                    .entry(*p)
                    .or_default()
                    .push(EvidenceRef::Unary { idx });
            }
        }
        self.unary.push(UnaryEvidence {
            token,
            class,
            llr,
            null: class.declared_null(),
            premises,
        });
        self.unary_idx.entry((token, class)).or_default().push(idx);
        // Intern the atom (this justification is appended to the key's
        // list) and propagate the new row.
        let key = EvidenceKey::Unary {
            token,
            class,
            llr_bits: llr.to_bits(),
        };
        self.intern.entry(key).or_default().push(idx);
        self.enqueue_row(PendingRow::Unary { idx });
        self.drain_pending();
        Ok(())
    }

    /// Attach a USER correction: the fixed `+U0`, positive only (§3 rules
    /// 4-5), gated on the owner's correction fact via `premises`.
        pub fn add_user(&mut self, token: FactId, correction_premise: FactId) -> DecisionResult<()> {
        self.known(token)?;
        let mut premises = BTreeSet::new();
        premises.insert(correction_premise);
        let idx = self.unary.len();
        if Self::stability_of(SourceClass::User, &premises) == Stability::Defeasible {
            self.premise_refs
                .entry(correction_premise)
                .or_default()
                .push(EvidenceRef::Unary { idx });
        }
        let u0 = self.params.u0;
        self.unary.push(UnaryEvidence {
            token,
            class: SourceClass::User,
            llr: u0,
            null: SourceClass::User.declared_null(),
            premises,
        });
        self.unary_idx
            .entry((token, SourceClass::User))
            .or_default()
            .push(idx);
        let key = EvidenceKey::Unary {
            token,
            class: SourceClass::User,
            llr_bits: u0.to_bits(),
        };
        self.intern.entry(key).or_default().push(idx);
        self.enqueue_row(PendingRow::Unary { idx });
        self.drain_pending();
        Ok(())
    }

    /// Attach pairwise SEQUENCE evidence `from -> to` (§3, §4).
        pub fn add_sequence(
        &mut self,
        from: FactId,
        to: FactId,
        llr: f64,
        premises: BTreeSet<FactId>,
    ) -> DecisionResult<()> {
        self.known(from)?;
        self.known(to)?;
        let idx = self.sequence.len();
        // Sequence evidence is always defeasible (class rule).
        debug_assert_eq!(
            Self::stability_of(SourceClass::Sequence, &premises),
            Stability::Defeasible
        );
        for p in &premises {
            self.premise_refs
                .entry(*p)
                .or_default()
                .push(EvidenceRef::Seq { idx });
        }
        self.sequence.push(SequenceEvidence {
            from: Some(from),
            to,
            llr,
            null: SourceClass::Sequence.declared_null(),
            premises,
        });
        self.seq_pair_idx
            .entry((Some(from), to))
            .or_default()
            .push(idx);
        self.seq_by_to.entry(to).or_default().push(idx);
        self.seq_by_from.entry(from).or_default().push(idx);
        let key = EvidenceKey::Seq {
            from: Some(from),
            to,
            llr_bits: llr.to_bits(),
        };
        self.intern.entry(key).or_default().push(idx);
        self.enqueue_row(PendingRow::Seq { idx });
        self.drain_pending();
        Ok(())
    }

    /// Attach the sentence-start boundary pair `<s> -> to` (§4).
        pub fn add_boundary_sequence(&mut self, to: FactId, llr: f64) -> DecisionResult<()> {
        self.known(to)?;
        let idx = self.sequence.len();
        self.sequence.push(SequenceEvidence {
            from: None,
            to,
            llr,
            null: SourceClass::Sequence.declared_null(),
            premises: BTreeSet::new(),
        });
        self.seq_pair_idx.entry((None, to)).or_default().push(idx);
        self.seq_by_to.entry(to).or_default().push(idx);
        let key = EvidenceKey::Seq {
            from: None,
            to,
            llr_bits: llr.to_bits(),
        };
        self.intern.entry(key).or_default().push(idx);
        self.enqueue_row(PendingRow::Seq { idx });
        self.drain_pending();
        Ok(())
    }

    fn premises_active(&self, premises: &BTreeSet<FactId>) -> bool {
        premises.iter().all(|p| self.fabric.is_active(*p))
    }

    /// Strongest active justification in one unary class for a token (§3
    /// rule 3); `None` means the class contributes no term (rule 2).
        pub fn active_unary_max(&self, token: FactId, class: SourceClass) -> Option<f64> {
        FabricStats::bump(&self.stats.unary_lookups);
        self.ensure_synced();
        self.component_unary_max(token, class)
    }

    /// Strongest active SEQUENCE `llr` for an ordered pair; `from == None`
    /// selects the `<s>` boundary pair (§4). Each pair enters `S(T)` once.
        pub fn active_sequence_max(&self, from: Option<FactId>, to: FactId) -> Option<f64> {
        FabricStats::bump(&self.stats.seq_pair_lookups);
        self.ensure_synced();
        self.component_seq_pair(from, to)
    }

    /// The per-token unary sum shared by `S(T)` and `L(h)` (§5 decision 5).
    pub fn unary_sum(&self, token: FactId) -> f64 {
        SourceClass::UNARY
            .iter()
            .map(|c| {
                self.active_unary_max(token, *c)
                    .map_or(0.0, |llr| self.params.weight(*c) * llr)
            })
            .sum()
    }

    /// Liveness `L(h)` (§5): unary sum plus max left/right SEQUENCE terms.
    /// Used only for the live/dormant threshold and budget accounting;
    /// never summed into `S(T)` and never published.
        pub fn liveness(&self, token: FactId) -> f64 {
        FabricStats::bump(&self.stats.liveness_calls);
        self.ensure_synced();
        {
            let cached = self.cache.borrow();
            if let Some(v) = cached.liveness.get(&token) {
                return *v;
            }
        }
        self.refresh_liveness(token)
    }

    /// Live iff `L(h) >= theta_live` (§8) and the token fact is active.
    pub fn is_live(&self, token: FactId) -> bool {
        self.fabric.is_active(token) && self.liveness(token) >= self.params.theta_live
    }

    /// Dormant iff retained and active but below `theta_live` (§8).
    /// Arm B has no dormant set (§11): below-threshold tokens are pruned,
    /// so this is always false there — which also keeps pruned tokens
    /// out of later context attachment.
    pub fn is_dormant(&self, token: FactId) -> bool {
        self.params.arm == Arm::C
            && self.fabric.is_active(token)
            && self.liveness(token) < self.params.theta_live
    }

        pub fn live_tokens(&self) -> Vec<FactId> {
        self.ensure_synced();
        self.sync_pools();
        self.cache.borrow().live_set.iter().copied().collect()
    }

        pub fn dormant_tokens(&self) -> Vec<FactId> {
        self.ensure_synced();
        self.sync_pools();
        self.cache.borrow().dormant_set.iter().copied().collect()
    }

    /// Arm-B beam prune (§11): retract every still-active token below
    /// `theta_live` at once. Deliberate retraction: pruned tokens never
    /// revive, never join tilings, and `attach_context_for_new_token`
    /// never attaches later evidence to them (they are not dormant).
    /// Scoring, evidence, and commit are untouched — identical to arm C.
    /// No-op in arm C (dormancy is computed state there).
    /// Drivers call this via `enforce_budgets`, which beam-prunes first
    /// in arm B, before any commit.
            pub fn prune_below_threshold(&mut self) {
        if self.params.arm != Arm::B {
            return;
        }
        self.sync_open();
        // Mutations iterate every active token (committed regions
        // included), exactly as the pre-index full scan did.
        let victims: Vec<FactId> = self
            .active_all
            .iter()
            .copied()
            .filter(|t| self.fabric.is_active(*t) && self.liveness(*t) < self.params.theta_live)
            .collect();
        for victim in victims {
            let liveness = self.liveness(victim);
            self.retract_internal(victim);
            self.open.remove(&victim);
            self.active_all.remove(&victim);
            FabricStats::bump(&self.stats.pruned);
            self.log.push(DecisionEvent::Pruned {
                token: victim,
                liveness,
            });
        }
    }

        fn in_committed_region(&self, token: FactId) -> bool {
        self.committed_tokens.contains(&token)
    }

    /// Enforce budgets (§8, §11): in arm B, beam-prune below-threshold
    /// tokens first (live tokens only are counted); then evict lowest
    /// `L(h)` first, ties by §10 order. Eviction is a deliberate retraction
    /// recorded in provenance.
            pub fn enforce_budgets(&mut self) {
        // Arm B (§11): below-threshold tokens are retracted at once, so
        // budget accounting sees live tokens only. No-op in arm C.
        self.prune_below_threshold();
        // O(1) no-op proof: live/dormant counts are bounded above by the
        // active-token count, so cleared caps mean no loop below can execute.
        // Mutations iterate every active token (committed regions included),
        // exactly as the pre-index full scans did.
        let live_ok = self.active_all.len() <= self.params.live_budget;
        let dormant_ok =
            self.params.arm == Arm::B || self.active_all.len() <= self.params.dormant_budget;
        let combined_ok = self
            .params
            .combined_budget
            .map_or(true, |total| self.active_all.len() <= total);
        if live_ok && dormant_ok && combined_ok {
            return;
        }
        self.sync_open();
        // Freeze the §10 tie order as ranks (token_tie_cmp is total: the
        // token id breaks all ties).
        let mut order: Vec<FactId> = self.active_all.iter().copied().collect();
        order.sort_by(|a, b| self.token_tie_cmp(*a, *b));
        let rank: HashMap<FactId, usize> =
            order.iter().enumerate().map(|(i, t)| (*t, i)).collect();
        // Current (liveness, version) per open token; evicted tokens leave.
        // One heap per pool: entries never move pools (a transition pushes
        // a fresh entry to the new pool's heap and stales the old one), so
        // a fresh entry is always in its token's current pool.
        let mut state: HashMap<FactId, (f64, u64)> = HashMap::new();
        let mut live_heap: BinaryHeap<Reverse<HeapEntry>> = BinaryHeap::new();
        let mut dormant_heap: BinaryHeap<Reverse<HeapEntry>> = BinaryHeap::new();
        let mut live_count = 0usize;
        let mut dormant_count = 0usize;
        let arm_c = self.params.arm == Arm::C;
        for t in &order {
            let liv = self.liveness(*t);
            state.insert(*t, (liv, 0));
            let entry = Reverse(HeapEntry {
                liveness: liv,
                rank: rank[t],
                version: 0,
                token: *t,
            });
            if liv >= self.params.theta_live {
                live_heap.push(entry);
                FabricStats::bump(&self.stats.heap_pushes);
                live_count += 1;
            } else if arm_c {
                dormant_heap.push(entry);
                FabricStats::bump(&self.stats.heap_pushes);
                dormant_count += 1;
            }
        }
        // R2 (spec docs/design/a4-budget-semantics.md): live cap first,
        // then dormant + combined. Victim order everywhere: lowest L(h),
        // ties by §10 tie order.
        while live_count > self.params.live_budget {
            let Some(victim) = self.pop_fresh(&mut live_heap, &state) else {
                break;
            };
            let liveness = self.liveness(victim);
            self.retract_internal(victim);
            self.open.remove(&victim);
            self.active_all.remove(&victim);
            state.remove(&victim);
            live_count -= 1;
            FabricStats::bump(&self.stats.evicted);
            self.log.push(DecisionEvent::Evicted {
                token: victim,
                liveness,
                budget: BudgetKind::Live,
            });
            self.refresh_neighbors(
                &mut live_heap,
                &mut dormant_heap,
                &mut state,
                &rank,
                &mut live_count,
                &mut dormant_count,
                victim,
            );
        }
        // Arm B (§11 table): the combined total counts live tokens only
        // (there is no dormant set), so over-total victims come from live.
        if self.params.arm == Arm::B {
            if let Some(total) = self.params.combined_budget {
                while live_count > total {
                    let Some(victim) = self.pop_fresh(&mut live_heap, &state) else {
                        break;
                    };
                    let liveness = self.liveness(victim);
                    self.retract_internal(victim);
                    self.open.remove(&victim);
                    self.active_all.remove(&victim);
                    state.remove(&victim);
                    live_count -= 1;
                    FabricStats::bump(&self.stats.evicted);
                    self.log.push(DecisionEvent::Evicted {
                        token: victim,
                        liveness,
                        budget: BudgetKind::Combined,
                    });
                    self.refresh_neighbors(
                        &mut live_heap,
                        &mut dormant_heap,
                        &mut state,
                        &rank,
                        &mut live_count,
                        &mut dormant_count,
                        victim,
                    );
                }
            }
            return;
        }
        loop {
            let over_dormant = dormant_count > self.params.dormant_budget;
            let over_combined = self
                .params
                .combined_budget
                .is_some_and(|total| live_count + dormant_count > total);
            if !over_dormant && !over_combined {
                return;
            }
            let budget = if over_dormant {
                BudgetKind::Dormant
            } else {
                BudgetKind::Combined
            };
            let Some(victim) = self.pop_fresh(&mut dormant_heap, &state) else {
                return;
            };
            let liveness = self.liveness(victim);
            self.retract_internal(victim);
            self.open.remove(&victim);
            self.active_all.remove(&victim);
            state.remove(&victim);
            dormant_count -= 1;
            FabricStats::bump(&self.stats.evicted);
            self.log.push(DecisionEvent::Evicted {
                token: victim,
                liveness,
                budget,
            });
            self.refresh_neighbors(
                &mut live_heap,
                &mut dormant_heap,
                &mut state,
                &rank,
                &mut live_count,
                &mut dormant_count,
                victim,
            );
        }
    }

    /// §10 tie order between two tokens: earlier start, lexicographic
    /// label, lower token id (score is compared by the caller).
    fn token_tie_cmp(&self, a: FactId, b: FactId) -> std::cmp::Ordering {
        let (ta, tb) = (&self.tokens[&a], &self.tokens[&b]);
        ta.start_frame
            .cmp(&tb.start_frame)
            .then_with(|| ta.label.cmp(&tb.label))
            .then_with(|| a.0.cmp(&b.0))
    }

    /// Lattice-mode successors (§9.1): tokens starting within ±J frames of
    /// `end(h)`. They exist whether or not `h` is live.
            pub fn lattice_successors(&self, token: FactId) -> Vec<FactId> {
        let end = self.tokens[&token].end_frame;
        let (lo, hi) = Self::frame_window(end, self.params.j);
        let mut out = Vec::new();
        for facts in self.by_start.range(lo..=hi).map(|(_, facts)| facts) {
            out.extend(facts.iter().copied().filter(|f| *f != token));
        }
        // Order is part of the contract: ascending token id, as the old
        // full-map scan produced.
        out.sort_unstable();
        out
    }

    /// Streaming-mode context attachment (§9.2): a newly created token `s`
    /// (from any live path) supplies right context to dormant predecessors
    /// found through the dormant-predecessor index, and left context to
    /// dormant successors, symmetrically. `lm` computes `llr_SEQ(a -> b)`
    /// (§3). Records a revival event when a dormant token crosses
    /// `theta_live` before its region commits (§9.4).
        pub fn attach_context_for_new_token(
        &mut self,
        new_token: FactId,
        lm: &dyn Fn(&Token, &Token) -> f64,
    ) {
        let s_tok = self.tokens[&new_token].clone();
        // Right context: dormant h with end in [start(s)-J, start(s)+J].
        // Range order matches the old full-index scan (end, then token id).
        let (lo, hi) = Self::frame_window(s_tok.start_frame, self.params.j);
        let mut predecessors: Vec<FactId> = Vec::new();
        for facts in self.by_end.range(lo..=hi).map(|(_, facts)| facts) {
            predecessors.extend(facts.iter().copied());
        }
        for h in predecessors {
            if h == new_token || !self.is_dormant(h) {
                continue;
            }
            let h_tok = self.tokens[&h].clone();
            let llr = lm(&h_tok, &s_tok);
            let mut premises = BTreeSet::new();
            premises.insert(h);
            premises.insert(new_token);
            let _ = self.add_sequence(h, new_token, llr, premises);
            self.maybe_revive(h, new_token, llr);
        }
        // Left context, symmetric: dormant h that `new_token` precedes.
        let (slo, shi) = Self::frame_window(s_tok.end_frame, self.params.j);
        let mut successors: Vec<FactId> = Vec::new();
        for facts in self.by_start.range(slo..=shi).map(|(_, facts)| facts) {
            successors.extend(facts.iter().copied().filter(|f| *f != new_token));
        }
        // Order is part of the contract: ascending token id, as the old
        // full-map scan produced.
        successors.sort_unstable();
        for h in successors {
            if !self.is_dormant(h) {
                continue;
            }
            let h_tok = self.tokens[&h].clone();
            let llr = lm(&s_tok, &h_tok);
            let mut premises = BTreeSet::new();
            premises.insert(new_token);
            premises.insert(h);
            let _ = self.add_sequence(new_token, h, llr, premises);
            self.maybe_revive(h, new_token, llr);
        }
    }

    fn maybe_revive(&mut self, token: FactId, via: FactId, llr: f64) {
        if self.in_committed_region(token) {
            return; // a committed region cannot revive (§9.4, §10)
        }
        if self.is_live(token) {
            self.log.push(DecisionEvent::Revived { token, via, llr });
        }
    }

    /// Reverse index (§9.4): tokens whose scores can change when `fact`
    /// retracts — evidence premises and SEQUENCE endpoints depending on it.
        pub fn dependents_of(&self, fact: FactId) -> BTreeSet<FactId> {
        let mut out = BTreeSet::new();
        if let Some(refs) = self.premise_refs.get(&fact) {
            for r in refs {
                match *r {
                    EvidenceRef::Unary { idx } => {
                        out.insert(self.unary[idx].token);
                    }
                    EvidenceRef::Seq { idx } => {
                        out.insert(self.sequence[idx].to);
                    }
                }
            }
        }
        if let Some(idxs) = self.seq_by_from.get(&fact) {
            for i in idxs {
                out.insert(self.sequence[*i].to);
            }
        }
        if let Some(idxs) = self.seq_by_to.get(&fact) {
            for i in idxs {
                if let Some(f) = self.sequence[*i].from {
                    out.insert(f);
                }
            }
        }
        out
    }

    fn tiling_cmp(&self, a: &ScoredTiling, b: &ScoredTiling) -> std::cmp::Ordering {
        // Higher S(T) first; ties by §10 order. Deterministic: no
        // randomness, no iteration-order dependence.
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.tie_key(&self.tokens).cmp(&b.tie_key(&self.tokens)))
    }

    /// `S(T)` for an explicit token sequence (§4): unary sums plus one
    /// SEQUENCE term per adjacent pair, boundary pair included once.
    pub fn tiling_score(&self, tiling: &[FactId]) -> f64 {
        let mut score: f64 = tiling.iter().map(|t| self.unary_sum(*t)).sum();
        let mut prev: Option<FactId> = None;
        for t in tiling {
            if let Some(llr) = self.active_sequence_max(prev, *t) {
                score += self.params.w_sequence * llr;
            }
            prev = Some(*t);
        }
        score
    }

    /// Best and runner-up tilings of `[s, e)` by DP (§10): exact cover by
    /// non-overlapping WORD/SIL tokens (§7), maximising `S(T)` (§4).
        pub fn best_tilings(&self, region: (u32, u32)) -> (Option<ScoredTiling>, Option<ScoredTiling>) {
        let (s, e) = region;
        // Candidates start in [s, e): start >= s, and start < end <= e.
        // (Empty when s >= e, exactly as the old full scan.)
        let mut candidates: Vec<&Token> = Vec::new();
        if s < e {
            for facts in self.by_start.range(s..e).map(|(_, facts)| facts) {
                for f in facts {
                    let t = &self.tokens[f];
                    if t.tiling_eligible()
                        && t.end_frame <= e
                        // R1: tilings admit live tokens only.
                        && self.is_live(t.fact)
                    {
                        candidates.push(t);
                    }
                }
            }
        }
        candidates.sort_by(|a, b| {
            a.start_frame
                .cmp(&b.start_frame)
                .then_with(|| a.end_frame.cmp(&b.end_frame))
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.fact.0.cmp(&b.fact.0))
        });
        // Per-call unary-sum cache (reused across partial tilings); liveness
        // is already one call per candidate, in the filter above.
        let usum: HashMap<FactId, f64> = candidates
            .iter()
            .map(|t| (t.fact, self.unary_sum(t.fact)))
            .collect();
        // DP over frames, keeping the top two distinct partial tilings per
        // frame so the runner-up survives to the region end.
        let mut partials: BTreeMap<u32, Vec<ScoredTiling>> = BTreeMap::new();
        partials.insert(
            s,
            vec![ScoredTiling {
                tokens: Vec::new(),
                score: 0.0,
            }],
        );
        let mut frames: Vec<u32> = candidates.iter().map(|t| t.start_frame).collect();
        frames.push(s);
        frames.sort_unstable();
        frames.dedup();
        // Group candidates by start frame once: a per-frame full filter
        // would be O(frames x candidates). Grouping is O(candidates) with
        // the identical visit order (stable groups over the sorted
        // candidates), hence identical DP outcomes.
        let mut by_frame: HashMap<u32, Vec<&Token>> = HashMap::new();
        for t in &candidates {
            by_frame.entry(t.start_frame).or_default().push(*t);
        }
        let no_toks: Vec<&Token> = Vec::new();
        for frame in frames {
            let Some(current) = partials.get(&frame).cloned() else {
                continue;
            };
            for tok in by_frame.get(&frame).unwrap_or(&no_toks) {
                for partial in &current {
                    let prev = partial.tokens.last().copied();
                    let mut score = partial.score + usum[&tok.fact];
                    if let Some(llr) = self.active_sequence_max(prev, tok.fact) {
                        score += self.params.w_sequence * llr;
                    }
                    let mut tokens = partial.tokens.clone();
                    tokens.push(tok.fact);
                    let entry = partials.entry(tok.end_frame).or_default();
                    entry.push(ScoredTiling { tokens, score });
                    entry.sort_by(|a, b| self.tiling_cmp(a, b));
                    entry.dedup_by(|a, b| a.tokens == b.tokens);
                    entry.truncate(2);
                }
            }
        }
        let mut finals = partials.get(&e).cloned().unwrap_or_default();
        finals.sort_by(|a, b| self.tiling_cmp(a, b));
        finals.dedup_by(|a, b| a.tokens == b.tokens);
        let winner = finals.first().cloned();
        let runner_up = finals.get(1).cloned();
        (winner, runner_up)
    }

    /// Commit a region (§10): eligible once the frontier passes `e + H`;
    /// commits when the best tiling beats the runner-up by >= delta, or
    /// unconditionally at `e + H_max`. Winners go through the fabric's
    /// `commit()`; non-winners are defeated by deliberate retraction with
    /// provenance naming the winning tiling and the margin.
        pub fn commit_region(&mut self, region: (u32, u32), frontier_frame: u32) -> CommitOutcome {
        let (s, e) = region;
        if frontier_frame < e + self.params.h {
            return CommitOutcome::NotEligible;
        }
        let (winner, runner_up) = self.best_tilings(region);
        let Some(winner) = winner else {
            return CommitOutcome::NoTiling;
        };
        // Margin against the runner-up (§10, amendment 1.2): `None` when
        // the winner is unopposed (no live runner-up tiling). There is no
        // finite number that means "no rival", so absence is typed.
        let margin = runner_up.as_ref().map(|r| winner.score - r.score);
        if let Some(m) = margin
            && m < self.params.delta
            && frontier_frame < e + self.params.h_max
        {
            return CommitOutcome::MarginTooSmall { margin: m };
        }
        for t in &winner.tokens {
            self.fabric.commit(*t);
        }
        let winner_set: BTreeSet<FactId> = winner.tokens.iter().copied().collect();
        // Region members start in [s, e] (start >= s, start < end <= e).
        let mut members: Vec<FactId> = Vec::new();
        if s <= e {
            for facts in self.by_start.range(s..=e).map(|(_, facts)| facts) {
                for f in facts {
                    let t = &self.tokens[f];
                    if t.start_frame >= s && t.end_frame <= e {
                        members.push(*f);
                    }
                }
            }
        }
        for m in &members {
            self.committed_tokens.insert(*m);
            // Committed-region members leave the working set (bounded by
            // the commit window); winners stay in `active_all` so later
            // prune/budget mutations match the pre-index outcomes exactly.
            self.open.remove(m);
            self.drop_from_pools(*m);
        }
        let mut losers: Vec<FactId> = members
            .into_iter()
            .filter(|f| {
                let t = &self.tokens[f];
                t.tiling_eligible() && !winner_set.contains(f) && self.fabric.is_active(*f)
            })
            .collect();
        // Order is part of the contract: ascending token id, as the old
        // full-map scan produced.
        losers.sort_unstable();
        for loser in &losers {
            self.retract_internal(*loser);
            self.open.remove(loser);
            self.active_all.remove(loser);
            FabricStats::bump(&self.stats.defeated);
            self.log.push(DecisionEvent::Defeated {
                token: *loser,
                winner: winner.tokens.clone(),
                margin,
            });
        }
        self.committed_regions.push(region);
        FabricStats::bump(&self.stats.committed);
        self.log.push(DecisionEvent::Committed {
            region,
            winner: winner.tokens.clone(),
            margin,
        });
        CommitOutcome::Committed {
            winner,
            runner_up,
            margin,
            defeated: losers,
        }
    }

    /// One unary class term of a tiling-score breakdown (P2, bead
    /// hearsay-four-ofy): the strongest active `llr`, its frozen weight,
    /// and the weighted contribution `w_c * llr`. `llr: None` is the
    /// absent term of §3 rule 2 (contributes 0, never a negative).
    pub fn class_term(&self, token: FactId, class: SourceClass) -> (Option<f64>, f64, f64) {
        let llr = self.active_unary_max(token, class);
        let weight = self.params.weight(class);
        (llr, weight, llr.map_or(0.0, |v| weight * v))
    }

    /// Per-token tiling-score breakdown for `token` in a tiling whose
    /// predecessor is `prev` (`None` selects the `<s>` boundary pair).
    /// Every value is an `S(T)` component (§4); liveness `L(h)` (§5) is
    /// never computed here and never published (P2 contract).
    pub fn token_breakdown(&self, prev: Option<FactId>, token: FactId) -> TokenBreakdown {
        let terms: Vec<UnaryTerm> = SourceClass::UNARY
            .iter()
            .map(|c| {
                let (llr, weight, weighted) = self.class_term(token, *c);
                UnaryTerm {
                    class: *c,
                    llr,
                    weight,
                    weighted,
                }
            })
            .collect();
        let unary_total: f64 = terms.iter().map(|t| t.weighted).sum();
        let sequence_llr = self.active_sequence_max(prev, token);
        let sequence_weighted = sequence_llr.map_or(0.0, |v| self.params.w_sequence * v);
        TokenBreakdown {
            terms,
            sequence_llr,
            unary_total,
            total: unary_total + sequence_weighted,
        }
    }

    /// Full `S(T)` breakdown of a tiling (§4): per-token unary terms plus
    /// one SEQUENCE term per adjacent pair, boundary pair included once.
    /// The `total` equals [`DecisionFabric::tiling_score`] of the same
    /// tiling; both are recomputed from active justifications (§2).
    pub fn tiling_breakdown(&self, tiling: &[FactId]) -> (Vec<FactId>, Vec<f64>, f64) {
        let mut totals = Vec::with_capacity(tiling.len());
        let mut prev: Option<FactId> = None;
        let mut total = 0.0;
        for t in tiling {
            let breakdown = self.token_breakdown(prev, *t);
            totals.push(breakdown.total);
            total += breakdown.total;
            prev = Some(*t);
        }
        (tiling.to_vec(), totals, total)
    }

    /// Active unary evidence attached to a token (P2 rendering source).
        pub fn active_unary_evidence(&self, token: FactId) -> Vec<UnaryEvidence> {
        // Per-class buckets concatenated, then restored to global push
        // order: the old full scan returned push order.
        let mut indexed: Vec<(usize, UnaryEvidence)> = Vec::new();
        for class in SourceClass::UNARY {
            if let Some(bucket) = self.unary_idx.get(&(token, class)) {
                for i in bucket {
                    let e = &self.unary[*i];
                    if self.premises_active(&e.premises) {
                        indexed.push((*i, e.clone()));
                    }
                }
            }
        }
        indexed.sort_by_key(|(i, _)| *i);
        indexed.into_iter().map(|(_, e)| e).collect()
    }

    /// Revival events recorded for a token (§9.4), in log order.
    pub fn revivals_for(&self, token: FactId) -> Vec<(FactId, f64)> {
        self.log
            .iter()
            .filter_map(|e| match e {
                DecisionEvent::Revived { token: t, via, llr } if *t == token => Some((*via, *llr)),
                _ => None,
            })
            .collect()
    }

    /// Defeat recorded for a token (§10): winning tiling + margin
    /// (`None` when the winner was unopposed, amendment 1.2).
    pub fn defeat_for(&self, token: FactId) -> Option<(Vec<FactId>, Option<f64>)> {
        self.log.iter().find_map(|e| match e {
            DecisionEvent::Defeated {
                token: t,
                winner,
                margin,
            } if *t == token => Some((winner.clone(), *margin)),
            _ => None,
        })
    }

    /// USER correction facts gating a token's USER term (§3 rules 4-5):
    /// the premise facts of its active USER evidence.
        pub fn user_correction_facts(&self, token: FactId) -> Vec<FactId> {
        let mut out = Vec::new();
        if let Some(bucket) = self.unary_idx.get(&(token, SourceClass::User)) {
            for i in bucket {
                let e = &self.unary[*i];
                if self.premises_active(&e.premises) {
                    out.extend(e.premises.iter().copied());
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The published ACOUSTIC `llr` for a token (§6.2): computed from frames
    /// only, no LM term, unweighted. `None` if no acoustic evidence.
    pub fn published_acoustic_llr(&self, token: FactId) -> Option<f64> {
        self.active_unary_max(token, SourceClass::Acoustic)
    }

    /// Reference published-lattice bytes (§6.4): for every token in id
    /// order, its id plus the exact bits of its published ACOUSTIC `llr`.
    /// The decoder's internal LM `lm_weight` is accepted to mirror the
    /// decoder knob and is deliberately never read into any published byte:
    /// LM evidence enters only via SEQUENCE terms (§6.3).
    pub fn published_lattice_bytes(&self, _lm_weight: f64) -> Vec<u8> {
        let mut bytes = Vec::new();
        for fact in self.tokens.keys() {
            bytes.extend_from_slice(&fact.0.to_be_bytes());
            let bits = self.published_acoustic_llr(*fact).map_or(0, f64::to_bits);
            bytes.extend_from_slice(&bits.to_be_bytes());
        }
        bytes
    }
}

impl DecisionFabric {
    /// Stability of one justification: acoustic evidence with no premises
    /// is unconditional (never traversed); everything else is defeasible.
    /// Contract reconciliation (bead nlvs): a premise-gated acoustic row
    /// stays defeasible so weakening still applies; the bridge always
    /// passes empty premises, so real-path acoustic is unconditional.
    fn stability_of(class: SourceClass, premises: &BTreeSet<FactId>) -> Stability {
        if class == SourceClass::Acoustic && premises.is_empty() {
            Stability::Unconditional
        } else {
            Stability::Defeasible
        }
    }

    /// Re-sync after external mutation: if `fabric_mut` handed out access
    /// since the last sync, drop all memoized components (compute-on-miss
    /// rebuilds them) and clear the flag. Called by every cache reader and
    /// at the head of every drain.
    fn ensure_synced(&self) {
        if self.external_dirty.get() {
            self.cache.borrow_mut().clear();
            self.external_dirty.set(false);
        }
    }

    /// Enqueue one evidence row on the propagation frontier (deduped).
    fn enqueue_row(&mut self, row: PendingRow) {
        if self.pending_set.insert(row) {
            self.pending.push_back(row);
        }
    }

    /// Alpha dispatch: re-score the components one evidence row feeds and
    /// recompute the affected tokens' liveness. Bucket folds are the exact
    /// pre-saturation folds, so refreshed values equal fresh queries.
    fn dispatch_row(&self, row: PendingRow) {
        match row {
            PendingRow::Unary { idx } => {
                let (token, class) = {
                    let e = &self.unary[idx];
                    (e.token, e.class)
                };
                self.refresh_unary_component(token, class);
                self.refresh_liveness(token);
            }
            PendingRow::Seq { idx } => {
                let (from, to) = {
                    let e = &self.sequence[idx];
                    (e.from, e.to)
                };
                self.refresh_seq_pair_component(from, to);
                self.refresh_seq_side_component(to, true);
                self.refresh_liveness(to);
                if let Some(f) = from {
                    self.refresh_seq_side_component(f, false);
                    self.refresh_liveness(f);
                }
            }
        }
    }

    /// Drain the propagation frontier. Called eagerly at the end of every
    /// mutation, so caches are current whenever a query runs.
    fn drain_pending(&mut self) {
        self.ensure_synced();
        while let Some(row) = self.pending.pop_front() {
            self.pending_set.remove(&row);
            self.dispatch_row(row);
        }
    }

    /// Internal retraction: retract in the fabric, then enqueue every
    /// evidence row premised on the victim and drain. All internal
    /// retracts (prune, budgets, commit losers) route through here so
    /// caches never observe a retraction they did not process.
    fn retract_internal(&mut self, victim: FactId) {
        self.fabric.retract(victim);
        let rows: Vec<PendingRow> = self
            .premise_refs
            .get(&victim)
            .map(|refs| {
                refs.iter()
                    .map(|r| match *r {
                        EvidenceRef::Unary { idx } => PendingRow::Unary { idx },
                        EvidenceRef::Seq { idx } => PendingRow::Seq { idx },
                    })
                    .collect()
            })
            .unwrap_or_default();
        for row in rows {
            self.enqueue_row(row);
        }
        self.drain_pending();
        // The victim leaves all pools: evict its memoized entries so the
        // caches stay bounded by the live set (recomputable on miss).
        self.evict_token_caches(victim);
        self.drop_from_pools(victim);
    }

    /// Fresh fold of one unary bucket: the exact pre-saturation fold.
    fn fold_unary_bucket(&self, token: FactId, class: SourceClass) -> Option<f64> {
        let bucket = self.unary_idx.get(&(token, class))?;
        bucket
            .iter()
            .filter(|i| self.premises_active(&self.unary[**i].premises))
            .map(|i| self.unary[*i].llr)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }

    /// Memoized unary component: cache hit, else fold and store.
    fn component_unary_max(&self, token: FactId, class: SourceClass) -> Option<f64> {
        {
            let cached = self.cache.borrow();
            if let Some(v) = cached.unary_max.get(&(token, class)) {
                return *v;
            }
        }
        let v = self.fold_unary_bucket(token, class);
        self.cache
            .borrow_mut()
            .unary_max
            .insert((token, class), v);
        v
    }

    /// Force-refresh one unary component (propagation path, no stats bump).
    fn refresh_unary_component(&self, token: FactId, class: SourceClass) {
        let v = self.fold_unary_bucket(token, class);
        self.cache
            .borrow_mut()
            .unary_max
            .insert((token, class), v);
    }

    /// Fresh fold of one sequence pair bucket: the exact pre-saturation fold.
    fn fold_seq_pair(&self, from: Option<FactId>, to: FactId) -> Option<f64> {
        let bucket = self.seq_pair_idx.get(&(from, to))?;
        bucket
            .iter()
            .filter(|i| self.premises_active(&self.sequence[**i].premises))
            .map(|i| self.sequence[*i].llr)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }

    /// Memoized pair component: cache hit, else fold and store.
    fn component_seq_pair(&self, from: Option<FactId>, to: FactId) -> Option<f64> {
        {
            let cached = self.cache.borrow();
            if let Some(v) = cached.seq_pair_max.get(&(from, to)) {
                return *v;
            }
        }
        let v = self.fold_seq_pair(from, to);
        self.cache.borrow_mut().seq_pair_max.insert((from, to), v);
        v
    }

    /// Force-refresh one pair component (propagation path, no stats bump).
    fn refresh_seq_pair_component(&self, from: Option<FactId>, to: FactId) {
        let v = self.fold_seq_pair(from, to);
        self.cache.borrow_mut().seq_pair_max.insert((from, to), v);
    }

    /// Fresh fold of one sequence side bucket: the exact pre-saturation
    /// fold (boundary pairs excluded, as in `seq_side_max`).
    fn fold_seq_side(&self, token: FactId, to_side: bool) -> Option<f64> {
        let idx = if to_side {
            &self.seq_by_to
        } else {
            &self.seq_by_from
        };
        let bucket = idx.get(&token)?;
        bucket
            .iter()
            .filter(|i| {
                let e = &self.sequence[**i];
                e.from.is_some() && self.premises_active(&e.premises)
            })
            .map(|i| self.sequence[*i].llr)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }

    /// Memoized side component: cache hit, else fold and store.
    fn component_seq_side(&self, token: FactId, to_side: bool) -> Option<f64> {
        {
            let cached = self.cache.borrow();
            let map = if to_side {
                &cached.seq_to_max
            } else {
                &cached.seq_from_max
            };
            if let Some(v) = map.get(&token) {
                return *v;
            }
        }
        let v = self.fold_seq_side(token, to_side);
        {
            let mut cached = self.cache.borrow_mut();
            let map = if to_side {
                &mut cached.seq_to_max
            } else {
                &mut cached.seq_from_max
            };
            map.insert(token, v);
        }
        v
    }

    /// Force-refresh one side component (propagation path, no stats bump).
    fn refresh_seq_side_component(&self, token: FactId, to_side: bool) {
        let v = self.fold_seq_side(token, to_side);
        {
            let mut cached = self.cache.borrow_mut();
            let map = if to_side {
                &mut cached.seq_to_max
            } else {
                &mut cached.seq_from_max
            };
            map.insert(token, v);
        }
    }

    /// Recompute one token's liveness from its components, in the exact
    /// `unary_sum` + `liveness` operation order, and memoize it.
    fn refresh_liveness(&self, token: FactId) -> f64 {
        let mut score: f64 = SourceClass::UNARY
            .iter()
            .map(|c| {
                self.component_unary_max(token, *c)
                    .map_or(0.0, |llr| self.params.weight(*c) * llr)
            })
            .sum();
        if let Some(llr) = self.component_seq_side(token, true) {
            score += self.params.w_sequence * llr;
        }
        if let Some(llr) = self.component_seq_side(token, false) {
            score += self.params.w_sequence * llr;
        }
        let in_open = self.open.contains(&token);
        let active = self.fabric.is_active(token);
        {
            let mut cached = self.cache.borrow_mut();
            cached.liveness.insert(token, score);
            // Derived pool transition: idempotent sync of this token's
            // pool membership to truth (mirrors the live/dormant scan).
            cached.live_set.remove(&token);
            cached.dormant_set.remove(&token);
            if in_open && active {
                if score >= self.params.theta_live {
                    cached.live_set.insert(token);
                } else if self.params.arm == Arm::C {
                    cached.dormant_set.insert(token);
                }
            }
        }
        score
    }

    /// Drop working-set entries for facts no longer active. Internal
    /// retracts remove eagerly; external retraction via `fabric_mut`
    /// bypasses maintenance, so `&mut` scans re-sync here (exact).
    /// Evict one departed token's memoized entries (liveness, unary and
    /// side components, touching pair components) so caches stay bounded
    /// by the live set instead of growing with history. Every entry is
    /// recomputable on miss, so eviction only costs a refold if the dead
    /// token is ever queried again (rare: dead tokens leave all pools).
    fn evict_token_caches(&self, victim: FactId) {
        let pair_keys: Vec<(Option<FactId>, FactId)> = self
            .seq_by_to
            .get(&victim)
            .into_iter()
            .flatten()
            .chain(self.seq_by_from.get(&victim).into_iter().flatten())
            .map(|i| {
                let e = &self.sequence[*i];
                (e.from, e.to)
            })
            .collect();
        let mut cached = self.cache.borrow_mut();
        cached.liveness.remove(&victim);
        for class in SourceClass::UNARY {
            cached.unary_max.remove(&(victim, class));
        }
        cached.seq_to_max.remove(&victim);
        cached.seq_from_max.remove(&victim);
        for key in pair_keys {
            cached.seq_pair_max.remove(&key);
        }
    }

    /// Rebuild the derived pools from scratch when external mutation
    /// dirtied them. Each open token re-syncs through
    /// `refresh_liveness`, whose pool update is idempotent.
    fn sync_pools(&self) {
        let dirty = self.cache.borrow().pools_dirty;
        if !dirty {
            return;
        }
        {
            let mut cached = self.cache.borrow_mut();
            cached.live_set.clear();
            cached.dormant_set.clear();
        }
        let members: Vec<FactId> = self.open.iter().copied().collect();
        for t in members {
            self.refresh_liveness(t);
        }
        self.cache.borrow_mut().pools_dirty = false;
    }

    /// Drop one token from the derived pools (removal-time companion to
    /// the `open.remove` calls; blind removes, no status evaluation).
    fn drop_from_pools(&self, token: FactId) {
        let mut cached = self.cache.borrow_mut();
        cached.live_set.remove(&token);
        cached.dormant_set.remove(&token);
    }

    fn sync_open(&mut self) {
        let fabric = &self.fabric;
        self.open.retain(|t| fabric.is_active(*t));
        self.active_all.retain(|t| fabric.is_active(*t));
        // External truth may have changed behind propagation's back:
        // drop memoized components (compute-on-miss rebuilds them).
        self.cache.get_mut().clear();
        self.external_dirty.set(false);
    }

    fn track_open(&mut self) {
        self.peak_open = self.peak_open.max(self.open.len());
    }

    /// Clamp a ±J frame window around `center` to the u32 range.
    fn frame_window(center: u32, j: u32) -> (u32, u32) {
        let (c, j) = (center as i64, j as i64);
        (
            (c - j).max(0) as u32,
            (c + j).min(u32::MAX as i64) as u32,
        )
    }

    /// Tokens whose `liveness` can change when `fact` deactivates: the unary
    /// tokens and both sequence endpoints of evidence gated on `fact`.
    /// Complete: `liveness(T)` moves only when evidence relevant to `T`
    /// (unary for `T`, pairs into/out of `T`) loses an active premise.
    fn budget_neighbors(&self, fact: FactId) -> Vec<FactId> {
        let mut out = BTreeSet::new();
        if let Some(refs) = self.premise_refs.get(&fact) {
            for r in refs {
                match *r {
                    EvidenceRef::Unary { idx } => {
                        out.insert(self.unary[idx].token);
                    }
                    EvidenceRef::Seq { idx } => {
                        let e = &self.sequence[idx];
                        if let Some(f) = e.from {
                            out.insert(f);
                        }
                        out.insert(e.to);
                    }
                }
            }
        }
        out.remove(&fact);
        out.into_iter().collect()
    }

    /// Pop the lowest-(`liveness`, §10-rank) fresh heap entry from one pool
    /// (live or dormant), skipping evicted/stale/foreign-pool entries.
        fn pop_fresh(
        &self,
        heap: &mut BinaryHeap<Reverse<HeapEntry>>,
        state: &HashMap<FactId, (f64, u64)>,
    ) -> Option<FactId> {
        loop {
            let Some(Reverse(entry)) = heap.pop() else {
                return None;
            };
            FabricStats::bump(&self.stats.heap_pops);
            let Some((_, ver)) = state.get(&entry.token) else {
                continue;
            };
            if *ver != entry.version {
                continue;
            }
            return Some(entry.token);
        }
    }

    /// Recompute the evicted token's neighbors after its retraction and push
    /// fresh heap entries (lazy deletion: old entries go stale by version).
    /// Counts follow every transition symmetrically, so they stay exact
    /// under any liveness evolution.
        fn refresh_neighbors(
        &self,
        live_heap: &mut BinaryHeap<Reverse<HeapEntry>>,
        dormant_heap: &mut BinaryHeap<Reverse<HeapEntry>>,
        state: &mut HashMap<FactId, (f64, u64)>,
        rank: &HashMap<FactId, usize>,
        live_count: &mut usize,
        dormant_count: &mut usize,
        evicted: FactId,
    ) {
        let arm_c = self.params.arm == Arm::C;
        for n in self.budget_neighbors(evicted) {
            let Some(slot) = state.get_mut(&n) else {
                continue;
            };
            let was_live = slot.0 >= self.params.theta_live;
            let new_liv = self.liveness(n);
            slot.0 = new_liv;
            slot.1 += 1;
            let version = slot.1;
            let is_live = new_liv >= self.params.theta_live;
            match (was_live, is_live) {
                (true, false) => {
                    *live_count -= 1;
                    if arm_c {
                        *dormant_count += 1;
                    }
                }
                (false, true) => {
                    if arm_c {
                        *dormant_count -= 1;
                    }
                    *live_count += 1;
                }
                _ => {}
            }
            let entry = Reverse(HeapEntry {
                liveness: new_liv,
                rank: rank[&n],
                version,
                token: n,
            });
            if is_live {
                live_heap.push(entry);
                FabricStats::bump(&self.stats.heap_pushes);
            } else if arm_c {
                dormant_heap.push(entry);
                FabricStats::bump(&self.stats.heap_pushes);
            }
        }
    }

    /// Live + dormant counts in one open-set pass (one `liveness` call per
    /// token). Exact: same predicate pair as `live_tokens`/`dormant_tokens`.
    pub fn live_dormant_counts(&self) -> (usize, usize) {
        self.ensure_synced();
        self.sync_pools();
        let cached = self.cache.borrow();
        (cached.live_set.len(), cached.dormant_set.len())
    }

    /// Current working-set size: active tokens outside committed regions
    /// (bounded by the commit window under item-8 cadence).
    pub fn open_set_len(&self) -> usize {
        self.open.len()
    }

    /// Peak working-set size since construction (PERF nlvs proof data).
    pub fn peak_open_set(&self) -> usize {
        self.peak_open
    }

    /// Snapshot of the operation counters (PERF nlvs proof data).
    pub fn stats_snapshot(&self) -> FabricStatsSnapshot {
        let s = &self.stats;
        FabricStatsSnapshot {
            liveness_calls: s.liveness_calls.get(),
            unary_lookups: s.unary_lookups.get(),
            seq_pair_lookups: s.seq_pair_lookups.get(),
            heap_pushes: s.heap_pushes.get(),
            heap_pops: s.heap_pops.get(),
            evicted: s.evicted.get(),
            pruned: s.pruned.get(),
            defeated: s.defeated.get(),
            committed: s.committed.get(),
        }
    }
}

/// Span conflict (§7): overlap strictly greater than 50% of the shorter
/// span. Exactly 50% is not a conflict. Spans are half-open (§2).
pub fn spans_conflict(a: &Token, b: &Token) -> bool {
    let overlap = a
        .end_frame
        .min(b.end_frame)
        .saturating_sub(a.start_frame.max(b.start_frame));
    if overlap == 0 {
        return false;
    }
    let shorter = a.len_frames().min(b.len_frames());
    // Compare without floats: overlap / shorter > 1/2.
    overlap * 2 > shorter
}
