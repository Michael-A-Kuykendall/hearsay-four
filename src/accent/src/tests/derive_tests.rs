//! 91x.1 gate tests: derive accent-signature fact from committed phoneme facts.
//!
//! DoD:
//! - derives a per-phoneme displacement vector from committed phoneme facts
//! - the signature has exactly one entry per phoneme in the alphabet (39)
//! - entries are finite (INV `signature_bounded`)

use crate::derive::{CommittedPhoneme, PHONEME_COUNT, derive_and_assert, derive_signature};
use fabric::Fabric;
use fse::ARPABET;

fn committed(phonemes: &[(usize, f64)]) -> Vec<CommittedPhoneme> {
    phonemes
        .iter()
        .map(|&(phoneme, displacement)| CommittedPhoneme {
            phoneme,
            displacement,
        })
        .collect()
}

#[test]
fn derive_signature_has_one_entry_per_phoneme() {
    let sig = derive_signature(&committed(&[(0, 0.1), (5, 0.2)]));
    assert_eq!(sig.displacements.len(), PHONEME_COUNT, "one Δ per phoneme");
    assert_eq!(ARPABET.len(), PHONEME_COUNT, "39-phone alphabet");
}

#[test]
fn derive_signature_means_committed_samples() {
    // Phoneme 1 gets samples {0.2, 0.4} -> Δ = 0.3.
    let sig = derive_signature(&committed(&[(1, 0.2), (1, 0.4), (2, 0.5), (2, 0.7)]));
    let d1 = sig.delta(1);
    let d2 = sig.delta(2);
    assert!((d1 - 0.3).abs() < 1e-9, "mean of {{0.2, 0.4}}");
    assert!((d2 - 0.6).abs() < 1e-9, "mean of {{0.5, 0.7}}");
}

#[test]
fn derive_unobserved_phonemes_stay_zero() {
    let sig = derive_signature(&committed(&[(30, 0.5)]));
    assert_eq!(sig.delta(30), 0.5);
    assert_eq!(sig.delta(0), 0.0, "unobserved phoneme Δ = 0");
    assert_eq!(sig.delta(10), 0.0);
}

#[test]
fn derive_entries_are_finite() {
    // Finite inputs (INV `signature_bounded`); no NaN/inf can enter.
    let sig = derive_signature(&committed(&[(0, 1.0), (1, -0.5), (2, 1e9), (3, -1e-9)]));
    assert!(sig.displacements.iter().all(|d| d.is_finite()));
    for &f in &sig.displacements {
        assert!(f.is_finite(), "signature_bounded");
    }
}

#[test]
fn derive_asserts_into_fabric_as_tier2_fact() {
    let mut fabric = Fabric::new();
    let source1 = fabric.intern();
    let source2 = fabric.intern();
    // Committed phoneme facts: promote them as assumptions (Tier 2 source).
    fabric.assert_assumption(source1);
    fabric.assert_assumption(source2);
    let committed = committed(&[(0, 0.2), (0, 0.4)]);
    let (signature, fact) = derive_and_assert(&mut fabric, &committed, &[source1, source2]);
    assert!((signature.displacements[0] - 0.3).abs() < 1e-9);
    assert!(fabric.is_active(fact), "derived fact asserted + active");
    assert_eq!(fabric.justifications(fact).len(), 1);
}

#[test]
fn derive_out_of_alphabet_phoneme_panics() {
    // A committed phoneme outside the 39-phone alphabet violates the source
    // invariant (source_phoneme_in_alphabet) and must panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        derive_signature(&committed(&[(PHONEME_COUNT, 1.0)]));
    }));
    assert!(result.is_err(), "out-of-alphabet phoneme rejected");
}
