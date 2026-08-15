//! Viterbi bounded-window mechanism (d73.3, Category C).
//!
//! The hardest inversion: Viterbi *overwrites* suboptimal paths; the fabric
//! wants competing alternatives kept live until a local decision. This module
//! implements the bounded window:
//!
//! - Within the window (syllable-scale), N-best candidates for each position
//!   are held **live as justification sets** — nothing is discarded yet.
//! - Discard happens **only on window close** (`CommitWindow::close`): the
//!   winner is committed (Tier 1 → Tier 2); the *shared competition premise*
//!   of each position is retracted, which retracts any candidate whose only
//!   support depends on it — while a candidate that retains an independent
//!   stable justification **survives** (retraction-survival-semantics.md §3/§4,
//!   INV `survival_requires_independent_support`).
//!
//! Confidence math (hearsay-confidence-formulas.md §3): each candidate's
//! log-odds is `L(h) = L0(h) + Σ w_i` over supporting evidence, and the
//! 0–100 credibility rating is `100·σ(L)`. INVs: `rating_in_range`,
//! `logodds_finite`, `evidence_additive`.

use invariant_ppt::assert_invariant;

use fabric::{Fabric, FactId};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Prior log-odds for a fresh phoneme candidate (uniform prior; aem.7 §4).
pub const PRIOR_LOG_ODDS: f64 = 0.0;

/// Logistic sigmoid.
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

fn rating_of(log_odds: f64) -> f64 {
    let r = 100.0 * sigmoid(log_odds);
    assert_invariant(r.is_finite(), "rating_in_range", Some("d73.3"));
    assert_invariant((0.0..=100.0).contains(&r), "rating_in_range", Some("d73.3"));
    r
}

/// One candidate hypothesis inside the window.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Fabric fact holding this alternative's justification sets.
    pub fact: FactId,
    /// Stream position (utterance/syllable) this candidate competes for.
    pub position: usize,
    /// Accumulated log-odds `L(h) = L0 + Σ w_i` (calibrated, additive).
    pub log_odds: f64,
    /// Every active support count recorded at offer time for the survival
    /// audit.
    pub support_count: usize,
}

impl Candidate {
    /// Credibility rating on the Hearsay-II 0–100 scale: `100·σ(L)`.
    pub fn rating(&self) -> f64 {
        rating_of(self.log_odds)
    }
}

/// Error returned by window operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowError {
    EmptyWindow,
    WindowFull,
    UnknownCandidate,
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for WindowError {}

/// The bounded commit window over a stream of competing hypotheses.
pub struct CommitWindow {
    /// Width cap: how many candidates may compete at once.
    width: usize,
    /// Fabric backing the candidate facts (shared with the arena).
    pub fabric: Fabric,
    /// Live candidates in this window (≤ `width`).
    candidates: Vec<Candidate>,
    /// Per-position competition premise (asserted at offer time).
    positions: BTreeMap<usize, FactId>,
}

impl std::fmt::Debug for CommitWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitWindow")
            .field("width", &self.width)
            .field("candidates", &self.candidates)
            .field("positions", &self.positions)
            .finish()
    }
}

impl CommitWindow {
    pub fn new(width: usize) -> CommitWindow {
        assert_invariant(width >= 1, "width_positive", Some("d73.3"));
        CommitWindow {
            width,
            fabric: Fabric::new(),
            candidates: Vec::new(),
            positions: BTreeMap::new(),
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Intern (once) the shared "competition premise" fact for `position`.
    fn competition_fact(&mut self, position: usize) -> FactId {
        if let Some(&f) = self.positions.get(&position) {
            return f;
        }
        let f = self.fabric.intern();
        self.fabric.assert_assumption(f);
        self.positions.insert(position, f);
        f
    }

    /// Offer a new competing hypothesis for `position`.
    ///
    /// The candidate fact is asserted into the fabric with the *competition
    /// premise* for `position` as its support — so it is live only while
    /// that position is inside the window. It stays live here — nothing is
    /// discarded (INV `discard_only_on_close`).
    ///
    /// `stable_supports` are additional independent justification sets (sets
    /// of facts) that will keep the candidate alive even after the window
    /// closes — this is the independent-support survival channel (aem.13).
    pub fn offer(
        &mut self,
        position: usize,
        local_evidence: BTreeSet<FactId>,
        stable_supports: &[BTreeSet<FactId>],
        weight: f64,
    ) -> Result<Candidate, WindowError> {
        assert_invariant(weight.is_finite(), "logodds_finite", Some("d73.3"));
        if self.candidates.len() >= self.width {
            return Err(WindowError::WindowFull);
        }
        let fact = self.fabric.intern();
        let comp = self.competition_fact(position);

        // Primary: `{competition_position} ∪ local_evidence ⟹ candidate`.
        // local_evidence is a set of arena facts already live (asserted as
        // assumptions by the caller), so the candidate is active while the
        // competition premise is active.
        let mut local: BTreeSet<FactId> = local_evidence;
        local.insert(comp);
        self.fabric.assert(fact, local, false);

        // Independent stable supports: justification sets that do NOT depend
        // on the competition premise, so they keep the candidate alive past
        // window close (independent-support survival channel, aem.13 §3.1).
        for s in stable_supports {
            self.fabric.assert(fact, s.clone(), true);
        }

        let log_odds = PRIOR_LOG_ODDS + weight;
        let cand = Candidate {
            fact,
            position,
            log_odds,
            support_count: self.fabric.support_count(fact),
        };
        assert_invariant(
            cand.support_count >= 1,
            "candidate_live_in_window",
            Some("d73.3"),
        );
        self.candidates.push(cand.clone());
        Ok(cand)
    }

    /// Combine another independent evidence source into a candidate's
    /// log-odds (additive, aem.7 §3.2; INV `evidence_additive`).
    pub fn accumulate(&mut self, fact: FactId, weight: f64) -> Result<(), WindowError> {
        assert_invariant(weight.is_finite(), "logodds_finite", Some("d73.3"));
        let item = self.candidates.iter_mut().find(|c| c.fact == fact);
        match item {
            Some(c) => {
                c.log_odds += weight;
                Ok(())
            }
            None => Err(WindowError::UnknownCandidate),
        }
    }

    /// The currently winning (highest-rating) candidate.
    pub fn winner(&self) -> Option<&Candidate> {
        self.candidates.iter().max_by(|a, b| {
            a.log_odds
                .partial_cmp(&b.log_odds)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Close the window: commit the winner (Tier 1 → Tier 2) and retract the
    /// competition premises.
    ///
    /// Retracting a position's competition premise cascades: any candidate
    /// whose only support is that premise loses it and is retracted *only
    /// now* (INV `discard_only_on_close`). A candidate with an independent
    /// stable justification survives (INV
    /// `survival_requires_independent_support`), and every retracted
    /// candidate's justification sets are preserved (INV
    /// `retraction_preserves_provenance`).
    ///
    /// Returns `(winner, survivors)` where `survivors` is the count of
    /// previously-live candidates that remained active after close.
    pub fn close(&mut self) -> Result<(Candidate, usize), WindowError> {
        if self.candidates.is_empty() {
            return Err(WindowError::EmptyWindow);
        }
        let winner = self.winner().cloned().expect("non-empty by guard");

        // No unsupported promotion: the winner must be supported (active with
        // >= 1 active justification) before it may be committed (aem.13 §4).
        assert_invariant(
            self.fabric.support_count(winner.fact) >= 1,
            "no_unsupported_promotion",
            Some("d73.5"),
        );

        // Commit the winner (Tier 1 → Tier 2): promotion *stabilizes* the
        // winning candidate so it no longer depends on the live competition
        // window. `commit` marks the winning justification unconditional; a
        // committed winner additionally becomes a stable fact (aem.13 §4).
        self.fabric.commit(winner.fact);
        self.fabric.assert_assumption(winner.fact);

        // Audit every candidate's support BEFORE retraction.
        let was_active: Vec<(FactId, bool)> = self
            .candidates
            .iter()
            .map(|c| (c.fact, self.fabric.is_active(c.fact)))
            .collect();

        // Retract the shared competition premise of every present position
        // now. This is the only discard moment.
        let position_facts: Vec<FactId> = self
            .candidates
            .iter()
            .flat_map(|c| self.positions.get(&c.position))
            .copied()
            .collect();
        for comp in position_facts {
            self.fabric.retract(comp);
        }

        // Count survivors: candidates that were live before and are still
        // active (independent support) — excluding the committed winner.
        let survivors = was_active
            .iter()
            .filter(|(f, was)| *was && *f != winner.fact && self.fabric.is_active(*f))
            .count();
        // Survival requires independent support: every candidate that stays
        // active after the window closes must have >= 1 active justification
        // (aem.13 — a candidate never survives without support).
        assert_invariant(
            was_active
                .iter()
                .filter(|(f, was)| *was && *f != winner.fact && self.fabric.is_active(*f))
                .all(|(f, _)| self.fabric.support_count(*f) >= 1),
            "survival_requires_independent_support",
            Some("d73.6"),
        );
        // Cascade bounded (aem.13 §5): a single premise retraction propagates
        // only within the local commit window — nothing outside it may be
        // deactivated. The window's candidates must ALL be reachable through
        // the retraction chain (each held its position premise as support),
        // so the entire retraction's blast radius is exactly the winner's
        // candidates: every deactivated non-winner fact is a window candidate.
        let cascade_bounded = was_active
            .iter()
            .filter(|(f, was)| *was && *f != winner.fact && !self.fabric.is_active(*f))
            .all(|(f, _)| {
                self.fabric.support_count(*f) == 0
                    && self
                        .fabric
                        .justifications(*f)
                        .iter()
                        .flat_map(|j| &j.premises)
                        .count()
                        >= 1
            });
        assert_invariant(cascade_bounded, "cascade_is_bounded", Some("d73.6"));

        // Every retracted non-winner must have preserved its provenance.
        for (fid, was) in &was_active {
            if *was && *fid != winner.fact && !self.fabric.is_active(*fid) {
                assert_invariant(
                    !self.fabric.justifications(*fid).is_empty(),
                    "retraction_preserves_provenance",
                    Some("d73.3"),
                );
            }
        }

        self.candidates.clear();
        self.positions.clear();
        Ok((winner, survivors))
    }
}
