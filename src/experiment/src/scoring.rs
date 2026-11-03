//! Scoring: 1-best baseline path, WER aggregation, delta CIs.
//!
//! Word error counts come from [`output::wer`] (the same scorer the
//! `wer_devclean` baseline uses). Corpus delta CIs come from
//! [`eval::corpus_delta_ci`] with the pinned default config (10k
//! resamples, pinned seed, two-sided 95 pct), so every run recomputes
//! bit-identical intervals.

use eval::{BootstrapCi, BootstrapConfig, corpus_delta_ci};
use lattice::Lattice;
use std::collections::HashMap;

/// Corpus WER totals (sums over utterances).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WerTotals {
    pub subs: usize,
    pub dels: usize,
    pub inss: usize,
    pub ref_len: usize,
}

impl WerTotals {
    pub fn errors(&self) -> usize {
        self.subs + self.dels + self.inss
    }

    pub fn wer(&self) -> f64 {
        if self.ref_len == 0 {
            0.0
        } else {
            self.errors() as f64 / self.ref_len as f64
        }
    }
}

/// Score hypotheses against references with [`output::wer`], summed.
pub fn score_corpus(refs: &[Vec<String>], hyps: &[Vec<String>]) -> WerTotals {
    assert_eq!(
        refs.len(),
        hyps.len(),
        "refs and hyps must align per utterance"
    );
    let mut totals = WerTotals {
        subs: 0,
        dels: 0,
        inss: 0,
        ref_len: 0,
    };
    for (r, h) in refs.iter().zip(hyps.iter()) {
        let s = output::wer(r, h);
        totals.subs += s.substitutions;
        totals.dels += s.deletions;
        totals.inss += s.insertions;
        totals.ref_len += s.reference_len;
    }
    totals
}

/// Per-utterance `(errors, normalized reference lengths)` for [`delta_ci`].
pub fn per_utterance_errors(refs: &[Vec<String>], hyps: &[Vec<String>]) -> (Vec<u32>, Vec<u32>) {
    assert_eq!(
        refs.len(),
        hyps.len(),
        "refs and hyps must align per utterance"
    );
    let mut errors = Vec::with_capacity(refs.len());
    let mut denoms = Vec::with_capacity(refs.len());
    for (r, h) in refs.iter().zip(hyps.iter()) {
        let s = output::wer(r, h);
        errors.push((s.substitutions + s.deletions + s.insertions) as u32);
        denoms.push(s.reference_len as u32);
    }
    (errors, denoms)
}

/// Paired-bootstrap CI on corpus delta-WER (new minus base).
pub fn delta_ci(base_errs: &[u32], new_errs: &[u32], denoms: &[u32]) -> BootstrapCi {
    corpus_delta_ci(base_errs, new_errs, denoms, BootstrapConfig::default())
}

/// Arm-A baseline: the max-`acoustic_llr` path through the lattice DAG.
///
/// Viterbi over tokens in `(end, start, id)` order — strictly topological
/// for forward arcs (`from.end <= to.start` implies `from.end < to.end`).
/// Ties resolve to the earliest-ranked path, so output is deterministic.
pub fn one_best_path(lat: &Lattice) -> Vec<String> {
    let n = lat.tokens.len();
    if n == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        (lat.tokens[a].end, lat.tokens[a].start, lat.tokens[a].id).cmp(&(
            lat.tokens[b].end,
            lat.tokens[b].start,
            lat.tokens[b].id,
        ))
    });
    let mut rank_of: HashMap<u64, usize> = HashMap::new();
    for (rank, &i) in order.iter().enumerate() {
        rank_of.insert(lat.tokens[i].id, rank);
    }
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    for a in &lat.arcs {
        if let (Some(&u), Some(&v)) = (rank_of.get(&a.from), rank_of.get(&a.to)) {
            let from = &lat.tokens[order[u]];
            let to = &lat.tokens[order[v]];
            if from.end <= to.start {
                incoming[v].push(u);
            }
        }
    }
    let mut best = vec![f64::NEG_INFINITY; n];
    let mut back: Vec<Option<usize>> = vec![None; n];
    for (rank, &i) in order.iter().enumerate() {
        let mut b = lat.tokens[i].acoustic_llr;
        let mut p = None;
        for &u in &incoming[rank] {
            let c = best[u] + lat.tokens[i].acoustic_llr;
            if c > b {
                b = c;
                p = Some(u);
            }
        }
        best[rank] = b;
        back[rank] = p;
    }
    let mut end = 0;
    for r in 1..n {
        if best[r] > best[end] {
            end = r;
        }
    }
    let mut rev = Vec::new();
    let mut r = end;
    loop {
        rev.push(lat.tokens[order[r]].word.clone());
        match back[r] {
            Some(u) => r = u,
            None => break,
        }
    }
    rev.reverse();
    rev
}
