//! Multi-support justification sets with survival semantics — the D-1 fix.
//!
//! Implements the design in `docs/design/retraction-survival-semantics.md`
//! (bead aem.13). This is the load-bearing mechanism that fixes the D-1
//! defect present in dzero and MikeDB fact stores: facts were *overwritten*
//! on second derivation, losing prior justification sets. Here a fact holds
//! a *set* of justification sets and tracks a support count, so a fact that
//! loses one justification survives if it retains another independent one.
//!
//! Core rule (invariant `fact_active_iff_supported`):
//! > A fact is active iff it has at least one active justification set.

use invariant_ppt::assert_invariant;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

pub mod provenance;
pub mod stats;
/// A fact identifier. Facts are hash-consed to a single id per content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId(pub u64);

/// A node in the justification graph. A justification set is a set of
/// premise fact ids that jointly entail a conclusion fact id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Justification {
    /// The conclusion fact this justification entails.
    pub conclusion: FactId,
    /// The premise fact ids (active iff all premises active).
    pub premises: BTreeSet<FactId>,
    /// Unconditional justifications never have a failing premise
    /// (every premise is keyed solely on the conclusion's own nodes and the
    /// arena is append-only, so they cannot change). See aem.13 §3.2.
    pub unconditional: bool,
}

/// A fact's runtime state in the fabric.
#[derive(Debug, Clone)]
struct FactState {
    /// All justification sets that entail this fact (never deleted on
    /// retraction — provenance preserved, invariant `retraction_preserves`).
    justifications: Vec<Justification>,
    /// Whether the fact is currently active.
    active: bool,
}

impl FactState {
    fn new() -> Self {
        FactState {
            justifications: Vec::new(),
            active: false,
        }
    }

    /// Number of justification sets whose premises are all active.
    fn support_count(&self, fabric: &Fabric) -> usize {
        self.justifications
            .iter()
            .filter(|j| fabric.is_justification_active(j))
            .count()
    }
}

/// The saturation-fabric fact store (D-1 fixed).
pub struct Fabric {
    facts: BTreeMap<FactId, FactState>,
    next_id: AtomicU64,
    /// Facts that were *explicitly* retracted (aem.13: a deliberate local
    /// decision, e.g. drift retiring the old calibration). `reactivate_ready`
    /// must NOT resurrect these merely because an unrelated assert ran — only
    /// a fresh `assert` targeting them re-activates. Without this, an
    /// ATMS-pure fabric would instantly revive any explicitly retired fact.
    deliberately_retracted: std::collections::BTreeSet<FactId>,
}

impl Default for Fabric {
    fn default() -> Self {
        Self::new()
    }
}

impl Fabric {
    pub fn new() -> Self {
        Fabric {
            facts: BTreeMap::new(),
            next_id: AtomicU64::new(1),
            deliberately_retracted: std::collections::BTreeSet::new(),
        }
    }

    /// Intern a fact, returning its id (hash-consing: same content, same id).
    pub fn intern(&mut self) -> FactId {
        let id = FactId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.facts.entry(id).or_insert_with(FactState::new);
        id
    }

    /// Assert a justification `premises ⟹ conclusion`.
    ///
    /// This is the core of the D-1 fix: rather than *overwriting* the
    /// conclusion's prior justification, this *adds* a new justification set
    /// to it. If the new set's premises are all active and the conclusion was
    /// inactive, the conclusion reactivates (survival across re-assertion).
    ///
    /// Subsumption-based minimality (6za.1): the justification list for a
    /// conclusion is kept minimal under the subset order. A set J2 subsumed
    /// by J1 (`J1.premises ⊆ J2.premises`) is inert — whenever J2 is active,
    /// J1 is too, so J2 can never be the deciding support. Subsumed sets are
    /// pruned at assert time (normalization); retraction still never deletes
    /// anything (`retraction_preserves`).
    pub fn assert(&mut self, conclusion: FactId, premises: BTreeSet<FactId>, unconditional: bool) {
        let j = Justification {
            conclusion,
            premises,
            unconditional,
        };
        // A fresh direct assertion re-arms a deliberately-retracted fact:
        // reactivation is now permitted again (aem.13 §4).
        self.deliberately_retracted.remove(&conclusion);
        {
            let state = self
                .facts
                .get_mut(&conclusion)
                .expect("fact must be interned");
            // Exact duplicate -> no-op.
            if state.justifications.contains(&j) {
                return;
            }
            // Dominated by an existing set with a premise-subset of ours ->
            // the new set is subsumed and can never decide support. Skip it.
            if state
                .justifications
                .iter()
                .any(|existing| existing.premises.is_subset(&j.premises))
            {
                return;
            }
            // Our set strictly dominates some existing sets -> prune them.
            state
                .justifications
                .retain(|existing| !j.premises.is_subset(&existing.premises));
            state.justifications.push(j.clone());
        }
        self.assert_minimal_invariant(conclusion);

        // If the new set is fully active and the conclusion was inactive,
        // reactivate it (survival semantics).
        let active = self.is_justification_active(&j);
        let was_active = self.is_active(conclusion);
        if active && !was_active {
            self.set_active(conclusion, true);
        }

        // Reactivation cascade: any inactive fact whose justifications are
        // now all-active (because a premise came online) reactivates too.
        self.reactivate_ready();
    }

    /// Reactivate any inactive fact that now has an active justification set.
    /// Runs after assertions that bring premises online.
    ///
    /// Facts that were *deliberately retracted* are skipped: only a fresh
    /// `assert` targeting them may re-activate (aem.13 §4 — reactivation is
    /// explicit, not a side effect of unrelated assertions).
    fn reactivate_ready(&mut self) {
        let ready: Vec<FactId> = self
            .facts
            .iter()
            .filter(|(id, s)| {
                !s.active && s.support_count(self) > 0 && !self.deliberately_retracted.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ready {
            self.set_active(id, true);
        }
    }

    /// Assert a justification with no premises — an assumption/axiom.
    /// Assumptions are always active (they have no premises to fail).
    pub fn assert_assumption(&mut self, conclusion: FactId) {
        self.assert(conclusion, BTreeSet::new(), true);
    }

    /// Retract a fact: mark it inactive and cascade to dependents.
    ///
    /// A fact is retracted iff it loses its *last* active justification set.
    /// If it retains another independent active justification set, it
    /// survives (invariant `survival_requires_independent_support`).
    ///
    /// The `fact_active_iff_supported` invariant is checked once after the
    /// ENTIRE cascade. Checking inside the recursive calls spuriously fires
    /// while sibling dependents are still active-but-unsupported mid-walk.
    pub fn retract(&mut self, fact: FactId) {
        // Deliberate retraction: this explicit call must stick until the fact
        // is re-asserted (aem.13 §4). Motivated dependents are NOT marked —
        // they may legitimately revive if an earlier premise comes back.
        self.deliberately_retracted.insert(fact);
        if !self.retract_inner(fact) {
            return;
        }
        // Global survival invariant (`fact_active_iff_supported`): after any
        // retraction, every fact still active must retain at least one active
        // justification set. A fact is never active while unsupported.
        let violated = self
            .facts
            .iter()
            .any(|(_, s)| s.active && s.support_count(self) == 0);
        assert_invariant(!violated, "fact_active_iff_supported", Some("fabric"));
    }

    /// Deactivate `fact` and cascade to dependents. Returns `true` if
    /// anything was deactivated (the public [`retract`] then runs the global
    /// invariant exactly once). Recursion never re-checks the global
    /// invariant — intermediate states may legitimately contain a dependent
    /// not yet cascaded.
    fn retract_inner(&mut self, fact: FactId) -> bool {
        let was_active = self.facts.get(&fact).map(|s| s.active).unwrap_or(false);
        if !was_active {
            return false;
        }
        self.set_active(fact, false);
        // Cascade: any active conclusion that references `fact` as a premise
        // and has now lost its last active justification set must be retracted.
        let dependents: Vec<FactId> = self
            .facts
            .iter()
            .filter(|(_, s)| {
                s.active
                    && s.justifications.iter().any(|j| j.premises.contains(&fact))
                    && s.support_count(self) == 0
            })
            .map(|(id, _)| *id)
            .collect();
        for d in dependents {
            self.retract_inner(d);
        }
        true
    }

    pub(crate) fn is_justification_active(&self, j: &Justification) -> bool {
        if j.unconditional && j.premises.is_empty() {
            return true; // assumptions are always active
        }
        j.premises
            .iter()
            .all(|p| self.facts.get(p).map(|s| s.active).unwrap_or(false))
    }

    /// Subsumption-minimality invariant (`justification_set_minimal`): no two
    /// justification sets of one conclusion may be comparable under the
    /// premise-subset order. Checked after every mutation of a conclusion's
    /// justification list.
    fn assert_minimal_invariant(&self, conclusion: FactId) {
        let js: &[Justification] = self
            .facts
            .get(&conclusion)
            .map(|s| s.justifications.as_slice())
            .unwrap_or(&[]);
        let violated = js.iter().enumerate().any(|(i, a)| {
            js[i + 1..]
                .iter()
                .any(|b| a.premises.is_subset(&b.premises))
        });
        assert_invariant(!violated, "justification_set_minimal", Some("fabric"));
    }

    fn set_active(&mut self, fact: FactId, active: bool) {
        if let Some(s) = self.facts.get_mut(&fact) {
            s.active = active;
        }
    }

    /// Is the fact currently active?
    pub fn is_active(&self, fact: FactId) -> bool {
        self.facts.get(&fact).map(|s| s.active).unwrap_or(false)
    }

    /// Number of justification sets currently supporting `fact`.
    pub fn support_count(&self, fact: FactId) -> usize {
        self.facts
            .get(&fact)
            .map(|s| s.support_count(self))
            .unwrap_or(0)
    }

    /// The full set of justification sets for a fact (provenance preserved —
    /// never cleared on retraction).
    pub fn justifications(&self, fact: FactId) -> &[Justification] {
        self.facts
            .get(&fact)
            .map(|s| s.justifications.as_slice())
            .unwrap_or(&[])
    }

    /// Promote a fact to committed status. Per aem.13 §4, promotion marks the
    /// winning justification unconditional so it is stable thereafter.
    /// This is a no-op placeholder; in the real Stage 3 it marks the winning
    /// justification set unconditional.
    pub fn commit(&mut self, fact: FactId) {
        // Promotion: ensure at least one active justification, then mark the
        // first active one unconditional so it cannot be un-derived.
        let active_idx = {
            let s = self.facts.get(&fact);
            match s {
                None => return,
                Some(s) => s
                    .justifications
                    .iter()
                    .position(|j| self.is_justification_active(j)),
            }
        };
        if let Some(idx) = active_idx {
            let s = self.facts.get_mut(&fact);
            let j = s.and_then(|s| s.justifications.get_mut(idx));
            if let Some(j) = j {
                j.unconditional = true;
            }
        }
        // No-unsupported-promotion invariant: a committed fact must be active
        // (i.e. have at least one active justification set).
        assert_invariant(
            self.is_active(fact),
            "no_unsupported_promotion",
            Some("fabric"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn premises(ids: &[u64]) -> BTreeSet<FactId> {
        ids.iter().map(|&i| FactId(i)).collect()
    }

    #[test]
    fn assumption_is_active() {
        let mut f = Fabric::new();
        let a = f.intern();
        f.assert_assumption(a);
        assert!(f.is_active(a));
        assert_eq!(f.support_count(a), 1);
    }

    #[test]
    fn derived_fact_is_inactive_until_supported() {
        let mut f = Fabric::new();
        let premise = f.intern();
        let concl = f.intern();
        // conclusion has a justification but premise is inactive -> inactive
        f.assert(concl, premises(&[premise.0]), false);
        assert!(!f.is_active(concl));
        assert_eq!(f.support_count(concl), 0);

        // activate premise -> conclusion becomes active
        f.assert_assumption(premise);
        // re-assert to trigger reactivation check
        f.assert(concl, premises(&[premise.0]), false);
        assert!(f.is_active(concl));
        assert_eq!(f.support_count(concl), 1);
    }

    #[test]
    fn retraction_with_survival() {
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();

        f.assert_assumption(p1);
        f.assert_assumption(p2);
        // Two independent justification sets for concl.
        f.assert(concl, premises(&[p1.0]), false);
        f.assert(concl, premises(&[p2.0]), false);
        assert!(f.is_active(concl));
        assert_eq!(f.support_count(concl), 2);

        // Retract p1 -> concl survives via p2 (independent support).
        f.retract(p1);
        assert!(f.is_active(concl));
        assert_eq!(f.support_count(concl), 1);

        // Retract p2 -> concl loses last active justification -> retracted.
        f.retract(p2);
        assert!(!f.is_active(concl));
        assert_eq!(f.support_count(concl), 0);

        // Provenance preserved: justifications still present after retraction.
        assert_eq!(f.justifications(concl).len(), 2);
    }

    #[test]
    fn overlapping_support_does_not_survive() {
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let shared = f.intern();
        let concl = f.intern();

        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert_assumption(shared);
        // Both justifications share `shared` premise — not fully independent.
        f.assert(concl, premises(&[p1.0, shared.0]), false);
        f.assert(concl, premises(&[p2.0, shared.0]), false);
        assert!(f.is_active(concl));
        assert_eq!(f.support_count(concl), 2);

        // Retract shared -> BOTH justifications fail (shared premise) -> concl retracted.
        f.retract(shared);
        assert!(!f.is_active(concl));
        assert_eq!(f.support_count(concl), 0);
    }

    #[test]
    fn cascade_retraction() {
        let mut f = Fabric::new();
        let a = f.intern();
        let b = f.intern();
        let c = f.intern();

        f.assert_assumption(a);
        f.assert(b, premises(&[a.0]), false);
        f.assert(c, premises(&[b.0]), false);
        assert!(f.is_active(b));
        assert!(f.is_active(c));

        // Retract a -> b loses support -> b retracted -> c loses support -> c retracted.
        f.retract(a);
        assert!(!f.is_active(b));
        assert!(!f.is_active(c));
    }

    #[test]
    fn cascade_sibling_dependents_all_retracted() {
        // Regression: retracting a shared premise with MULTIPLE independent
        // dependents must deactivate all of them. The global invariant
        // `fact_active_iff_supported` is checked once after the whole cascade,
        // not mid-walk (previously it fired spuriously while a sibling
        // dependent was still active-but-unsupported).
        let mut f = Fabric::new();
        let shared = f.intern();
        let d1 = f.intern();
        let d2 = f.intern();
        let d3 = f.intern();

        f.assert_assumption(shared);
        f.assert(d1, premises(&[shared.0]), false);
        f.assert(d2, premises(&[shared.0]), false);
        f.assert(d3, premises(&[shared.0]), false);
        assert!(f.is_active(d1));
        assert!(f.is_active(d2));
        assert!(f.is_active(d3));
        assert_eq!(f.support_count(d1), 1);

        // Retracting the one shared premise cascades to all three dependents.
        f.retract(shared);
        assert!(!f.is_active(d1));
        assert!(!f.is_active(d2));
        assert!(!f.is_active(d3));
        // Provenance preserved for every dependent.
        assert_eq!(f.justifications(d1).len(), 1);
        assert_eq!(f.justifications(d2).len(), 1);
        assert_eq!(f.justifications(d3).len(), 1);
    }

    #[test]
    fn cascade_sibling_survivor_kept() {
        // A dependent with an independent second justification survives a
        // shared-premise retraction even when its siblings do not.
        let mut f = Fabric::new();
        let shared = f.intern();
        let independent = f.intern();
        let d_shared_only = f.intern();
        let d_survivor = f.intern();

        f.assert_assumption(shared);
        f.assert_assumption(independent);
        f.assert(d_shared_only, premises(&[shared.0]), false);
        // Survivor: {shared} AND {independent} — independent support.
        f.assert(d_survivor, premises(&[shared.0]), false);
        f.assert(d_survivor, premises(&[independent.0]), false);
        assert!(f.is_active(d_shared_only));
        assert!(f.is_active(d_survivor));
        assert_eq!(f.support_count(d_survivor), 2);

        f.retract(shared);
        assert!(
            !f.is_active(d_shared_only),
            "no independent support -> retracted"
        );
        assert!(f.is_active(d_survivor), "independent support -> survives");
        assert_eq!(f.support_count(d_survivor), 1);
    }

    #[test]
    fn commit_stabilizes() {
        let mut f = Fabric::new();
        let p = f.intern();
        let concl = f.intern();
        f.assert_assumption(p);
        f.assert(concl, premises(&[p.0]), false);
        f.commit(concl);
        // After commit the winning justification is unconditional.
        assert!(f.is_active(concl));
        // Unconditional premise-keyed justification cannot be retracted by
        // the premise failing (in a real arena the premise is append-only).
        let j = &f.justifications(concl)[0];
        assert!(j.unconditional);
    }

    #[test]
    fn deliberate_retraction_sticks_despite_surviving_premise() {
        // Regression (Stage 3 drift, 91x.3): a fact explicitly retracted must
        // NOT be auto-resurrected by `reactivate_ready` when an unrelated
        // assert later runs — even though its premise is still active. Only a
        // fresh direct assert may re-activate.
        let mut f = Fabric::new();
        let leaf = f.intern();
        f.assert_assumption(leaf);
        let retired = f.intern();
        f.assert(retired, premises(&[leaf.0]), false);
        assert!(f.is_active(retired), "active before retraction");

        f.retract(retired);
        assert!(!f.is_active(retired), "deliberately retracted");

        // Unrelated assert brings premises online — must NOT resurrect.
        let unrelated = f.intern();
        f.assert_assumption(unrelated);
        assert!(
            !f.is_active(retired),
            "deliberate retraction survives unrelated assert (reactivate_ready skips)"
        );

        // A fresh direct assertion (a NEW justification set — the drift
        // re-derivation) re-arms the fact.
        let new_premise = f.intern();
        f.assert_assumption(new_premise);
        f.assert(retired, premises(&[new_premise.0]), false);
        assert!(f.is_active(retired), "fresh justif re-assert revives");
    }

    #[test]
    fn subsumed_superset_not_stored() {
        // 6za.1: asserting a set dominated by an existing smaller set is a
        // no-op — {p1} already decides everything {p1,p2} could.
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();
        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert(concl, premises(&[p1.0]), false);
        f.assert(concl, premises(&[p1.0, p2.0]), false);
        assert_eq!(f.justifications(concl).len(), 1);
        assert_eq!(f.justifications(concl)[0].premises.len(), 1);
    }

    #[test]
    fn subsuming_assert_prunes_existing_superset() {
        // Asserting a strictly smaller set prunes the stored superset.
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();
        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert(concl, premises(&[p1.0, p2.0]), false);
        assert_eq!(f.justifications(concl).len(), 1);
        f.assert(concl, premises(&[p1.0]), false);
        assert_eq!(f.justifications(concl).len(), 1, "superset pruned");
        assert_eq!(f.justifications(concl)[0].premises.len(), 1);
    }

    #[test]
    fn disjoint_sets_both_kept() {
        // Guard against over-pruning: incomparable sets must all survive.
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();
        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert(concl, premises(&[p1.0]), false);
        f.assert(concl, premises(&[p2.0]), false);
        assert_eq!(f.justifications(concl).len(), 2);
        assert_eq!(f.support_count(concl), 2);
    }

    #[test]
    fn assumption_subsumes_conditional_derivations_of_same_fact() {
        // An empty-premise (axiom) justification is a subset of every other
        // premise set for the same conclusion — later conditional derivations
        // of an already-axiomatic fact are inert and are not stored.
        let mut f = Fabric::new();
        let p = f.intern();
        let concl = f.intern();
        f.assert_assumption(p);
        f.assert_assumption(concl);
        f.assert(concl, premises(&[p.0]), false);
        assert_eq!(f.justifications(concl).len(), 1);
        assert!(f.justifications(concl)[0].premises.is_empty());
        // Survival behavior unchanged by the pruning:
        f.retract(p);
        assert!(f.is_active(concl), "axiom keeps concl active regardless");
    }

    #[test]
    fn minimality_preserves_retraction_outcomes() {
        // Pruning is semantically inert: with {p1} kept and {p1,p2} pruned,
        // retracting p2 must NOT retract concl, and retracting p1 must.
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();
        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert(concl, premises(&[p1.0]), false);
        f.assert(concl, premises(&[p1.0, p2.0]), false);
        f.retract(p2);
        assert!(f.is_active(concl), "pruned set was not load-bearing");
        f.retract(p1);
        assert!(!f.is_active(concl), "minimal set was load-bearing");
    }
}
