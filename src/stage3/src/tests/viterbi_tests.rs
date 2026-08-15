//! d73.3 gate tests: Viterbi bounded-window mechanism (Category C).
//!
//! DoD:
//! - keeps N-best as live justification sets within the window
//! - discards only on window close (INV `discard_only_on_close`)
//! - demonstrates a surviving alternative (independent support, aem.13)
//! - confidence math: calibrated log-odds combination and 0–100 rating
//!   (INV `rating_in_range`, `evidence_additive`)

use std::collections::BTreeSet;

use crate::window::{CommitWindow, PRIOR_LOG_ODDS, WindowError, sigmoid};

/// A live evidence fact inside `w`'s fabric (interned + assumed active).
fn evidence(w: &mut CommitWindow) -> fabric::FactId {
    let ev = w.fabric.intern();
    w.fabric.assert_assumption(ev);
    ev
}

fn set(ids: &[fabric::FactId]) -> BTreeSet<fabric::FactId> {
    ids.iter().copied().collect()
}

#[test]
fn viterbi_window_holds_nbest_live() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    // No close yet — two candidates for one position stay live.
    let c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("offer");
    let c2 = w.offer(0, set(&[e2]), &[], 0.5).expect("offer");
    assert_eq!(w.len(), 2);
    assert!(w.fabric.is_active(c1.fact), "candidate 1 live");
    assert!(w.fabric.is_active(c2.fact), "candidate 2 live");
    let win = w.winner().expect("winner");
    assert_eq!(win.fact, c1.fact);
}

#[test]
fn viterbi_losing_candidate_survives_with_independent_support() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    // Independent stable support (already active assumption).
    let stable = evidence(&mut w);
    let c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("offer c1");
    let c2 = w
        .offer(0, set(&[e2]), &[set(&[stable])], 0.6)
        .expect("offer c2");
    assert!(w.fabric.is_active(c2.fact));

    let (winner, survivors) = w.close().expect("close");
    assert_eq!(winner.fact, c1.fact, "c1 wins on log-odds 1.0 > 0.6");
    assert_eq!(survivors, 1, "c2 survives via stable support");
    assert!(w.fabric.is_active(c2.fact), "c2 still active after close");
    assert!(w.fabric.is_active(winner.fact), "winner committed");
    assert_eq!(w.len(), 0, "window emptied on close");
}

#[test]
fn viterbi_loser_without_independent_support_is_retracted() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("c1");
    let c2 = w.offer(0, set(&[e2]), &[], 0.2).expect("c2");
    let (winner, survivors) = w.close().expect("close");
    assert_eq!(winner.fact, c1.fact);
    assert_eq!(survivors, 0, "no independent support -> retracted");
    assert!(
        !w.fabric.is_active(c2.fact),
        "loser without support retracted"
    );
    // Provenance preserved even for the discarded loser.
    assert_eq!(w.fabric.justifications(c2.fact).len(), 1);
}

#[test]
fn viterbi_close_empty_returns_err() {
    let mut w = CommitWindow::new(4);
    assert_eq!(w.close().unwrap_err(), WindowError::EmptyWindow);
}

#[test]
fn viterbi_accumulate_allows_independent_combination() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let c = w.offer(0, set(&[e1]), &[], 0.5).expect("c");
    w.accumulate(c.fact, 0.5).expect("accumulate");
    let winner = w.winner().expect("winner");
    // evidence_additive: L = L0 + 0.5 + 0.5
    assert_eq!(winner.log_odds, PRIOR_LOG_ODDS + 1.0);
    assert_eq!(winner.rating(), 100.0 * sigmoid(1.0));
    assert!((0.0..=100.0).contains(&winner.rating()));
}

#[test]
fn viterbi_width_cap_enforced() {
    let mut w = CommitWindow::new(2);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let e3 = evidence(&mut w);
    w.offer(0, set(&[e1]), &[], 1.0).expect("one");
    w.offer(0, set(&[e2]), &[], 1.0).expect("two");
    assert_eq!(
        w.offer(0, set(&[e3]), &[], 1.0).unwrap_err(),
        WindowError::WindowFull
    );
}
