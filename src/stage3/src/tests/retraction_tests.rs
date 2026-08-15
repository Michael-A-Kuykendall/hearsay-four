//! d73.6 gate tests: retraction-with-provenance of losing candidates.
//!
//! DoD (from bead acceptance):
//! 1. retracting a losing candidate preserves its justification sets
//!    (INV `retraction_preserves_provenance`)
//! 2. a candidate with an independent active justification survives retraction
//!    (INV `survival_requires_independent_support`)
//! 3. no active fact is ever unsupported (INV `fact_active_iff_supported`)
//!
//! These are exercised through the Stage-3 window (see window.rs `close`), on
//! top of the fabric's retraction semantics (aem.13).

use std::collections::BTreeSet;

use crate::window::CommitWindow;

fn evidence(w: &mut CommitWindow) -> fabric::FactId {
    let e = w.fabric.intern();
    w.fabric.assert_assumption(e);
    e
}

fn set(ids: &[fabric::FactId]) -> BTreeSet<fabric::FactId> {
    ids.iter().copied().collect()
}

#[test]
fn retraction_preserves_provenance_of_loser() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let c1 = w.offer(0, set(&[e1]), &[], 0.2).expect("loser");
    let _c2 = w.offer(0, set(&[e2]), &[], 1.0).expect("winner");

    let before_loser = w.fabric.justifications(c1.fact).len();
    let _ = w.close().expect("close");
    // c1 is the loser (0.2 < 1.0). Losing does NOT add or strip justification
    // sets: provenance is preserved (INV retraction_preserves_provenance).
    assert_eq!(
        w.fabric.justifications(c1.fact).len(),
        before_loser,
        "retraction_preserves_provenance"
    );
    // Without independent support the loser is not active anymore.
    assert!(!w.fabric.is_active(c1.fact));
}

#[test]
fn loser_survives_with_independent_support() {
    let mut w = CommitWindow::new(4);
    let stable = evidence(&mut w); // independent support
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let _c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("c1");
    // c2 has an independent active support -> survives window close.
    let c2 = w.offer(0, set(&[e2]), &[set(&[stable])], 0.6).expect("c2");
    let (_, survivors) = w.close().expect("close");
    assert_eq!(survivors, 1, "c2 survives via independent support");
    assert!(
        w.fabric.is_active(c2.fact),
        "INV survival_requires_independent_support"
    );
}

#[test]
fn no_active_fact_is_unsupported_after_close() {
    let mut w = CommitWindow::new(4);
    let stable = evidence(&mut w);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let _ = w.offer(0, set(&[e1]), &[], 1.0).expect("c1");
    // This candidate has independent support and therefore must stay active
    // AFTER close — and being active it must be supported (INV
    // fact_active_iff_supported).
    let c2 = w.offer(0, set(&[e2]), &[set(&[stable])], 0.4).expect("c2");
    let (_, survivors) = w.close().expect("close");
    assert_eq!(survivors, 1);
    assert!(w.fabric.is_active(c2.fact), "survivor is active");
    assert!(
        w.fabric.support_count(c2.fact) >= 1,
        "INV fact_active_iff_supported: active survivor must be supported"
    );
}

#[test]
fn nothing_is_active_before_close_when_closed_with_no_winner_is_err() {
    // Regression guard: closing an empty window is an error, never a
    // zero-commit silently.
    let mut w = CommitWindow::new(2);
    assert!(w.close().is_err());
}
