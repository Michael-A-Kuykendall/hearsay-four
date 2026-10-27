//! PERF-1 (bead hearsay-four-gmp) scaling gate: per-assert cost must stay
//! bounded independent of total fabric facts.
//!
//! Old code ran `reactivate_ready()` — a full scan over every fact with a
//! per-fact `support_count` — on EVERY `assert`, so K asserts over an N-fact
//! arena cost O(N*K). Incremental maintenance makes one assert touch only
//! its dependent subgraph (here: nothing), i.e. O(1) in N. This test builds
//! a large arena of unsupported facts and bounds the wall time of a fixed
//! batch of GPS-positioned (independent) asserts; it is RED on the full-scan
//! implementation and GREEN on the incremental one.
//!
//! Model-free, synthetic only, no network.

use fabric::{Fabric, FactId};
use std::collections::BTreeSet;
use std::time::Instant;

/// Arena size: large enough that the old O(N) scan per assert dominates.
const ARENA_FACTS: usize = 6_000;
/// Fixed batch of independent asserts whose cost must not grow with N.
const PROBE_ASSERTS: usize = 500;
/// Wall bound for the probe batch on incremental code. The old full-scan
/// code takes ~2s here (debug profile); incremental code takes
/// milliseconds. Margin is >100x on the probe machine.
const PROBE_WALL_BOUND_SECS: f64 = 0.5;

#[test]
fn assert_cost_bounded_independent_of_arena_size() {
    let mut f = Fabric::new();
    // Build an arena of facts that are all inactive and unsupported: each
    // conclusion's single premise is never asserted, so every old-code
    // `reactivate_ready` scan evaluates a real (failing) justification per
    // fact instead of short-circuiting on an empty list.
    for _ in 0..ARENA_FACTS {
        let premise = f.intern();
        let concl = f.intern();
        let mut ps = BTreeSet::new();
        ps.insert(premise);
        f.assert(concl, ps, false);
    }
    assert_eq!(f.support_count(FactId(2)), 0);

    // Probe: time a fixed batch of independent assumption asserts. None of
    // these has dependents, so incremental maintenance does O(1) work each.
    let t = Instant::now();
    let mut probed = Vec::with_capacity(PROBE_ASSERTS);
    for _ in 0..PROBE_ASSERTS {
        let a = f.intern();
        f.assert_assumption(a);
        assert!(f.is_active(a));
        probed.push(a);
    }
    let dt = t.elapsed().as_secs_f64();

    // Semantics unchanged by the arena: arena facts still unsupported and
    // inactive, probe facts all active.
    assert_eq!(f.support_count(FactId(2)), 0);
    assert!(!f.is_active(FactId(2)));
    for a in &probed {
        assert!(f.is_active(*a));
    }

    assert!(
        dt < PROBE_WALL_BOUND_SECS,
        "per-assert cost scales with arena size: {PROBE_ASSERTS} asserts over \
         {ARENA_FACTS}-fact arena took {dt:.3}s (bound {PROBE_WALL_BOUND_SECS:.1}s)"
    );
}
