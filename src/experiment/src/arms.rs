//! Arm runners: A (1-best, no fabric), B (live prune), C (dormant).
//!
//! Feed protocol mirrors the lattice-fabric bridge design: lattice tokens
//! enter the [`DecisionFabric`] in frame order with acoustic unary
//! evidence; boundary pairs attach at utterance start; sequence evidence
//! per arc attaches when the successor arrives (which revives dormant
//! predecessors under arm C); budgets enforce after every arrival; the
//! whole utterance commits once at the end per contract section 10.
//!
//! Arm semantics use the native [`fabric::decision::Arm`] flag
//! (`hearsay-four-bpg2--fabric-prune-arm`, landed):
//!
//! - Arm B: a token below `theta_live` is beam-pruned at once
//!   ([`DecisionEvent::Pruned`]), never revivable, never receiving later
//!   evidence. Budgets count live tokens only.
//! - Arm C: below-threshold tokens linger dormant and revive when later
//!   sequence context pushes them over `theta_live`. Live plus dormant
//!   share the same combined total.
//!
//! Driver cadence per the prune-arm handoff: `enforce_budgets()` after
//! every intake step and once more before `commit_region()`.
//!
//! Scoring, evidence, and the commit rule are identical in both arms; only
//! the below-threshold fate differs.

use fabric::FactId;
use fabric::decision::{
    Arm as FabricArm, CommitOutcome, DecisionEvent, DecisionFabric, DecisionParams,
    SourceClass, TokenKind,
};
use lattice::Lattice;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Instant;

/// Experiment arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// 1-best baseline: max-`acoustic_llr` path, no fabric.
    A,
    /// Live prune: below-threshold tokens are retracted at once.
    B,
    /// Dormant: below-threshold tokens linger and may revive.
    C,
}

/// Tunable arm parameters. Arm C always uses values identical to arm B;
/// only the budget triple differs between arms.
#[derive(Debug, Clone)]
pub struct ArmParams {
    pub budget: usize,
    pub w_acoustic: f64,
    pub w_sequence: f64,
    pub theta_live: f64,
    pub delta: f64,
    pub h: u32,
    pub h_max: u32,
}

impl ArmParams {
    /// Fixture defaults (contract §12 leaves these DEV-tuned; DEV tuning
    /// on real data happens on main once lattices exist).
    pub fn fixture_defaults(budget: usize) -> Self {
        Self {
            budget,
            w_acoustic: 1.0,
            w_sequence: 1.0,
            theta_live: 0.0,
            delta: 0.0,
            h: 10,
            h_max: 20,
        }
    }

    fn decision_params(&self, arm: Arm) -> DecisionParams {
        // Arm B: dormant_budget is ignored natively (no dormant set exists);
        // combined counts live only. Arm C: live + dormant share the total.
        let (live, dormant, combined, fabric_arm) = match arm {
            Arm::A => (usize::MAX, usize::MAX, None, FabricArm::C),
            Arm::B => (self.budget, 0, Some(self.budget), FabricArm::B),
            Arm::C => (self.budget, self.budget, Some(self.budget), FabricArm::C),
        };
        DecisionParams {
            w_acoustic: self.w_acoustic,
            w_sequence: self.w_sequence,
            theta_live: self.theta_live,
            live_budget: live,
            dormant_budget: dormant,
            combined_budget: combined,
            h: self.h,
            h_max: self.h_max,
            delta: self.delta,
            arm: fabric_arm,
            ..DecisionParams::default()
        }
    }
}

/// One arm run over one utterance.
#[derive(Debug, Clone)]
pub struct ArmOutcome {
    /// Committed transcript (empty when the region did not commit).
    pub hypothesis: Vec<String>,
    pub committed: bool,
    /// Post-enforcement peak pool sizes over the feed.
    pub peak_live: usize,
    pub peak_dormant: usize,
    /// Post-enforcement peak live+dormant total over the feed.
    pub peak_total: usize,
    /// Labels that crossed dormant -> live after a later arrival.
    pub revivals: Vec<String>,
    /// Labels beam-pruned below `theta_live` (arm B only), in event order.
    pub pruned: Vec<String>,
    /// Labels evicted under budget exhaustion, in event order.
    pub evicted: Vec<String>,
    pub elapsed_sec: f64,
    /// Lattice span in seconds (frames x `frame_shift_ms`).
    pub span_sec: f64,
}

impl ArmOutcome {
    /// Wall time over lattice span (fixture stand-in for audio RTF).
    pub fn rtf(&self) -> f64 {
        if self.span_sec > 0.0 {
            self.elapsed_sec / self.span_sec
        } else {
            0.0
        }
    }
}

/// Run arm B or C over one lattice.
///
/// `lm(a, b)` supplies `llr_SEQ(a -> b)` over word labels; `("<s>", w)`
/// supplies utterance-start boundary pairs. Non-forward or dangling arcs
/// are skipped (the oracle applies the same forward guard).
pub fn run_fabric_arm(
    lat: &Lattice,
    arm: Arm,
    params: &ArmParams,
    lm: &dyn Fn(&str, &str) -> f64,
) -> Result<ArmOutcome, String> {
    assert!(
        arm == Arm::B || arm == Arm::C,
        "arm A is the 1-best baseline; it never reaches the fabric"
    );
    let t0 = Instant::now();
    let mut d = DecisionFabric::new(params.decision_params(arm));
    let mut order: Vec<usize> = (0..lat.tokens.len()).collect();
    order.sort_by(|&a, &b| {
        (lat.tokens[a].start, lat.tokens[a].end, lat.tokens[a].id)
            .cmp(&(lat.tokens[b].start, lat.tokens[b].end, lat.tokens[b].id))
    });
    let mut idx_of: HashMap<u64, usize> = HashMap::new();
    for (i, t) in lat.tokens.iter().enumerate() {
        idx_of.insert(t.id, i);
    }
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); lat.tokens.len()];
    for a in &lat.arcs {
        if let (Some(&u), Some(&v)) = (idx_of.get(&a.from), idx_of.get(&a.to)) {
            incoming[v].push(u);
        }
    }
    let mut fact_of: HashMap<usize, FactId> = HashMap::new();
    let empty = BTreeSet::new();
    let mut peak_live = 0usize;
    let mut peak_dormant = 0usize;
    let mut peak_total = 0usize;
    let mut revived: HashSet<FactId> = HashSet::new();
    for &i in &order {
        let tok = &lat.tokens[i];
        let fact = d
            .mint_token(tok.word.clone(), TokenKind::Word, tok.start, tok.end)
            .map_err(|e| format!("mint token {}: {e:?}", tok.id))?;
        fact_of.insert(i, fact);
        d.add_unary(
            fact,
            SourceClass::Acoustic,
            tok.acoustic_llr,
            empty.clone(),
        )
        .map_err(|e| format!("acoustic token {}: {e:?}", tok.id))?;
        if incoming[i].is_empty() {
            d.add_boundary_sequence(fact, lm("<s>", &tok.word))
                .map_err(|e| format!("boundary token {}: {e:?}", tok.id))?;
        }
        let was_dormant: HashSet<FactId> = d.dormant_tokens().into_iter().collect();
        for &u in &incoming[i] {
            let Some(&from_fact) = fact_of.get(&u) else {
                continue;
            };
            let mut premises = BTreeSet::new();
            premises.insert(from_fact);
            premises.insert(fact);
            d.add_sequence(from_fact, fact, lm(&lat.tokens[u].word, &tok.word), premises)
                .map_err(|e| {
                    format!("sequence {} -> {}: {e:?}", lat.tokens[u].id, tok.id)
                })?;
        }
        d.attach_context_for_new_token(fact, &|a, b| lm(&a.label, &b.label));
        d.enforce_budgets();
        for t in d.live_tokens() {
            if t != fact && was_dormant.contains(&t) {
                revived.insert(t);
            }
        }
        let (nl, nd) = (d.live_tokens().len(), d.dormant_tokens().len());
        peak_live = peak_live.max(nl);
        peak_dormant = peak_dormant.max(nd);
        peak_total = peak_total.max(nl + nd);
    }
    d.enforce_budgets();
    let label = |f: FactId| -> String {
        d.token(f)
            .map(|t| t.label.clone())
            .unwrap_or_else(|| "<unknown>".to_string())
    };
    let mut revivals: Vec<String> = revived.iter().map(|&f| label(f)).collect();
    revivals.sort();
    let mut pruned: Vec<String> = Vec::new();
    let mut evicted: Vec<String> = Vec::new();
    for e in d.events() {
        match e {
            DecisionEvent::Pruned { token, .. } => pruned.push(label(*token)),
            DecisionEvent::Evicted { token, .. } => evicted.push(label(*token)),
            _ => {}
        }
    }
    let winner_facts: Option<Vec<FactId>> = if lat.tokens.is_empty() {
        None
    } else {
        let s = lat.tokens.iter().map(|t| t.start).min().unwrap_or(0);
        let e = lat.tokens.iter().map(|t| t.end).max().unwrap_or(0);
        match d.commit_region((s, e), e.saturating_add(params.h)) {
            CommitOutcome::Committed { winner, .. } => Some(winner.tokens),
            _ => None,
        }
    };
    // NB: `label` is not used past the event loop, so its borrow has ended
    // before the commit above; the lookup below re-borrows immutably.
    let (hypothesis, committed) = match winner_facts {
        Some(fs) => (
            fs.iter()
                .map(|&f| {
                    d.token(f)
                        .map(|t| t.label.clone())
                        .unwrap_or_else(|| "<unknown>".to_string())
                })
                .collect(),
            true,
        ),
        None => (Vec::new(), false),
    };
    let elapsed_sec = t0.elapsed().as_secs_f64();
    let span_frames = lat
        .tokens
        .iter()
        .map(|t| t.end)
        .max()
        .unwrap_or(0)
        .saturating_sub(lat.tokens.iter().map(|t| t.start).min().unwrap_or(0));
    let span_sec = span_frames as f64 * lat.frame_shift_ms / 1000.0;
    Ok(ArmOutcome {
        hypothesis,
        committed,
        peak_live,
        peak_dormant,
        peak_total,
        revivals,
        pruned,
        evicted,
        elapsed_sec,
        span_sec,
    })
}
