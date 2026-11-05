//! Property tests for the fabric decision contract, spec version 1.0
//! (`docs/design/fabric-decision-contract.md`, bead hearsay-four-dg8 / A4b).
//! Covers spec §13 items 6: LM-accounting byte-identity (§6.4),
//! dormancy/revival/defeat (§8-§10), conflict tiling (§7), deterministic
//! tie order (§10), plus the §3/§9.4 evidence rules they rest on.

use fabric::FactId;
use fabric::decision::{
    Arm, BudgetKind, CommitOutcome, DecisionError, DecisionEvent, DecisionFabric, DecisionParams,
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
    assert_eq!(SPEC_VERSION, "1.5");
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
fn lexical_class_shares_sequence_weight() {
    // Contract v1.5, section 3: LEXICAL carries ln P_unigram and must
    // share the SEQUENCE weight so the unigram cancel is exact; the
    // w_lexical field is retained for struct compatibility and ignored.
    let mut p = DecisionParams::default();
    p.w_sequence = 6.0;
    p.w_lexical = 1.0;
    assert_eq!(p.weight(SourceClass::Lexical), 6.0);
    assert_eq!(p.weight(SourceClass::Sequence), 6.0);
    let mut d = DecisionFabric::new(p);
    let h = d.mint_token("reef", TokenKind::Word, 0, 10).expect("token");
    d.add_unary(h, SourceClass::Lexical, 2.0, no_premises())
        .expect("lexical");
    assert_eq!(d.unary_sum(h), 12.0, "6.0 * 2.0, not 1.0 * 2.0");
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
    // §7 v1.4: intended tiling pairs ride lattice arcs.
    d.add_lattice_arc(wreath, now).expect("arc");
    d.add_lattice_arc(reef, now).expect("arc");
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
    // §7 v1.4: intended tiling pairs ride lattice arcs.
    d.add_lattice_arc(wreath, now).expect("arc");
    d.add_lattice_arc(reef, now).expect("arc");
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
        let mut firsts = Vec::new();
        for label in labels_in_order {
            let t = d.mint_token(*label, TokenKind::Word, 0, 10).expect("token");
            d.add_unary(t, SourceClass::Acoustic, 1.5, no_premises())
                .expect("acoustic");
            firsts.push(t);
        }
        let sil = d.mint_token("<sil>", TokenKind::Sil, 10, 20).expect("sil");
        d.add_unary(sil, SourceClass::Acoustic, 0.0, no_premises())
            .expect("acoustic");
        // §7 v1.4: intended tiling pairs ride lattice arcs.
        for t in firsts {
            d.add_lattice_arc(t, sil).expect("arc");
        }
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
    // §7 v1.4: intended tiling pairs ride lattice arcs.
    d.add_lattice_arc(first, sil).expect("arc");
    d.add_lattice_arc(second, sil).expect("arc");
    let (winner, _) = d.best_tilings((0, 20));
    assert_eq!(winner.expect("tiling").tokens[0], first);
}

#[test]
fn tiling_follows_arcs_not_frames() {
    // §7 v1.4: adjacent tiling tokens must ride a lattice arc, however
    // hot the arc-free alternative is acoustically. Pre-fix the DP
    // stitches any abutting pair and junk (9.0) beats the arc path.
    let mut d = DecisionFabric::new(DecisionParams::default());
    let a = d.mint_token("a", TokenKind::Word, 0, 10).expect("t");
    let good = d.mint_token("good", TokenKind::Word, 10, 20).expect("t");
    let junk = d.mint_token("junk", TokenKind::Word, 10, 20).expect("t");
    d.add_unary(a, SourceClass::Acoustic, 1.0, no_premises())
        .expect("a");
    d.add_unary(good, SourceClass::Acoustic, 1.0, no_premises())
        .expect("a");
    d.add_unary(junk, SourceClass::Acoustic, 9.0, no_premises())
        .expect("a");
    d.add_lattice_arc(a, good).expect("arc");
    let (winner, runner_up) = d.best_tilings((0, 20));
    let winner = winner.expect("arc path tiles");
    assert_eq!(winner.tokens, vec![a, good]);
    assert!(
        runner_up.is_none(),
        "arc-free junk forms no tiling at all, so no runner-up"
    );
    // And the control: the same hot pair WITH an arc tiles and wins.
    let mut e = DecisionFabric::new(DecisionParams::default());
    let a2 = e.mint_token("a", TokenKind::Word, 0, 10).expect("t");
    let junk2 = e.mint_token("junk", TokenKind::Word, 10, 20).expect("t");
    e.add_unary(a2, SourceClass::Acoustic, 1.0, no_premises())
        .expect("a");
    e.add_unary(junk2, SourceClass::Acoustic, 9.0, no_premises())
        .expect("a");
    e.add_lattice_arc(a2, junk2).expect("arc");
    let (w2, _) = e.best_tilings((0, 20));
    assert_eq!(w2.expect("arced pair tiles").tokens, vec![a2, junk2]);
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

#[test]
fn decision_arm_b_prunes_with_no_revive() {
    // §11 arm B: below-threshold tokens are retracted at once and never
    // revived, even when the late evidence that revives arm C arrives.
    // Identical inputs to decision_arm_c_revives_on_identical_late_evidence.
    let params = DecisionParams {
        arm: Arm::B,
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
    assert!(d.is_live(wreath));
    assert!(!d.is_dormant(reef), "arm B has no dormant set");
    d.enforce_budgets();
    assert!(
        !d.fabric().is_active(reef),
        "below-threshold token retracted at once"
    );
    assert!(
        d.events()
            .iter()
            .any(|e| matches!(e, DecisionEvent::Pruned { token: t, .. } if *t == reef)),
        "prune is recorded in provenance"
    );

    // The same late evidence that revives arm C arrives: arm B ignores it.
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
    assert!(
        !d.fabric().is_active(reef),
        "pruned token never revives on late evidence"
    );
    assert!(
        d.active_sequence_max(Some(reef), now).is_none(),
        "pruned token never receives later evidence"
    );
    assert!(
        !d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Revived { token: t, .. } if *t == reef
        )),
        "no revival is recorded for a pruned token"
    );

    // The next sweep prunes the below-threshold successor too: arm B
    // keeps live tokens only.
    d.enforce_budgets();
    assert!(
        !d.fabric().is_active(now),
        "below-threshold successor pruned at the next sweep"
    );
    assert!(
        d.events()
            .iter()
            .any(|e| matches!(e, DecisionEvent::Pruned { token: t, .. } if *t == now)),
        "successor prune is recorded in provenance"
    );

    // Commit the surviving span: wreath won alone (arm C commits
    // [reef, now] on these same inputs — the rescue is the treatment
    // effect). Pruned tokens are neither winners nor defeated: they
    // left via prune, not defeat.
    let outcome = d.commit_region((0, 10), 10 + 10);
    let CommitOutcome::Committed {
        winner, defeated, ..
    } = outcome
    else {
        panic!("region must commit, got {outcome:?}");
    };
    assert_eq!(winner.tokens, vec![wreath]);
    assert!(defeated.is_empty(), "pruned tokens are not re-defeated");
    assert!(
        !d.events()
            .iter()
            .any(|e| matches!(e, DecisionEvent::Revived { .. })),
        "no revival is ever recorded in arm B"
    );
}

#[test]
fn decision_arm_c_revives_on_identical_late_evidence() {
    // §11 arm C on the IDENTICAL inputs as arm B above: the
    // below-threshold token goes dormant, then revives on late evidence.
    let params = DecisionParams {
        arm: Arm::C,
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
    assert!(d.is_live(wreath));
    assert!(d.is_dormant(reef), "correct word starts dormant");
    d.enforce_budgets();
    assert!(d.fabric().is_active(reef), "arm C retains dormant tokens");
    assert!(
        !d.events()
            .iter()
            .any(|e| matches!(e, DecisionEvent::Pruned { .. })),
        "arm C never prunes"
    );

    let now = d.mint_token("now", TokenKind::Word, 10, 20).expect("token");
    d.add_unary(now, SourceClass::Acoustic, 2.0, no_premises())
        .expect("acoustic");
    d.add_sequence(wreath, now, 0.1, no_premises())
        .expect("seq");
    // §7 v1.4: intended tiling pairs ride lattice arcs.
    d.add_lattice_arc(wreath, now).expect("arc");
    d.add_lattice_arc(reef, now).expect("arc");
    d.attach_context_for_new_token(now, &|a: &Token, b: &Token| {
        if a.label == "reef" && b.label == "now" {
            5.0
        } else {
            0.0
        }
    });
    assert!(d.is_live(reef), "late evidence revives the dormant token");
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Revived { token: t, via, .. } if *t == reef && *via == now
        )),
        "revival is recorded with its successor"
    );

    let outcome = d.commit_region((0, 20), 20 + 10);
    let CommitOutcome::Committed {
        winner, defeated, ..
    } = outcome
    else {
        panic!("region must commit, got {outcome:?}");
    };
    assert_eq!(winner.tokens, vec![reef, now]);
    assert_eq!(defeated, vec![wreath]);
}

#[test]
fn decision_arms_b_and_c_score_identically() {
    // §11: scoring, evidence, and commit rule are IDENTICAL across arms;
    // only the below-threshold fate differs.
    assert_eq!(
        DecisionParams::default().arm,
        Arm::C,
        "frozen default arm is C (existing behavior)"
    );
    let build = |arm: Arm| {
        let params = DecisionParams {
            arm,
            theta_live: 0.0,
            delta: 0.5,
            ..DecisionParams::default()
        };
        let mut d = DecisionFabric::new(params);
        let wreath = d.mint_token("wreath", TokenKind::Word, 0, 10).expect("t");
        let reef = d.mint_token("reef", TokenKind::Word, 0, 10).expect("t");
        let now = d.mint_token("now", TokenKind::Word, 10, 20).expect("t");
        d.add_unary(wreath, SourceClass::Acoustic, 3.0, no_premises())
            .expect("a");
        d.add_unary(reef, SourceClass::Acoustic, 2.8, no_premises())
            .expect("a");
        d.add_unary(now, SourceClass::Acoustic, 2.0, no_premises())
            .expect("a");
        d.add_sequence(wreath, now, 0.1, no_premises()).expect("s");
        d.add_sequence(reef, now, 5.0, no_premises()).expect("s");
        // §7 v1.4: intended tiling pairs ride lattice arcs.
        d.add_lattice_arc(wreath, now).expect("arc");
        d.add_lattice_arc(reef, now).expect("arc");
        d.enforce_budgets();
        (d, wreath, reef, now)
    };
    let (mut b, bw, br, bn) = build(Arm::B);
    let (mut c, cw, cr, cn) = build(Arm::C);
    // Same evidence state (nothing below threshold): same scores.
    assert_eq!(b.unary_sum(bw), c.unary_sum(cw));
    assert_eq!(b.unary_sum(br), c.unary_sum(cr));
    assert_eq!(b.liveness(bn), c.liveness(cn));
    assert_eq!(b.tiling_score(&[bw, bn]), c.tiling_score(&[cw, cn]));
    assert_eq!(b.tiling_score(&[br, bn]), c.tiling_score(&[cr, cn]));
    let (_, b_totals, _) = b.tiling_breakdown(&[br, bn]);
    let (_, c_totals, _) = c.tiling_breakdown(&[cr, cn]);
    assert_eq!(b_totals, c_totals);
    // Same commit rule: same winner, same margin.
    let bo = b.commit_region((0, 20), 30);
    let co = c.commit_region((0, 20), 30);
    let CommitOutcome::Committed {
        winner: wb,
        margin: mb,
        ..
    } = bo
    else {
        panic!("arm B must commit, got {bo:?}");
    };
    let CommitOutcome::Committed {
        winner: wc,
        margin: mc,
        ..
    } = co
    else {
        panic!("arm C must commit, got {co:?}");
    };
    assert_eq!(wb.score, wc.score);
    assert_eq!(mb, mc);
    // Same labels in the same order (FactIds differ across fabrics).
    let bl: Vec<_> = wb
        .tokens
        .iter()
        .map(|t| b.token(*t).unwrap().label.clone())
        .collect();
    let cl: Vec<_> = wc
        .tokens
        .iter()
        .map(|t| c.token(*t).unwrap().label.clone())
        .collect();
    assert_eq!(bl, cl);
}

#[test]
fn decision_arm_b_combined_budget_counts_live_only() {
    // §11 table: arm B counts live tokens only against the combined total.
    let params = DecisionParams {
        arm: Arm::B,
        theta_live: 0.0,
        combined_budget: Some(1),
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let strong = d.mint_token("strong", TokenKind::Word, 0, 10).expect("t");
    let weak = d.mint_token("weak", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(strong, SourceClass::Acoustic, 3.5, no_premises())
        .expect("a");
    d.add_unary(weak, SourceClass::Acoustic, 3.0, no_premises())
        .expect("a");
    d.enforce_budgets();
    assert!(d.dormant_tokens().is_empty(), "arm B has no dormant set");
    assert_eq!(d.live_tokens(), vec![strong]);
    assert!(!d.fabric().is_active(weak));
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Evicted {
                token: t,
                budget: BudgetKind::Combined,
                ..
            } if *t == weak
        )),
        "live eviction is attributed to the combined budget"
    );
}

#[test]
fn decision_arm_c_combined_budget_counts_live_plus_dormant() {
    // §11 table: arm C counts live + dormant against the SAME total.
    let params = DecisionParams {
        arm: Arm::C,
        theta_live: 3.2,
        combined_budget: Some(1),
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let strong = d.mint_token("strong", TokenKind::Word, 0, 10).expect("t");
    let weak = d.mint_token("weak", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(strong, SourceClass::Acoustic, 3.5, no_premises())
        .expect("a");
    d.add_unary(weak, SourceClass::Acoustic, 3.0, no_premises())
        .expect("a");
    assert!(d.is_live(strong));
    assert!(d.is_dormant(weak));
    d.enforce_budgets();
    // live + dormant = 2 > 1: the dormant token is evicted, live survives.
    assert_eq!(d.live_tokens(), vec![strong]);
    assert!(!d.fabric().is_active(weak));
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Evicted {
                token: t,
                budget: BudgetKind::Combined,
                ..
            } if *t == weak
        )),
        "dormant eviction is attributed to the combined budget"
    );
}
#[test]
fn sealed_token_never_evicted_from_live_budget() {
    // §8 v1.3: sealed tokens stay for tiling but are exempt from budgets
    // and eviction. New-sealed pattern (far first, then old with a small
    // end) keeps the sealed entry in the victim heap so the eviction
    // skip is load-bearing. Falsification: removing the sealed-skip in
    // eviction victim selection must fail this test (old has lowest L).
    let params = DecisionParams {
        theta_live: 0.0,
        live_budget: 1,
        j: 3,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let far1 = d.mint_token("far1", TokenKind::Word, 20, 30).expect("t");
    d.add_unary(far1, SourceClass::Acoustic, 2.0, no_premises())
        .expect("a");
    let far2 = d.mint_token("far2", TokenKind::Word, 20, 30).expect("t");
    d.add_unary(far2, SourceClass::Acoustic, 3.0, no_premises())
        .expect("a");
    let old = d.mint_token("old", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(old, SourceClass::Acoustic, 1.0, no_premises())
        .expect("a");
    assert_eq!(d.intake_frontier(), 30);
    assert!(d.is_sealed(old), "frontier 30 > 10 + 3");
    assert!(!d.is_sealed(far1));
    assert!(!d.is_sealed(far2));
    assert!(d.is_live(old), "sealed stays live for tiling");
    d.enforce_budgets();
    assert!(
        d.fabric().is_active(old),
        "sealed with lowest L must survive"
    );
    assert!(!d.fabric().is_active(far1), "lowest unsealed evicted");
    assert!(d.fabric().is_active(far2));
    assert!(
        d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Evicted {
                token: t,
                budget: BudgetKind::Live,
                ..
            } if *t == far1
        )),
        "unsealed eviction attributed to live budget"
    );
    assert!(
        !d.events().iter().any(|e| matches!(
            e,
            DecisionEvent::Evicted { token: t, .. } if *t == old
        )),
        "sealed must never appear in eviction log"
    );
    // Sealed still tiles (§8 v1.3: tiling/commit unchanged).
    let (winner, _) = d.best_tilings((0, 10));
    let w = winner.expect("sealed live tiling wins");
    assert!(w.tokens.contains(&old));
}
#[test]
fn sealed_token_never_evicted_from_dormant_budget() {
    // §8 v1.3: sealed dormant exempt from dormant/combined tallies.
    // New-sealed pattern (far first) keeps the sealed entry in the
    // victim heap so the dormant-loop skip is load-bearing.
    let params = DecisionParams {
        arm: Arm::C,
        theta_live: 10.0,
        dormant_budget: 1,
        j: 3,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let far1 = d.mint_token("far1", TokenKind::Word, 20, 30).expect("t");
    d.add_unary(far1, SourceClass::Acoustic, 2.0, no_premises())
        .expect("a");
    let far2 = d.mint_token("far2", TokenKind::Word, 20, 30).expect("t");
    d.add_unary(far2, SourceClass::Acoustic, 3.0, no_premises())
        .expect("a");
    let old = d.mint_token("old", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(old, SourceClass::Acoustic, 1.0, no_premises())
        .expect("a");
    assert!(d.is_sealed(old));
    assert!(d.is_dormant(old));
    d.enforce_budgets();
    assert!(d.fabric().is_active(old), "sealed dormant survives");
    assert!(!d.fabric().is_active(far1));
    assert!(d.fabric().is_active(far2));
}
#[test]
fn sealed_token_exempt_from_arm_b_prune() {
    // §8 v1.3: sealed below-threshold exempt from arm-B beam prune.
    // New-sealed pattern (far first) for consistency with eviction tests.
    let params = DecisionParams {
        arm: Arm::B,
        theta_live: 10.0,
        j: 3,
        ..DecisionParams::default()
    };
    let mut d = DecisionFabric::new(params);
    let far = d.mint_token("far", TokenKind::Word, 20, 30).expect("t");
    d.add_unary(far, SourceClass::Acoustic, 2.0, no_premises())
        .expect("a");
    let old = d.mint_token("old", TokenKind::Word, 0, 10).expect("t");
    d.add_unary(old, SourceClass::Acoustic, 1.0, no_premises())
        .expect("a");
    assert!(d.is_sealed(old));
    assert!(!d.is_sealed(far));
    d.enforce_budgets();
    assert!(d.fabric().is_active(old), "sealed survives prune");
    assert!(!d.fabric().is_active(far), "unsealed pruned");
    assert!(d.events().iter().any(|e| matches!(
        e,
        DecisionEvent::Pruned { token: t, .. } if *t == far
    )));
    assert!(!d.events().iter().any(|e| matches!(
        e,
        DecisionEvent::Pruned { token: t, .. } if *t == old
    )));
}
