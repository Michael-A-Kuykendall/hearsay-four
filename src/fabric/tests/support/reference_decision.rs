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

use fabric::{Fabric, FactId};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The frozen spec version implemented by this module (§14).
pub const SPEC_VERSION: &str = "1.3";

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

/// The decision contract layer over a [`Fabric`] (spec version 1.0).
pub struct DecisionFabric {
    fabric: Fabric,
    params: DecisionParams,
    tokens: BTreeMap<FactId, Token>,
    unary: Vec<UnaryEvidence>,
    sequence: Vec<SequenceEvidence>,
    /// Dormant-predecessor index (§9.2): end frame -> token facts.
    by_end: BTreeMap<u32, BTreeSet<FactId>>,
    committed_regions: Vec<(u32, u32)>,
    log: Vec<DecisionEvent>,
    /// Intake frontier (§8 v1.3): max `end_frame` over minted tokens.
    /// Sealed(f) iff frontier > f.end + J; sealed stay for tiling but
    /// are exempt from budgets, eviction, and prune.
    intake_frontier: u32,
    /// Lattice arc index (§7 v1.4), mirrored with the real fabric.
    arcs: HashSet<(FactId, FactId)>,
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
            committed_regions: Vec::new(),
            log: Vec::new(),
            intake_frontier: 0,
            arcs: HashSet::new(),
        }
    }

    pub fn params(&self) -> &DecisionParams {
        &self.params
    }
    pub fn fabric(&self) -> &Fabric {
        &self.fabric
    }
    pub fn fabric_mut(&mut self) -> &mut Fabric {
        &mut self.fabric
    }
    pub fn token(&self, fact: FactId) -> Option<&Token> {
        self.tokens.get(&fact)
    }
    pub fn events(&self) -> &[DecisionEvent] {
        &self.log
    }
    pub fn intake_frontier(&self) -> u32 {
        self.intake_frontier
    }
    pub fn is_sealed(&self, token: FactId) -> bool {
        let Some(t) = self.tokens.get(&token) else {
            return false;
        };
        self.intake_frontier > t.end_frame.saturating_add(self.params.j)
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
        self.tokens.insert(fact, token);
        // Intake frontier (§8 v1.3): max end over minted tokens.
        self.intake_frontier = self.intake_frontier.max(end_frame);
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
        self.unary.push(UnaryEvidence {
            token,
            class,
            llr,
            null: class.declared_null(),
            premises,
        });
        Ok(())
    }

    /// Attach a USER correction: the fixed `+U0`, positive only (§3 rules
    /// 4-5), gated on the owner's correction fact via `premises`.
    pub fn add_user(&mut self, token: FactId, correction_premise: FactId) -> DecisionResult<()> {
        self.known(token)?;
        let mut premises = BTreeSet::new();
        premises.insert(correction_premise);
        self.unary.push(UnaryEvidence {
            token,
            class: SourceClass::User,
            llr: self.params.u0,
            null: SourceClass::User.declared_null(),
            premises,
        });
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
        self.sequence.push(SequenceEvidence {
            from: Some(from),
            to,
            llr,
            null: SourceClass::Sequence.declared_null(),
            premises,
        });
        Ok(())
    }

    /// Record one lattice arc `from -> to` (§7 v1.4), mirrored.
    pub fn add_lattice_arc(&mut self, from: FactId, to: FactId) -> DecisionResult<()> {
        self.known(from)?;
        self.known(to)?;
        self.arcs.insert((from, to));
        Ok(())
    }

    /// Attach the sentence-start boundary pair `<s> -> to` (§4).
    pub fn add_boundary_sequence(&mut self, to: FactId, llr: f64) -> DecisionResult<()> {
        self.known(to)?;
        self.sequence.push(SequenceEvidence {
            from: None,
            to,
            llr,
            null: SourceClass::Sequence.declared_null(),
            premises: BTreeSet::new(),
        });
        Ok(())
    }

    fn premises_active(&self, premises: &BTreeSet<FactId>) -> bool {
        premises.iter().all(|p| self.fabric.is_active(*p))
    }

    /// Strongest active justification in one unary class for a token (§3
    /// rule 3); `None` means the class contributes no term (rule 2).
    pub fn active_unary_max(&self, token: FactId, class: SourceClass) -> Option<f64> {
        self.unary
            .iter()
            .filter(|e| e.token == token && e.class == class && self.premises_active(&e.premises))
            .map(|e| e.llr)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }

    /// Strongest active SEQUENCE `llr` for an ordered pair; `from == None`
    /// selects the `<s>` boundary pair (§4). Each pair enters `S(T)` once.
    pub fn active_sequence_max(&self, from: Option<FactId>, to: FactId) -> Option<f64> {
        self.sequence
            .iter()
            .filter(|e| e.from == from && e.to == to && self.premises_active(&e.premises))
            .map(|e| e.llr)
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
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
        let mut score = self.unary_sum(token);
        let left = self
            .tokens
            .keys()
            .filter_map(|p| self.active_sequence_max(Some(*p), token))
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            });
        if let Some(llr) = left {
            score += self.params.w_sequence * llr;
        }
        let right = self
            .tokens
            .keys()
            .filter_map(|s| self.active_sequence_max(Some(token), *s))
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            });
        if let Some(llr) = right {
            score += self.params.w_sequence * llr;
        }
        score
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
        self.tokens
            .keys()
            .copied()
            .filter(|t| self.is_live(*t))
            .collect()
    }

    pub fn dormant_tokens(&self) -> Vec<FactId> {
        self.tokens
            .keys()
            .copied()
            .filter(|t| self.is_dormant(*t))
            .collect()
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
        // Sealed (§8 v1.3) are exempt from prune.
        let victims: Vec<FactId> = self
            .tokens
            .keys()
            .copied()
            .filter(|t| {
                self.fabric.is_active(*t)
                    && self.liveness(*t) < self.params.theta_live
                    && !self.is_sealed(*t)
            })
            .collect();
        for victim in victims {
            let liveness = self.liveness(victim);
            self.fabric.retract(victim);
            self.log.push(DecisionEvent::Pruned {
                token: victim,
                liveness,
            });
        }
    }

    fn in_committed_region(&self, token: FactId) -> bool {
        let t = &self.tokens[&token];
        self.committed_regions
            .iter()
            .any(|(s, e)| t.start_frame >= *s && t.end_frame <= *e)
    }

    /// Enforce budgets (§8, §11): in arm B, beam-prune below-threshold
    /// tokens first (live tokens only are counted); then evict lowest
    /// `L(h)` first, ties by §10 order. Eviction is a deliberate retraction
    /// recorded in provenance.
    pub fn enforce_budgets(&mut self) {
        // Arm B (§11): below-threshold tokens are retracted at once, so
        // budget accounting sees live tokens only. No-op in arm C.
        self.prune_below_threshold();
        // R2 (spec docs/design/a4-budget-semantics.md): live cap first,
        // then dormant + combined. Victim order everywhere: lowest L(h),
        // ties by §10 tie order. Sealed (§8 v1.3) are exempt from all
        // tallies and victim selection.
        while self
            .live_tokens()
            .iter()
            .copied()
            .filter(|t| !self.is_sealed(*t))
            .count()
            > self.params.live_budget
        {
            let Some(victim) = self
                .live_tokens()
                .iter()
                .copied()
                .filter(|t| !self.is_sealed(*t))
                .min_by(|a, b| {
                    self.liveness(*a)
                        .total_cmp(&self.liveness(*b))
                        .then_with(|| self.token_tie_cmp(*a, *b))
                })
            else {
                break;
            };
            let liveness = self.liveness(victim);
            self.fabric.retract(victim);
            self.log.push(DecisionEvent::Evicted {
                token: victim,
                liveness,
                budget: BudgetKind::Live,
            });
        }
        // Arm B (§11 table): the combined total counts live tokens only
        // (there is no dormant set), so over-total victims come from live.
        if self.params.arm == Arm::B {
            if let Some(total) = self.params.combined_budget {
                while self
                    .live_tokens()
                    .iter()
                    .copied()
                    .filter(|t| !self.is_sealed(*t))
                    .count()
                    > total
                {
                    let Some(victim) = self
                        .live_tokens()
                        .iter()
                        .copied()
                        .filter(|t| !self.is_sealed(*t))
                        .min_by(|a, b| {
                            self.liveness(*a)
                                .total_cmp(&self.liveness(*b))
                                .then_with(|| self.token_tie_cmp(*a, *b))
                        })
                    else {
                        break;
                    };
                    let liveness = self.liveness(victim);
                    self.fabric.retract(victim);
                    self.log.push(DecisionEvent::Evicted {
                        token: victim,
                        liveness,
                        budget: BudgetKind::Combined,
                    });
                }
            }
            return;
        }
        loop {
            let dormant: Vec<FactId> = self
                .dormant_tokens()
                .into_iter()
                .filter(|t| !self.is_sealed(*t))
                .collect();
            let live_unsealed = self
                .live_tokens()
                .iter()
                .copied()
                .filter(|t| !self.is_sealed(*t))
                .count();
            let over_dormant = dormant.len() > self.params.dormant_budget;
            let over_combined = self
                .params
                .combined_budget
                .is_some_and(|total| live_unsealed + dormant.len() > total);
            if !over_dormant && !over_combined {
                return;
            }
            let budget = if over_dormant {
                BudgetKind::Dormant
            } else {
                BudgetKind::Combined
            };
            let Some(victim) = dormant.iter().copied().min_by(|a, b| {
                self.liveness(*a)
                    .total_cmp(&self.liveness(*b))
                    .then_with(|| self.token_tie_cmp(*a, *b))
            }) else {
                return;
            };
            let liveness = self.liveness(victim);
            self.fabric.retract(victim);
            self.log.push(DecisionEvent::Evicted {
                token: victim,
                liveness,
                budget,
            });
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
        let end = self.tokens[&token].end_frame as i64;
        let j = self.params.j as i64;
        self.tokens
            .values()
            .filter(|t| t.fact != token && (t.start_frame as i64 - end).abs() <= j)
            .map(|t| t.fact)
            .collect()
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
        let j = self.params.j as i64;
        // Right context: dormant h with end in [start(s)-J, start(s)+J].
        let mut predecessors: Vec<FactId> = Vec::new();
        for (end, facts) in &self.by_end {
            if (*end as i64 - s_tok.start_frame as i64).abs() <= j {
                predecessors.extend(facts.iter().copied());
            }
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
        let successors: Vec<FactId> = self
            .tokens
            .values()
            .filter(|t| {
                t.fact != new_token && (t.start_frame as i64 - s_tok.end_frame as i64).abs() <= j
            })
            .map(|t| t.fact)
            .collect();
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
        for e in &self.unary {
            if e.premises.contains(&fact) {
                out.insert(e.token);
            }
        }
        for e in &self.sequence {
            if e.premises.contains(&fact) || e.from == Some(fact) {
                out.insert(e.to);
            }
            if e.to == fact
                && let Some(f) = e.from
            {
                out.insert(f);
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
        let mut candidates: Vec<&Token> = self
            .tokens
            .values()
            .filter(|t| {
                t.tiling_eligible()
                    && t.start_frame >= s
                    && t.end_frame <= e
                    // R1: tilings admit live tokens only.
                    && self.is_live(t.fact)
            })
            .collect();
        candidates.sort_by(|a, b| {
            a.start_frame
                .cmp(&b.start_frame)
                .then_with(|| a.end_frame.cmp(&b.end_frame))
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.fact.0.cmp(&b.fact.0))
        });
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
        for frame in frames {
            let Some(current) = partials.get(&frame).cloned() else {
                continue;
            };
            for tok in candidates.iter().filter(|t| t.start_frame == frame) {
                for partial in &current {
                    let prev = partial.tokens.last().copied();
                    // §7 v1.4: tilings extend only along lattice arcs.
                    if let Some(p) = prev {
                        if !self.arcs.contains(&(p, tok.fact)) {
                            continue;
                        }
                    }
                    let mut score = partial.score + self.unary_sum(tok.fact);
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
        let losers: Vec<FactId> = self
            .tokens
            .values()
            .filter(|t| {
                t.tiling_eligible()
                    && t.start_frame >= s
                    && t.end_frame <= e
                    && !winner_set.contains(&t.fact)
                    && self.fabric.is_active(t.fact)
            })
            .map(|t| t.fact)
            .collect();
        for loser in &losers {
            self.fabric.retract(*loser);
            self.log.push(DecisionEvent::Defeated {
                token: *loser,
                winner: winner.tokens.clone(),
                margin,
            });
        }
        self.committed_regions.push(region);
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
        self.unary
            .iter()
            .filter(|e| e.token == token && self.premises_active(&e.premises))
            .cloned()
            .collect()
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
        for e in &self.unary {
            if e.token == token && e.class == SourceClass::User && self.premises_active(&e.premises)
            {
                out.extend(e.premises.iter().copied());
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
