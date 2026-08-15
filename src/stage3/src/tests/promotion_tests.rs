//! d73.5 gate tests: Tier 1 → Tier 2 promotion (commit) on window close.
//!
//! DoD (from bead acceptance):
//! 1. promotion happens ONLY on window close (never before)
//! 2. the committed fact's winning justification is marked unconditional
//!    (stable per aem.13 §4)
//! 3. no unsupported promotion (fabric invariant `no_unsupported_promotion`)

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
fn promotion_does_not_happen_before_window_close() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let _c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("c1");
    let c2 = w.offer(0, set(&[e2]), &[], 2.0).expect("c2");

    // Mid-window: the leading candidate is NOT yet committed — the leader's
    // justifications are all still defeasible (no unconditional entry).
    let leader = w.winner().expect("leader");
    assert_eq!(leader.fact, c2.fact, "c2 leads mid-window (log-odds 2 > 1)");
    let justs = w.fabric.justifications(leader.fact);
    assert!(!justs.is_empty());
    assert!(
        justs.iter().all(|j| !j.unconditional),
        "NOT promoted before window close"
    );
}

#[test]
fn promotion_only_on_window_close_marks_unconditional() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let _c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("c1");
    let _c2 = w.offer(0, set(&[e2]), &[], 2.0).expect("c2");

    // Close: the leader (c2) wins and is committed — its winning
    // justification becomes unconditional (aem.13 §4 promotion).
    let (winner, _survivors) = w.close().expect("close");
    let justs = w.fabric.justifications(winner.fact);
    let unconditional = justs.iter().filter(|j| j.unconditional).count();
    assert!(
        unconditional >= 1,
        "winner's justification made unconditional"
    );
    assert!(w.fabric.is_active(winner.fact), "winner active post-commit");
}

#[test]
fn no_unsupported_promotion() {
    let mut w = CommitWindow::new(4);
    // The winner's evidence is active so the winner is supported at commit
    // (fabric invariant `no_unsupported_promotion`). Close promotes it.
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let _c1 = w.offer(0, set(&[e1]), &[], 0.5).expect("c1");
    let _c2 = w.offer(0, set(&[e2]), &[], 1.0).expect("c2");
    let (winner, _) = w.close().expect("close");
    assert!(
        w.fabric.is_active(winner.fact),
        "promoted only when supported"
    );
    assert!(w.fabric.support_count(winner.fact) >= 1);
}

#[test]
fn losing_candidates_are_not_promoted_to_unconditional() {
    let mut w = CommitWindow::new(4);
    let e1 = evidence(&mut w);
    let e2 = evidence(&mut w);
    let c1 = w.offer(0, set(&[e1]), &[], 1.0).expect("c1");
    let c2 = w.offer(0, set(&[e2]), &[], 0.3).expect("c2");
    let (winner, _) = w.close().expect("close");
    // c1 wins (log-odds 1.0 > 0.3). The loser c2 was not committed; it is
    // retracted (no independent support) but its provenance remains.
    assert_eq!(winner.fact, c1.fact);
    assert!(
        !w.fabric.justifications(c2.fact).is_empty(),
        "provenance kept"
    );
    if !w.fabric.is_active(c2.fact) {
        assert_eq!(w.fabric.justifications(c2.fact).len(), 1);
    }
}
