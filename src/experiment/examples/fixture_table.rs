//! Print the fixture budget table plus the oracle sweep (evidence command).
//!
//! ```text
//! cargo run --release -p experiment --example fixture_table
//! ```

use experiment::fixtures::{all_fixtures, lattice_text};
use experiment::oracle_sweep::{SWEEP_KS, render_sweep_markdown, sweep_corpus};
use experiment::table::{render_markdown, run_budget_table};

fn main() {
    let fxs = all_fixtures();
    let table = run_budget_table(&fxs);
    println!("fixture budget table (4 utterances, 11 reference words)");
    println!();
    print!("{}", render_markdown(&table));
    println!();
    let texts: Vec<String> = fxs
        .iter()
        .map(|f| lattice_text(f).expect("write fixture lattice"))
        .collect();
    let items: Vec<(&str, &[String])> = texts
        .iter()
        .zip(fxs.iter())
        .map(|(t, f)| (t.as_str(), f.reference.as_slice()))
        .collect();
    let sweep = sweep_corpus(&items, &SWEEP_KS).expect("sweep fixtures");
    println!("fixture oracle sweep");
    println!();
    print!("{}", render_sweep_markdown(&sweep));
}
