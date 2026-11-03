//! Saturation-engine unit tests (bead nlvs rewrite): multi-support
//! disjunction, unconditional-acoustic stability, external
//! retract/re-assert through the `fabric_mut` flag, and internal-prune
//! propagation. Byte-identity against the pre-saturation engine is proven
//! by `decision_differential`; these pin the new machinery's semantics.

use fabric::decision::{Arm, DecisionFabric, DecisionParams, SourceClass, TokenKind};
use fabric::FactId;
use std::collections::BTreeSet;

fn no_premises() -> BTreeSet<FactId> {
    BTreeSet::new()
}

fn premised(p: FactId) -> BTreeSet<FactId> {
    let mut s = BTreeSet::new();
    s.insert(p);
    s
}

#[test]
fn multi_support_disjunction_survives_single_premise_loss() {
    // Same atom asserted twice with different premises: two
    // justifications. The evidence stays active while ANY support is
    // alive, and deactivates only when all supports are gone.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let h = d.mint_token("reef", TokenKind::Word, 0, 10).unwrap();
    let p1 = d.mint_token("p1", TokenKind::Word, 0, 5).unwrap();
    let p2 = d.mint_token("p2", TokenKind::Word, 5, 10).unwrap();
    d.add_unary(h, SourceClass::Lexical, 4.0, premised(p1))
        .unwrap();
    d.add_unary(h, SourceClass::Lexical, 4.0, premised(p2))
        .unwrap();
    assert_eq!(
        d.active_unary_max(h, SourceClass::Lexical),
        Some(4.0),
        "both supports alive"
    );
    d.fabric_mut().retract(p1);
    assert_eq!(
        d.active_unary_max(h, SourceClass::Lexical),
        Some(4.0),
        "second support keeps the atom alive"
    );
    d.fabric_mut().retract(p2);
    assert_eq!(
        d.active_unary_max(h, SourceClass::Lexical),
        None,
        "all supports gone"
    );
}

#[test]
fn unconditional_acoustic_survives_unrelated_retracts() {
    // Premise-free acoustic evidence is unconditional: retracting other
    // tokens never traverses it, and its score never moves.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let h = d.mint_token("reef", TokenKind::Word, 0, 10).unwrap();
    let a = d.mint_token("a", TokenKind::Word, 0, 5).unwrap();
    let b = d.mint_token("b", TokenKind::Word, 5, 10).unwrap();
    d.add_unary(h, SourceClass::Acoustic, 3.0, no_premises())
        .unwrap();
    d.add_unary(a, SourceClass::Acoustic, 1.0, no_premises())
        .unwrap();
    let before = d.liveness(h);
    d.fabric_mut().retract(a);
    d.fabric_mut().retract(b);
    assert_eq!(
        d.active_unary_max(h, SourceClass::Acoustic),
        Some(3.0),
        "unconditional acoustic never weakens"
    );
    assert_eq!(d.liveness(h), before, "liveness bit-stable");
}

#[test]
fn external_retract_then_reassert_restores_cached_scores() {
    // The `fabric_mut` flag covers both directions: an external retract
    // weakens memoized queries with no intervening `&mut` decision call,
    // and an external re-assert restores them the same way.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let h = d.mint_token("reef", TokenKind::Word, 0, 10).unwrap();
    let p = d.mint_token("premise", TokenKind::Word, 0, 5).unwrap();
    d.add_unary(h, SourceClass::Lexical, 2.0, no_premises())
        .unwrap();
    d.add_unary(h, SourceClass::Speaker, 4.0, premised(p))
        .unwrap();
    let before = d.liveness(h);
    assert!(before > 0.0);
    d.fabric_mut().retract(p);
    let weakened = d.liveness(h);
    assert!(
        weakened < before,
        "external retract weakens memoized liveness"
    );
    d.fabric_mut().assert_assumption(p);
    assert_eq!(
        d.liveness(h),
        before,
        "external re-assert restores memoized liveness"
    );
}

#[test]
fn internal_prune_rescores_premise_dependents() {
    // Arm B: `prune_below_threshold` retracts below-theta tokens through
    // the internal path; surviving evidence premised on a victim must
    // deactivate via propagation (no external flag involved).
    let mut p = DecisionParams::default();
    p.arm = Arm::B;
    p.theta_live = 3.0;
    let mut d = DecisionFabric::new(p);
    let weak = d.mint_token("weak", TokenKind::Word, 0, 5).unwrap();
    d.add_unary(weak, SourceClass::Acoustic, 1.0, no_premises())
        .unwrap();
    let h = d.mint_token("reef", TokenKind::Word, 5, 15).unwrap();
    d.add_unary(h, SourceClass::Lexical, 5.0, premised(weak))
        .unwrap();
    assert_eq!(
        d.active_unary_max(h, SourceClass::Lexical),
        Some(5.0),
        "support alive before prune"
    );
    d.prune_below_threshold();
    assert!(
        !d.fabric().is_active(weak),
        "below-theta victim pruned"
    );
    assert_eq!(
        d.active_unary_max(h, SourceClass::Lexical),
        None,
        "victim-premised evidence deactivates via propagation"
    );
}
