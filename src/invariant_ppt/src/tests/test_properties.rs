//! P-Tests: property tests over generic inputs.
//!
//! These verify that `assert_invariant` behaves correctly as a function of
//! its inputs — the stable, generic-input behavior that property testing is
//! meant to cover. They do not assert domain invariants of Hearsay-IV (those
//! live in the per-stage P-Tests); they test the invariant machinery itself.

use std::sync::MutexGuard;

use crate::{assert_invariant, clear_invariant_log, has_invariant, recorded_messages, test_lock};

fn setup(_g: &MutexGuard<'static, ()>) {
    clear_invariant_log();
}

#[test]
fn records_passing_invariant() {
    let g = test_lock::lock();
    setup(&g);
    assert_invariant(true, "always true", None);
    assert!(has_invariant("always true"));
    assert_eq!(recorded_messages().len(), 1);
}

#[test]
fn dedupes_identical_messages() {
    let g = test_lock::lock();
    setup(&g);
    assert_invariant(true, "same message", None);
    assert_invariant(true, "same message", None);
    assert_invariant(true, "same message", None);
    // Same message tracked as one key regardless of repetition count.
    let invs = crate::recorded_invariants();
    let matches = invs.iter().filter(|r| r.message == "same message").count();
    assert_eq!(matches, 1);
    assert_eq!(
        invs.iter()
            .find(|r| r.message == "same message")
            .map(|r| r.hits)
            .unwrap_or(0),
        3
    );
}

#[test]
fn distinct_messages_are_separate() {
    let g = test_lock::lock();
    setup(&g);
    assert_invariant(true, "message one", None);
    assert_invariant(true, "message two", None);
    assert_eq!(recorded_messages().len(), 2);
}

#[test]
fn clear_resets_state() {
    let g = test_lock::lock();
    setup(&g);
    assert_invariant(true, "temp", None);
    assert_eq!(recorded_messages().len(), 1);
    clear_invariant_log();
    assert_eq!(recorded_messages().len(), 0);
}

#[test]
#[should_panic(expected = "INVARIANT VIOLATION")]
fn panics_on_false_condition() {
    let g = test_lock::lock();
    setup(&g);
    assert_invariant(false, "must never be false", None);
}

#[test]
fn context_is_recorded() {
    let g = test_lock::lock();
    setup(&g);
    assert_invariant(true, "with context", Some("checkout flow"));
    let invs = crate::recorded_invariants();
    assert!(invs.iter().any(|r| r.context == "checkout flow"));
}
