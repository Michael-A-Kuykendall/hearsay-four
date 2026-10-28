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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneHmm {
    pub name: String,
    pub n_states: usize,
    pub emissions: Vec<Gaussian>,
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
        for f in utterances.iter().flatten() {
            if f.len() != dim {
                return Err(HmmError::DimensionMismatch);
            }
            count += 1.0;
            for d in 0..dim {
                mean[d] += f[d];
            }
        }
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
        let mut map = BTreeMap::new();
        for &p in phones {
            map.insert(
                p.to_string(),
                PhoneHmm {
                    name: p.into(),
                    n_states: 3,
                    emissions: (0..3)
                        .map(|_| Gaussian {
                            mean: mean.clone(),
                            var: var.clone(),
                        })
                        .collect(),
                    trans: ltr_trans(3),
                    tied_to: None,
                },
            );
        }
        map.entry(SIL.into()).or_insert_with(|| PhoneHmm {
            name: SIL.into(),
            n_states: 3,
            emissions: (0..3)
                .map(|_| Gaussian {
                    mean: mean.clone(),
                    var: var.clone(),
                })
                .collect(),
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

    pub fn emission(&self, phone: &str, state: usize) -> Result<&Gaussian> {
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
        #[derive(Default, Clone)]
        struct Acc {
            mean: Vec<f64>,
            var: Vec<f64>,
            trans: Vec<Vec<f64>>,
            trans_occ: Vec<f64>,
        }
        let mut total_ll = 0.0;
        for _iter in 0..cfg.bw_iterations.max(1) {
            let mut accs: BTreeMap<String, Acc> = BTreeMap::new();
            for (name, h) in &self.phones {
                if h.tied_to.is_none() {
                    accs.insert(
                        name.clone(),
                        Acc {
                            mean: vec![0.0; self.dim * h.n_states],
                            var: vec![0.0; self.dim * h.n_states],
                            trans: vec![vec![0.0; h.n_states + 2]; h.n_states + 2],
                            trans_occ: vec![0.0; h.n_states + 2],
                        },
                    );
                }
            }
            let mut occ: BTreeMap<(String, usize), f64> = BTreeMap::new();
            total_ll = 0.0;
            for (seq, obs) in data {
                if obs.is_empty() {
                    continue;
                }
                let states = self.composite(seq)?;
                let n = states.len();
                let t_len = obs.len();
                let emit: Vec<Vec<f64>> = obs
                    .iter()
                    .map(|o| {
                        states
                            .iter()
                            .map(|(p, s)| self.emission(p, *s).map(|g| g.log_prob(o).exp()))
                            .collect::<Result<Vec<_>>>()
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut alpha = vec![vec![0.0; n]; t_len];
                let mut scale = vec![1.0; t_len];
                alpha[0][0] = emit[0][0];
                let s0: f64 = alpha[0].iter().sum();
                scale[0] = if s0 > 0.0 { 1.0 / s0 } else { 1.0 };
                for v in &mut alpha[0] {
                    *v *= scale[0];
                }
                for t in 1..t_len {
                    for j in 0..n {
                        let mut v = alpha[t - 1][j] * 0.5;
                        if j > 0 {
                            v += alpha[t - 1][j - 1] * 0.5;
                        }
                        alpha[t][j] = v * emit[t][j];
                    }
                    let s: f64 = alpha[t].iter().sum();
                    scale[t] = if s > 0.0 { 1.0 / s } else { 1.0 };
                    for v in &mut alpha[t] {
                        *v *= scale[t];
                    }
                }
                let mut beta = vec![vec![0.0; n]; t_len];
                for v in &mut beta[t_len - 1] {
                    *v = scale[t_len - 1];
                }
                for t in (0..t_len - 1).rev() {
                    for i in 0..n {
                        let mut v = beta[t + 1][i] * emit[t + 1][i] * 0.5;
                        if i + 1 < n {
                            v += beta[t + 1][i + 1] * emit[t + 1][i + 1] * 0.5;
                        }
                        beta[t][i] = scale[t] * v;
                    }
                }
                total_ll += -scale.iter().map(|s| s.ln()).sum::<f64>();
                for t in 0..t_len {
                    for j in 0..n {
                        let g = alpha[t][j] * beta[t][j];
                        if g <= 0.0 {
                            continue;
                        }
                        let (p, s) = &states[j];
                        let kp = if p == SP { SIL.to_string() } else { p.clone() };
                        let ks = if p == SP { 1 } else { *s };
                        *occ.entry((kp.clone(), ks)).or_default() += g;
                        if let Some(a) = accs.get_mut(&kp) {
                            for d in 0..self.dim {
                                a.mean[ks * self.dim + d] += g * obs[t][d];
                                a.var[ks * self.dim + d] += g * obs[t][d] * obs[t][d];
                            }
                        }
                    }
                }
                for t in 0..t_len - 1 {
                    for j in 0..n {
                        let g = alpha[t][j] * beta[t][j];
                        let (p, s) = &states[j];
                        let kp = if p == SP { SIL.to_string() } else { p.clone() };
                        let ks = if p == SP { 1 } else { *s };
                        if let Some(a) = accs.get_mut(&kp) {
                            a.trans[ks + 1][ks + 1] += g * 0.5;
                            if ks + 2 < a.trans[ks + 1].len() {
                                a.trans[ks + 1][ks + 2] += g * 0.5;
                            }
                            a.trans_occ[ks + 1] += g;
                        }
                    }
                }
            }
            for (name, h) in self.phones.clone() {
                if h.tied_to.is_some() {
                    continue;
                }
                let a = &accs[&name];
                let mut nh = h.clone();
                for s in 0..h.n_states {
                    let o = occ.get(&(name.clone(), s)).copied().unwrap_or(0.0);
                    if o > 1e-8 {
                        for d in 0..self.dim {
                            let m = a.mean[s * self.dim + d] / o;
                            let v = (a.var[s * self.dim + d] / o - m * m)
                                .max(self.global_var[d] * cfg.var_floor_ratio);
                            nh.emissions[s].mean[d] = m;
                            nh.emissions[s].var[d] = v;
                        }
                    }
                    let row = &a.trans[s + 1];
                    let tot: f64 = row.iter().sum();
                    if tot > 1e-8 {
                        for (j, v) in row.iter().enumerate() {
                            nh.trans[s + 1][j] = v / tot;
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
                        h.emissions[*s].mean[d] = m;
                        h.emissions[*s].var[d] =
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
            let g = set.emission(p, s).unwrap();
            for _ in 0..frames_per_state {
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
