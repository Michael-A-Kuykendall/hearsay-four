//! Demo input generator (bead `hearsay-four-4lsp--fabric-path-demo`):
//! arm-C bridge handoff for the demo fixtures. Writes, per fixture,
//! `<id>.lat.jsonl` (source lattice), `<id>.transcript.txt` and
//! `<id>.events.jsonl` (the bridge handoff `hearsay-transcribe
//! --fabric-input` reads). Budget 50: the A6 table shows the u1 revival at
//! every budget, and 50 also binds u3 (eviction path in the demo).
//!
//! Usage: `cargo run -p experiment --example fabric_demo_inputs -- <out-dir>`
//! (release for the demo script; debug is fine for a smoke run).
use experiment::arms::decode_demo_lattice;
use experiment::fixtures::{all_fixtures, lattice_text};

/// A6 budget rung for the demo (revival present; u3 evictions bind).
const DEMO_BUDGET: usize = 50;

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: fabric_demo_inputs <out-dir>");
    std::fs::create_dir_all(&out).expect("create out dir");
    for fx in all_fixtures() {
        let outcome = decode_demo_lattice(&fx.lattice, DEMO_BUDGET).expect("demo decode succeeds");
        let lattice = lattice_text(&fx).expect("write fixture lattice");
        std::fs::write(format!("{out}/{}.lat.jsonl", fx.id), &lattice).expect("write lattice");
        std::fs::write(
            format!("{out}/{}.transcript.txt", fx.id),
            format!("{}\n", outcome.transcript.join(" ")),
        )
        .expect("write transcript");
        std::fs::write(
            format!("{out}/{}.events.jsonl", fx.id),
            &outcome.events_jsonl,
        )
        .expect("write events");
        println!(
            "{}: {} words, {} commits, {} event lines, peak live/dormant {}/{}",
            fx.id,
            outcome.transcript.len(),
            outcome.commits.len(),
            outcome.events_jsonl.lines().count(),
            outcome.peak_live,
            outcome.peak_dormant,
        );
    }
}
