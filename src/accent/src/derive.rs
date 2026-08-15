//! Accent-signature fact derivation (91x.1).
//!
//! `accent_signature = { Δ_i : i ∈ phoneme_alphabet }` — the per-phoneme mean
//! displacement of this speaker's realized phonemes from the canonical
//! reference (accent-feedback-semantics.md §2). This is a Tier 2 derived
//! fact: computed from *committed* phoneme facts (the same source Stage 4
//! consumes, d73.5 promotion) and assertable into the fabric.
//!
//! Ordering is fixed to the 39-phone ARPABET alphabet (aem.10), so index `i`
//! corresponds to `fse::ARPABET[i]`. INVs (accent-feedback-semantics.md §6):
//! - `signature_bounded`: every entry is finite (no divergence)
//! - `signature_height`: exactly one `Δ` per phoneme in the alphabet

use fabric::{Fabric, FactId};
use invariant_ppt::assert_invariant;
use std::collections::BTreeSet;

use fse::ARPABET;

/// Phoneme count == ARPABET len == 39 (aem.10).
pub const PHONEME_COUNT: usize = ARPABET.len();

/// One committed phoneme fact: the speaker realized `phoneme` with
/// `displacement` from the canonical reference (feature units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommittedPhoneme {
    /// Index of the phoneme in the 39-phone ARPABET alphabet.
    pub phoneme: usize,
    /// Measured displacement of this realization from the canonical reference.
    pub displacement: f64,
}

/// The derived accent-signature fact: one `Δ_i` per phoneme in ARPABET order.
#[derive(Debug, Clone, PartialEq)]
pub struct AccentSignature {
    /// `Δ_i`, length `PHONEME_COUNT` (index = ARPABET index).
    pub displacements: Vec<f64>,
}

impl AccentSignature {
    /// `Δ` for the given phoneme index (panics out of range via invariant).
    pub fn delta(&self, phoneme: usize) -> f64 {
        assert_invariant(
            phoneme < self.displacements.len(),
            "signature_delta_in_range",
            Some("91x.1"),
        );
        self.displacements[phoneme]
    }
}

/// Derive the accent-signature fact from committed phoneme facts.
///
/// `Δ_i` = mean displacement across the committed samples for phoneme `i`
/// (phonemes with no committed samples keep `Δ_i = 0`). The signature is a
/// Tier 2 derived fact: `derive_and_assert` interns it into the fabric backed
/// by the source facts as premises (INV `signature_derived_from_committed`).
pub fn derive_signature(committed: &[CommittedPhoneme]) -> AccentSignature {
    let mut sums: Vec<f64> = vec![0.0; PHONEME_COUNT];
    let mut counts: Vec<usize> = vec![0; PHONEME_COUNT];
    for &c in committed {
        // Every source phoneme is in the fixed alphabet.
        assert_invariant(
            c.phoneme < PHONEME_COUNT,
            "source_phoneme_in_alphabet",
            Some("91x.1"),
        );
        assert_invariant(
            c.displacement.is_finite(),
            "source_displacement_finite",
            Some("91x.1"),
        );
        sums[c.phoneme] += c.displacement;
        counts[c.phoneme] += 1;
    }
    let displacements: Vec<f64> = sums
        .into_iter()
        .zip(counts)
        .map(|(sum, n)| if n == 0 { 0.0 } else { sum / n as f64 })
        .collect();

    assert_invariant(
        displacements.len() == PHONEME_COUNT,
        "signature_height",
        Some("91x.1"),
    );
    assert_invariant(
        displacements.iter().all(|d| d.is_finite()),
        "signature_bounded",
        Some("91x.1"),
    );
    AccentSignature { displacements }
}

/// Derive AND assert the signature into the fabric as a Tier 2 derived fact.
///
/// `source_facts` are the committed phoneme facts the signature was derived
/// from (its premise set — the provenance chain into Stage 11). Returns the
/// new fact id.
pub fn derive_and_assert(
    fabric: &mut Fabric,
    committed: &[CommittedPhoneme],
    source_facts: &[FactId],
) -> (AccentSignature, FactId) {
    let signature = derive_signature(committed);
    let fact = fabric.intern();
    let premises: BTreeSet<FactId> = source_facts.iter().copied().collect();
    fabric.assert(fact, premises, false);
    assert_invariant(
        !fabric.justifications(fact).is_empty(),
        "signature_derived_from_committed",
        Some("91x.1"),
    );
    (signature, fact)
}
