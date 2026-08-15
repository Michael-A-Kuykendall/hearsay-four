//! d73.7 gate: `cd src/stage3 && cargo test --all-features`
//!
//! Contract test 'stage3 commit window' over the invariants from:
//! - retraction-survival-semantics.md §5 (provenance, survival, promotion,
//!   cascade bounded)
//! - hearsay-confidence-formulas.md §5 (rating_in_range, evidence_additive,
//!   calibration_consistent)
//! - phonological-formalism.md §5 (rule_content_is_spe, executed_as_fst,
//!   rewrite_is_assert)
//!
//! Plus proptest fuzz over N-best configurations. Serialized with
//! `invariant_ppt::test_lock`.

use std::collections::BTreeMap;

use proptest::prelude::*;

use invariant_ppt::{
    clear_contracts, clear_invariant_log, register_contract, test_lock, verify_all_contracts,
};

use crate::competition::calibration_weight;
use crate::fst::{Cascade, compile_rule, run_cascade};
use crate::spe::SPE_RULES;
use crate::window::CommitWindow;

const CONTRACT_INVARIANTS: &[&str] = &[
    "retraction_preserves_provenance",
    "survival_requires_independent_support",
    "no_unsupported_promotion",
    "cascade_is_bounded",
    "rating_in_range",
    "evidence_additive",
    "calibration_consistent",
    "rule_content_is_spe",
    "executed_as_fst",
    "rewrite_is_assert",
];

#[test]
fn contract_stage3_commit_window_invariants_enforced() {
    let _guard = test_lock::lock();
    clear_invariant_log();
    clear_contracts();
    register_contract("stage3 commit window", CONTRACT_INVARIANTS);

    // --- retraction/survival (window) ---
    let mut w = CommitWindow::new(5);
    let stable = w.fabric.intern();
    w.fabric.assert_assumption(stable);
    let e1 = w.fabric.intern();
    w.fabric.assert_assumption(e1);
    let e2 = w.fabric.intern();
    w.fabric.assert_assumption(e2);
    let e3 = w.fabric.intern();
    w.fabric.assert_assumption(e3);
    let mut s1 = std::collections::BTreeSet::new();
    s1.insert(e1);
    let _c1 = w.offer(0, s1, &[], 1.0).expect("c1");
    let mut s2 = std::collections::BTreeSet::new();
    s2.insert(e2);
    let mut stable_set = std::collections::BTreeSet::new();
    stable_set.insert(stable);
    w.offer(0, s2, &[stable_set], 0.6).expect("c2");
    // c3 has NO independent support and loses -> actually retracted, which
    // fires retraction_preserves_provenance.
    let mut s3 = std::collections::BTreeSet::new();
    s3.insert(e3);
    let _c3 = w.offer(3, s3, &[], 0.05).expect("c3");
    // Fires rating_in_range (via NBest rating) in the competition path; the
    // window close fires retraction/survival/promotion/bounded.
    let _ = w.close().expect("close");

    // confidence math
    let mut scores = BTreeMap::new();
    scores.insert("AA".to_string(), 0.9);
    let set = crate::competition::emit_nbest(&mut w.fabric, &scores, 1.0);
    let cand = &set.candidates[0];
    let _ = crate::competition::add_evidence(&mut cand.clone(), calibration_weight(0.7));

    // SPE + FST: validate inventory and run a cascade (fires
    // rule_content_is_spe, executed_as_fst, rewrite_is_assert).
    crate::spe::validate_spe_inventory(SPE_RULES);
    let mut cascade = Cascade::new();
    for rule in SPE_RULES {
        cascade.push(compile_rule(rule).expect("compile"));
    }
    let input: Vec<u8> = ["AA", "T", "IH", "D", "Y"]
        .iter()
        .map(|p| crate::fst::symbol_index(p).expect("phone"))
        .collect();
    let _ = run_cascade(&cascade, &input);

    // Every invariant fired.
    for inv in CONTRACT_INVARIANTS {
        assert!(
            invariant_ppt::has_invariant(inv),
            "invariant {inv} was not recorded"
        );
    }
    assert!(
        verify_all_contracts() >= 1,
        "'stage3 commit window' contract verified"
    );
}

// P-Test: proptest over N-best configurations (candidate counts, weights) —
// ratings always in range and winner deterministic.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_ratings_always_finite_in_range(
        n in 1usize..=5,
        posterior in 0.01f64..0.99,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("stage3 commit window", &["rating_in_range", "evidence_additive"]);
        let mut scores = BTreeMap::new();
        for i in 0..n {
            scores.insert(format!("P{i}"), posterior / (i as f64 + 1.0));
        }
        let mut w = CommitWindow::new(n.max(2));
        let set = crate::competition::emit_nbest(&mut w.fabric, &scores, 1.0);
        if let Some(win) = crate::competition::winner(&set) {
            prop_assert!((0.0..=100.0).contains(&win.rating()));
            prop_assert!(win.rating().is_finite());
        }
        // Fire evidence_additive by combining an extra calibrated channel.
        if let Some(first) = set.candidates.first() {
            let _ = crate::competition::add_evidence(
                &mut first.clone(),
                calibration_weight(posterior),
            );
        }
        prop_assert!(verify_all_contracts() >= 1);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn ppt_fst_cascade_deterministic(
        a in 0u32..10, b in 0u32..10, c in 0u32..10,
    ) {
        let _guard = test_lock::lock();
        clear_invariant_log();
        clear_contracts();
        register_contract("stage3 commit window", &["executed_as_fst", "rewrite_is_assert"]);
        // Two vowels with T between (flapping) + a couple boundaries.
        let mut input: Vec<u8> = vec![];
        for _ in 0..(a % 4) { input.push(crate::fst::symbol_index("AA").unwrap()); }
        input.push(crate::fst::symbol_index("T").unwrap());
        for _ in 0..(b % 3) { input.push(crate::fst::symbol_index("IH").unwrap()); }
        if c % 2 == 0 { input.push(crate::fst::symbol_index("D").unwrap()); }
        let mut cascade = Cascade::new();
        for rule in SPE_RULES {
            cascade.push(compile_rule(rule).expect("compile"));
        }
        let o1 = run_cascade(&cascade, &input);
        let o2 = run_cascade(&cascade, &input);
        prop_assert_eq!(o1, o2);
        prop_assert!(verify_all_contracts() >= 1);
    }
}
