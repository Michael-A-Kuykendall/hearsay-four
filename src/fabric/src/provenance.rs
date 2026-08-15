//! Live always-on provenance — proof-tree extraction from live justification
//! sets (no query-time reconstruction).
//!
//! Per the conversion inventory §2.9 and the synthesis: Soufflé and `egg`
//! produce proof structure by *reconstructing it at query time* — searching
//! after the fact for a minimal-height proof tree. The fabric instead keeps
//! justification sets **live and always-on**, so a proof is *read out* of the
//! live justification graph rather than rebuilt on demand. This is the
//! "proof-without-search" distinction.
//!
//! The output proof-tree format is modeled on egg / Soufflé's proof lattice:
//! each node names a fact, its supporting justification set, and the proof
//! for each premise — walking down to assumptions.

use crate::{Fabric, FactId};
use std::collections::BTreeMap;

/// A proof-tree node for one fact, derived from its live justifications.
#[derive(Debug, Clone)]
pub struct ProofNode {
    /// The fact this node proves.
    pub fact: FactId,
    /// Whether the fact is currently active in the fabric.
    pub active: bool,
    /// The label (human-readable) of the fact.
    pub label: String,
    /// The justification sets supporting this fact (live, not reconstructed).
    pub justifications: Vec<ProofJustification>,
}

/// One live justification set rendered as proof structure.
#[derive(Debug, Clone)]
pub struct ProofJustification {
    /// Whether this justification is currently active (all premises active).
    pub active: bool,
    /// Whether it is unconditional (cannot be un-derived).
    pub unconditional: bool,
    /// Recursive proofs for each premise.
    pub premises: Vec<ProofNode>,
}

impl Fabric {
    /// Produce a live proof tree for `fact`, walking its justification sets
    /// down to assumptions. This reads the always-on justification graph; it
    /// does not reconstruct anything.
    ///
    /// A `label` function maps a `FactId` to a human-readable name (e.g. a
    /// phoneme string), so the proof is legible.
    pub fn proof_tree(&self, fact: FactId, label: &dyn Fn(FactId) -> String) -> ProofNode {
        self.build_proof(fact, label, &mut BTreeMap::new())
    }

    fn build_proof(
        &self,
        fact: FactId,
        label: &dyn Fn(FactId) -> String,
        memo: &mut BTreeMap<FactId, ProofNode>,
    ) -> ProofNode {
        if let Some(node) = memo.get(&fact) {
            return node.clone();
        }
        let active = self.is_active(fact);
        let justifications = self
            .justifications(fact)
            .iter()
            .map(|j| {
                let j_active = self.is_justification_active(j);
                let premises = j
                    .premises
                    .iter()
                    .map(|&p| self.build_proof(p, label, memo))
                    .collect();
                ProofJustification {
                    active: j_active,
                    unconditional: j.unconditional,
                    premises,
                }
            })
            .collect();
        let node = ProofNode {
            fact,
            active,
            label: label(fact),
            justifications,
        };
        memo.insert(fact, node.clone());
        node
    }

    /// Prove a fact is active by showing at least one fully-active, live
    /// justification set (all premises active). Returns `None` if the fact is
    /// not currently supported.
    pub fn prove_active(
        &self,
        fact: FactId,
        label: &dyn Fn(FactId) -> String,
    ) -> Option<ProofNode> {
        if !self.is_active(fact) {
            return None;
        }
        Some(self.proof_tree(fact, label))
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ps(ids: &[u64]) -> BTreeSet<FactId> {
        ids.iter().map(|&i| FactId(i)).collect()
    }

    fn labeler(id: FactId) -> String {
        format!("fact{}", id.0)
    }

    #[test]
    fn proof_of_assumption_has_no_premises() {
        let mut f = Fabric::new();
        let a = f.intern();
        f.assert_assumption(a);
        let p = f.proof_tree(a, &labeler);
        assert!(p.active);
        assert_eq!(p.justifications.len(), 1);
        assert!(p.justifications[0].premises.is_empty());
    }

    #[test]
    fn proof_walks_premises_recursively() {
        let mut f = Fabric::new();
        let a = f.intern();
        let b = f.intern();
        f.assert_assumption(a);
        f.assert(b, ps(&[a.0]), false);
        let p = f.prove_active(b, &labeler).expect("b is active");
        assert_eq!(p.fact, b);
        assert_eq!(p.justifications.len(), 1);
        let j = &p.justifications[0];
        assert_eq!(j.premises.len(), 1);
        assert_eq!(j.premises[0].fact, a);
        assert!(j.premises[0].justifications[0].premises.is_empty());
    }

    #[test]
    fn prove_active_returns_none_for_unsupported() {
        let mut f = Fabric::new();
        let c = f.intern();
        // No justification at all -> not active -> None.
        assert!(f.prove_active(c, &labeler).is_none());
    }

    #[test]
    fn proof_reflects_live_survival() {
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();
        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert(concl, ps(&[p1.0]), false);
        f.assert(concl, ps(&[p2.0]), false);
        f.retract(p1);
        // concl survives via p2; the proof must show the surviving (active)
        // justification and the inactive one.
        let p = f.prove_active(concl, &labeler).expect("concl survives");
        assert_eq!(p.justifications.len(), 2);
        assert_eq!(p.justifications.iter().filter(|j| j.active).count(), 1);
    }
}
