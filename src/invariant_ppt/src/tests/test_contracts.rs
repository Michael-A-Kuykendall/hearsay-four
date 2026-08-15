//! C-Tests: contract tests + invariant coverage enforcement.
//!
//! A contract declares the set of invariants that a piece of real business
//! logic MUST keep enforcing. `verify_contract` (and the CI gate
//! `verify_all_contracts`) fail if any required invariant is no longer
//! actively asserted after a refactor — the "no test passes unless the
//! real-world expectations are still actively enforced" guarantee.
use std::sync::MutexGuard;

use crate::{
    assert_invariant, clear_contracts, clear_invariant_log, register_contract, test_lock,
    verify_all_contracts, verify_contract,
};

fn setup(_g: &MutexGuard<'static, ()>) {
    clear_invariant_log();
    clear_contracts();
}

// A minimal stand-in for real business logic so the contract machinery can be
// demonstrated. In Hearsay-IV the equivalent would be e.g. the D-1 survival
// fix or Stage 3's retraction semantics.

struct Payment;

impl Payment {
    fn authorize(amount: i64) -> bool {
        // This is the invariant the contract protects.
        assert_invariant(amount > 0, "Payment must be positive", Some("payment"));
        amount > 0
    }
}

#[test]
fn contract_verifies_present_invariant() {
    let g = test_lock::lock();
    setup(&g);
    register_contract("payment processing", &["Payment must be positive"]);
    Payment::authorize(50);
    assert!(verify_contract("payment processing"));
}

#[test]
#[should_panic(expected = "CONTRACT VIOLATION")]
fn contract_fails_when_invariant_missing() {
    let g = test_lock::lock();
    setup(&g);
    register_contract("payment processing", &["Payment must be positive"]);
    // NOTE: we do NOT call Payment::authorize, so the invariant is absent.
    // The contract must fail.
    verify_contract("payment processing");
}

#[test]
fn verify_all_contracts_checks_every_registered_contract() {
    let g = test_lock::lock();
    setup(&g);
    register_contract("contract a", &["invariant a"]);
    register_contract("contract b", &["invariant b"]);
    assert_invariant(true, "invariant a", None);
    assert_invariant(true, "invariant b", None);
    assert_eq!(verify_all_contracts(), 2);
}

/// The CI audit gate's core guarantee: if a refactor removes an invariant
/// that a contract requires, `verify_all_contracts` must fail. This is the
/// "no test passes unless real-world expectations are still enforced" rule.
struct Stage3Stub;

impl Stage3Stub {
    // Simulates a refactor that DROPPED the survival invariant. The contract
    // still requires it, so the gate must catch the regression.
    fn retract_without_invariant(_hypothesis: &str) {
        // NOTE: assert_invariant("...survival...", ...) was removed here.
    }
}

#[test]
#[should_panic(expected = "CONTRACT VIOLATION")]
fn ci_gate_catches_dropped_invariant_after_refactor() {
    let g = test_lock::lock();
    setup(&g);
    register_contract(
        "stage 3 survival",
        &["Hypothesis survives iff it retains independent support"],
    );
    // A refactor removed the invariant assertion — the gate must fail.
    Stage3Stub::retract_without_invariant("h1");
    verify_all_contracts();
}

#[test]
fn ci_gate_passes_when_invariant_reenforced() {
    let g = test_lock::lock();
    setup(&g);
    register_contract(
        "stage 3 survival",
        &["Hypothesis survives iff it retains independent support"],
    );
    assert_invariant(
        true,
        "Hypothesis survives iff it retains independent support",
        Some("stage 3"),
    );
    assert_eq!(verify_all_contracts(), 1);
}

#[test]
#[should_panic(expected = "CONTRACT VIOLATION")]
fn verify_all_contracts_fails_on_any_missing() {
    let g = test_lock::lock();
    setup(&g);
    register_contract("contract a", &["present"]);
    register_contract("contract b", &["missing"]);
    assert_invariant(true, "present", None);
    // 'missing' is never asserted — the CI gate must fail.
    verify_all_contracts();
}
