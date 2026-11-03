//! Oracle WER sweep over K, reusing the exact tested oracle.
//!
//! The DP oracle lives in [`a2::oracle`] (exact min-edit path over the
//! lattice DAG, K filter keeps top-K per end frame plus the `a2-1best`
//! backbone). This module is the experiment harness around it: sweep one
//! utterance, sweep a corpus, aggregate per-K totals.

use a2::oracle::{OracleResult, sweep_k};

/// Sweep Ks used by the experiment (bead step 1).
pub const SWEEP_KS: [usize; 3] = [3, 5, 10];

/// One corpus-aggregated sweep row: oracle totals at one K.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepRow {
    pub k: usize,
    pub errors: u32,
    pub ref_len: u32,
    pub tokens: usize,
    pub arcs: usize,
}

impl SweepRow {
    pub fn wer(&self) -> f64 {
        if self.ref_len == 0 {
            0.0
        } else {
            f64::from(self.errors) / f64::from(self.ref_len)
        }
    }
}

/// Sweep one utterance: `(K, oracle result, kept tokens, kept arcs)` per K.
pub fn sweep_utterance(
    lat_text: &str,
    reference: &[String],
    ks: &[usize],
) -> Result<Vec<(usize, OracleResult, usize, usize)>, String> {
    sweep_k(lat_text, reference, ks)
}

/// Sweep a corpus: aggregate oracle errors and reference lengths per K.
/// `items` holds `(lattice JSONL text, reference words)` per utterance.
pub fn sweep_corpus(items: &[(&str, &[String])], ks: &[usize]) -> Result<Vec<SweepRow>, String> {
    let mut rows: Vec<SweepRow> = ks
        .iter()
        .map(|&k| SweepRow {
            k,
            errors: 0,
            ref_len: 0,
            tokens: 0,
            arcs: 0,
        })
        .collect();
    for (lat_text, reference) in items {
        let per_k = sweep_k(lat_text, reference, ks)?;
        for (row, (_, oracle, nt, na)) in rows.iter_mut().zip(per_k.iter()) {
            row.errors += oracle.errors;
            row.ref_len += oracle.ref_len;
            row.tokens += nt;
            row.arcs += na;
        }
    }
    Ok(rows)
}

/// Render corpus sweep rows as a Markdown table.
pub fn render_sweep_markdown(rows: &[SweepRow]) -> String {
    let mut s = String::from("| K | oracle errs | oracle WER | tokens | arcs |\n");
    s.push_str("|---|-------------|------------|--------|------|\n");
    for r in rows {
        s.push_str(&format!(
            "| {} | {} | {:.4} | {} | {} |\n",
            r.k,
            r.errors,
            r.wer(),
            r.tokens,
            r.arcs
        ));
    }
    s
}
