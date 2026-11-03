//! Targeted index-edge tests (PERF nlvs): cases the random differential
//! driver covers only thinly — committed-token pruning, external token
//! retraction, frame-window edges, degenerate regions, and deep eviction
//! order. New arm vs reference arm must agree exactly.

#[allow(dead_code)]
#[path = "support/reference_decision.rs"]
mod reference_decision;

use fabric::FactId;
use fabric::decision as new;
use reference_decision as old;
use std::collections::BTreeSet;

fn no_premises() -> BTreeSet<FactId> {
    BTreeSet::new()
}

fn params_c() -> (new::DecisionParams, old::DecisionParams) {
    (
        new::DecisionParams::default(),
        old::DecisionParams::default(),
    )
}

fn params_b() -> (new::DecisionParams, old::DecisionParams) {
    let mut n = new::DecisionParams::default();
    n.arm = new::Arm::B;
    let mut o = old::DecisionParams::default();
    o.arm = old::Arm::B;
    (n, o)
}

#[test]
fn committed_winner_below_theta_pruned_identically() {
    // Arm B: a committed winner whose liveness later drops below theta is
    // beam-pruned — the open set must include committed tokens for this.
    let (mut pn, mut po) = params_b();
    pn.theta_live = 1.0;
    po.theta_live = 1.0;
    let mut n = new::DecisionFabric::new(pn);
    let mut o = old::DecisionFabric::new(po);
    let gate_n = n.fabric_mut().intern();
    n.fabric_mut().assert_assumption(gate_n);
    let gate_o = o.fabric_mut().intern();
    o.fabric_mut().assert_assumption(gate_o);
    assert_eq!(gate_n, gate_o);
    let a_n = n.mint_token("a", new::TokenKind::Word, 0, 10).unwrap();
    let a_o = o.mint_token("a", old::TokenKind::Word, 0, 10).unwrap();
    assert_eq!(a_n, a_o);
    let mut ps = BTreeSet::new();
    ps.insert(gate_n);
    n.add_unary(a_n, new::SourceClass::Acoustic, 5.0, ps.clone())
        .unwrap();
    o.add_unary(a_o, old::SourceClass::Acoustic, 5.0, ps)
        .unwrap();
    n.add_boundary_sequence(a_n, 1.0).unwrap();
    o.add_boundary_sequence(a_o, 1.0).unwrap();
    let rn = n.commit_region((0, 10), 100);
    let ro = o.commit_region((0, 10), 100);
    assert_eq!(format!("{rn:?}"), format!("{ro:?}"));
    assert!(matches!(rn, new::CommitOutcome::Committed { .. }));
    // Drop the winner below theta by retracting its gate premise.
    n.fabric_mut().retract(gate_n);
    o.fabric_mut().retract(gate_o);
    n.enforce_budgets();
    o.enforce_budgets();
    assert_eq!(
        format!("{:?}", n.events()),
        format!("{:?}", o.events()),
        "prune of committed winner must match"
    );
    assert!(
        format!("{:?}", n.events()).contains("Pruned"),
        "committed winner is pruned: {:?}",
        n.events()
    );
}

#[test]
fn external_token_retract_then_queries_match() {
    // External retraction of token facts via fabric_mut must not desync the
    // open set: every query still matches the reference.
    let (pn, po) = params_c();
    let mut n = new::DecisionFabric::new(pn);
    let mut o = old::DecisionFabric::new(po);
    let mut toks = Vec::new();
    for i in 0..12u64 {
        let s = (i * 5) as u32;
        let a = n
            .mint_token(format!("w{i}"), new::TokenKind::Word, s, s + 8)
            .unwrap();
        let b = o
            .mint_token(format!("w{i}"), old::TokenKind::Word, s, s + 8)
            .unwrap();
        assert_eq!(a, b);
        toks.push(a);
        n.add_unary(
            a,
            new::SourceClass::Acoustic,
            1.0 + i as f64 * 0.1,
            no_premises(),
        )
        .unwrap();
        o.add_unary(
            b,
            old::SourceClass::Acoustic,
            1.0 + i as f64 * 0.1,
            no_premises(),
        )
        .unwrap();
        if i > 0 {
            let mut ps = BTreeSet::new();
            ps.insert(toks[(i - 1) as usize]);
            ps.insert(a);
            n.add_sequence(toks[(i - 1) as usize], a, 0.5, ps.clone())
                .unwrap();
            o.add_sequence(toks[(i - 1) as usize], b, 0.5, ps).unwrap();
        }
    }
    // Retract every third token externally, then compare all queries.
    for t in toks.iter().step_by(3) {
        n.fabric_mut().retract(*t);
        o.fabric_mut().retract(*t);
    }
    assert_eq!(n.live_tokens(), o.live_tokens());
    assert_eq!(n.dormant_tokens(), o.dormant_tokens());
    for t in &toks {
        assert_eq!(n.liveness(*t).to_bits(), o.liveness(*t).to_bits());
        assert_eq!(n.is_live(*t), o.is_live(*t));
        assert_eq!(n.is_dormant(*t), o.is_dormant(*t));
        assert_eq!(n.dependents_of(*t), o.dependents_of(*t));
        assert_eq!(n.lattice_successors(*t), o.lattice_successors(*t));
        assert_eq!(
            format!("{:?}", n.active_unary_evidence(*t)),
            format!("{:?}", o.active_unary_evidence(*t))
        );
    }
    assert_eq!(
        format!("{:?}", n.best_tilings((0, 68))),
        format!("{:?}", o.best_tilings((0, 68)))
    );
    n.enforce_budgets();
    o.enforce_budgets();
    assert_eq!(format!("{:?}", n.events()), format!("{:?}", o.events()));
    assert_eq!(n.live_tokens(), o.live_tokens());
    assert_eq!(n.dormant_tokens(), o.dormant_tokens());
}

#[test]
fn frame_window_edges_match() {
    // J-window clamping: J=0, huge J, tokens at frame 0. Successor order
    // must equal the old full-scan order (ascending token id).
    for j in [0u32, 1, 3, 100, u32::MAX] {
        let mut pn = new::DecisionParams::default();
        pn.j = j;
        let mut po = old::DecisionParams::default();
        po.j = j;
        let mut n = new::DecisionFabric::new(pn);
        let mut o = old::DecisionFabric::new(po);
        let spans = [(0, 1), (0, 5), (1, 2), (4, 9), (30, 31), (100, 200)];
        let mut toks = Vec::new();
        for (k, (s, e)) in spans.iter().enumerate() {
            let a = n
                .mint_token(format!("t{k}"), new::TokenKind::Word, *s, *e)
                .unwrap();
            let b = o
                .mint_token(format!("t{k}"), old::TokenKind::Word, *s, *e)
                .unwrap();
            assert_eq!(a, b);
            toks.push(a);
            n.add_unary(a, new::SourceClass::Acoustic, -1.0, no_premises())
                .unwrap();
            o.add_unary(b, old::SourceClass::Acoustic, -1.0, no_premises())
                .unwrap();
        }
        for t in &toks {
            assert_eq!(
                n.lattice_successors(*t),
                o.lattice_successors(*t),
                "successors J={j} token {}",
                t.0
            );
            // Ascending token id order (old full-scan order).
            let mut sorted = n.lattice_successors(*t);
            sorted.sort_unstable();
            assert_eq!(n.lattice_successors(*t), sorted);
        }
        for t in &toks {
            n.attach_context_for_new_token(*t, &|_, _| 2.0);
            o.attach_context_for_new_token(*t, &|_, _| 2.0);
        }
        assert_eq!(
            format!("{:?}", n.events()),
            format!("{:?}", o.events()),
            "attach events J={j}"
        );
        for t in &toks {
            assert_eq!(
                n.liveness(*t).to_bits(),
                o.liveness(*t).to_bits(),
                "liveness J={j} token {}",
                t.0
            );
        }
    }
}

#[test]
fn degenerate_regions_match_without_panic() {
    // Empty/inverted regions: no panic, same (empty) tilings as reference.
    let (pn, po) = params_c();
    let mut n = new::DecisionFabric::new(pn);
    let mut o = old::DecisionFabric::new(po);
    let a = n.mint_token("a", new::TokenKind::Word, 0, 10).unwrap();
    let b = o.mint_token("a", old::TokenKind::Word, 0, 10).unwrap();
    assert_eq!(a, b);
    n.add_unary(a, new::SourceClass::Acoustic, 3.0, no_premises())
        .unwrap();
    o.add_unary(b, old::SourceClass::Acoustic, 3.0, no_premises())
        .unwrap();
    for region in [(5, 5), (9, 3), (10, 10), (0, 0), (u32::MAX, u32::MAX)] {
        assert_eq!(
            format!("{:?}", n.best_tilings(region)),
            format!("{:?}", o.best_tilings(region)),
            "region {region:?}"
        );
    }
    // Commit on a degenerate region matches too (NoTiling path).
    let rn = n.commit_region((5, 5), 100);
    let ro = o.commit_region((5, 5), 100);
    assert_eq!(format!("{rn:?}"), format!("{ro:?}"));
}

#[test]
fn deep_eviction_order_matches() {
    // 150 tokens under tiny budgets: the full eviction sequence (victim
    // order, logged liveness bits, budget labels) must match exactly.
    // Heavy endpoint premises stress the neighbor-recompute path.
    let mut pn = new::DecisionParams::default();
    pn.live_budget = 3;
    pn.dormant_budget = 3;
    pn.combined_budget = Some(5);
    pn.theta_live = 0.5;
    let mut po = old::DecisionParams::default();
    po.live_budget = 3;
    po.dormant_budget = 3;
    po.combined_budget = Some(5);
    po.theta_live = 0.5;
    let mut n = new::DecisionFabric::new(pn);
    let mut o = old::DecisionFabric::new(po);
    let mut toks = Vec::new();
    for i in 0..150u64 {
        let s = (i * 2) as u32;
        let label = format!("w{}", i % 7);
        let a = n
            .mint_token(label.clone(), new::TokenKind::Word, s, s + 5)
            .unwrap();
        let b = o.mint_token(label, old::TokenKind::Word, s, s + 5).unwrap();
        assert_eq!(a, b);
        toks.push(a);
        let llr = ((i * 37) % 200) as f64 / 100.0 - 1.0;
        n.add_unary(a, new::SourceClass::Acoustic, llr, no_premises())
            .unwrap();
        o.add_unary(b, old::SourceClass::Acoustic, llr, no_premises())
            .unwrap();
        if i > 0 {
            let f = toks[(i - 1) as usize];
            let mut ps = BTreeSet::new();
            ps.insert(f);
            ps.insert(a);
            let sllr = ((i * 53) % 150) as f64 / 100.0 - 0.5;
            n.add_sequence(f, a, sllr, ps.clone()).unwrap();
            o.add_sequence(f, b, sllr, ps).unwrap();
        }
        if i % 10 == 9 {
            n.enforce_budgets();
            o.enforce_budgets();
            assert_eq!(
                format!("{:?}", n.events()),
                format!("{:?}", o.events()),
                "events at token {i}"
            );
        }
    }
    n.enforce_budgets();
    o.enforce_budgets();
    assert_eq!(
        format!("{:?}", n.events()),
        format!("{:?}", o.events()),
        "final eviction sequence"
    );
    assert_eq!(n.live_tokens(), o.live_tokens());
    assert_eq!(n.dormant_tokens(), o.dormant_tokens());
    let evicted_n = n
        .events()
        .iter()
        .filter(|e| matches!(e, new::DecisionEvent::Evicted { .. }))
        .count();
    assert!(evicted_n > 100, "deep eviction: {evicted_n} evicted");
}

#[test]
fn counts_open_set_and_stats_are_sane() {
    // New-arm-only API sanity: single-pass counts match the filters, the
    // open set tracks active tokens, and stats counters move.
    let mut p = new::DecisionParams::default();
    p.live_budget = 4;
    p.dormant_budget = 4;
    let mut d = new::DecisionFabric::new(p);
    let mut toks = Vec::new();
    for i in 0..20u64 {
        let s = (i * 3) as u32;
        let t = d
            .mint_token(format!("w{i}"), new::TokenKind::Word, s, s + 6)
            .unwrap();
        toks.push(t);
        let llr = (i as f64) - 10.0;
        d.add_unary(t, new::SourceClass::Acoustic, llr, no_premises())
            .unwrap();
    }
    assert_eq!(d.open_set_len(), 20);
    assert_eq!(d.peak_open_set(), 20);
    let (live, dormant) = d.live_dormant_counts();
    assert_eq!(live, d.live_tokens().len());
    assert_eq!(dormant, d.dormant_tokens().len());
    d.enforce_budgets();
    let (live2, dormant2) = d.live_dormant_counts();
    assert_eq!(live2, d.live_tokens().len());
    assert_eq!(dormant2, d.dormant_tokens().len());
    assert!(d.open_set_len() < 20, "evictions shrink the open set");
    assert_eq!(d.peak_open_set(), 20, "peak is sticky");
    // Memoized liveness no longer funnels through the unary path; drive it
    // directly so the unary-lookup counter assertion stays meaningful.
    let _ = d.unary_sum(toks[0]);
    let st = d.stats_snapshot();
    assert!(st.liveness_calls > 0);
    assert!(st.unary_lookups > 0);
    assert!(st.heap_pushes > 0);
    assert!(st.heap_pops > 0);
    assert!(st.evicted > 0);
    assert_eq!(st.pruned, 0, "arm C never prunes");
}
