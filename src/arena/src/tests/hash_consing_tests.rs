//! Hash-consing gate tests (a6d.1): injective on identical content, distinct
//! on distinct content, and append-only content after intern.

use crate::{Arena, Candidate, quantize};

fn candidate(phoneme: &str, sig: &[i32]) -> Candidate {
    Candidate::new(phoneme, vec![], sig.to_vec())
}

#[test]
fn interning_identical_content_twice_returns_same_factid() {
    let mut arena = Arena::new();
    let a = candidate("IH", &[1, 2, 3]);
    let b = candidate("IH", &[1, 2, 3]);
    let id1 = arena.ingest(&a);
    let id2 = arena.ingest(&b);
    assert_eq!(
        id1, id2,
        "INV: hash_consing_injective — identical content, same NodeId"
    );
    assert_eq!(arena.fact_count(), 1, "one distinct fact interned");
}

#[test]
fn distinct_content_gets_distinct_factid() {
    let mut arena = Arena::new();
    let ids: Vec<_> = [
        candidate("AA", &[1, 2, 3]),
        candidate("AE", &[1, 2, 3]), // different phoneme
        candidate("AA", &[4, 5, 6]), // different signature
        candidate("AA", &[1, 2, 4]), // one bin off
    ]
    .iter()
    .map(|c| arena.ingest(c))
    .collect();
    let mut uniq = ids.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(
        uniq.len(),
        4,
        "INV: hash_consing_distinct — 4 distinct NodeIds"
    );
}

#[test]
fn nodeid_content_never_changes_after_intern() {
    let mut arena = Arena::new();
    let c = candidate("K", &[9, 9, 9]);
    let id = arena.ingest(&c);
    // Content lookup returns exactly what was interned, and re-ingesting
    // identical content cannot mutate the stored content.
    assert_eq!(arena.content(id).unwrap(), &c.content);
    let again = arena.ingest(&c);
    assert_eq!(again, id);
    assert_eq!(
        arena.content(id).unwrap(),
        &c.content,
        "INV: arena_append_only"
    );
}

#[test]
fn quantization_collapses_near_equal_floats() {
    let epsilon = 0.1f32;
    let a = quantize(&[0.01, 0.099, -0.099], epsilon);
    let b = quantize(&[0.02, 0.11, -0.11], epsilon);
    assert_eq!(a[0], b[0], "same bin");
    assert_eq!(a[1], b[1], "0.099 and 0.11 round to the same bin");
    // Distinct bins stay distinct.
    let c = quantize(&[0.5, 0.5, 0.5], epsilon);
    assert_ne!(a[0], c[0]);
    // Identical raw floats -> identical bins (bit-identical after quantize).
    let d = quantize(&[0.01, 0.099, -0.099], epsilon);
    assert_eq!(a, d);
}

#[test]
fn context_window_is_part_of_equality() {
    let mut arena = Arena::new();
    let with_ctx = Candidate::new("IH", vec!["T".to_string(), "K".to_string()], vec![1]);
    let without_ctx = Candidate::new("IH", vec![], vec![1]);
    let id1 = arena.ingest(&with_ctx);
    let id2 = arena.ingest(&without_ctx);
    assert_ne!(id1, id2, "context window participates in equality");
    // Same context, same signature, same phoneme -> same id.
    let id3 = arena.ingest(&Candidate::new(
        "IH",
        vec!["T".to_string(), "K".to_string()],
        vec![1],
    ));
    assert_eq!(id1, id3);
}

#[test]
fn node_id_resolution_by_content() {
    let mut arena = Arena::new();
    let c = candidate("SH", &[0, 0, 0]);
    let id = arena.ingest(&c);
    assert_eq!(arena.node_id(&c.content), Some(id));
    assert_eq!(arena.node_id(&candidate("SH", &[0, 0, 1]).content), None);
}
