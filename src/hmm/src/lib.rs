//! R2a HMM trainer core (bead hearsay-four-tnk).
//! 3-state left-to-right monophone HMMs, diagonal Gaussians (1 mixture),
//! SIL 3-state + SP 1-state tied to SIL centre; flat start, Viterbi forced
//! alignment, Baum-Welch embedded re-estimation, variance floors,
//! JSON text model + lossless binary twin. Feature vectors are supplied by
//! the caller; no audio/D0/D1 data is read by this crate.
#![allow(clippy::needless_range_loop)]
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub mod adapt;

pub const SIL: &str = "SIL";
pub const SP: &str = "SP";
pub const STATES_PER_PHONE: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum HmmError {
    EmptyData,
    DimensionMismatch,
    UnknownPhone(String),
    EmptyTranscript,
    InvalidModel(String),
    Io(String),
}
impl fmt::Display for HmmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for HmmError {}
pub type Result<T> = std::result::Result<T, HmmError>;

/// Training/dev utterance with a stable id: (id, phone sequence, frames).
pub type UttData = (String, Vec<String>, Vec<Vec<f64>>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gaussian {
    pub mean: Vec<f64>,
    pub var: Vec<f64>,
}
impl Gaussian {
    pub fn log_prob(&self, x: &[f64]) -> f64 {
        let mut s = 0.0;
        for d in 0..x.len() {
            let v = self.var[d].max(1e-12);
            let diff = x[d] - self.mean[d];
            s += -0.5 * (v.ln() + std::f64::consts::TAU.ln() + diff * diff / v);
        }
        s
    }
}

/// Provenance pointer: source utterances and frame count behind a parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Provenance {
    pub utterances: Vec<String>,
    pub frames: u64,
}

/// Sufficient statistics stored with every estimated parameter (R2b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamStats {
    pub count: f64,
    pub sum: Vec<f64>,
    pub sum_sq: Vec<f64>,
    pub provenance: Provenance,
}
impl ParamStats {
    pub fn zeros(dim: usize) -> Self {
        Self {
            count: 0.0,
            sum: vec![0.0; dim],
            sum_sq: vec![0.0; dim],
            provenance: Provenance::default(),
        }
    }
    pub fn global(
        count: f64,
        sum: Vec<f64>,
        sum_sq: Vec<f64>,
        utterances: Vec<String>,
        frames: u64,
    ) -> Self {
        Self {
            count,
            sum,
            sum_sq,
            provenance: Provenance { utterances, frames },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixtureComponent {
    pub weight: f64,
    pub gaussian: Gaussian,
    pub stats: ParamStats,
}

/// One emitting state's distribution: a Gaussian mixture (1 component pre-split).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mixture {
    pub components: Vec<MixtureComponent>,
}
impl Mixture {
    pub fn single(gaussian: Gaussian, stats: ParamStats) -> Self {
        Self {
            components: vec![MixtureComponent {
                weight: 1.0,
                gaussian,
                stats,
            }],
        }
    }
    pub fn log_prob(&self, x: &[f64]) -> f64 {
        let mut mx = f64::NEG_INFINITY;
        let mut terms = Vec::with_capacity(self.components.len());
        for c in &self.components {
            let t = c.weight.max(1e-300).ln() + c.gaussian.log_prob(x);
            terms.push(t);
            if t > mx {
                mx = t;
            }
        }
        if mx == f64::NEG_INFINITY {
            return mx;
        }
        mx + terms.iter().map(|t| (t - mx).exp()).sum::<f64>().ln()
    }
    /// Split the highest-weight component (mean perturbed +/-0.2 sigma on the max-variance dim).
    pub fn split_largest(&mut self) {
        if self.components.is_empty() {
            return;
        }
        let mut idx = 0;
        for (i, c) in self.components.iter().enumerate() {
            if c.weight > self.components[idx].weight {
                idx = i;
            }
        }
        let src = self.components[idx].clone();
        let dmax = src
            .gaussian
            .var
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        let off = 0.2 * src.gaussian.var[dmax].max(1e-12).sqrt();
        let mut a = src.clone();
        let mut b = src.clone();
        a.weight /= 2.0;
        b.weight /= 2.0;
        a.gaussian.mean[dmax] += off;
        b.gaussian.mean[dmax] -= off;
        for s in [&mut a.stats, &mut b.stats] {
            s.count /= 2.0;
            for v in s.sum.iter_mut().chain(s.sum_sq.iter_mut()) {
                *v /= 2.0;
            }
            s.provenance.frames /= 2;
        }
        self.components[idx] = a;
        self.components.push(b);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneHmm {
    pub name: String,
    pub n_states: usize,
    pub emissions: Vec<Mixture>,
    pub trans: Vec<Vec<f64>>,
    pub tied_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HmmSet {
    pub dim: usize,
    pub phones: BTreeMap<String, PhoneHmm>,
    pub global_mean: Vec<f64>,
    pub global_var: Vec<f64>,
    pub var_floor_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrainConfig {
    pub bw_iterations: usize,
    pub embedded_iterations: usize,
    pub var_floor_ratio: f64,
    pub optional_sp: bool,
}
impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            bw_iterations: 5,
            embedded_iterations: 3,
            var_floor_ratio: 0.01,
            optional_sp: true,
        }
    }
}

fn ltr_trans(n: usize) -> Vec<Vec<f64>> {
    let mut t = vec![vec![0.0; n + 2]; n + 2];
    t[0][1] = 1.0;
    for i in 1..=n {
        t[i][i] = 0.5;
        if i < n {
            t[i][i + 1] = 0.5;
        } else {
            t[i][n + 1] = 0.5;
        }
    }
    t
}
fn log_sum_exp(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        return b;
    }
    if b == f64::NEG_INFINITY {
        return a;
    }
    let m = a.max(b);
    m + ((a - m).exp() + (b - m).exp()).ln()
}
fn log_sum_exp_slice(xs: &[f64]) -> f64 {
    let mut mx = f64::NEG_INFINITY;
    for &x in xs {
        if x > mx {
            mx = x;
        }
    }
    if mx == f64::NEG_INFINITY {
        return mx;
    }
    mx + xs.iter().map(|x| (x - mx).exp()).sum::<f64>().ln()
}
impl HmmSet {
    pub fn flat_start(
        phones: &[&str],
        utterances: &[Vec<Vec<f64>>],
        var_floor_ratio: f64,
    ) -> Result<Self> {
        if utterances.is_empty() || utterances.iter().all(|u| u.is_empty()) {
            return Err(HmmError::EmptyData);
        }
        let dim = utterances.iter().flatten().next().map_or(0, |f| f.len());
        if dim == 0 {
            return Err(HmmError::EmptyData);
        }
        let (mut mean, mut var, mut count) = (vec![0.0; dim], vec![0.0; dim], 0f64);
        let mut sum_sq = vec![0.0; dim];
        for f in utterances.iter().flatten() {
            if f.len() != dim {
                return Err(HmmError::DimensionMismatch);
            }
            count += 1.0;
            for d in 0..dim {
                mean[d] += f[d];
                sum_sq[d] += f[d] * f[d];
            }
        }
        let raw_sum = mean.clone();
        for d in 0..dim {
            mean[d] /= count;
        }
        for f in utterances.iter().flatten() {
            for d in 0..dim {
                var[d] += (f[d] - mean[d]).powi(2);
            }
        }
        for d in 0..dim {
            var[d] = (var[d] / count).max(1e-6);
        }
        let ids: Vec<String> = (0..utterances.len()).map(|i| format!("utt:{i}")).collect();
        let stats = ParamStats::global(count, raw_sum, sum_sq, ids, count as u64);
        let mk = |m: &Vec<f64>, v: &Vec<f64>| {
            Mixture::single(
                Gaussian {
                    mean: m.clone(),
                    var: v.clone(),
                },
                stats.clone(),
            )
        };
        let mut map = BTreeMap::new();
        for &p in phones {
            map.insert(
                p.to_string(),
                PhoneHmm {
                    name: p.into(),
                    n_states: 3,
                    emissions: (0..3).map(|_| mk(&mean, &var)).collect(),
                    trans: ltr_trans(3),
                    tied_to: None,
                },
            );
        }
        map.entry(SIL.into()).or_insert_with(|| PhoneHmm {
            name: SIL.into(),
            n_states: 3,
            emissions: (0..3).map(|_| mk(&mean, &var)).collect(),
            trans: ltr_trans(3),
            tied_to: None,
        });
        let sil_centre = map[SIL].emissions[1].clone();
        map.insert(
            SP.into(),
            PhoneHmm {
                name: SP.into(),
                n_states: 1,
                emissions: vec![sil_centre],
                trans: vec![
                    vec![0.0, 1.0, 0.0],
                    vec![0.0, 0.5, 0.5],
                    vec![0.0, 0.0, 0.0],
                ],
                tied_to: Some(format!("{SIL}:1")),
            },
        );
        Ok(Self {
            dim,
            phones: map,
            global_mean: mean,
            global_var: var,
            var_floor_ratio,
        })
    }

    pub fn emission(&self, phone: &str, state: usize) -> Result<&Mixture> {
        let h = self
            .phones
            .get(phone)
            .ok_or_else(|| HmmError::UnknownPhone(phone.into()))?;
        if let Some(tie) = &h.tied_to {
            let (p, s) = tie
                .rsplit_once(':')
                .ok_or_else(|| HmmError::InvalidModel(tie.clone()))?;
            return self.emission(
                p,
                s.parse().map_err(|_| HmmError::InvalidModel(tie.clone()))?,
            );
        }
        h.emissions
            .get(state)
            .ok_or_else(|| HmmError::InvalidModel(phone.into()))
    }

    pub fn expand_transcript(
        &self,
        words: &[&str],
        dict: &BTreeMap<String, Vec<String>>,
        optional_sp: bool,
    ) -> Result<Vec<String>> {
        if words.is_empty() {
            return Err(HmmError::EmptyTranscript);
        }
        let mut out = vec![SIL.to_string()];
        for (i, w) in words.iter().enumerate() {
            let pron = dict
                .get(&w.to_uppercase())
                .ok_or_else(|| HmmError::UnknownPhone((*w).into()))?;
            out.extend(pron.iter().cloned());
            if i + 1 < words.len() && optional_sp {
                out.push(SP.into());
            }
        }
        out.push(SIL.into());
        Ok(out)
    }

    fn composite(&self, phones_seq: &[String]) -> Result<Vec<(String, usize)>> {
        let mut states = Vec::new();
        for p in phones_seq {
            let h = self
                .phones
                .get(p)
                .ok_or_else(|| HmmError::UnknownPhone(p.clone()))?;
            for s in 0..h.n_states {
                states.push((p.clone(), s));
            }
        }
        Ok(states)
    }

    pub fn forced_align(
        &self,
        phones_seq: &[String],
        obs: &[Vec<f64>],
    ) -> Result<(Vec<usize>, f64)> {
        if obs.is_empty() {
            return Err(HmmError::EmptyData);
        }
        let states = self.composite(phones_seq)?;
        let n = states.len();
        if n == 0 {
            return Err(HmmError::EmptyTranscript);
        }
        let neg = f64::NEG_INFINITY;
        let mut delta = vec![neg; n];
        let mut psi = vec![vec![0usize; n]; obs.len()];
        delta[0] = self.emission(&states[0].0, states[0].1)?.log_prob(&obs[0]);
        for t in 1..obs.len() {
            let mut nd = vec![neg; n];
            for j in 0..n {
                let e = self.emission(&states[j].0, states[j].1)?.log_prob(&obs[t]);
                let (mut best, mut arg) = (neg, j);
                for i in [j.saturating_sub(1), j] {
                    if delta[i] > neg {
                        let v = delta[i] + 0.5f64.ln();
                        if v > best {
                            best = v;
                            arg = i;
                        }
                    }
                }
                nd[j] = best + e;
                psi[t][j] = arg;
            }
            delta = nd;
        }
        let (mut best, mut last) = (neg, 0usize);
        for j in 0..n {
            if delta[j] > best {
                best = delta[j];
                last = j;
            }
        }
        let mut path = vec![0usize; obs.len()];
        path[obs.len() - 1] = last;
        for t in (1..obs.len()).rev() {
            path[t - 1] = psi[t][path[t]];
        }
        Ok((path, best))
    }

    pub fn baum_welch(
        &mut self,
        data: &[(Vec<String>, Vec<Vec<f64>>)],
        cfg: &TrainConfig,
    ) -> Result<f64> {
        let with_ids: Vec<(String, Vec<String>, Vec<Vec<f64>>)> = data
            .iter()
            .enumerate()
            .map(|(i, (s, o))| (format!("utt:{i}"), s.clone(), o.clone()))
            .collect();
        self.baum_welch_ids(&with_ids, cfg)
    }
    pub fn baum_welch_ids(&mut self, data: &[UttData], cfg: &TrainConfig) -> Result<f64> {
        use std::collections::BTreeSet;
        #[derive(Default, Clone)]
        struct CAcc {
            count: f64,
            sum: Vec<f64>,
            sumsq: Vec<f64>,
            utts: BTreeSet<String>,
            frames: u64,
        }
        // Transition choice (hearsay-four-bwf--baum-welch-underflow-fix): the
        // old M-step split every state's occupancy 50/50 between self-loop
        // and forward arc, so estimated transitions were always 0.5/0.5 (dead
        // estimator). Deleted; the E-step uses fixed ln(0.5), matching
        // forced_align and decode_phones. Proper xi-based transition training
        // is follow-up work, not needed for the underflow fix.
        let log_half = 0.5f64.ln();
        let mut total_ll = 0.0;
        for _iter in 0..cfg.bw_iterations.max(1) {
            let mut cacc: BTreeMap<(String, usize, usize), CAcc> = BTreeMap::new();
            for (name, h) in &self.phones {
                if h.tied_to.is_none() {
                    for s in 0..h.n_states {
                        for c in 0..h.emissions[s].components.len() {
                            cacc.insert(
                                (name.clone(), s, c),
                                CAcc {
                                    sum: vec![0.0; self.dim],
                                    sumsq: vec![0.0; self.dim],
                                    ..Default::default()
                                },
                            );
                        }
                    }
                }
            }
            total_ll = 0.0;
            for (utt_id, seq, obs) in data {
                if obs.is_empty() {
                    continue;
                }
                let states = self.composite(seq)?;
                let n = states.len();
                let t_len = obs.len();
                // Log-domain emits: never exp() here (39 dims underflow to 0).
                let log_emit: Vec<Vec<f64>> = obs
                    .iter()
                    .map(|o| {
                        states
                            .iter()
                            .map(|(p, s)| self.emission(p, *s).map(|m| m.log_prob(o)))
                            .collect::<Result<Vec<_>>>()
                    })
                    .collect::<Result<Vec<_>>>()?;
                // Forward pass in log domain with per-frame normalization.
                // log_c[t] = log(sum_j exp(unnorm[t][j])); total LL = sum log_c.
                let mut log_alpha = vec![vec![f64::NEG_INFINITY; n]; t_len];
                let mut log_c = vec![0.0; t_len];
                log_alpha[0][0] = log_emit[0][0];
                log_c[0] = log_sum_exp_slice(&log_alpha[0]);
                for v in &mut log_alpha[0] {
                    *v -= log_c[0];
                }
                for t in 1..t_len {
                    let mut unnorm = vec![f64::NEG_INFINITY; n];
                    for (j, u) in unnorm.iter_mut().enumerate() {
                        let stay = log_alpha[t - 1][j] + log_half;
                        let enter = if j > 0 {
                            log_alpha[t - 1][j - 1] + log_half
                        } else {
                            f64::NEG_INFINITY
                        };
                        *u = log_emit[t][j] + log_sum_exp(stay, enter);
                    }
                    log_c[t] = log_sum_exp_slice(&unnorm);
                    for j in 0..n {
                        log_alpha[t][j] = unnorm[j] - log_c[t];
                    }
                }
                // Backward pass in log domain (scaled to match forward).
                let mut log_beta = vec![vec![f64::NEG_INFINITY; n]; t_len];
                for v in &mut log_beta[t_len - 1] {
                    *v = -log_c[t_len - 1];
                }
                for t in (0..t_len.saturating_sub(1)).rev() {
                    for i in 0..n {
                        let stay = log_half + log_emit[t + 1][i] + log_beta[t + 1][i];
                        let next = if i + 1 < n {
                            log_half + log_emit[t + 1][i + 1] + log_beta[t + 1][i + 1]
                        } else {
                            f64::NEG_INFINITY
                        };
                        log_beta[t][i] = -log_c[t] + log_sum_exp(stay, next);
                    }
                }
                total_ll += log_c.iter().sum::<f64>();
                // Gammas: renormalize per frame, assert they sum to ~1.
                for t in 0..t_len {
                    let mut lg = vec![f64::NEG_INFINITY; n];
                    for j in 0..n {
                        lg[j] = log_alpha[t][j] + log_beta[t][j];
                    }
                    let lse = log_sum_exp_slice(&lg);
                    if !lse.is_finite() {
                        continue;
                    }
                    let mut gammas = vec![0.0; n];
                    let mut gsum = 0.0;
                    for j in 0..n {
                        let g = (lg[j] - lse).exp();
                        gammas[j] = g;
                        gsum += g;
                    }
                    debug_assert!(
                        (gsum - 1.0).abs() < 1e-6,
                        "gamma row must sum to 1, got {gsum}"
                    );
                    for j in 0..n {
                        let g = gammas[j];
                        if g <= 0.0 {
                            continue;
                        }
                        let (p, s) = &states[j];
                        let kp = if p == SP { SIL.to_string() } else { p.clone() };
                        let ks = if p == SP { 1 } else { *s };
                        let mix = self.emission(p, *s)?.clone();
                        // Mixture responsibilities in log domain.
                        let log_denom = mix.log_prob(&obs[t]);
                        if !log_denom.is_finite() {
                            continue;
                        }
                        let log_g = g.ln();
                        for (ci, comp) in mix.components.iter().enumerate() {
                            let log_r = log_g
                                + comp.weight.max(1e-300).ln()
                                + comp.gaussian.log_prob(&obs[t])
                                - log_denom;
                            let r = log_r.exp();
                            if r <= 1e-12 {
                                continue;
                            }
                            if let Some(a) = cacc.get_mut(&(kp.clone(), ks, ci)) {
                                a.count += r;
                                for d in 0..self.dim {
                                    a.sum[d] += r * obs[t][d];
                                    a.sumsq[d] += r * obs[t][d] * obs[t][d];
                                }
                                a.utts.insert(utt_id.clone());
                                a.frames += 1;
                            }
                        }
                    }
                }
            }
            // M-step (means/variances/mixture weights only; transitions fixed).
            for (name, h) in self.phones.clone() {
                if h.tied_to.is_some() {
                    continue;
                }
                let mut nh = h.clone();
                for s in 0..h.n_states {
                    let mut wsum = 0.0;
                    for c in 0..nh.emissions[s].components.len() {
                        if let Some(ca) = cacc.get(&(name.clone(), s, c))
                            && ca.count > 1e-8
                        {
                            wsum += ca.count;
                        }
                    }
                    for c in 0..nh.emissions[s].components.len() {
                        if let Some(ca) = cacc.get(&(name.clone(), s, c))
                            && ca.count > 1e-8
                        {
                            let comp = &mut nh.emissions[s].components[c];
                            for d in 0..self.dim {
                                let m = ca.sum[d] / ca.count;
                                comp.gaussian.mean[d] = m;
                                comp.gaussian.var[d] = (ca.sumsq[d] / ca.count - m * m)
                                    .max(self.global_var[d] * cfg.var_floor_ratio);
                            }
                            if wsum > 1e-8 {
                                comp.weight = ca.count / wsum;
                            }
                            comp.stats = ParamStats {
                                count: ca.count,
                                sum: ca.sum.clone(),
                                sum_sq: ca.sumsq.clone(),
                                provenance: Provenance {
                                    utterances: ca.utts.iter().cloned().collect(),
                                    frames: ca.frames,
                                },
                            };
                        }
                    }
                }
                self.phones.insert(name, nh);
            }
            self.refresh_sp();
        }
        Ok(total_ll)
    }
    fn refresh_sp(&mut self) {
        if let Some(sil) = self.phones.get(SIL).cloned()
            && let Some(sp) = self.phones.get_mut(SP)
        {
            sp.emissions = vec![sil.emissions[1].clone()];
        }
    }

    pub fn train_embedded(
        &mut self,
        data: &[(Vec<String>, Vec<Vec<f64>>)],
        cfg: &TrainConfig,
    ) -> Result<Vec<f64>> {
        // HInit-style symmetry breaking: uniform segmentation hard assignment.
        {
            type SegAcc = (f64, Vec<f64>, Vec<f64>);
            let mut sums: BTreeMap<(String, usize), SegAcc> = BTreeMap::new();
            for (seq, obs) in data {
                let states = self.composite(seq)?;
                let n = states.len();
                for (t, frame) in obs.iter().enumerate() {
                    let idx = ((t * n) / obs.len().max(1)).min(n - 1);
                    let (p, s) = &states[idx];
                    let kp = if p == SP { SIL.to_string() } else { p.clone() };
                    let ks = if p == SP { 1 } else { *s };
                    let e = sums
                        .entry((kp, ks))
                        .or_insert_with(|| (0.0, vec![0.0; self.dim], vec![0.0; self.dim]));
                    e.0 += 1.0;
                    for d in 0..self.dim {
                        e.1[d] += frame[d];
                        e.2[d] += frame[d] * frame[d];
                    }
                }
            }
            for ((phone, s), (count, sum, sq)) in &sums {
                if *count > 1.0
                    && let Some(h) = self.phones.get_mut(phone)
                {
                    for d in 0..self.dim {
                        let m = sum[d] / count;
                        h.emissions[*s].components[0].gaussian.mean[d] = m;
                        h.emissions[*s].components[0].gaussian.var[d] =
                            (sq[d] / count - m * m).max(self.global_var[d] * cfg.var_floor_ratio);
                    }
                }
            }
            self.refresh_sp();
        }
        let mut history = Vec::new();
        for _ in 0..cfg.embedded_iterations.max(1) {
            for (seq, obs) in data {
                let _ = self.forced_align(seq, obs)?;
            }
            history.push(self.baum_welch(data, cfg)?);
        }
        Ok(history)
    }

    /// Free phone-loop Viterbi decoding (for DEV PER). SIL/SP included in output.
    pub fn decode_phones(&self, obs: &[Vec<f64>]) -> Result<Vec<String>> {
        if obs.is_empty() {
            return Err(HmmError::EmptyData);
        }
        let mut phones: Vec<String> = self.phones.keys().cloned().collect();
        phones.sort();
        let mut states: Vec<(String, usize)> = Vec::new();
        for p in &phones {
            let h = &self.phones[p];
            for s in 0..h.n_states {
                states.push((p.clone(), s));
            }
        }
        let n = states.len();
        let np = phones.len() as f64;
        let neg = f64::NEG_INFINITY;
        let start_log = -(np.ln());
        let jump_log = 0.5f64.ln() - np.ln();
        let mut delta = vec![neg; n];
        for (j, (_, s)) in states.iter().enumerate() {
            if *s == 0 {
                delta[j] = start_log + self.emission(&states[j].0, states[j].1)?.log_prob(&obs[0]);
            }
        }
        let mut psi = vec![vec![0usize; n]; obs.len()];
        for t in 1..obs.len() {
            let mut nd = vec![neg; n];
            for j in 0..n {
                let e = self.emission(&states[j].0, states[j].1)?.log_prob(&obs[t]);
                let mut best = neg;
                let mut arg = j;
                for (i, (ip, is)) in states.iter().enumerate() {
                    if delta[i] == neg {
                        continue;
                    }
                    let tr = if i == j || (*is + 1 == states[j].1 && ip == &states[j].0) {
                        0.5f64.ln()
                    } else if states[j].1 == 0 && *is == self.phones[ip].n_states - 1 {
                        jump_log
                    } else {
                        continue;
                    };
                    let v = delta[i] + tr;
                    if v > best {
                        best = v;
                        arg = i;
                    }
                }
                nd[j] = best + e;
                psi[t][j] = arg;
            }
            delta = nd;
        }
        let (mut best, mut last) = (neg, 0usize);
        for j in 0..n {
            if delta[j] > best {
                best = delta[j];
                last = j;
            }
        }
        let mut path = vec![0usize; obs.len()];
        path[obs.len() - 1] = last;
        for t in (1..obs.len()).rev() {
            path[t - 1] = psi[t][path[t]];
        }
        let mut out: Vec<String> = Vec::new();
        for &idx in &path {
            let p = states[idx].0.clone();
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
        Ok(out)
    }
    /// DEV phone error rate. SIL/SP stripped from both reference and hypothesis.
    pub fn phone_error_rate(&self, data: &[(Vec<String>, Vec<Vec<f64>>)]) -> Result<PerReport> {
        let strip = |seq: &[String]| -> Vec<String> {
            seq.iter()
                .filter(|p| *p != SIL && *p != SP)
                .cloned()
                .collect()
        };
        let mut per_utterance = Vec::with_capacity(data.len());
        let (mut errors, mut total) = (0usize, 0usize);
        for (seq, obs) in data {
            let reference = strip(seq);
            let hyp = strip(&self.decode_phones(obs)?);
            let e = phone_edit_distance(&reference, &hyp);
            errors += e;
            total += reference.len();
            per_utterance.push(if reference.is_empty() {
                0.0
            } else {
                e as f64 / reference.len() as f64
            });
        }
        Ok(PerReport {
            per: if total == 0 {
                0.0
            } else {
                errors as f64 / total as f64
            },
            per_utterance,
            total_errors: errors,
            total_ref_phones: total,
        })
    }
    /// Every estimated parameter must carry sufficient statistics + a provenance pointer.
    pub fn validate_provenance(&self) -> Result<()> {
        let mut missing: Vec<String> = Vec::new();
        for (name, h) in &self.phones {
            if h.tied_to.is_some() {
                continue;
            }
            for (s, mix) in h.emissions.iter().enumerate() {
                if mix.components.is_empty() {
                    missing.push(format!("{name}:{s} (no components)"));
                    continue;
                }
                for (c, comp) in mix.components.iter().enumerate() {
                    let st = &comp.stats;
                    if st.sum.len() != self.dim
                        || st.sum_sq.len() != self.dim
                        || st.provenance.utterances.is_empty()
                    {
                        missing.push(format!("{name}:{s}/{c}"));
                    }
                }
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(HmmError::InvalidModel(format!(
                "parameters missing provenance: {}",
                missing.join(", ")
            )))
        }
    }
    /// Grow every non-tied state's mixture to `target` components by repeated splitting.
    pub fn split_mixtures_to(&mut self, target: usize) {
        for h in self.phones.values_mut() {
            if h.tied_to.is_some() {
                continue;
            }
            for mix in h.emissions.iter_mut() {
                while !mix.components.is_empty() && mix.components.len() < target {
                    mix.split_largest();
                }
                let w: f64 = mix.components.iter().map(|c| c.weight).sum();
                if w > 0.0 {
                    for c in mix.components.iter_mut() {
                        c.weight /= w;
                    }
                }
            }
        }
        self.refresh_sp();
    }
    /// Checkpoint every BW iteration; select the best DEV-PER checkpoint.
    /// Likelihood is recorded as a diagnostic only. Splitting stages in
    /// `split_targets` (e.g. [2, 4, 8]) are attempted from the best model so
    /// far; each decision is recorded from DEV PER.
    pub fn train_checkpoints(
        &mut self,
        train: &[UttData],
        dev: &[(Vec<String>, Vec<Vec<f64>>)],
        cfg: &TrainConfig,
        split_targets: &[usize],
    ) -> Result<CheckpointRun> {
        let one = TrainConfig {
            bw_iterations: 1,
            ..*cfg
        };
        let train_plain: Vec<(Vec<String>, Vec<Vec<f64>>)> = train
            .iter()
            .map(|(_, s, o)| (s.clone(), o.clone()))
            .collect();
        let _ = train_plain;
        let mut checkpoints: Vec<Checkpoint> = Vec::new();
        let eval = |m: &HmmSet| -> Result<(f64, f64)> {
            let per = m.phone_error_rate(dev)?.per;
            let mut ll = 0.0;
            for (_, seq, obs) in train.iter().take(64) {
                ll += m.forced_align(seq, obs)?.1;
            }
            Ok((ll, per))
        };
        let (ll0, per0) = eval(self)?;
        checkpoints.push(Checkpoint {
            iteration: 0,
            mixtures: 1,
            train_ll: ll0,
            dev_per: per0,
            model: self.clone(),
        });
        let mut mixtures = 1usize;
        for iter in 1..=cfg.bw_iterations.max(1) {
            self.baum_welch_ids(train, &one)?;
            let (ll, per) = eval(self)?;
            checkpoints.push(Checkpoint {
                iteration: iter,
                mixtures,
                train_ll: ll,
                dev_per: per,
                model: self.clone(),
            });
        }
        let mut split_decisions: Vec<SplitDecision> = Vec::new();
        let mut best_idx = 0usize;
        for (i, c) in checkpoints.iter().enumerate() {
            if c.dev_per < checkpoints[best_idx].dev_per {
                best_idx = i;
            }
        }
        for &target in split_targets {
            if target <= mixtures {
                continue;
            }
            let before = checkpoints[best_idx].dev_per;
            let mut cand = checkpoints[best_idx].model.clone();
            cand.split_mixtures_to(target);
            for _ in 0..cfg.bw_iterations.max(1) {
                cand.baum_welch_ids(train, &one)?;
            }
            let (ll, per) = eval(&cand)?;
            let accepted = per < before;
            split_decisions.push(SplitDecision {
                from_mixtures: mixtures,
                to_mixtures: target,
                dev_per_before: before,
                dev_per_after: per,
                accepted,
            });
            if accepted {
                mixtures = target;
                checkpoints.push(Checkpoint {
                    iteration: checkpoints.len(),
                    mixtures: target,
                    train_ll: ll,
                    dev_per: per,
                    model: cand,
                });
                best_idx = checkpoints.len() - 1;
            }
        }
        let selected = best_idx;
        let flat_dev_per = checkpoints[0].dev_per;
        *self = checkpoints[selected].model.clone();
        Ok(CheckpointRun {
            checkpoints,
            selected,
            split_decisions,
            flat_dev_per,
        })
    }
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
    pub fn from_text(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| HmmError::InvalidModel(e.to_string()))
    }
    pub fn to_binary(&self) -> Vec<u8> {
        let text = serde_json::to_string(self).unwrap_or_default();
        let mut out = b"HMM1".to_vec();
        out.extend_from_slice(&(text.len() as u64).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out
    }
    pub fn from_binary(b: &[u8]) -> Result<Self> {
        if b.len() < 12 || &b[0..4] != b"HMM1" {
            return Err(HmmError::InvalidModel("bad magic".into()));
        }
        let len = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
        if b.len() < 12 + len {
            return Err(HmmError::InvalidModel("truncated".into()));
        }
        Self::from_text(
            std::str::from_utf8(&b[12..12 + len])
                .map_err(|e| HmmError::InvalidModel(e.to_string()))?,
        )
    }
    pub fn save_text(&self, p: &std::path::Path) -> Result<()> {
        std::fs::write(p, self.to_text()).map_err(|e| HmmError::Io(e.to_string()))
    }
    pub fn load_text(p: &std::path::Path) -> Result<Self> {
        Self::from_text(&std::fs::read_to_string(p).map_err(|e| HmmError::Io(e.to_string()))?)
    }
}

/// Per-utterance and aggregate phone error rate on DEV (R2b selection metric).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerReport {
    pub per: f64,
    pub per_utterance: Vec<f64>,
    pub total_errors: usize,
    pub total_ref_phones: usize,
}

/// One checkpoint per BW iteration (iteration 0 = flat start).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub iteration: usize,
    pub mixtures: usize,
    pub train_ll: f64,
    pub dev_per: f64,
    pub model: HmmSet,
}

/// Mixture-splitting decision, recorded from DEV PER (R2b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitDecision {
    pub from_mixtures: usize,
    pub to_mixtures: usize,
    pub dev_per_before: f64,
    pub dev_per_after: f64,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRun {
    pub checkpoints: Vec<Checkpoint>,
    pub selected: usize,
    pub split_decisions: Vec<SplitDecision>,
    pub flat_dev_per: f64,
}

/// Paired mean difference with a normal-approximation 95% CI.
pub fn paired_ci(diffs: &[f64]) -> (f64, f64, f64) {
    if diffs.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let n = diffs.len() as f64;
    let mean = diffs.iter().sum::<f64>() / n;
    if diffs.len() < 2 {
        return (mean, mean, mean);
    }
    let var = diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let half = 1.96 * (var / n).sqrt();
    (mean, mean - half, mean + half)
}

/// Levenshtein distance over phone sequences.
pub fn phone_edit_distance(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j]
            } else {
                1 + prev[j].min(prev[j + 1]).min(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self, mean: f64, var: f64) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        mean + var.sqrt() * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

pub fn generate(
    set: &HmmSet,
    seq: &[String],
    frames_per_state: usize,
    rng: &mut Rng,
) -> Vec<Vec<f64>> {
    let mut out = Vec::new();
    for p in seq {
        let h = &set.phones[p];
        for s in 0..h.n_states {
            let mix = set.emission(p, s).unwrap();
            for _ in 0..frames_per_state {
                let mut pick = rng.next_f64();
                let mut comp = &mix.components[0];
                for c in &mix.components {
                    if pick < c.weight {
                        comp = c;
                        break;
                    }
                    pick -= c.weight;
                    comp = c;
                }
                let g = &comp.gaussian;
                out.push(
                    (0..set.dim)
                        .map(|d| rng.normal(g.mean[d], g.var[d]))
                        .collect(),
                );
            }
        }
    }
    out
}
