//! Arm runners: A (1-best, no fabric), B (live prune), C (dormant).
//!
//! Arms B/C run through the real lattice-fabric bridge
//! ([`hearsay::decode_lattice`]): the bridge feeds lattice tokens into the
//! decision fabric, attaches sequence evidence from a
//! [`hearsay::SequenceScorer`], commits per contract section 10, and
//! returns the transcript plus the v1 JSONL event log. This module adapts
//! a plain bigram closure to the scorer trait, maps [`Arm`] to the native
//! [`hearsay::Arm`] flag, and derives revivals, prunes, evictions, and
//! pool peaks from the bridge outcome.
//!
//! Arm semantics (`hearsay-four-bpg2--fabric-prune-arm`, landed):
//!
//! - Arm B: a token below `theta_live` is beam-pruned at once, never
//!   revivable, never receiving later evidence. Budgets count live only.
//! - Arm C: below-threshold tokens linger dormant and revive when later
//!   sequence context pushes them over `theta_live`. Live plus dormant
//!   share the same combined total.
//!
//! Scoring, evidence, and the commit rule are identical in both arms; only
//! the below-threshold fate differs.

use fabric::decision::DecisionParams;
use hearsay::Arm as FabricArm;
use lattice::Lattice;
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
    pub j: u32,
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
            j: 3,
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
            j: self.j,
            ..DecisionParams::default()
        }
    }
}

/// Adapts a plain bigram closure to [`hearsay::SequenceScorer`].
/// `None` (utterance-start boundary) maps to the `"<s>"` predecessor.
struct ClosureScorer<'a> {
    lm: &'a dyn Fn(&str, &str) -> f64,
}

impl hearsay::SequenceScorer for ClosureScorer<'_> {
    fn sequence_llr(&self, prev: Option<&str>, word: &str) -> f64 {
        (self.lm)(prev.unwrap_or("<s>"), word)
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
    /// Proven-dormant ids (revived or dormant-evicted) on the oracle path.
    pub dormant_oracle_touch: usize,
    /// Revived ids that made a committed winner.
    pub revivals_in_winner: usize,
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

/// Run arm B or C over one lattice through the bridge.
///
/// `lm(a, b)` supplies `llr_SEQ(a -> b)` over word labels; `("<s>", w)`
/// supplies utterance-start boundary pairs. The bridge rejects lattices
/// with dangling arcs (returned as `Err`); an empty lattice commits
/// nothing without error.
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
    let span_frames = lat
        .tokens
        .iter()
        .map(|t| t.end)
        .max()
        .unwrap_or(0)
        .saturating_sub(lat.tokens.iter().map(|t| t.start).min().unwrap_or(0));
    let span_sec = span_frames as f64 * lat.frame_shift_ms / 1000.0;
    if lat.tokens.is_empty() {
        return Ok(ArmOutcome {
            hypothesis: Vec::new(),
            committed: false,
            peak_live: 0,
            peak_dormant: 0,
            peak_total: 0,
            revivals: Vec::new(),
            pruned: Vec::new(),
            evicted: Vec::new(),
            dormant_oracle_touch: 0,
            revivals_in_winner: 0,
            elapsed_sec: 0.0,
            span_sec,
        });
    }
    let scorer = ClosureScorer { lm };
    let t0 = Instant::now();
    let out = hearsay::decode_lattice(lat, &scorer, params.decision_params(arm))?;
    let elapsed_sec = t0.elapsed().as_secs_f64();
    let mut revivals = Vec::new();
    let mut pruned = Vec::new();
    let mut evicted = Vec::new();
    for line in out.events_jsonl.lines() {
        let v: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("bridge event not JSON: {e}"))?;
        let kind = v.get("event").and_then(|k| k.as_str()).unwrap_or("");
        let word = v
            .get("token")
            .and_then(|t| t.get("word"))
            .and_then(|w| w.as_str())
            .unwrap_or("")
            .to_string();
        match kind {
            "revived" => revivals.push(word),
            "pruned" => pruned.push(word),
            "evicted" => evicted.push(word),
            _ => {}
        }
    }
    revivals.sort();
    revivals.dedup();
    let oracle_ids = a2::oracle::acoustic_best_path_ids(lat);
    let (dormant_oracle_touch, revivals_in_winner) =
        commit_diagnostics(&out.events_jsonl, &oracle_ids);
    Ok(ArmOutcome {
        hypothesis: out.transcript,
        committed: !out.commits.is_empty(),
        peak_live: out.peak_live,
        peak_dormant: out.peak_dormant,
        peak_total: out.peak_total,
        revivals,
        pruned,
        evicted,
        dormant_oracle_touch,
        revivals_in_winner,
        elapsed_sec,
        span_sec,
    })
}
/// Headline-sweep diagnostics from one decode event log.
///
/// Returns `(dormant_oracle_touch, revivals_in_winner)`. The first counts
/// proven-dormant token ids (revived at some point, or evicted from the
/// dormant budget) lying on the oracle (acoustic-best) path; it is a LOWER
/// bound on dormant-at-commit oracle tokens because quiet-dormant tokens
/// (never revived/evicted) leave no event. The second counts revived ids
/// that made a committed winner. Malformed lines are skipped. Both are 0
/// for arm B.
pub fn commit_diagnostics(events_jsonl: &str, oracle_ids: &[u64]) -> (usize, usize) {
    use std::collections::BTreeSet;
    let oracle: BTreeSet<u64> = oracle_ids.iter().copied().collect();
    let mut revived: BTreeSet<u64> = BTreeSet::new();
    let mut dormant_evicted: BTreeSet<u64> = BTreeSet::new();
    let mut winners: BTreeSet<u64> = BTreeSet::new();
    for line in events_jsonl.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = v.get("event").and_then(|k| k.as_str()).unwrap_or("");
        let tid = v
            .get("token")
            .and_then(|t| t.get("id"))
            .and_then(|i| i.as_u64());
        match kind {
            "revived" => {
                if let Some(id) = tid {
                    revived.insert(id);
                }
            }
            "evicted" => {
                let dormant = v
                    .get("budget")
                    .and_then(|b| b.as_str())
                    .is_some_and(|b| b == "dormant");
                if let Some(id) = tid.filter(|_| dormant) {
                    dormant_evicted.insert(id);
                }
            }
            "committed" => {
                if let Some(ws) = v.get("winner").and_then(|w| w.as_array()) {
                    for w in ws {
                        if let Some(id) = w.get("id").and_then(|i| i.as_u64()) {
                            winners.insert(id);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let touch = revived
        .union(&dormant_evicted)
        .filter(|id| oracle.contains(id))
        .count();
    let changed = revived.intersection(&winners).count();
    (touch, changed)
}

/// Demo driver (bead `hearsay-four-4lsp--fabric-path-demo`): arm-C decode
/// of one lattice with the fixture scorer, returning the full bridge
/// outcome. Same [`hearsay::decode_lattice`] call as [`run_fabric_arm`]
/// with [`ArmParams::fixture_defaults`]; the [`ArmOutcome`] summary drops
/// the event log, so the demo needs this fuller return to write the bridge
/// handoff (transcript + v1 event JSONL).
pub fn decode_demo_lattice(lat: &Lattice, budget: usize) -> Result<hearsay::BridgeOutcome, String> {
    let params = ArmParams::fixture_defaults(budget);
    let scorer = ClosureScorer {
        lm: &crate::fixtures::fixture_bigram,
    };
    hearsay::decode_lattice(lat, &scorer, params.decision_params(Arm::C))
}
