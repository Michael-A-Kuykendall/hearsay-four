//! A1 gate protocols: DEV leave-one-take-out and PHASE-TEST (bead hearsay-four-b7p).

use crate::dtw::{DtwVariant, dtw_distance_with};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Clip {
    pub label: String,
    pub take: usize,
    pub frames: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    pub truth: String,
    pub predicted: String,
    pub take: usize,
    pub correct: bool,
}

#[derive(Debug, Clone)]
pub struct EvalResult {
    pub predictions: Vec<Prediction>,
    pub labels: Vec<String>,
    pub confusion: Vec<Vec<usize>>, // rows truth, cols predicted, in `labels` order
    pub degenerate: usize, // queries with no finite DTW distance: NOT COMPARABLE, never scored
    pub truth_ranks: Vec<Option<usize>>, // per-prediction rank of truth in the finite N-best list (None: truth not finite)
}
impl EvalResult {
    pub fn accuracy(&self) -> f64 {
        if self.predictions.is_empty() {
            return 0.0;
        }
        self.predictions.iter().filter(|p| p.correct).count() as f64 / self.predictions.len() as f64
    }
    /// Top-K inclusion rate: fraction of scored queries whose truth ranks
    /// in the top K finite N-best entries (0-based rank < k).
    pub fn topk_inclusion(&self, k: usize) -> f64 {
        if self.predictions.is_empty() {
            return 0.0;
        }
        self.truth_ranks
            .iter()
            .filter(|r| r.map(|r| r < k).unwrap_or(false))
            .count() as f64
            / self.predictions.len() as f64
    }
    pub fn confusion_csv(&self) -> String {
        let mut s = format!("truth\\pred,{}\n", self.labels.join(","));
        for (i, lab) in self.labels.iter().enumerate() {
            s.push_str(&format!(
                "{},{}\n",
                lab,
                self.confusion[i]
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        s
    }
}

fn classify_nbest(
    query: &Clip,
    templates: &[&Clip],
    band_frac: f64,
    variant: DtwVariant,
) -> Option<(String, Vec<(String, f64)>)> {
    let mut ranked: Vec<(String, f64)> = templates
        .iter()
        .map(|t| {
            (
                t.label.clone(),
                dtw_distance_with(&query.frames, &t.frames, band_frac, variant),
            )
        })
        .filter(|(_, d)| d.is_finite())
        .collect();
    // Stable sort: ties keep manifest template order, so ranked[0] is the
    // strict-<-argmin winner, identical to the top-1 path.
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    if ranked.is_empty() {
        return None;
    }
    let best = ranked[0].0.clone();
    Some((best, ranked))
}

fn build_result(
    preds: Vec<Prediction>,
    clips: &[Clip],
    degenerate: usize,
    truth_ranks: Vec<Option<usize>>,
) -> EvalResult {
    let mut labels: Vec<String> = clips
        .iter()
        .map(|c| c.label.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    labels.sort();
    let idx: BTreeMap<&str, usize> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| (l.as_str(), i))
        .collect();
    let mut confusion = vec![vec![0usize; labels.len()]; labels.len()];
    for p in &preds {
        confusion[idx[p.truth.as_str()]][idx[p.predicted.as_str()]] += 1;
    }
    debug_assert_eq!(preds.len(), truth_ranks.len());
    EvalResult {
        predictions: preds,
        labels,
        confusion,
        degenerate,
        truth_ranks,
    }
}

/// DEV: hold out each take in turn; templates are the other takes.
pub fn leave_one_take_out(clips: &[Clip], band_frac: f64) -> EvalResult {
    leave_one_take_out_with_dtw(clips, band_frac, DtwVariant::Baseline)
}

pub fn leave_one_take_out_with_dtw(
    clips: &[Clip],
    band_frac: f64,
    variant: DtwVariant,
) -> EvalResult {
    let takes: Vec<usize> = clips
        .iter()
        .map(|c| c.take)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut preds = Vec::new();
    let mut ranks = Vec::new();
    let mut degenerate = 0usize;
    for take in takes {
        let templates: Vec<&Clip> = clips.iter().filter(|c| c.take != take).collect();
        for q in clips.iter().filter(|c| c.take == take) {
            match classify_nbest(q, &templates, band_frac, variant) {
                Some((p, ranked)) => {
                    ranks.push(ranked.iter().position(|(l, _)| l == &q.label));
                    preds.push(Prediction {
                        truth: q.label.clone(),
                        correct: p == q.label,
                        predicted: p,
                        take,
                    })
                }
                None => degenerate += 1,
            }
        }
    }
    build_result(preds, clips, degenerate, ranks)
}

/// PHASE-TEST: templates = original takes, queries = fresh session. Scored once by the caller.
pub fn phase_test(templates: &[Clip], queries: &[Clip], band_frac: f64) -> EvalResult {
    phase_test_with_dtw(templates, queries, band_frac, DtwVariant::Baseline)
}

pub fn phase_test_with_dtw(
    templates: &[Clip],
    queries: &[Clip],
    band_frac: f64,
    variant: DtwVariant,
) -> EvalResult {
    let refs: Vec<&Clip> = templates.iter().collect();
    let mut degenerate = 0usize;
    let mut preds = Vec::new();
    let mut ranks = Vec::new();
    for q in queries {
        match classify_nbest(q, &refs, band_frac, variant) {
            Some((p, ranked)) => {
                ranks.push(ranked.iter().position(|(l, _)| l == &q.label));
                preds.push(Prediction {
                    truth: q.label.clone(),
                    correct: p == q.label,
                    predicted: p,
                    take: q.take,
                });
            }
            None => degenerate += 1,
        }
    }
    let mut all: Vec<Clip> = templates.to_vec();
    all.extend(queries.iter().cloned());
    build_result(preds, &all, degenerate, ranks)
}
/// True when any scored query was degenerate (no finite DTW distance).
/// Callers must treat a degenerate split as NOT COMPARABLE, never a score.
pub fn is_degenerate(res: &EvalResult) -> bool {
    res.degenerate > 0
}
