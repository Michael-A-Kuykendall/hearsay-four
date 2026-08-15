//! Provenance statistics — the instrument for §5 long-session measurement.
//!
//! The synthesis §5 open question is whether long-session memory and
//! provenance growth stay bounded. This module provides `provenance_stats()`
//! over a [`Fabric`], reporting the memory/provenance growth curve so the
//! fire-control precedent (state smaller than a photograph) can be checked
//! against the speech system's open, continuous-stream fact volume.

use crate::Fabric;

/// Snapshot of provenance/memory statistics for a fabric at a point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceStats {
    /// Total number of interned facts (memory footprint driver).
    pub total_facts: usize,
    /// Number of facts currently active.
    pub active_facts: usize,
    /// Number of facts currently inactive/retracted (kept for provenance).
    pub inactive_facts: usize,
    /// Total number of justification sets stored across all facts.
    pub total_justifications: usize,
    /// Average justifications per fact (provenance density).
    pub avg_justifications_per_fact: f64,
    /// Number of facts that survived at least one retraction (survival count).
    pub surviving_facts: usize,
    /// Total memory estimate in bytes (facts + justifications).
    pub estimated_bytes: usize,
}

impl Fabric {
    /// Compute provenance/memory statistics over the current fabric state.
    ///
    /// This is the `provenance_stats()` instrument referenced in synthesis §5
    /// and conversion inventory §2.9 — used to measure whether memory and
    /// provenance growth remain bounded over a long session.
    pub fn provenance_stats(&self) -> ProvenanceStats {
        let total_facts = self.facts.len();
        let active_facts = self.facts.values().filter(|s| s.active).count();
        let inactive_facts = total_facts - active_facts;
        let total_justifications: usize = self.facts.values().map(|s| s.justifications.len()).sum();
        // A fact "survived" if it currently has multiple justifications that
        // once supported it (proxy: has >1 justification set, meaning it
        // accumulated independent support rather than being overwritten).
        let surviving_facts = self
            .facts
            .values()
            .filter(|s| s.justifications.len() > 1)
            .count();
        let avg = if total_facts == 0 {
            0.0
        } else {
            total_justifications as f64 / total_facts as f64
        };

        // Rough byte estimate: each FactId (8B) + justification bookkeeping.
        // This is a conservative structural lower bound, not a precise heap
        // measurement.
        let per_fact_overhead = 64usize; // FactState + active flag + map node
        let per_justification = 80usize; // Justification + premises set node
        let estimated_bytes =
            total_facts * per_fact_overhead + total_justifications * per_justification;

        ProvenanceStats {
            total_facts,
            active_facts,
            inactive_facts,
            total_justifications,
            avg_justifications_per_fact: avg,
            surviving_facts,
            estimated_bytes,
        }
    }
}

#[cfg(test)]
mod stats_tests {
    use super::*;
    use crate::FactId;
    use std::collections::BTreeSet;

    fn ps(ids: &[u64]) -> BTreeSet<FactId> {
        ids.iter().map(|&i| FactId(i)).collect()
    }

    #[test]
    fn empty_fabric_reports_zeros() {
        let f = Fabric::new();
        let s = f.provenance_stats();
        assert_eq!(s.total_facts, 0);
        assert_eq!(s.active_facts, 0);
        assert_eq!(s.total_justifications, 0);
        assert_eq!(s.avg_justifications_per_fact, 0.0);
    }

    #[test]
    fn reports_growth_as_facts_are_interned() {
        let mut f = Fabric::new();
        for _ in 0..5 {
            f.intern();
        }
        let s = f.provenance_stats();
        assert_eq!(s.total_facts, 5);
        assert_eq!(s.active_facts, 0);
        assert_eq!(s.estimated_bytes, 5 * 64);
    }

    #[test]
    fn counts_justifications_and_survival() {
        let mut f = Fabric::new();
        let p1 = f.intern();
        let p2 = f.intern();
        let concl = f.intern();
        f.assert_assumption(p1);
        f.assert_assumption(p2);
        f.assert(concl, ps(&[p1.0]), false);
        f.assert(concl, ps(&[p2.0]), false);

        let s = f.provenance_stats();
        assert_eq!(s.total_facts, 3);
        assert_eq!(s.total_justifications, 4); // 2 assumptions + 2 justifications
        assert_eq!(s.surviving_facts, 1); // concl has 2 justification sets
        assert_eq!(s.avg_justifications_per_fact, 4.0 / 3.0);

        // Retract one premise: concl survives via the other.
        f.retract(p1);
        let s2 = f.provenance_stats();
        assert!(s2.active_facts >= 2); // p2 + concl still active
        assert_eq!(s2.inactive_facts, 1); // p1 retracted
        // Provenance preserved: justifications not cleared by retraction.
        assert_eq!(s2.total_justifications, 4);
    }

    #[test]
    fn inactive_facts_are_counted_for_memory() {
        let mut f = Fabric::new();
        let p = f.intern();
        let c = f.intern();
        f.assert_assumption(p);
        f.assert(c, ps(&[p.0]), false);
        f.retract(p); // c loses support and is retracted
        let s = f.provenance_stats();
        // Both facts retained in memory (provenance preserved).
        assert_eq!(s.total_facts, 2);
        assert_eq!(s.inactive_facts, 2);
        assert_eq!(s.active_facts, 0);
    }
}
