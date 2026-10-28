//! Linear interpolation of two KN trigram models (bead hearsay-four-2a0,
//! R1b): a general LM and a personal/adapted LM, mixed as
//! `P(w|h) = (1 - lambda) * P_general(w|h) + lambda * P_personal(w|h)`.
//! The personal weight `lambda` is tuned on DEV by grid search
//! (`tune_personal_weight`); DEV text is scored, never trained on.

use crate::kneser_ney::{KnTrigram, PerplexityReport};
use crate::normalize::{BOS_TOKEN, EOS_TOKEN};

/// Two-component interpolated trigram LM.
#[derive(Debug, Clone)]
pub struct InterpolatedLm {
    general: KnTrigram,
    personal: KnTrigram,
    personal_weight: f64,
}

impl InterpolatedLm {
    /// `personal_weight` must lie in `[0, 1]`. Components keep their own
    /// vocabularies; each maps out-of-vocabulary words to `<unk>` (the
    /// standard interpolation convention), so the endpoints are exact:
    /// weight 0 is the general model, weight 1 is the personal model.
    pub fn new(
        general: KnTrigram,
        personal: KnTrigram,
        personal_weight: f64,
    ) -> Result<Self, String> {
        if !personal_weight.is_finite() || !(0.0..=1.0).contains(&personal_weight) {
            return Err(format!("personal_weight {personal_weight} outside [0, 1]"));
        }
        Ok(Self {
            general,
            personal,
            personal_weight,
        })
    }

    pub fn general(&self) -> &KnTrigram {
        &self.general
    }
    pub fn personal(&self) -> &KnTrigram {
        &self.personal
    }
    pub fn personal_weight(&self) -> f64 {
        self.personal_weight
    }

    /// Interpolated `P(w | w1, w2)`: convex mix of the two component
    /// probabilities, each with its own `<unk>` mapping.
    pub fn prob(&self, w1: &str, w2: &str, w: &str) -> f64 {
        let lambda = self.personal_weight;
        ((1.0 - lambda) * self.general.prob(w1, w2, w) + lambda * self.personal.prob(w1, w2, w))
            .clamp(0.0, 1.0)
    }

    /// Log probability of a normalized sentence (boundary padded).
    pub fn sentence_log_prob(&self, sent: &[String]) -> f64 {
        let mut padded: Vec<&str> = vec![BOS_TOKEN, BOS_TOKEN];
        padded.extend(sent.iter().map(|s| s.as_str()));
        padded.push(EOS_TOKEN);
        let mut lp = 0.0;
        for t in padded.windows(3) {
            lp += self.prob(t[0], t[1], t[2]).max(1e-12).ln();
        }
        lp
    }

    /// Perplexity on held-out normalized sentences.
    pub fn perplexity(&self, sentences: &[Vec<String>]) -> PerplexityReport {
        let mut log_prob = 0.0;
        let mut tokens: u64 = 0;
        for s in sentences {
            log_prob += self.sentence_log_prob(s);
            tokens += s.len() as u64 + 1;
        }
        let perplexity = if tokens == 0 {
            f64::INFINITY
        } else {
            (-log_prob / tokens as f64).exp()
        };
        PerplexityReport {
            sentences: sentences.len(),
            predicted_tokens: tokens,
            log_prob,
            perplexity,
        }
    }
}

/// Outcome of a DEV weight grid search.
#[derive(Debug, Clone, PartialEq)]
pub struct TuneResult {
    pub best_weight: f64,
    pub best_perplexity: f64,
    /// Every evaluated `(personal_weight, dev_perplexity)`, ascending weight.
    pub evaluations: Vec<(f64, f64)>,
}

/// Tune the personal weight on DEV sentences over the grid
/// `0.00, 0.05, ..., 1.00`, minimizing DEV perplexity. Ties break toward
/// the smaller weight (grid order), so tuning is deterministic.
pub fn tune_personal_weight(
    general: &KnTrigram,
    personal: &KnTrigram,
    dev_sentences: &[Vec<String>],
) -> Result<TuneResult, String> {
    if dev_sentences.is_empty() {
        return Err("tune_personal_weight: empty DEV sentences".into());
    }
    let mut evaluations = Vec::new();
    let mut best_weight = 0.0;
    let mut best_perplexity = f64::INFINITY;
    for i in 0..=20u32 {
        let w = f64::from(i) / 20.0;
        let lm = InterpolatedLm::new(general.clone(), personal.clone(), w)?;
        let ppl = lm.perplexity(dev_sentences).perplexity;
        evaluations.push((w, ppl));
        if ppl < best_perplexity {
            best_perplexity = ppl;
            best_weight = w;
        }
    }
    Ok(TuneResult {
        best_weight,
        best_perplexity,
        evaluations,
    })
}
