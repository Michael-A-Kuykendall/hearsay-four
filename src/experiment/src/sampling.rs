//! TEST-200 sampling spec: stratified 200-utterance test-clean sample.
//!
//! The real sample is drawn on main (the lane has no Librispeech manifest)
//! with the corpus `sample-manifest` CLI:
//!
//! ```text
//! cargo run --release -p corpus --bin sample-manifest -- \
//!   data/librispeech/wav/test-clean.manifest.tsv \
//!   data/wer71g/test200s/strat200.manifest.tsv 5 20261103
//! ```
//!
//! This module pins the spec constants and verifies a sampled row set, so
//! the on-main run and the experiment agree on what TEST-200 means.

use corpus::manifest::{LibriRow, sample_stratified};
use std::collections::BTreeMap;

/// Rows per speaker in TEST-200.
pub const TEST200_PER_SPEAKER: usize = 5;
/// Fixed sampling seed for TEST-200.
pub const TEST200_SEED: u64 = 20261103;
/// TEST-200 has exactly 200 rows (40 test-clean speakers x 5).
pub const TEST200_ROWS: usize = 200;

/// Draw TEST-200 from parsed manifest rows: stratified sample plus spec
/// verification (exactly 200 rows, exactly 5 per speaker).
pub fn sample_test200_rows(rows: &[LibriRow]) -> Result<Vec<LibriRow>, String> {
    let picked = sample_stratified(rows, TEST200_PER_SPEAKER, TEST200_SEED);
    if picked.len() != TEST200_ROWS {
        return Err(format!(
            "TEST-200 sampled {} rows, spec requires exactly {TEST200_ROWS}",
            picked.len()
        ));
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &picked {
        *counts
            .entry(r.id.split('-').next().unwrap_or(""))
            .or_default() += 1;
    }
    for (spk, n) in &counts {
        if *n != TEST200_PER_SPEAKER {
            return Err(format!(
                "TEST-200 speaker {spk} has {n} rows, spec requires {TEST200_PER_SPEAKER}"
            ));
        }
    }
    Ok(picked)
}

/// Render rows in the 4-column no-header TSV layout the sampler CLI writes
/// (`id`, `rel`, `seconds`, `transcript`).
pub fn render_manifest_tsv(rows: &[LibriRow]) -> String {
    let mut s = String::new();
    for r in rows {
        s.push_str(&format!("{}\t{}\t{}\t{}\n", r.id, r.rel, r.sec, r.text));
    }
    s
}
