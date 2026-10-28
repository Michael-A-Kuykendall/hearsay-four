//! Owner enrollment + MAP adaptation overlay (bead
//! hearsay-four-jem--owner-enrollment-adaptation).
//!
//! Stage 1 (gate): means-only MAP, `mu = (tau * mu_base + sum_x) / (tau + n)`
//! per Gaussian per dimension, variance stays base. Stage 2 (optional):
//! variance from the second moment, kept only if owner DEV improves with a
//! paired CI excluding 0.
//!
//! Each enrollment contribution is a retractable [`AdaptFact`] (count, sum-x,
//! sum-x2, source frames, supporting hypothesis id). Retracting a fact drops
//! it and recomputes the overlay from the remaining facts, so retraction is
//! exact by construction. The base model is immutable; adaptation is an
//! overlay derived on demand via [`AdaptOverlay::adapted_model`]. Facts mirror
//! fabric assert/retract semantics (hmm-local records; wiring facts into the
//! `fabric` crate itself is follow-up work, not needed for the gate).
//!
//! Enrollment here is supervised: the supporting hypothesis id is the
//! reference transcript (`ref:<utt>`). Unsupervised enrollment (decoded
//! hypothesis) is supported by passing the decode id instead; the accumulation
//! math is identical.

use crate::{HmmError, HmmSet, Provenance, Result, SIL, SP, paired_ci};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One Gaussian in the overlay: phone, emitting-state index, mixture index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AdaptKey {
    pub phone: String,
    pub state: usize,
    pub comp: usize,
}

/// Occupancy-weighted sufficient statistics for one Gaussian.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptStats {
    /// Occupancy mass (n).
    pub n: f64,
    /// Occupancy-weighted first moment (sum-x) per dimension.
    pub sum: Vec<f64>,
    /// Occupancy-weighted second moment (sum-x2) per dimension.
    pub sumsq: Vec<f64>,
    /// Source frames that contributed (responsibility above threshold).
    pub frames: u64,
}

impl AdaptStats {
    pub fn zeros(dim: usize) -> Self {
        Self {
            n: 0.0,
            sum: vec![0.0; dim],
            sumsq: vec![0.0; dim],
            frames: 0,
        }
    }

    fn add_in(&mut self, other: &AdaptStats) {
        debug_assert_eq!(self.sum.len(), other.sum.len());
        self.n += other.n;
        for d in 0..self.sum.len() {
            self.sum[d] += other.sum[d];
            self.sumsq[d] += other.sumsq[d];
        }
        self.frames += other.frames;
    }
}

/// JSON-compatible encoding for per-Gaussian stats: struct keys cannot be JSON
/// map keys, so the map travels as a sorted entry list and round-trips
/// exactly.
mod stats_serde {
    use super::{AdaptKey, AdaptStats};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S>(map: &BTreeMap<AdaptKey, AdaptStats>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.iter().collect::<Vec<_>>().serialize(ser)
    }

    pub fn deserialize<'de, D>(de: D) -> Result<BTreeMap<AdaptKey, AdaptStats>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = Vec::<(AdaptKey, AdaptStats)>::deserialize(de)?;
        Ok(entries.into_iter().collect())
    }
}

/// One enrollment contribution: a retractable fact.
///
/// `id` is the fact identity (`enroll:<utt>`); `hyp_id` is the supporting
/// hypothesis (`ref:<utt>` for supervised enrollment, a decode id otherwise).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptFact {
    pub id: String,
    pub hyp_id: String,
    pub frames: u64,
    #[serde(with = "stats_serde")]
    pub stats: BTreeMap<AdaptKey, AdaptStats>,
}

/// Adaptation stage: means-only (Stage 1 gate) or means+variances (Stage 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdaptStage {
    MeansOnly,
    MeansAndVariances,
}

/// MAP adaptation overlay: immutable base + retractable enrollment facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptOverlay {
    base: HmmSet,
    tau: f64,
    stage: AdaptStage,
    facts: BTreeMap<String, AdaptFact>,
}

impl AdaptOverlay {
    /// New overlay over an immutable base. `tau` must be finite and >= 0.
    pub fn new(base: HmmSet, tau: f64, stage: AdaptStage) -> Result<Self> {
        if !tau.is_finite() || tau < 0.0 {
            return Err(HmmError::InvalidModel(format!("bad MAP tau: {tau}")));
        }
        Ok(Self {
            base,
            tau,
            stage,
            facts: BTreeMap::new(),
        })
    }

    pub fn tau(&self) -> f64 {
        self.tau
    }

    pub fn stage(&self) -> AdaptStage {
        self.stage
    }

    pub fn base(&self) -> &HmmSet {
        &self.base
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub fn fact_ids(&self) -> Vec<String> {
        self.facts.keys().cloned().collect()
    }

    pub fn get_fact(&self, id: &str) -> Option<&AdaptFact> {
        self.facts.get(id)
    }

    /// Assert an enrollment fact. Duplicate ids and dimension mismatches fail.
    pub fn assert_fact(&mut self, fact: AdaptFact) -> Result<()> {
        if self.facts.contains_key(&fact.id) {
            return Err(HmmError::InvalidModel(format!(
                "duplicate enrollment fact: {}",
                fact.id
            )));
        }
        for (key, st) in &fact.stats {
            if st.sum.len() != self.base.dim || st.sumsq.len() != self.base.dim {
                return Err(HmmError::DimensionMismatch);
            }
            if key.state >= 1024 || key.comp >= 1024 {
                return Err(HmmError::InvalidModel(format!(
                    "fact {} key out of range: {key:?}",
                    fact.id
                )));
            }
        }
        self.facts.insert(fact.id.clone(), fact);
        Ok(())
    }

    /// Retract a fact by id, returning it. Unknown ids fail; the overlay is
    /// recomputed from the remaining facts on the next [`Self::adapted_model`]
    /// call, so retraction is exact.
    pub fn retract_fact(&mut self, id: &str) -> Result<AdaptFact> {
        self.facts
            .remove(id)
            .ok_or_else(|| HmmError::InvalidModel(format!("unknown enrollment fact: {id}")))
    }

    /// Fold live facts into per-Gaussian totals. Recomputed from scratch so
    /// retraction can never drift.
    pub fn totals(&self) -> BTreeMap<AdaptKey, AdaptStats> {
        let mut totals: BTreeMap<AdaptKey, AdaptStats> = BTreeMap::new();
        for fact in self.facts.values() {
            for (key, st) in &fact.stats {
                totals
                    .entry(key.clone())
                    .or_insert_with(|| AdaptStats::zeros(self.base.dim))
                    .add_in(st);
            }
        }
        totals
    }

    /// Derive the adapted model. The base is never mutated; with zero facts
    /// the result equals the base exactly.
    pub fn adapted_model(&self) -> HmmSet {
        let mut model = self.base.clone();
        if self.facts.is_empty() {
            return model;
        }
        let totals = self.totals();
        // Contributing fact ids per key, for provenance on touched components.
        let mut contributors: BTreeMap<AdaptKey, Vec<String>> = BTreeMap::new();
        for fact in self.facts.values() {
            for key in fact.stats.keys() {
                contributors
                    .entry(key.clone())
                    .or_default()
                    .push(fact.id.clone());
            }
        }
        for (key, st) in &totals {
            let denom = self.tau + st.n;
            if denom <= 0.0 {
                continue;
            }
            let Some(base_h) = self.base.phones.get(&key.phone) else {
                continue;
            };
            if key.state >= base_h.n_states {
                continue;
            }
            let Some(base_mix) = base_h.emissions.get(key.state) else {
                continue;
            };
            let Some(base_comp) = base_mix.components.get(key.comp) else {
                continue;
            };
            let base_mean = base_comp.gaussian.mean.clone();
            let base_var = base_comp.gaussian.var.clone();
            let Some(h) = model.phones.get_mut(&key.phone) else {
                continue;
            };
            if key.state >= h.emissions.len() || key.comp >= h.emissions[key.state].components.len()
            {
                continue;
            }
            let comp = &mut h.emissions[key.state].components[key.comp];
            for d in 0..model.dim {
                let mu_new = (self.tau * base_mean[d] + st.sum[d]) / denom;
                comp.gaussian.mean[d] = mu_new;
                if self.stage == AdaptStage::MeansAndVariances {
                    let second = (self.tau * (base_var[d] + base_mean[d] * base_mean[d])
                        + st.sumsq[d])
                        / denom;
                    comp.gaussian.var[d] =
                        (second - mu_new * mu_new).max(model.global_var[d] * model.var_floor_ratio);
                }
            }
            // MAP pseudo-statistics + enrollment provenance on touched params.
            let dim = model.dim;
            let mut pseudo_sum = vec![0.0; dim];
            let mut pseudo_sq = vec![0.0; dim];
            for d in 0..dim {
                pseudo_sum[d] = self.tau * base_mean[d] + st.sum[d];
                pseudo_sq[d] = self.tau * (base_var[d] + base_mean[d] * base_mean[d]) + st.sumsq[d];
            }
            comp.stats.count = denom;
            comp.stats.sum = pseudo_sum;
            comp.stats.sum_sq = pseudo_sq;
            comp.stats.provenance = Provenance {
                utterances: contributors.get(key).cloned().unwrap_or_default(),
                frames: st.frames,
            };
        }
        // SP is tied to the SIL centre: mirror the base refresh.
        if let Some(sil) = model.phones.get(SIL).cloned()
            && let Some(sp) = model.phones.get_mut(SP)
        {
            sp.emissions = vec![sil.emissions[1].clone()];
        }
        model
    }
}

/// Accumulate one enrollment utterance into a fact.
///
/// Alignment occupancies come from forced alignment (Viterbi state path) plus
/// per-frame mixture responsibilities in log domain — the same E-step math as
/// [`HmmSet::baum_welch_ids`], without any M-step. SP frames fold into the
/// SIL centre, matching training.
pub fn collect_fact(
    base: &HmmSet,
    utt_id: &str,
    hyp_id: &str,
    seq: &[String],
    obs: &[Vec<f64>],
) -> Result<AdaptFact> {
    if obs.is_empty() {
        return Err(HmmError::EmptyData);
    }
    if obs.iter().any(|f| f.len() != base.dim) {
        return Err(HmmError::DimensionMismatch);
    }
    // Composite state list, mirroring HmmSet::composite.
    let mut states: Vec<(String, usize)> = Vec::new();
    for p in seq {
        let h = base
            .phones
            .get(p)
            .ok_or_else(|| HmmError::UnknownPhone(p.clone()))?;
        for s in 0..h.n_states {
            states.push((p.clone(), s));
        }
    }
    let (path, _) = base.forced_align(seq, obs)?;
    let mut stats: BTreeMap<AdaptKey, AdaptStats> = BTreeMap::new();
    for (t, frame) in obs.iter().enumerate() {
        let (phone, state) = &states[path[t]];
        let mix = base.emission(phone, *state)?;
        let log_denom = mix.log_prob(frame);
        if !log_denom.is_finite() {
            continue;
        }
        let key_phone = if phone == SP {
            SIL.to_string()
        } else {
            phone.clone()
        };
        let key_state = if phone == SP { 1 } else { *state };
        for (ci, comp) in mix.components.iter().enumerate() {
            let log_r = comp.weight.max(1e-300).ln() + comp.gaussian.log_prob(frame) - log_denom;
            let r = log_r.exp();
            if r <= 1e-12 {
                continue;
            }
            let key = AdaptKey {
                phone: key_phone.clone(),
                state: key_state,
                comp: ci,
            };
            let entry = stats
                .entry(key)
                .or_insert_with(|| AdaptStats::zeros(base.dim));
            entry.n += r;
            for d in 0..base.dim {
                entry.sum[d] += r * frame[d];
                entry.sumsq[d] += r * frame[d] * frame[d];
            }
            entry.frames += 1;
        }
    }
    Ok(AdaptFact {
        id: format!("enroll:{utt_id}"),
        hyp_id: hyp_id.to_string(),
        frames: obs.len() as u64,
        stats,
    })
}

/// DEV verdict for one stage: best tau, paired CI of adapted-minus-base
/// per-utterance PER, and the keep/reject decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptDecision {
    pub stage: AdaptStage,
    pub best_tau: f64,
    pub taus_tried: Vec<f64>,
    pub dev_per_base: f64,
    pub dev_per_adapted: f64,
    pub mean_diff: f64,
    pub ci_lo: f64,
    pub ci_hi: f64,
    pub keep: bool,
    pub n_dev: usize,
    pub per_utt_base: Vec<f64>,
    pub per_utt_adapted: Vec<f64>,
}

/// Tune `tau` on owner DEV over `taus`, keeping the overlay only when adapted
/// DEV PER beats base DEV PER with a paired CI excluding 0. Ties on DEV PER
/// break toward the larger (more conservative) tau.
pub fn tune_tau(
    base: &HmmSet,
    facts: &[AdaptFact],
    dev: &[(Vec<String>, Vec<Vec<f64>>)],
    taus: &[f64],
    stage: AdaptStage,
) -> Result<AdaptDecision> {
    if taus.is_empty() {
        return Err(HmmError::InvalidModel("tau grid is empty".into()));
    }
    if dev.is_empty() {
        return Err(HmmError::EmptyData);
    }
    let base_rep = base.phone_error_rate(dev)?;
    let mut best: Option<(f64, crate::PerReport)> = None;
    for &tau in taus {
        let mut overlay = AdaptOverlay::new(base.clone(), tau, stage)?;
        for fact in facts {
            overlay.assert_fact(fact.clone())?;
        }
        let rep = overlay.adapted_model().phone_error_rate(dev)?;
        let is_best = match &best {
            None => true,
            Some((best_tau, best_rep)) => {
                rep.per < best_rep.per || (rep.per == best_rep.per && tau > *best_tau)
            }
        };
        if is_best {
            best = Some((tau, rep));
        }
    }
    let (best_tau, best_rep) = best.expect("non-empty tau grid yields a best");
    let diffs: Vec<f64> = best_rep
        .per_utterance
        .iter()
        .zip(&base_rep.per_utterance)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, lo, hi) = paired_ci(&diffs);
    Ok(AdaptDecision {
        stage,
        best_tau,
        taus_tried: taus.to_vec(),
        dev_per_base: base_rep.per,
        dev_per_adapted: best_rep.per,
        mean_diff: mean,
        ci_lo: lo,
        ci_hi: hi,
        keep: best_rep.per < base_rep.per && hi < 0.0,
        n_dev: dev.len(),
        per_utt_base: base_rep.per_utterance,
        per_utt_adapted: best_rep.per_utterance,
    })
}

/// Stage selection: Stage 2 (variances) is kept only when it beats Stage 1 on
/// owner DEV with a paired CI excluding 0. The overlay keep/reject verdict is
/// the selected stage's own verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageSelection {
    pub stage1: AdaptDecision,
    pub stage2: AdaptDecision,
    pub selected: AdaptStage,
    pub stage2_mean_diff: f64,
    pub stage2_ci_lo: f64,
    pub stage2_ci_hi: f64,
}

impl StageSelection {
    pub fn decision(&self) -> &AdaptDecision {
        match self.selected {
            AdaptStage::MeansOnly => &self.stage1,
            AdaptStage::MeansAndVariances => &self.stage2,
        }
    }
}

pub fn select_stage(
    base: &HmmSet,
    facts: &[AdaptFact],
    dev: &[(Vec<String>, Vec<Vec<f64>>)],
    taus: &[f64],
) -> Result<StageSelection> {
    let stage1 = tune_tau(base, facts, dev, taus, AdaptStage::MeansOnly)?;
    let stage2 = tune_tau(base, facts, dev, taus, AdaptStage::MeansAndVariances)?;
    let diffs: Vec<f64> = stage2
        .per_utt_adapted
        .iter()
        .zip(&stage1.per_utt_adapted)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, lo, hi) = paired_ci(&diffs);
    let stage2_wins = stage2.dev_per_adapted < stage1.dev_per_adapted && hi < 0.0;
    Ok(StageSelection {
        stage1,
        stage2,
        selected: if stage2_wins {
            AdaptStage::MeansAndVariances
        } else {
            AdaptStage::MeansOnly
        },
        stage2_mean_diff: mean,
        stage2_ci_lo: lo,
        stage2_ci_hi: hi,
    })
}
