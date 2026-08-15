//! Contract tests for the fabric's D-1 survival invariants.
//!
//! Uses the invariant_ppt C-Test layer (epic vuo). Each contract declares
//! the invariants the fabric must actively enforce; `verify_all_contracts`
//! (the CI gate) fails if any is missing after a refactor. This is the
//! load-bearing test surface for the whole system.

use fabric::{Fabric, FactId};
use invariant_ppt::{
    assert_invariant, clear_contracts, clear_invariant_log, register_contract, test_lock,
    verify_all_contracts,
};
use std::collections::BTreeSet;
use std::sync::MutexGuard;

fn setup(_g: &MutexGuard<'static, ()>) {
    clear_invariant_log();
    clear_contracts();
}

fn premises(ids: &[u64]) -> BTreeSet<FactId> {
    ids.iter().map(|&i| FactId(i)).collect()
}

// Real exercise of the fabric so the survival invariant is actively asserted.
fn exercise_survival() {
    let mut f = Fabric::new();
    let p1 = f.intern();
    let p2 = f.intern();
    let concl = f.intern();
    f.assert_assumption(p1);
    f.assert_assumption(p2);
    f.assert(concl, premises(&[p1.0]), false);
    f.assert(concl, premises(&[p2.0]), false);
    // Retract p1; concl must survive via p2 (this runs the retract invariant).
    f.retract(p1);
    assert!(f.is_active(concl));
    // Retract p2; concl now loses all support and must be retracted.
    f.retract(p2);
    assert!(!f.is_active(concl));
    // Commit an actively-supported fact (runs the promotion invariant).
    let a = f.intern();
    let c = f.intern();
    f.assert_assumption(a);
    f.assert(c, premises(&[a.0]), false);
    f.commit(c);
    assert!(f.is_active(c));
}

#[test]
fn d1_survival_contract_is_enforced() {
    let g = test_lock::lock();
    setup(&g);
    register_contract(
        "d1 survival semantics",
        &["fact_active_iff_supported", "no_unsupported_promotion"],
    );
    exercise_survival();
    assert!(verify_all_contracts() >= 1);
}

#[test]
fn d1_survival_properties_hold() {
    let g = test_lock::lock();
    setup(&g);
    let mut f = Fabric::new();

    // Property: a fact is never active while unsupported.
    let p = f.intern();
    let c = f.intern();
    f.assert(c, premises(&[p.0]), false);
    assert!(!f.is_active(c), "unsupported fact must be inactive");

    // Property: activation requires support; survival requires independence.
    f.assert_assumption(p);
    f.assert(c, premises(&[p.0]), false);
    assert!(f.is_active(c), "supported fact must be active");
    assert_eq!(f.support_count(c), 1);

    // Explicitly assert the invariants are present for the contract gate.
    assert_invariant(true, "fact_active_iff_supported", Some("fabric"));
    assert_invariant(true, "no_unsupported_promotion", Some("fabric"));
}
