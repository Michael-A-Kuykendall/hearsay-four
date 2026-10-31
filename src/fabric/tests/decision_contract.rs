//! Property tests for the fabric decision contract, spec version 1.0
//! (`docs/design/fabric-decision-contract.md`, bead hearsay-four-dg8 / A4b).
//! Covers spec §13 items 6: LM-accounting byte-identity (§6.4),
//! dormancy/revival/defeat (§8-§10), conflict tiling (§7), deterministic
//! tie order (§10), plus the §3/§9.4 evidence rules they rest on.

use fabric::FactId;
use fabric::decision::{
    BudgetKind, CommitOutcome, DecisionError, DecisionEvent, DecisionFabric, DecisionParams,
    SPEC_VERSION, SourceClass, Token, TokenKind, spans_conflict,
};
use std::collections::BTreeSet;

fn no_premises() -> BTreeSet<FactId> {
    BTreeSet::new()
}

fn token(fact: u64, label: &str, start: u32, end: u32) -> Token {
    Token {
        fact: FactId(fact),
        label: label.to_string(),
        kind: TokenKind::Word,
        start_frame: start,
        end_frame: end,
    }
}

#[test]
fn spec_version_is_cited() {
    assert_eq!(SPEC_VERSION, "1.2");
    // Every class names its declared null (§3 rule 1).
    for class in [
        SourceClass::Acoustic,
        SourceClass::Lexical,
        SourceClass::Sequence,
        SourceClass::Speaker,
        SourceClass::Verify,
        SourceClass::User,
    ] {
        assert!(!class.declared_null().is_empty());
    }
}

#[test]
fn lm_accounting_byte_identity() {
    // §6.4: LM weight 0 vs 1 -> byte-identical published token llrs.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let reef = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    let now = d.mint_token("now", TokenKind::Word, 10, 20).expect("token");
    d.add_unary(reef, SourceClass::Acoustic, 2.8, no_premises())
        .expect("acoustic");
    d.add_unary(now, SourceClass::Acoustic, 2.0, no_premises())
        .expect("acoustic");
    // LM evidence exists in the fabric (SEQUENCE), and the tiling score
    // uses it — but no published byte may depend on the LM weight (§6).
    d.add_sequence(reef, now, 5.0, no_premises()).expect("seq");
    assert!(d.tiling_score(&[reef, now]) > 4.8);

    let weight0 = d.published_lattice_bytes(0.0);
    let weight1 = d.published_lattice_bytes(1.0);
    assert_eq!(weight0, weight1, "published bytes must ignore LM weight");
    // Published llr is the ACOUSTIC llr alone (§6.2), unweighted.
    assert_eq!(d.published_acoustic_llr(reef), Some(2.8));
    assert_eq!(d.published_acoustic_llr(now), Some(2.0));
}

#[test]
fn dormancy_revival_defeat_scenario() {
    // §13: acoustics slightly favour the wrong word; later context revives
    // the dormant correct word; the record shows both supports, the
    // revival, and the defeat.
    let params = DecisionParams {
        theta_live: 3.0,
        delta: 0.5,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let wreath = d
        .mint_token("wreath", TokenKind::Word, 0, 10)
        .expect("token");
    let reef = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    d.add_unary(wreath, SourceClass::Acoustic, 3.0, no_premises())
        .expect("acoustic");
    d.add_unary(reef, SourceClass::Acoustic, 2.8, no_premises())
        .expect("acoustic");
    assert!(d.is_live(wreath), "wrong word starts live on acoustics");
    assert!(d.is_dormant(reef), "correct word starts dormant");

    // Successor arrives from a live path; LM right context revives reef.
    let now = d.mint_token("now", TokenKind::Word, 10, 20).expect("token");
    d.add_unary(now, SourceClass::Acoustic, 2.0, no_premises())
        .expect("acoustic");
    d.add_sequence(wreath, now, 0.1, no_premises())
        .expect("seq");
    d.attach_context_for_new_token(now, &|a: &Token, b: &Token| {
        if a.label == "reef" && b.label == "now" {
            5.0
        } else {
            0.0
        }
    });
    assert!(d.is_live(reef), "right context revived the dormant token");
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Revived { token: t, via, .. } if *t == reef && *via == now
        )),
        "revival is recorded with its successor"
    );

    // Commit: reef tiling wins on S(T); wreath is defeated with provenance.
    let outcome = d.commit_region((0, 20), 20 + 10);
    let CommitOutcome::Committed {
        winner, defeated, ..
    } = outcome
    else {
        panic!("region must commit, got {outcome:?}");
    };
    assert_eq!(winner.tokens, vec![reef, now]);
    assert_eq!(defeated, vec![wreath]);
    assert!(d.fabric().is_active(reef));
    assert!(
        !d.fabric().is_active(wreath),
        "defeated token is deliberately retracted"
    );
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Defeated { token: t, margin, .. }
                if *t == wreath && matches!(margin, Some(m) if *m > 0.0)
        )),
        "defeat names the winner and the margin"
    );
    // Both supports remain in the record (provenance is never deleted).
    assert_eq!(d.active_unary_max(reef, SourceClass::Acoustic), Some(2.8));
    assert_eq!(d.active_unary_max(wreath, SourceClass::Acoustic), Some(3.0));
}
#[test]
fn unopposed_commit_margin_absent() {
    // §10, amendment 1.2: a winner unopposed among live tilings (rival is
    // dormant, excluded by §8 amendment 1.1) commits with margin None —
    // absence, never +inf. Mirrors output's regression scenario.
    let params = DecisionParams {
        theta_live: 6.0,
        delta: 0.5,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let wreath = d
        .mint_token("wreath", TokenKind::Word, 0, 10)
        .expect("token");
    let reef = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    let now = d.mint_token("now", TokenKind::Word, 10, 20).expect("token");
    d.add_unary(wreath, SourceClass::Acoustic, 3.0, no_premises())
        .expect("acoustic");
    d.add_unary(reef, SourceClass::Acoustic, 2.8, no_premises())
        .expect("acoustic");
    d.add_unary(reef, SourceClass::Lexical, 1.2, no_premises())
        .expect("lexical");
    d.add_unary(reef, SourceClass::Speaker, 0.6, no_premises())
        .expect("speaker");
    d.add_unary(now, SourceClass::Acoustic, 2.0, no_premises())
        .expect("acoustic");
    d.add_boundary_sequence(reef, 0.3).expect("boundary");
    d.add_boundary_sequence(wreath, 0.2).expect("boundary");
    d.add_sequence(wreath, now, 0.1, no_premises())
        .expect("seq");
    d.attach_context_for_new_token(now, &|a: &Token, b: &Token| {
        if a.label == "reef" && b.label == "now" {
            5.0
        } else {
            0.0
        }
    });
    let correction = d.fabric_mut().intern();
    d.fabric_mut().assert_assumption(correction);
    d.add_user(reef, correction).expect("user");
    assert!(d.is_dormant(wreath), "rival stays dormant below theta");
    assert!(d.is_live(reef), "context plus user revive the winner");
    let outcome = d.commit_region((0, 20), 20 + 10);
    let CommitOutcome::Committed {
        winner,
        runner_up,
        margin,
        defeated,
    } = outcome
    else {
        panic!("region must commit, got {outcome:?}");
    };
    assert_eq!(winner.tokens, vec![reef, now]);
    assert!(runner_up.is_none(), "no live runner-up tiling");
    assert_eq!(margin, None, "unopposed margin is absent, not +inf");
    assert_eq!(defeated, vec![wreath], "dormant rival still defeated");
    for e in d.events() {
        match e {
            DecisionEvent::Defeated { margin: m, .. }
            | DecisionEvent::Committed { margin: m, .. } => match m {
                None => {}
                Some(v) => assert!(v.is_finite(), "no non-finite margin: {e:?}"),
            },
            _ => {}
        }
    }
}
#[test]
fn conflict_tiling_threshold() {
    // §7: >50% of the shorter span conflicts; exactly 50% does not.
    let a = token(1, "a", 0, 10);
    let over = token(2, "b", 4, 14); // overlap 6 > 50% of 10
    let exact = token(3, "c", 5, 15); // overlap 5 == 50% of 10
    let apart = token(4, "d", 10, 20); // half-open: no overlap
    assert!(spans_conflict(&a, &over));
    assert!(!spans_conflict(&a, &exact), "exactly 50% is not a conflict");
    assert!(!spans_conflict(&a, &apart));

    // Zero-length spans are invalid tokens, rejected at emission (§7).
    let mut d = DecisionFabric::new(DecisionParams::default());
    assert_eq!(
        d.mint_token("bad", TokenKind::Word, 7, 7),
        Err(DecisionError::ZeroLengthSpan)
    );
}

#[test]
fn deterministic_tie_order() {
    // §10: equal S(T) -> earlier start -> lexicographic label -> lower id,
    // on repeated runs and shuffled insertion order.
    let build = |labels_in_order: &[&str]| {
        let mut d = DecisionFabric::new(DecisionParams::default());
        for label in labels_in_order {
            let t = d.mint_token(*label, TokenKind::Word, 0, 10).expect("token");
            d.add_unary(t, SourceClass::Acoustic, 1.5, no_premises())
                .expect("acoustic");
        }
        let sil = d.mint_token("<sil>", TokenKind::Sil, 10, 20).expect("sil");
        d.add_unary(sil, SourceClass::Acoustic, 0.0, no_premises())
            .expect("acoustic");
        d
    };
    for order in [&["beta", "alpha"][..], &["alpha", "beta"][..]] {
        let d = build(order);
        for _ in 0..3 {
            let (winner, _) = d.best_tilings((0, 20));
            let winner = winner.expect("tiling");
            assert_eq!(
                d.token(winner.tokens[0]).expect("token").label,
                "alpha",
                "lexicographic label breaks the tie regardless of insertion order"
            );
        }
    }
    // Same label, same span: the lower token id wins.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let first = d.mint_token("same", TokenKind::Word, 0, 10).expect("t");
    let second = d.mint_token("same", TokenKind::Word, 0, 10).expect("t");
    for t in [first, second] {
        d.add_unary(t, SourceClass::Acoustic, 1.0, no_premises())
            .expect("acoustic");
    }
    let sil = d.mint_token("<sil>", TokenKind::Sil, 10, 20).expect("sil");
    d.add_unary(sil, SourceClass::Acoustic, 0.0, no_premises())
        .expect("acoustic");
    let (winner, _) = d.best_tilings((0, 20));
    assert_eq!(winner.expect("tiling").tokens[0], first);
}

#[test]
fn max_per_class_absent_is_neutral_and_weakening() {
    // §3 rules 2-3 + §2 weakening: only the strongest active justification
    // per unary class counts; an absent class contributes nothing; a
    // retracted premise stops its llr counting (no score caching).
    let mut d = DecisionFabric::new(DecisionParams::default());
    let h = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    let premise = d.fabric_mut().intern();
    d.fabric_mut().assert_assumption(premise);
    let mut gated = BTreeSet::new();
    gated.insert(premise);
    d.add_unary(h, SourceClass::Acoustic, 1.0, no_premises())
        .expect("weak");
    d.add_unary(h, SourceClass::Acoustic, 4.0, gated)
        .expect("strong");
    assert_eq!(d.unary_sum(h), 4.0, "max per class, weak cannot stack");
    d.fabric_mut().retract(premise);
    assert_eq!(d.unary_sum(h), 1.0, "retracted premise weakens the llr");
}

#[test]
fn cascade_via_reverse_index() {
    // §9.4: a premise retraction re-scores dependents through the reverse
    // index; a revived token can go dormant again.
    let params = DecisionParams {
        theta_live: 3.0,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let left = d.mint_token("the", TokenKind::Word, 0, 5).expect("token");
    let h = d.mint_token("reef", TokenKind::Word, 5, 15).expect("token");
    d.add_unary(h, SourceClass::Acoustic, 1.0, no_premises())
        .expect("acoustic");
    let mut gated = BTreeSet::new();
    gated.insert(left);
    d.add_sequence(left, h, 5.0, gated).expect("seq");
    assert!(d.is_live(h), "left context holds the token live");
    assert!(d.dependents_of(left).contains(&h));
    d.fabric_mut().retract(left);
    assert!(d.is_dormant(h), "cascade drops the token back to dormant");
}

#[test]
fn user_correction_is_fixed_positive_and_retractable() {
    // §3 rules 4-5: USER is the fixed +U0, positive only, and retracting
    // the correction retracts the term.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let h = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    d.add_unary(h, SourceClass::Acoustic, 1.0, no_premises())
        .expect("acoustic");
    let correction = d.fabric_mut().intern();
    d.fabric_mut().assert_assumption(correction);
    d.add_user(h, correction).expect("user");
    let with_user = d.unary_sum(h);
    assert_eq!(with_user, 1.0 + d.params().u0);
    d.fabric_mut().retract(correction);
    assert_eq!(d.unary_sum(h), 1.0, "correction retracted, term absent");
}

#[test]
fn dormant_budget_eviction_order() {
    // §8: eviction drops lowest L(h) first and records provenance.
    let params = DecisionParams {
        theta_live: 10.0,
        dormant_budget: 1,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let low = d.mint_token("low", TokenKind::Word, 0, 10).expect("t");
    let high = d.mint_token("high", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(low, SourceClass::Acoustic, 1.0, no_premises())
        .expect("acoustic");
    d.add_unary(high, SourceClass::Acoustic, 2.0, no_premises())
        .expect("acoustic");
    d.enforce_budgets();
    assert!(!d.fabric().is_active(low), "lowest L evicted first");
    assert!(d.fabric().is_active(high));
    assert!(
        d.events()
            .iter()
            .any(|e| matches!(e, DecisionEvent::Evicted { token: t, .. } if *t == low))
    );
}
#[test]
fn dormant_token_excluded_from_tilings_before_revival() {
    // §8 exclusive reading (spec docs/design/a4-budget-semantics.md R1):
    // a dormant token must appear in neither winner nor runner-up.
    let params = DecisionParams {
        theta_live: 3.0,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let wreath = d
        .mint_token("wreath", TokenKind::Word, 0, 10)
        .expect("token");
    let reef = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    d.add_unary(wreath, SourceClass::Acoustic, 3.0, no_premises())
        .expect("acoustic");
    d.add_unary(reef, SourceClass::Acoustic, 2.8, no_premises())
        .expect("acoustic");
    assert!(d.is_live(wreath));
    assert!(d.is_dormant(reef), "correct word starts dormant");
    // No revival: reef must stay out of tilings entirely.
    let (winner, runner_up) = d.best_tilings((0, 10));
    for tiling in winner.iter().chain(runner_up.iter()) {
        assert!(
            !tiling.tokens.contains(&reef),
            "dormant token must not appear in tilings before revival"
        );
    }
    // Sanity: the live token still wins.
    let w = winner.expect("live tiling wins");
    assert_eq!(w.tokens, vec![wreath]);
}
#[test]
fn live_budget_constrains_candidate_pool() {
    // R2 (spec docs/design/a4-budget-semantics.md): over-budget live pool
    // is pruned lowest-L(h) first with budget attribution.
    let params = DecisionParams {
        theta_live: 3.0,
        live_budget: 1,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let strong = d.mint_token("strong", TokenKind::Word, 0, 10).expect("t");
    let weak = d.mint_token("weak", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(strong, SourceClass::Acoustic, 3.5, no_premises())
        .expect("acoustic");
    d.add_unary(weak, SourceClass::Acoustic, 3.0, no_premises())
        .expect("acoustic");
    assert_eq!(d.live_tokens().len(), 2);
    d.enforce_budgets();
    assert_eq!(d.live_tokens(), vec![strong], "lowest L evicted first");
    assert!(!d.fabric().is_active(weak));
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Evicted {
                token: t,
                budget: BudgetKind::Live,
                ..
            } if *t == weak
        )),
        "live eviction is attributed to the live budget"
    );
}
