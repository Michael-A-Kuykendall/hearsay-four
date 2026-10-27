//! A3 regression (bead hearsay-four-pri): token identity.
//! Same word twice in one utterance -> two token facts, distinct spans,
//! independent retraction, distinct provenance chains; type facts stay
//! hash-consed; premise order/multiplicity is preserved.

use fabric::{Fabric, TokenFact};

fn label(id: fabric::FactId) -> String {
    format!("fact{}", id.0)
}

#[test]
fn same_word_twice_yields_two_distinct_tokens() {
    let mut f = Fabric::new();
    let t1 = f.intern_word_token("hello", 0, 10, "decoder");
    let t2 = f.intern_word_token("hello", 20, 30, "decoder");
    assert_ne!(t1, t2);
    assert!(f.is_token(t1) && f.is_token(t2));
    match (f.token_fact(t1), f.token_fact(t2)) {
        (Some(TokenFact::Word(a)), Some(TokenFact::Word(b))) => {
            assert_eq!((a.start_frame, a.end_frame), (0, 10));
            assert_eq!((b.start_frame, b.end_frame), (20, 30));
            assert_eq!(a.word, b.word);
        }
        other => panic!("expected word tokens, got {other:?}"),
    }
    // Even byte-identical span+source is a distinct occurrence.
    let t3 = f.intern_word_token("hello", 0, 10, "decoder");
    assert_ne!(t1, t3);
}

#[test]
fn token_retraction_is_independent_and_provenance_distinct() {
    let mut f = Fabric::new();
    let t1 = f.intern_word_token("hello", 0, 10, "decoder");
    let t2 = f.intern_word_token("hello", 20, 30, "decoder");
    f.assert_assumption(t1);
    f.assert_assumption(t2);
    let d1 = f.intern();
    let d2 = f.intern();
    f.assert_ordered(d1, vec![t1], false);
    f.assert_ordered(d2, vec![t2], false);
    assert!(f.is_active(d1) && f.is_active(d2));

    let p1 = f.proof_tree(d1, &label);
    let p2 = f.proof_tree(d2, &label);
    assert_eq!(p1.justifications[0].premises[0].fact, t1);
    assert_eq!(p2.justifications[0].premises[0].fact, t2);
    assert_ne!(
        p1.justifications[0].premises[0].fact,
        p2.justifications[0].premises[0].fact
    );

    f.retract(t1);
    assert!(!f.is_active(t1) && !f.is_active(d1));
    assert!(f.is_active(t2) && f.is_active(d2), "second token survives");
    // Provenance retained after retraction.
    assert_eq!(f.justifications(d1).len(), 1);
}

#[test]
fn type_facts_hash_consed_tokens_not() {
    let mut f = Fabric::new();
    let a = f.intern_type("WORD_TYPE:hello");
    let b = f.intern_type("WORD_TYPE:hello");
    assert_eq!(a, b, "type facts merge globally");
    assert_eq!(f.type_content(a), Some("WORD_TYPE:hello"));
    assert!(!f.is_token(a));

    let p1 = f.intern_phone_token("HH", 0, 5);
    let p2 = f.intern_phone_token("HH", 0, 5);
    assert_ne!(p1, p2);
    let s1 = f.intern_sil_token(5, 8);
    let s2 = f.intern_sil_token(5, 8);
    assert_ne!(s1, s2);
    assert!(matches!(f.token_fact(p1), Some(TokenFact::Phone(_))));
    assert!(matches!(f.token_fact(s1), Some(TokenFact::Silence(_))));
}

#[test]
fn premise_order_and_multiplicity_preserved() {
    let mut f = Fabric::new();
    let a = f.intern_phone_token("AH", 0, 3);
    let b = f.intern_phone_token("HH", 3, 6);
    f.assert_assumption(a);
    f.assert_assumption(b);
    let concl = f.intern();
    f.assert_ordered(concl, vec![b, a, b], false);
    assert_eq!(f.justifications(concl)[0].premises, vec![b, a, b]);
    let proof = f.proof_tree(concl, &label);
    let facts: Vec<_> = proof.justifications[0]
        .premises
        .iter()
        .map(|n| n.fact)
        .collect();
    assert_eq!(facts, vec![b, a, b]);
}
