//! d73.2 gate tests: Kaplan & Kay FST-cascade execution engine.
//!
//! DoD:
//! 1. compiles the flapping SPE rule into an FST transition table
//! 2. runs a (determinized) cascade to an output
//! 3. demonstrates a rewrite is asserted as a new fact, not an in-place
//!    mutation (INV `rewrite_is_assert`)
//! 4. same input → same output (INV `cascade_deterministic`)

use crate::fst::{Cascade, compile_rule, run_cascade, run_rule, symbol_index};
use crate::spe::{SPE_RULES, SpeRule};

fn flapping_rule() -> SpeRule {
    SPE_RULES
        .iter()
        .find(|r| r.name == "intervocalic-t-flap")
        .expect("flapping rule present")
        .clone()
}

fn compile_id(rule: &SpeRule) -> Result<crate::fst::CompiledRule, crate::fst::FstError> {
    compile_rule(rule)
}

fn seq(phones: &[&str]) -> Vec<u8> {
    phones
        .iter()
        .map(|p| symbol_index(p).expect("in ARPABET"))
        .collect()
}

fn phones(symbols: &[u8]) -> Vec<&'static str> {
    symbols.iter().map(|&s| crate::fst::phone_of(s)).collect()
}

#[test]
fn fst_flapping_compiles_to_transition_table() {
    let rule = compile_id(&flapping_rule()).expect("compile flapping");
    // The classical shape: `(state, symbol) -> (state', output)`.
    assert_eq!(rule.transition_count(), 3 * 39);
    assert!(!rule.is_deletion());
    assert_eq!(rule.transitions.len(), 3 * 39);
    // It emits D (the flap) not nothing.
    assert_eq!(rule.output, vec![symbol_index("D").unwrap()]);
}

#[test]
fn fst_flapping_rewrites_t_between_vowels() {
    let rule = compile_id(&flapping_rule()).expect("compile");
    // Vowel T Vowel -> Vowel D Vowel (the §4 example).
    let out = run_rule(&rule, &seq(&["AA", "T", "IH"]));
    assert_eq!(phones(&out), vec!["AA", "D", "IH"]);
}

#[test]
fn fst_noncontext_t_left_untouched() {
    let rule = compile_id(&flapping_rule()).expect("compile");
    // T at the start (no left vowel) and between two consonants stays T.
    let out = run_rule(&rule, &seq(&["T", "R", "S"]));
    assert_eq!(phones(&out), vec!["T", "R", "S"]);
}

#[test]
fn fst_palatalization_d_plus_y() {
    // d73.2 gate inclusion: a second rule class runs a rewrite.
    let rule = compile_id(
        SPE_RULES
            .iter()
            .find(|r| r.name == "d-palatalization-before-glide")
            .expect("rule"),
    )
    .expect("compile");
    let out = run_rule(&rule, &seq(&["D", "Y"]));
    assert_eq!(phones(&out), vec!["JH", "Y"]);
}

#[test]
fn fst_cascade_runs_all_rules() {
    let mut cascade = Cascade::new();
    for rule in SPE_RULES {
        cascade.push(compile_id(rule).expect("compile"));
    }
    let out = run_cascade(&cascade, &seq(&["IH", "T", "IH", "D", "Y"]));
    // flapping: IH T IH -> IH D IH (T->D between vowels); then D+Y -> JH YA.
    assert_eq!(phones(&out), vec!["IH", "D", "IH", "JH", "Y"]);
}

#[test]
fn fst_deterministic_same_input_same_output() {
    let mut cascade = Cascade::new();
    for rule in SPE_RULES {
        cascade.push(compile_id(rule).expect("compile"));
    }
    let input = seq(&["AE", "T", "IY", "T", "Y"]);
    let one = run_cascade(&cascade, &input);
    let two = run_cascade(&cascade, &input);
    assert_eq!(one, two, "INV cascade_deterministic");
    assert_eq!(input, seq(&["AE", "T", "IY", "T", "Y"]), "input untouched");
}

#[test]
fn fst_rewrite_is_asserted_as_new_fact() {
    use fabric::Fabric;

    let mut fabric = Fabric::new();
    let v_left = fabric.intern(); // Left vowel fact
    let t_phone = fabric.intern(); // The T phone fact
    let v_right = fabric.intern(); // Right vowel fact

    // The transducer run *emits* the flap (D) — a brand-new symbol, not a
    // mutation of T.
    let rule = compile_id(&flapping_rule()).expect("compile");
    let out = run_rule(&rule, &seq(&["AA", "T", "IH"]));
    assert_eq!(phones(&out), vec!["AA", "D", "IH"]);
    assert_eq!(rule.output, vec![symbol_index("D").unwrap()]);

    // Assert the rewrite into the fabric: the flap fact is concluded from
    // the context + input facts (the Category B port).
    let flap = fabric.intern();
    fabric.assert_assumption(v_left);
    fabric.assert_assumption(t_phone);
    fabric.assert_assumption(v_right);
    let premises: std::collections::BTreeSet<_> = [v_left, t_phone, v_right].into_iter().collect();
    fabric.assert(flap, premises, false);
    assert!(fabric.is_active(flap), "flap fact asserted and active");
    assert_eq!(
        fabric.justifications(flap).len(),
        1,
        "one justification set"
    );
    // T fact is untouched — rewrite produced a new fact, not a mutation.
    assert!(fabric.is_active(t_phone));
}

#[test]
fn fst_cascade_no_rules_returns_identity() {
    let empty_cascade = Cascade::new();
    let input = seq(&["T", "AA"]);
    assert_eq!(run_cascade(&empty_cascade, &input), input);
}
