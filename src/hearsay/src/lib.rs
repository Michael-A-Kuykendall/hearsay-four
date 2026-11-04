//! Lattice → [`DecisionFabric`](fabric::decision::DecisionFabric) bridge.
//!
//! Feeds one [`lattice::Lattice`] through the fabric decision pipeline
//! (`docs/design/fabric-decision-contract.md`, spec 1.2) and returns the
//! committed transcript plus the full decision-event trail:
//!
//! 1. Tokens are minted **in frame order** with their ACOUSTIC unary
//!    evidence (plus LEXICAL / SPEAKER / VERIFY when the lattice carries
//!    them). Lattice `user_llr` is deliberately *not* fed: decision.rs
//!    exposes USER evidence only via `add_user(token, correction_premise)`,
//!    and a lattice LLR carries no correction-premise provenance to cite.
//! 2. When each successor arrives, its SEQUENCE evidence attaches: tokens
//!    with no incoming lattice arcs get a boundary pair (`<s>` history,
//!    contract §4); every lattice arc gets one `add_sequence` from the LM
//!    (§3 SEQUENCE null = unigram word model). The fabric's own
//!    `attach_context_for_new_token` (§9) runs first so dormant
//!    predecessors within `J` frames revive with provenance.
//! 3. The utterance commits as greedy maximal tileable regions per §10:
//!    each region first tries the stream frontier (utterance end, so `H`
//!    and `delta` apply); regions that are not yet eligible or below the
//!    margin flush unconditionally at `e + H_max`. Frame gaps with no
//!    covering token are recorded in [`BridgeOutcome::gaps`], never
//!    papered over with invented SIL.
//!
//! # Event-log format (v1, handoff contract)
//!
//! [`BridgeOutcome::events_jsonl`] holds one JSON object per line, in
//! fabric emission order, one file per utterance (`<utterance>.events.jsonl`).
//! Every line carries `v` (this [`EVENT_LOG_VERSION`]), `utterance`, `seq`
//! (0-based emission order), and `event` (one of `pruned`, `revived`,
//! `defeated`, `evicted`, `committed`). Token references resolve to
//! `{"id","word","start","end"}` from the source lattice; a fact with no
//! lattice token renders as `{"fact": <u64>}`.
//!
//! - `pruned`: `{"token","liveness"}` — arm-B beam prune: a token below
//!   `theta_live` retracted at enforcement, never revivable.
//! - `revived`: `{"token","via","llr"}` — dormant token crossed
//!   `theta_live` via new context (§9.4).
//! - `defeated`: `{"token","winner","margin"}` — non-winning token in a
//!   committed region; `winner` is the winning tiling's token list,
//!   `margin` is `null` when the winner was unopposed (§10 amendment 1.2).
//! - `evicted`: `{"token","liveness","budget"}` — budget exhaustion (§8);
//!   `budget` is `live`, `dormant`, or `combined`.
//! - `committed`: `{"region":[s,e],"winner","margin"}` — region commit (§10).
//!
//! The experiment bead (arms A/B/C) and the demo bead (why-panel) consume
//! this format plus [`BridgeOutcome::transcript`].
//!
//! Arm selection is [`DecisionParams`] passthrough ([`Arm`] re-exported;
//! default C preserves dormant-and-revivable behavior). The driver calls
//! `enforce_budgets()` after every token's intake and before every commit,
//! so arm B beam-prunes below-threshold tokens with `pruned` provenance.

use fabric::FactId;
pub use fabric::decision::Arm;
use fabric::decision::{
    CommitOutcome, DecisionError, DecisionEvent, DecisionFabric, DecisionParams, SourceClass,
    TokenKind,
};

use lattice::Lattice;
use lm::interpolate::InterpolatedLm;
use lm::normalize::BOS_TOKEN;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Event-log schema version. Bump only with a format change; consumers
/// switch on this field.
pub const EVENT_LOG_VERSION: u32 = 1;

/// Probability floor before `ln`, mirroring the `.max(1e-12)` precedent in
/// `InterpolatedLm::sentence_log_prob`. KN backoff can return exact 0.0 for
/// unseen continuations, and an LLR must stay finite.
const PROB_FLOOR: f64 = 1e-12;

/// SEQUENCE log-likelihood-ratio scorer: one LLR per (history, word) pair.
///
/// `prev = None` is utterance start and scores against sentence-start
/// history (`<s>`, contract §4 boundary pair). The null is the unigram word
/// model (contract §3): implementations return
/// `ln(P(word | history) / P_unigram(word))`.
pub trait SequenceScorer {
    fn sequence_llr(&self, prev: Option<&str>, word: &str) -> f64;
}

/// Interpolated-KN bigram-over-unigram [`SequenceScorer`].
///
/// Conditional rows mix the general/personal components with
/// `personal_weight` exactly like [`InterpolatedLm::prob`] mixes trigram
/// rows; the unigram null mixes the same way. Arcs carry single-word
/// history, so the bigram row — not the trigram row — is the correct
/// conditional here.
pub struct InterpolatedScorer<'a> {
    lm: &'a InterpolatedLm,
}

impl<'a> InterpolatedScorer<'a> {
    pub fn new(lm: &'a InterpolatedLm) -> Self {
        Self { lm }
    }

    fn cond_prob(&self, prev: &str, word: &str) -> f64 {
        let lambda = self.lm.personal_weight();
        ((1.0 - lambda) * self.lm.general().bigram_prob(prev, word)
            + lambda * self.lm.personal().bigram_prob(prev, word))
        .clamp(0.0, 1.0)
    }

    fn uni_prob(&self, word: &str) -> f64 {
        let lambda = self.lm.personal_weight();
        ((1.0 - lambda) * self.lm.general().unigram_prob(word)
            + lambda * self.lm.personal().unigram_prob(word))
        .clamp(0.0, 1.0)
    }
}

impl SequenceScorer for InterpolatedScorer<'_> {
    fn sequence_llr(&self, prev: Option<&str>, word: &str) -> f64 {
        // The LM vocabulary is lowercase (normalize.rs); lattice words are
        // uppercase (decoder vocab). Without this, every pair is OOV and
        // KN backs off to exactly the unigram, so llr is 0.0 everywhere.
        // The decoder lowercases at its call sites (moore.rs); the scorer
        // normalizes centrally so all bridge paths agree.
        let history = prev.unwrap_or(BOS_TOKEN);
        let h_low = history.to_lowercase();
        let w_low = word.to_lowercase();
        (self.cond_prob(&h_low, &w_low).max(PROB_FLOOR) / self.uni_prob(&w_low).max(PROB_FLOOR))
            .ln()
    }
}

/// A minted lattice token: the join key between fabric facts, lattice ids,
/// and the event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenRef {
    pub id: u64,
    pub word: String,
    pub start: u32,
    pub end: u32,
}

/// One committed region (§10): winner, provenance, and how it committed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommitRecord {
    pub region: (u32, u32),
    pub winner_words: Vec<String>,
    pub winner_score: f64,
    pub runner_up_score: Option<f64>,
    /// `None` when the winner was unopposed (§10 amendment 1.2).
    pub margin: Option<f64>,
    pub defeated_words: Vec<String>,
    /// Frontier value the commit ran under.
    pub frontier: u32,
    /// True when the stream frontier (`H`/`delta`) did not commit and the
    /// region flushed unconditionally at `e + H_max`.
    pub flushed: bool,
}

/// Full decode of one lattice: transcript, commits, gaps, and event log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BridgeOutcome {
    pub utterance: String,
    /// Committed winner labels in region order.
    pub transcript: Vec<String>,
    pub commits: Vec<CommitRecord>,
    /// Frame gaps `(cursor, next_start)` no token covers; the transcript
    /// simply concatenates across them.
    pub gaps: Vec<(u32, u32)>,
    /// Lattice ids that ended in no committed winner and no defeat record
    /// (tokens straddling a gap or region edge).
    pub uncommitted_ids: Vec<u64>,
    /// Every minted token in frame order, for downstream joins.
    pub tokens: Vec<TokenRef>,
    /// [`EVENT_LOG_VERSION`] JSONL, one [`DecisionEvent`] per line.
    pub events_jsonl: String,
    /// Post-enforcement peak live pool size over the phase-2 feed.
    pub peak_live: usize,
    /// Post-enforcement peak dormant pool size over the phase-2 feed.
    pub peak_dormant: usize,
    /// Post-enforcement peak live+dormant total over the phase-2 feed.
    pub peak_total: usize,
}

fn dec_err(e: DecisionError) -> String {
    match e {
        DecisionError::ZeroLengthSpan => "bridge: zero-length token span".to_string(),
        DecisionError::UnknownToken(f) => format!("bridge: unknown token fact {}", f.0),
        DecisionError::SequenceIsPairwise => {
            "bridge: internal error: sequence attached as unary".to_string()
        }
        DecisionError::UserNeedsFixedValue => {
            "bridge: internal error: user attached as unary".to_string()
        }
    }
}

fn outcome_name(o: &CommitOutcome) -> &'static str {
    match o {
        CommitOutcome::Committed { .. } => "committed",
        CommitOutcome::NotEligible => "not-eligible",
        CommitOutcome::MarginTooSmall { .. } => "margin-too-small",
        CommitOutcome::NoTiling => "no-tiling",
    }
}

/// Decode one lattice through a fresh [`DecisionFabric`].
///
/// Tokens feed in frame order (start, end, lattice id); SEQUENCE evidence
/// attaches per successor as it arrives; regions commit per §10 with an
/// `e + H_max` flush fallback; the transcript is the concatenation of
/// committed winners. Fails loudly on empty lattices, dangling arcs, and
/// commits that cannot complete — never silently.
pub fn decode_lattice(
    lat: &Lattice,
    scorer: &dyn SequenceScorer,
    params: DecisionParams,
) -> Result<BridgeOutcome, String> {
    if lat.tokens.is_empty() {
        return Err(format!(
            "bridge: utterance {:?} has no tokens",
            lat.utterance
        ));
    }
    let mut by_id: BTreeMap<u64, usize> = BTreeMap::new();
    for (i, t) in lat.tokens.iter().enumerate() {
        if by_id.insert(t.id, i).is_some() {
            return Err(format!("bridge: duplicate token id {}", t.id));
        }
    }
    let mut order: Vec<usize> = (0..lat.tokens.len()).collect();
    order.sort_by(|&a, &b| {
        let (ta, tb) = (&lat.tokens[a], &lat.tokens[b]);
        (ta.start, ta.end, ta.id).cmp(&(tb.start, tb.end, tb.id))
    });
    let mut incoming: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for a in &lat.arcs {
        if !by_id.contains_key(&a.from) {
            return Err(format!(
                "bridge: arc from={} dangles (no such token)",
                a.from
            ));
        }
        if !by_id.contains_key(&a.to) {
            return Err(format!("bridge: arc to={} dangles (no such token)", a.to));
        }
        incoming.entry(a.to).or_default().push(a.from);
    }
    for froms in incoming.values_mut() {
        froms.sort_by(|&a, &b| {
            let (ta, tb) = (&lat.tokens[by_id[&a]], &lat.tokens[by_id[&b]]);
            (ta.start, ta.end, ta.id).cmp(&(tb.start, tb.end, tb.id))
        });
        froms.dedup();
    }

    let h_max = params.h_max;
    let mut fabric = DecisionFabric::new(params);
    let mut fact_of: BTreeMap<u64, FactId> = BTreeMap::new();
    let mut refs: BTreeMap<FactId, TokenRef> = BTreeMap::new();
    let mut minted: Vec<TokenRef> = Vec::new();
    // Single-pass intake, in frame order (7mge): each token mints, adds its
    // unaries, runs the §9 context hook, attaches its SEQUENCE structure
    // (boundary pair for arc-less tokens, one pair term per incoming arc,
    // contract §4 exactly-once), enforces budgets, and records peak counts
    // — all before the next token arrives. Budgets therefore only ever see
    // frontier-near tokens: a weak-acoustic mid-chain word survives until
    // its successor's strong bigram lands, instead of being evicted while
    // far-future acoustic-only tokens compete (the two-phase failure mode).
    // Arcs run forward in time, so every incoming arc's `from` fact is
    // already minted; a backward arc fails loudly rather than attaching
    // half a pair.
    let mut peak_live = 0usize;
    let mut peak_dormant = 0usize;
    let mut peak_total = 0usize;
    for &i in &order {
        let t = &lat.tokens[i];
        let fact = fabric
            .mint_token(t.word.clone(), TokenKind::Word, t.start, t.end)
            .map_err(dec_err)?;
        fabric
            .add_unary(fact, SourceClass::Acoustic, t.acoustic_llr, BTreeSet::new())
            .map_err(dec_err)?;
        for (class, llr) in [
            (SourceClass::Lexical, t.lexical_llr),
            (SourceClass::Speaker, t.speaker_llr),
            (SourceClass::Verify, t.verify_llr),
        ] {
            if let Some(llr) = llr {
                fabric
                    .add_unary(fact, class, llr, BTreeSet::new())
                    .map_err(dec_err)?;
            }
        }
        fact_of.insert(t.id, fact);
        let r = TokenRef {
            id: t.id,
            word: t.word.clone(),
            start: t.start,
            end: t.end,
        };
        refs.insert(fact, r.clone());
        minted.push(r);
        fabric.attach_context_for_new_token(fact, &|from_tok, to_tok| {
            scorer.sequence_llr(Some(from_tok.label.as_str()), to_tok.label.as_str())
        });
        match incoming.get(&t.id) {
            None => {
                let llr = scorer.sequence_llr(None, &t.word);
                fabric.add_boundary_sequence(fact, llr).map_err(dec_err)?;
            }
            Some(froms) => {
                for f in froms {
                    let from_fact = *fact_of.get(f).ok_or_else(|| {
                        format!("bridge: arc {f} -> {} runs backward in frame order", t.id)
                    })?;
                    let from_word = &lat.tokens[by_id[f]].word;
                    let llr = scorer.sequence_llr(Some(from_word), &t.word);
                    let mut premises = BTreeSet::new();
                    premises.insert(from_fact);
                    premises.insert(fact);
                    fabric
                        .add_sequence(from_fact, fact, llr, premises)
                        .map_err(dec_err)?;
                }
            }
        }
        // Driver cadence (W3 handoff): budgets enforce after every intake
        // step — arm B beam-prunes below-threshold tokens here; arm C only
        // evicts past budget.
        fabric.enforce_budgets();
        // Single-pass counts: identical values to the two vector lens,
        // without building vectors per intake (nlvs perf; O(open) total).
        let (nl, nd) = fabric.live_dormant_counts();
        peak_live = peak_live.max(nl);
        peak_dormant = peak_dormant.max(nd);
        peak_total = peak_total.max(nl + nd);
    }

    // Commit (§10): greedy maximal tileable regions from the utterance
    // start. Each region first tries the stream frontier (utterance end),
    // so H and delta apply; regions that are not eligible or below the
    // margin flush unconditionally at e + H_max. The cursor strictly
    // increases every iteration, so this always terminates.
    let utter_start = lat.tokens[order[0]].start;
    let utter_end = lat.tokens.iter().map(|t| t.end).max().unwrap_or(0);
    let mut commits: Vec<CommitRecord> = Vec::new();
    let mut transcript: Vec<String> = Vec::new();
    let mut gaps: Vec<(u32, u32)> = Vec::new();
    let mut cursor = utter_start;
    loop {
        // Single-pass tileable ends (f6tp): one DP reveals every end
        // tileable from cursor. The old largest-first loop ran a full DP
        // per candidate end — O(ends) DPs per region, pathological once
        // budget evictions fragment coverage (34+ CPU-min, killed).
        let mut region: Option<(u32, u32)> = None;
        if let Some(e) = fabric.tileable_ends(cursor, utter_end).into_iter().max() {
            region = Some((cursor, e));
        }
        match region {
            Some((s, e)) => {
                fabric.enforce_budgets();
                let outcome = fabric.commit_region((s, e), utter_end);
                let (winner, runner_up, margin, defeated, flushed) = match outcome {
                    CommitOutcome::Committed {
                        winner,
                        runner_up,
                        margin,
                        defeated,
                    } => (winner, runner_up, margin, defeated, false),
                    CommitOutcome::MarginTooSmall { .. } | CommitOutcome::NotEligible => {
                        let flush_at = e.saturating_add(h_max);
                        fabric.enforce_budgets();
                        match fabric.commit_region((s, e), flush_at) {
                            CommitOutcome::Committed {
                                winner,
                                runner_up,
                                margin,
                                defeated,
                            } => (winner, runner_up, margin, defeated, true),
                            other => {
                                return Err(format!(
                                    "bridge: flush commit of [{s},{e}) failed: {}",
                                    outcome_name(&other)
                                ));
                            }
                        }
                    }
                    CommitOutcome::NoTiling => {
                        return Err(format!(
                            "bridge: region [{s},{e}) lost its tiling between probe and commit"
                        ));
                    }
                };
                let mut winner_words = Vec::with_capacity(winner.tokens.len());
                for f in &winner.tokens {
                    let r = refs.get(f).ok_or_else(|| {
                        format!("bridge: winner fact {} has no lattice token", f.0)
                    })?;
                    winner_words.push(r.word.clone());
                }
                transcript.extend(winner_words.clone());
                let mut defeated_words = Vec::with_capacity(defeated.len());
                for f in &defeated {
                    let r = refs.get(f).ok_or_else(|| {
                        format!("bridge: defeated fact {} has no lattice token", f.0)
                    })?;
                    defeated_words.push(r.word.clone());
                }
                commits.push(CommitRecord {
                    region: (s, e),
                    winner_words,
                    winner_score: winner.score,
                    runner_up_score: runner_up.as_ref().map(|r| r.score),
                    margin,
                    defeated_words,
                    frontier: if flushed {
                        e.saturating_add(h_max)
                    } else {
                        utter_end
                    },
                    flushed,
                });
                cursor = e;
                if cursor >= utter_end {
                    break;
                }
            }
            None => match lat
                .tokens
                .iter()
                .map(|t| t.start)
                .filter(|&s| s > cursor)
                .min()
            {
                Some(next) => {
                    gaps.push((cursor, next));
                    cursor = next;
                }
                None => break,
            },
        }
    }

    let events_jsonl = render_events_jsonl(&lat.utterance, fabric.events(), &refs);
    let mut won: BTreeSet<u64> = BTreeSet::new();
    let mut lost: BTreeSet<u64> = BTreeSet::new();
    for ev in fabric.events() {
        match ev {
            DecisionEvent::Committed { winner, .. } => {
                for f in winner {
                    if let Some(r) = refs.get(f) {
                        won.insert(r.id);
                    }
                }
            }
            DecisionEvent::Defeated { token, .. } => {
                if let Some(r) = refs.get(token) {
                    lost.insert(r.id);
                }
            }
            _ => {}
        }
    }
    let uncommitted_ids: Vec<u64> = lat
        .tokens
        .iter()
        .map(|t| t.id)
        .filter(|id| !won.contains(id) && !lost.contains(id))
        .collect();
    Ok(BridgeOutcome {
        utterance: lat.utterance.clone(),
        transcript,
        commits,
        gaps,
        uncommitted_ids,
        tokens: minted,
        events_jsonl,
        peak_live,
        peak_dormant,
        peak_total,
    })
}

fn token_json(f: FactId, refs: &BTreeMap<FactId, TokenRef>) -> Value {
    match refs.get(&f) {
        Some(r) => json!({
            "id": r.id,
            "word": r.word,
            "start": r.start,
            "end": r.end,
        }),
        None => json!({ "fact": f.0 }),
    }
}

fn logged_event(
    utterance: &str,
    seq: usize,
    ev: &DecisionEvent,
    refs: &BTreeMap<FactId, TokenRef>,
) -> Value {
    match ev {
        DecisionEvent::Pruned { token, liveness } => json!({
            "v": EVENT_LOG_VERSION,
            "utterance": utterance,
            "seq": seq,
            "event": "pruned",
            "token": token_json(*token, refs),
            "liveness": liveness,
        }),
        DecisionEvent::Revived { token, via, llr } => json!({
            "v": EVENT_LOG_VERSION,
            "utterance": utterance,
            "seq": seq,
            "event": "revived",
            "token": token_json(*token, refs),
            "via": token_json(*via, refs),
            "llr": llr,
        }),
        DecisionEvent::Defeated {
            token,
            winner,
            margin,
        } => json!({
            "v": EVENT_LOG_VERSION,
            "utterance": utterance,
            "seq": seq,
            "event": "defeated",
            "token": token_json(*token, refs),
            "winner": winner.iter().map(|f| token_json(*f, refs)).collect::<Vec<_>>(),
            "margin": margin,
        }),
        DecisionEvent::Evicted {
            token,
            liveness,
            budget,
        } => json!({
            "v": EVENT_LOG_VERSION,
            "utterance": utterance,
            "seq": seq,
            "event": "evicted",
            "token": token_json(*token, refs),
            "liveness": liveness,
            "budget": match budget {
                fabric::decision::BudgetKind::Live => "live",
                fabric::decision::BudgetKind::Dormant => "dormant",
                fabric::decision::BudgetKind::Combined => "combined",
            },
        }),
        DecisionEvent::Committed {
            region,
            winner,
            margin,
        } => json!({
            "v": EVENT_LOG_VERSION,
            "utterance": utterance,
            "seq": seq,
            "event": "committed",
            "region": [region.0, region.1],
            "winner": winner.iter().map(|f| token_json(*f, refs)).collect::<Vec<_>>(),
            "margin": margin,
        }),
    }
}

/// Render fabric events as v1 JSONL (see module docs): one object per line,
/// in emission order, each line terminated by `\n`.
pub fn render_events_jsonl(
    utterance: &str,
    events: &[DecisionEvent],
    refs: &BTreeMap<FactId, TokenRef>,
) -> String {
    let mut out = String::new();
    for (seq, ev) in events.iter().enumerate() {
        let line = serde_json::to_string(&logged_event(utterance, seq, ev, refs))
            .expect("event JSON object serializes");
        out.push_str(&line);
        out.push('\n');
    }
    out
}
