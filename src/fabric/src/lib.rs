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

pub mod decision;
pub mod provenance;
pub mod stats;
pub mod token;

pub use token::{PhoneToken, SilToken, TokenFact, WordToken};
/// A fact identifier. Facts are hash-consed to a single id per content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId(pub u64);

/// A node in the justification graph. A justification set is a set of
/// premise fact ids that jointly entail a conclusion fact id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Justification {
    /// The conclusion fact this justification entails.
    pub conclusion: FactId,
    /// Premise fact ids in derivation order, with multiplicity preserved
    /// (A3): a token premise used twice appears twice, in place. Support
    /// and subsumption compare the underlying set via `premise_set()`.
    pub premises: Vec<FactId>,
    /// Unconditional justifications never have a failing premise
    /// (every premise is keyed solely on the conclusion's own nodes and the
    /// arena is append-only, so they cannot change). See aem.13 §3.2.
    pub unconditional: bool,
}

impl Justification {
    /// Set view of the premises, for support/subsumption comparisons.
    pub fn premise_set(&self) -> BTreeSet<FactId> {
        self.premises.iter().copied().collect()
    }
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
    /// decision, e.g. drift retiring the old calibration). Reactivation
    /// must NOT resurrect these merely because an unrelated assert ran — only
    /// a fresh `assert` targeting them re-activates. Without this, an
    /// ATMS-pure fabric would instantly revive any explicitly retired fact.
    deliberately_retracted: std::collections::BTreeSet<FactId>,
    /// Reverse premise index (PERF-1, bead hearsay-four-gmp): premise fact
    /// id -> conclusions whose justification lists mention it. Maintained
    /// incrementally on every `assert` (edges only ever added for the new
    /// justification; subsumption-pruned sets may leave stale edges, which
    /// are harmless — every candidate is re-validated by `support_count`
    /// before any activation change). Lets activation propagate and
    /// retraction cascade touch only the affected subgraph instead of
    /// scanning all facts on every op.
    dependents: BTreeMap<FactId, BTreeSet<FactId>>,
    /// Hash-cons table for type facts (A3): content -> the one FactId.
    type_cons: BTreeMap<String, FactId>,
    /// Token payloads by fact id (A3): never hash-consed, one entry per
    /// occurrence.
    tokens: BTreeMap<FactId, TokenFact>,
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
            dependents: BTreeMap::new(),
            type_cons: BTreeMap::new(),
            tokens: BTreeMap::new(),
        }
    }

    /// Intern a type fact by content (A3): hash-consed globally, so the
    /// same content (`WORD_TYPE`, `PRONUNCIATION`, n-gram entry, template
    /// id, model parameter) always returns the one existing `FactId`.
    pub fn intern_type(&mut self, content: impl Into<String>) -> FactId {
        let content = content.into();
        if let Some(&id) = self.type_cons.get(&content) {
            return id;
        }
        let id = self.intern();
        self.type_cons.insert(content, id);
        id
    }

    /// The type content for a hash-consed type fact, if `id` is one.
    pub fn type_content(&self, id: FactId) -> Option<&str> {
        self.type_cons
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.as_str())
    }

    fn intern_token(&mut self, token: TokenFact) -> FactId {
        // Never hash-consed: every occurrence gets a fresh id, even for
        // byte-identical payload and span.
        let id = self.intern();
        self.tokens.insert(id, token);
        id
    }

    /// Intern a `WORD_TOKEN` occurrence (A3): always a distinct fact.
    pub fn intern_word_token(
        &mut self,
        word: impl Into<String>,
        start_frame: u64,
        end_frame: u64,
        source: impl Into<String>,
    ) -> FactId {
        self.intern_token(TokenFact::Word(WordToken {
            word: word.into(),
            start_frame,
            end_frame,
            source: source.into(),
        }))
    }

    /// Intern a `PHONE_TOKEN` occurrence (A3): always a distinct fact.
    pub fn intern_phone_token(
        &mut self,
        phone: impl Into<String>,
        start_frame: u64,
        end_frame: u64,
    ) -> FactId {
        self.intern_token(TokenFact::Phone(PhoneToken {
            phone: phone.into(),
            start_frame,
            end_frame,
        }))
    }

    /// Intern a `SIL_TOKEN` occurrence (A3): always a distinct fact.
    pub fn intern_sil_token(&mut self, start_frame: u64, end_frame: u64) -> FactId {
        self.intern_token(TokenFact::Silence(SilToken {
            start_frame,
            end_frame,
        }))
    }

    /// Token payload for `id`, if it is a token fact.
    pub fn token_fact(&self, id: FactId) -> Option<&TokenFact> {
        self.tokens.get(&id)
    }

    /// Whether `id` is a token (occurrence) fact rather than a type fact.
    pub fn is_token(&self, id: FactId) -> bool {
        self.tokens.contains_key(&id)
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
        // Set-based callers keep sorted order; ordered/token derivations
        // use `assert_ordered` to preserve order and multiplicity.
        self.assert_ordered(conclusion, premises.into_iter().collect(), unconditional);
    }

    /// Assert with premises in derivation order, multiplicity preserved
    /// (A3). Subsumption compares premise sets, but set-equal justifications
    /// in different orders are distinct provenance and both are kept.
    pub fn assert_ordered(
        &mut self,
        conclusion: FactId,
        premises: Vec<FactId>,
        unconditional: bool,
    ) {
        let j = Justification {
            conclusion,
            premises,
            unconditional,
        };
        // A fresh direct assertion re-arms a deliberately-retracted fact:
        // reactivation is now permitted again (aem.13 §4).
        self.deliberately_retracted.remove(&conclusion);
        let added = {
            let state = self
                .facts
                .get_mut(&conclusion)
                .expect("fact must be interned");
            // Exact duplicate, or dominated by an existing set with a
            // premise-subset of ours (subsumed, can never decide support)
            // -> justification list unchanged.
            let j_set = j.premise_set();
            if state.justifications.contains(&j)
                || state.justifications.iter().any(|existing| {
                    let e_set = existing.premise_set();
                    e_set != j_set && e_set.is_subset(&j_set)
                })
            {
                false
            } else {
                // Our set strictly dominates some existing sets -> prune them.
                state.justifications.retain(|existing| {
                    let e_set = existing.premise_set();
                    !(e_set != j_set && j_set.is_subset(&e_set))
                });
                state.justifications.push(j.clone());
                true
            }
        };
        if added {
            // Incremental reverse-index maintenance for the new justification.
            for p in &j.premises {
                self.dependents.entry(*p).or_default().insert(conclusion);
            }
            self.assert_minimal_invariant(conclusion);
        }

        // Re-activation (survival semantics): a fresh `assert` targeting an
        // inactive conclusion re-arms it whenever ANY justification set
        // (new or pre-existing) is fully active — then propagation revives
        // dependents transitively. This covers the duplicate re-assertion of
        // an evicted assumption (streaming re-arm): the old full-scan
        // `reactivate_ready` resurrected such facts on the next unrelated
        // assert; the incremental fabric reactivates them here, at the
        // assert that re-arms them (assumptions are always active — they
        // have no premises to fail).
        if !self.is_active(conclusion) && self.support_count(conclusion) > 0 {
            self.set_active(conclusion, true);
            self.propagate_activation(conclusion);
        }
    }

    /// Reactivate the dependent subgraph newly supported by `newly_active`.
    ///
    /// Incremental replacement for the old full-fact `reactivate_ready`
    /// scan (PERF-1): a fact's support can only have changed if one of its
    /// premises just activated, so walking the reverse premise index from
    /// each newly activated fact visits exactly the candidates. Transitive
    /// closure (the old single-pass scan could strand deep chains — a
    /// dependent of a dependent reactivated in the same pass stayed
    /// inactive until some later unrelated assert re-scanned).
    ///
    /// Facts that were *deliberately retracted* are skipped: only a fresh
    /// `assert` targeting them may re-activate (aem.13 §4 — reactivation is
    /// explicit, not a side effect of unrelated assertions).
    fn propagate_activation(&mut self, newly_active: FactId) {
        let mut queue = vec![newly_active];
        while let Some(fid) = queue.pop() {
            let dependents: Vec<FactId> = self
                .dependents
                .get(&fid)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            for d in dependents {
                if self.deliberately_retracted.contains(&d) || self.is_active(d) {
                    continue;
                }
                if self.support_count(d) > 0 {
                    self.set_active(d, true);
                    queue.push(d);
                }
            }
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
    /// ENTIRE cascade. Checking inside the cascade spuriously fires
    /// while sibling dependents are still active-but-unsupported mid-walk.
    pub fn retract(&mut self, fact: FactId) {
        // Deliberate retraction: this explicit call must stick until the fact
        // is re-asserted (aem.13 §4). Motivated dependents are NOT marked —
        // they may legitimately revive if an earlier premise comes back.
        self.deliberately_retracted.insert(fact);
        let deactivated = self.cascade_deactivate(fact);
        if deactivated.is_empty() {
            return;
        }
        self.check_supported_scoped(&deactivated);
    }

    /// Retract a set of facts as ONE cascade with ONE scoped invariant
    /// check (PERF-1: the streaming eviction loop's per-fact `retract`
    /// cascade). Marks every listed fact deliberately retracted — exactly as
    /// sequential [`retract`] calls would — then deactivates the combined
    /// dependent closure once. Deactivation is a monotone fixed point, so
    /// the final active set equals the sequential outcome; the invariant is
    /// checked once over the affected subgraph instead of once per fact.
    pub fn retract_all(&mut self, facts: impl IntoIterator<Item = FactId>) {
        let mut seeds = Vec::new();
        for fact in facts {
            // Mark even already-inactive facts (matches `retract`, which
            // marks before its active check).
            self.deliberately_retracted.insert(fact);
            if self.is_active(fact) {
                seeds.push(fact);
            }
        }
        if seeds.is_empty() {
            return;
        }
        let mut deactivated = Vec::new();
        for seed in seeds {
            deactivated.extend(self.cascade_deactivate(seed));
        }
        self.check_supported_scoped(&deactivated);
    }

    /// Scoped survival check (`fact_active_iff_supported`): after a
    /// retraction cascade, every still-active fact in the affected subgraph
    /// must retain at least one active justification set. Only facts
    /// reachable from the deactivated set through the reverse premise index
    /// can have lost support (support changes only when a premise
    /// deactivates, and facts outside the closure have no deactivated
    /// premise) — so this visits the evicted subgraph, not all facts.
    fn check_supported_scoped(&self, deactivated: &[FactId]) {
        let mut seen = BTreeSet::new();
        let mut queue: Vec<FactId> = deactivated.to_vec();
        let mut violated = false;
        while let Some(fid) = queue.pop() {
            let dependents: Vec<FactId> = self
                .dependents
                .get(&fid)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            for d in dependents {
                if !seen.insert(d) {
                    continue;
                }
                if self.is_active(d) {
                    // Still active: its own dependents' support is unchanged
                    // (this premise stayed live), so the closure need not
                    // grow through it — but it must itself be supported.
                    if self.support_count(d) == 0 {
                        violated = true;
                        break;
                    }
                } else {
                    // Newly inactive (or already so): its dependents may
                    // have lost support — keep walking.
                    queue.push(d);
                }
            }
            if violated {
                break;
            }
        }
        assert_invariant(!violated, "fact_active_iff_supported", Some("fabric"));
    }

    /// Deactivate `fact` and cascade through the reverse premise index,
    /// returning every fact deactivated (including `fact` itself).
    /// Re-examines dependents on each new deactivation: a dependent kept
    /// alive by a still-active premise is revisited when that premise falls,
    /// so the outcome equals the old full-scan cascade's fixed point while
    /// touching only the affected subgraph. Never re-checks the global
    /// invariant — intermediate states may legitimately contain a dependent
    /// not yet cascaded.
    fn cascade_deactivate(&mut self, fact: FactId) -> Vec<FactId> {
        if !self.is_active(fact) {
            return Vec::new();
        }
        let mut deactivated = Vec::new();
        let mut queue = vec![fact];
        while let Some(fid) = queue.pop() {
            if !self.is_active(fid) {
                continue;
            }
            self.set_active(fid, false);
            deactivated.push(fid);
            // Cascade: any active conclusion that references `fid` as a
            // premise and has now lost its last active justification set
            // must be retracted.
            let dependents: Vec<FactId> = self
                .dependents
                .get(&fid)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();
            for d in dependents {
                if self.is_active(d) && self.support_count(d) == 0 {
                    queue.push(d);
                }
            }
        }
        deactivated
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
        // Proper-subset comparability only: set-equal justifications in
        // different premise orders are distinct provenance (A3), not a
        // minimality violation.
        let violated = js.iter().enumerate().any(|(i, a)| {
            let a_set = a.premise_set();
            js[i + 1..].iter().any(|b| {
                let b_set = b.premise_set();
                a_set != b_set && a_set.is_subset(&b_set)
            })
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
