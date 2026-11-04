//! Tileable-ends consistency/completeness (f6tp).
use fabric::decision::{Arm, DecisionFabric, DecisionParams, SourceClass, TokenKind};
use std::collections::BTreeSet;

fn build(params: DecisionParams) -> DecisionFabric {
    // 4-word chain 0-10-20-30-40 plus a detached token at 50-60 (gap:
    // nothing starts at 40, so 50/60 are unreachable from 0).
    let mut d = DecisionFabric::new(params);
    let mut prev = None;
    for (i, (s, e)) in [(0, 10), (10, 20), (20, 30), (30, 40)].iter().enumerate() {
        let f = d
            .mint_token(format!("w{i}"), TokenKind::Word, *s, *e)
            .unwrap();
        d.add_unary(f, SourceClass::Acoustic, 2.0, BTreeSet::new())
            .unwrap();
        if let Some(p) = prev {
            let mut pre = BTreeSet::new();
            pre.insert(p);
            pre.insert(f);
            d.add_sequence(p, f, 1.0, pre).unwrap();
            // §7 v1.4: the chain rides lattice arcs.
            d.add_lattice_arc(p, f).unwrap();
        }
        prev = Some(f);
    }
    let g = d
        .mint_token("gap".to_string(), TokenKind::Word, 50, 60)
        .unwrap();
    d.add_unary(g, SourceClass::Acoustic, 2.0, BTreeSet::new())
        .unwrap();
    d
}

#[test]
fn tileable_ends_match_chain_coverage() {
    let d = build(DecisionParams::default());
    assert_eq!(d.tileable_ends(0, 60), vec![10, 20, 30, 40]);
    assert_eq!(d.tileable_ends(0, 25), vec![10, 20]);
    assert_eq!(d.tileable_ends(40, 60), Vec::<u32>::new());
    assert_eq!(d.tileable_ends(50, 60), vec![60]);
    assert_eq!(d.tileable_ends(60, 60), Vec::<u32>::new());
}

#[test]
fn tileable_ends_consistent_with_best_tilings() {
    let d = build(DecisionParams::default());
    let ends = d.tileable_ends(0, 60);
    // Consistency: every returned end tiles.
    for e in &ends {
        assert!(d.best_tilings((0, *e)).0.is_some(), "end {e} must tile");
    }
    // Completeness: every lattice end that tiles is returned.
    for e in [10u32, 20, 30, 40, 50, 60] {
        assert_eq!(
            d.best_tilings((0, e)).0.is_some(),
            ends.contains(&e),
            "end {e}"
        );
    }
    // Largest-first equivalence: the bridge's old scan picked the max.
    assert_eq!(ends.into_iter().max(), Some(40));
}

#[test]
fn tileable_ends_consistent_after_eviction() {
    let mut d = build(DecisionParams {
        arm: Arm::B,
        live_budget: 2,
        dormant_budget: 0,
        combined_budget: None,
        ..Default::default()
    });
    d.enforce_budgets();
    // Eviction may drop coverage anywhere; whatever remains must agree.
    let ends = d.tileable_ends(0, 60);
    for e in &ends {
        assert!(d.best_tilings((0, *e)).0.is_some(), "end {e} must tile");
    }
    for e in [10u32, 20, 30, 40, 50, 60] {
        assert_eq!(
            d.best_tilings((0, e)).0.is_some(),
            ends.contains(&e),
            "end {e}"
        );
    }
}
