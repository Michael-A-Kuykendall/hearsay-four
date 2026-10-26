//! Stage3 commit-window → accent-signature feedback loop (bead hearsay-four-s3t).
//!
//! Wires the live loop the 91x/d73 crates left open: a stage3
//! [`CommitWindow`](stage3_close) close commits one winner per window; the
//! winner's phoneme observation updates the accent signature via the bounded
//! EMA (accent-feedback-semantics.md §3, [`ema_update`]); and the signature
//! biases the *next* window's Stage 1 per-phone scores (§4,
//! [`apply_feedback`]). Mid-session drift takes the §5 retraction-and-recascade
//! path ([`DriftDetector`] + [`retrain_after_drift`]).
//!
//! The update math is the same preregistered rule the fail-fast treatment
//! ([`ConditionB`]) measures: `Δ ← (1−α)·Δ + α·δ` with `α = 0.2`, and
//! `adjusted = raw + Δ·β` with `β = 1.0` — here in the 39-phone phoneme space
//! rather than the feature space, so the bias applies directly to Stage 1
//! `fse::score_phones` candidates.
//!
//! The Tier 2 signature fact lives in an accent-side [`Fabric`]: each commit
//! banks one live source assumption; on a drift trigger the epoch's sources
//! are retracted and the signature is re-derived from the recent committed
//! phonemes, so dependents recascade automatically (INV `recascade_automatic`).
//! Between triggers the live value is the EMA state; at a retrain it is
//! re-based to the re-derived fact (semantics §5 step 3).

use invariant_ppt::assert_invariant;

use crate::derive::{AccentSignature, CommittedPhoneme, PHONEME_COUNT, derive_signature};
use crate::drift::{DriftDetector, UtteranceSample, retrain_after_drift};
use crate::feedback::{EMA_ALPHA, FEEDBACK_BETA, apply_feedback, ema_update};
use fabric::{Fabric, FactId};

/// How many committed phoneme observations are kept per epoch. Cleared on
/// every drift retrain; comfortably above the drift watch window (5) so a
/// trigger always re-derives from the evidence that fired it.
pub const RECENT_CAP: usize = 20;

/// The live stage3 → accent feedback loop.
pub struct WindowFeedback {
    /// Live accent signature (EMA state between retrains).
    signature: AccentSignature,
    /// Drift guard (3-of-5, INV `drift_trigger_guarded`).
    drift: DriftDetector,
    /// Accent-side fabric holding the Tier 2 signature fact.
    fabric: Fabric,
    /// Current Tier 2 signature fact.
    signature_fact: FactId,
    /// Epoch source facts backing `signature_fact` (retracted on drift).
    epoch_sources: Vec<FactId>,
    /// Recent committed observations: (live source fact, phoneme).
    recent: Vec<(FactId, CommittedPhoneme)>,
    /// EMA learning rate (preregistered, default 0.2).
    alpha: f64,
    /// Feedback gain into Stage 1 (preregistered, default 1.0).
    beta: f64,
}

impl WindowFeedback {
    /// A fresh loop: zero signature derived from a genesis source, default
    /// preregistered rates, drift tolerance `tau` (see `DEFAULT_TOLERANCE`).
    pub fn new(tau: f64) -> WindowFeedback {
        let mut fabric = Fabric::new();
        let genesis = fabric.intern();
        fabric.assert_assumption(genesis);
        let (signature, fact) = crate::derive::derive_and_assert(&mut fabric, &[], &[genesis]);
        WindowFeedback {
            signature,
            drift: DriftDetector::new(tau),
            fabric,
            signature_fact: fact,
            epoch_sources: vec![genesis],
            recent: Vec::new(),
            alpha: EMA_ALPHA,
            beta: FEEDBACK_BETA,
        }
    }

    /// The live accent signature.
    pub fn signature(&self) -> &AccentSignature {
        &self.signature
    }

    /// Whether the current Tier 2 signature fact is active in the fabric.
    pub fn signature_fact_is_active(&self) -> bool {
        self.fabric.is_active(self.signature_fact)
    }

    /// Fold one closed stage3 window into the loop.
    ///
    /// `phoneme` is the ARPABET index of the window's committed winner and
    /// `displacement` its measured displacement from the canonical reference
    /// (feature units, finite). The signature EMA-updates toward the new
    /// evidence (only the observed phoneme moves); the sample is then checked
    /// against the drift guard. Returns `true` iff drift fired and the
    /// signature was retrained through the §5 retract-and-recascade path.
    pub fn observe_commit(&mut self, phoneme: usize, displacement: f64) -> bool {
        assert_invariant(
            phoneme < PHONEME_COUNT,
            "feedback_phoneme_in_alphabet",
            Some("s3t"),
        );
        assert_invariant(displacement.is_finite(), "signature_bounded", Some("s3t"));

        // Bank the observation as a live source assumption.
        let source = self.fabric.intern();
        self.fabric.assert_assumption(source);
        self.recent.push((
            source,
            CommittedPhoneme {
                phoneme,
                displacement,
            },
        ));
        if self.recent.len() > RECENT_CAP {
            // Superseded evidence leaves the buffer; its fabric fact stays
            // live (it never premises the current signature fact, so nothing
            // cascades — the epoch sources alone back the Tier 2 fact).
            self.recent.remove(0);
        }

        // Bounded EMA: only the observed phoneme gains new evidence (§3).
        let mut delta = self.signature.displacements.clone();
        delta[phoneme] = displacement;
        ema_update(&mut self.signature, &delta, self.alpha);

        // Drift guard over the full phoneme vector (§5 step 1).
        let mut full = vec![0.0; PHONEME_COUNT];
        full[phoneme] = displacement;
        let fired = self.drift.observe(
            UtteranceSample { displacement: full },
            &self.signature.displacements,
        );
        if fired {
            // Retraction-and-recascade (§5 steps 2–4): retract the epoch's
            // sources, re-derive from the recent committed phonemes, and
            // re-base the live signature on the re-derived fact.
            let committed: Vec<CommittedPhoneme> = self.recent.iter().map(|(_, c)| *c).collect();
            let premises: Vec<FactId> = self.recent.iter().map(|(f, _)| *f).collect();
            let new_fact =
                retrain_after_drift(&mut self.fabric, &self.epoch_sources, &committed, &premises);
            self.signature_fact = new_fact;
            self.epoch_sources = premises;
            self.signature = derive_signature(&committed);
            self.recent.clear();
        }
        fired
    }

    /// Bias the next window's Stage 1 per-phone candidates toward the
    /// speaker's realized pronunciations (§4): `adjusted = raw + Δ·β`.
    pub fn score_next_window(&self, scored: &[(&'static str, f64)]) -> Vec<(&'static str, f64)> {
        apply_feedback(&self.signature, scored, self.beta)
    }
}
