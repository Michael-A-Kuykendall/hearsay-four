//! tune_grid: print the DEV tuning candidate list (bead hearsay-four-7i2o).
//!
//! One source of truth for the queue run-list: prints
//! `TuningGrid::real_coarse()` candidates as
//! `cand_idx w_ac w_seq theta delta h hmax` lines (grid order; ties in
//! `tune_freeze` resolve to the earliest line, matching `tune_arm_b`).
//!
//! usage: tune_grid [--coarse|--synthetic-default]

use experiment::sweep::TuningGrid;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let grid = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["--coarse"] => TuningGrid::real_coarse(),
        ["--synthetic-default"] => TuningGrid::default(),
        other => panic!("unknown arguments {other:?}"),
    };
    for (k, p) in grid.candidates(0).iter().enumerate() {
        println!(
            "{k} {} {} {} {} {} {}",
            p.w_acoustic, p.w_sequence, p.theta_live, p.delta, p.h, p.h_max
        );
    }
}
