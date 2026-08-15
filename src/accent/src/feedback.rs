//! Accent-signature feedback into Stage 1 scoring (91x.2).
//!
//! accent-feedback-semantics.md §3–4:
//!
//! - **Bounded EMA update** (§3): `Δ_i ← (1−α)·Δ_i_prev + α·δ_i(new)`, with
//!   α = 0.2 default. The signature sharpens toward the speaker's realized
//!   mean without diverging (INV `signature_bounded`; INV
//!   `feedback_converges` — repeated consistent evidence converges
//!   monotonically).
//! - **Feedback bias** (§4): `adjusted_score(phoneme_i) = raw_score(
//!   phoneme_i) + Δ_i·β` with β = 1.0 default. Applied to every per-phone
//!   Stage-1 candidate score so subsequent utterances favor the speaker's
//!   realized pronunciations (carry-forward).

use invariant_ppt::assert_invariant;

use crate::derive::{AccentSignature, PHONEME_COUNT};

/// Default EMA learning rate (pre-registered, α = 0.2).
pub const EMA_ALPHA: f64 = 0.2;
/// Default feedback gain (pre-registered, β = 1.0).
pub const FEEDBACK_BETA: f64 = 1.0;

/// Update an accent signature with a new per-phoneme displacement sample via
/// the bounded EMA (§3). Returns the previously-applied bias vector for
/// score adjustment.
///
/// INV `feedback_converges`: with repeated consistent evidence the signature
/// moves monotonically toward the realized mean. INV `signature_bounded`:
/// every entry stays finite.
pub fn ema_update(signature: &mut AccentSignature, delta: &[f64], alpha: f64) {
    assert_invariant(
        signature.displacements.len() == delta.len(),
        "feedback_dimensions_match",
        Some("91x.2"),
    );
    assert_invariant(
        alpha > 0.0 && alpha <= 1.0,
        "ema_alpha_in_range",
        Some("91x.2"),
    );
    assert_invariant(
        delta.iter().all(|d| d.is_finite()),
        "signature_bounded",
        Some("91x.2"),
    );
    let old: Vec<f64> = signature.displacements.clone();
    for (sig, &d) in signature.displacements.iter_mut().zip(delta.iter()) {
        *sig = (1.0 - alpha) * *sig + alpha * d;
    }
    assert_invariant(
        signature.displacements.iter().all(|v| v.is_finite()),
        "signature_bounded",
        Some("91x.2"),
    );
    // INV `feedback_converges`: with repeated consistent evidence the
    // signature moves monotonically toward the realized mean. Per component,
    // `new = (1−α)·old + α·target` is a convex combination, so the distance
    // to the target cannot increase: `|target − new| ≤ |target − old|`.
    assert_invariant(
        signature
            .displacements
            .iter()
            .zip(delta.iter())
            .zip(old.iter())
            .all(|((&new, &target), &old_v)| {
                (target - new).abs() <= (target - old_v).abs() + 1e-12
            }),
        "feedback_converges",
        Some("91x.2"),
    );
}

/// Apply the feedback bias to a per-phone Stage-1 score (§4):
/// `adjusted = raw + Δ_i·β`, where `Δ_i` is `signature.delta(phoneme)`.
///
/// `phoneme_ix` is the ARPABET alphabet index of the phoneme candidate.
/// `raw` is the Stage-1 score for that candidate (fse::score_phones `(phoneme,
/// score)` pairs). Higher = closer to the speaker as the signature grows.
pub fn adjusted_score(signature: &AccentSignature, phoneme_ix: usize, raw: f64, beta: f64) -> f64 {
    assert_invariant(
        phoneme_ix < PHONEME_COUNT,
        "feedback_phoneme_in_alphabet",
        Some("91x.2"),
    );
    assert_invariant(raw.is_finite(), "raw_score_finite", Some("91x.2"));
    let adjusted = raw + signature.delta(phoneme_ix) * beta;
    assert_invariant(adjusted.is_finite(), "adjusted_score_finite", Some("91x.2"));
    adjusted
}

/// Apply the feedback to every per-phone candidate from
/// `fse::score_phones`: map `(phoneme, raw)` pairs to their adjusted scores,
/// locating each phoneme in the ARPABET alphabet.
///
/// Returns `(phoneme, adjusted_score)` pairs in the same order as input.
pub fn apply_feedback(
    signature: &AccentSignature,
    scored: &[(&'static str, f64)],
    beta: f64,
) -> Vec<(&'static str, f64)> {
    let index_of = |phoneme: &str| {
        fse::ARPABET
            .iter()
            .position(|p| *p == phoneme)
            .expect("phoneme in alphabet")
    };
    scored
        .iter()
        .map(|&(phoneme, raw)| {
            let adjusted = adjusted_score(signature, index_of(phoneme), raw, beta);
            (phoneme, adjusted)
        })
        .collect()
}
