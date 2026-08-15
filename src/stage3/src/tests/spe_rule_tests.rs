//! d73.1 gate tests: SPE phonological rule inventory.
//!
//! Asserts the DoD:
//! - inventory is non-empty and covers the four SPE families
//! - every rule references only ARPABET phonemes (INV `rule_content_is_spe`)
//! - every rule is expressible as a context-sensitive rewrite `A → B / X _ Y`
//!   (the Category B porting precondition, conversion-inventory.md §2.2)

use crate::spe::{SPE_RULES, SpeClass, validate_spe_inventory};

#[test]
fn spe_inventory_is_non_empty() {
    assert!(!SPE_RULES.is_empty(), "SPE inventory must be populated");
}

#[test]
fn spe_rules_cover_all_four_families() {
    let mut classes = std::collections::HashSet::new();
    for r in SPE_RULES {
        classes.insert(r.class);
    }
    for want in [
        SpeClass::Flapping,
        SpeClass::Assimilation,
        SpeClass::Elision,
        SpeClass::Palatalization,
    ] {
        assert!(classes.contains(&want), "missing SPE rule family {want:?}");
    }
}

#[test]
fn spe_rules_reference_only_arpabet() {
    let offending = validate_spe_inventory(SPE_RULES);
    assert!(
        offending.is_none(),
        "rule_content_is_spe violated: {offending:?}"
    );
}

#[test]
fn spe_rules_are_context_sensitive_rewrites() {
    // Category B precondition: every rule has the shape A → B / X _ Y, i.e.
    // at least the rewritten phoneme plus either an output or a context.
    for r in SPE_RULES {
        assert!(
            !r.input.is_empty(),
            "rule '{}' has empty rewritten phoneme",
            r.name
        );
        // Every rule renders to a non-trivial rewrite string.
        let rendered = r.rendered();
        assert!(
            rendered.contains('→') && rendered.contains('_'),
            "rule '{}' is not a context-sensitive rewrite: {rendered}",
            r.name
        );
        // Same-family rules must have distinct names (no dupes).
    }
    // Name uniqueness.
    let mut names = std::collections::HashSet::new();
    for r in SPE_RULES {
        assert!(names.insert(r.name), "duplicate rule name '{}'", r.name);
    }
}

#[test]
fn spe_rule_flap_renders_with_vowel_context() {
    let flap = SPE_RULES
        .iter()
        .find(|r| r.name == "intervocalic-t-flap")
        .expect("flapping rule present");
    let rendered = flap.rendered();
    assert!(
        rendered.contains("→ D /"),
        "flap rewrite must emit D in vowel context, got {rendered}"
    );
    assert!(!flap.output.is_empty());
}

#[test]
fn spe_rules_elision_renders_deletion() {
    let elision = SPE_RULES
        .iter()
        .find(|r| r.class == SpeClass::Elision)
        .expect("elision rule present");
    assert!(elision.output.is_empty(), "elision deletes the phone");
    let rendered = elision.rendered();
    assert!(
        rendered.contains('ε'),
        "deletion renders with epsilon, got {rendered}"
    );
}
