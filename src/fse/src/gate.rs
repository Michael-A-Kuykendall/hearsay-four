//! A1 gate protocols: DEV leave-one-take-out and PHASE-TEST (bead hearsay-four-b7p).

use crate::dtw::dtw_distance;
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
}
impl EvalResult {
    pub fn accuracy(&self) -> f64 {
        if self.predictions.is_empty() {
            return 0.0;
        }
        self.predictions.iter().filter(|p| p.correct).count() as f64 / self.predictions.len() as f64
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

fn classify(query: &Clip, templates: &[&Clip], band_frac: f64) -> Option<(String, f64)> {
    let mut best: Option<(f64, &str)> = None;
    for t in templates {
        let d = dtw_distance(&query.frames, &t.frames, band_frac);
        if best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, t.label.as_str()));
        }
    }
    best.filter(|(d, _)| d.is_finite())
        .map(|(d, l)| (l.to_string(), d))
}

fn build_result(preds: Vec<Prediction>, clips: &[Clip], degenerate: usize) -> EvalResult {
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
    EvalResult {
        predictions: preds,
        labels,
        confusion,
        degenerate,
    }
}

/// DEV: hold out each take in turn; templates are the other takes.
pub fn leave_one_take_out(clips: &[Clip], band_frac: f64) -> EvalResult {
    let takes: Vec<usize> = clips
        .iter()
        .map(|c| c.take)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut preds = Vec::new();
    let mut degenerate = 0usize;
    for take in takes {
        let templates: Vec<&Clip> = clips.iter().filter(|c| c.take != take).collect();
        for q in clips.iter().filter(|c| c.take == take) {
            match classify(q, &templates, band_frac) {
                Some((p, _)) => preds.push(Prediction {
                    truth: q.label.clone(),
                    correct: p == q.label,
                    predicted: p,
                    take,
                }),
                None => degenerate += 1,
            }
        }
    }
    build_result(preds, clips, degenerate)
}

/// PHASE-TEST: templates = original takes, queries = fresh session. Scored once by the caller.
pub fn phase_test(templates: &[Clip], queries: &[Clip], band_frac: f64) -> EvalResult {
    let refs: Vec<&Clip> = templates.iter().collect();
    let mut degenerate = 0usize;
    let preds: Vec<Prediction> = queries
        .iter()
        .filter_map(|q| match classify(q, &refs, band_frac) {
            Some((p, _)) => Some(Prediction {
                truth: q.label.clone(),
                correct: p == q.label,
                predicted: p,
                take: q.take,
            }),
            None => {
                degenerate += 1;
                None
            }
        })
        .collect();
    let mut all: Vec<Clip> = templates.to_vec();
    all.extend(queries.iter().cloned());
    build_result(preds, &all, degenerate)
}
/// True when any scored query was degenerate (no finite DTW distance).
/// Callers must treat a degenerate split as NOT COMPARABLE, never a score.
pub fn is_degenerate(res: &EvalResult) -> bool {
    res.degenerate > 0
}
