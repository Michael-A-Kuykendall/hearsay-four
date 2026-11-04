//! Synthetic dry-run driver: DEV-tune arm B, sweep TEST over budgets x {B, C}.
//!
//! Usage: sweep_synth --out <tsv> [--dev-n N] [--test-n N] [--seed S]
//! (defaults: dev 50, test 200, seed 20261103). Arm C reuses the DEV-tuned
//! arm-B params identically.

use experiment::sweep::{
    FeedSource, TuningGrid, load_experiment_feed, render_sweep_tsv, sweep_test, tune_arm_b,
};
use experiment::table::BUDGETS;
use std::path::PathBuf;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out: Option<PathBuf> = None;
    let mut dev_n: usize = 50;
    let mut test_n: usize = 200;
    let mut seed: u64 = 20261103;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => out = Some(PathBuf::from(take_value(&args, &mut i, "--out").unwrap())),
            "--dev-n" => {
                dev_n = take_value(&args, &mut i, "--dev-n")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--test-n" => {
                test_n = take_value(&args, &mut i, "--test-n")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--seed" => {
                seed = take_value(&args, &mut i, "--seed")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let out = out.expect("--out <tsv>");
    let feed = load_experiment_feed(&FeedSource::Synthetic {
        dev_n,
        test_n,
        seed,
    })
    .expect("synthetic feed");
    eprintln!(
        "dev {} test {} seed {seed}",
        feed.dev.len(),
        feed.test.len()
    );
    let grid = TuningGrid::default();
    let tuned = tune_arm_b(&feed.dev, &feed.lm, &BUDGETS, &grid).expect("DEV tuning");
    for (p, errs) in &tuned.scoreboard {
        match errs {
            Some(e) => println!(
                "TUNE w_acoustic={} theta_live={} delta={} dev_errors={e}",
                p.w_acoustic, p.theta_live, p.delta
            ),
            None => println!(
                "TUNE w_acoustic={} theta_live={} delta={} FAILED",
                p.w_acoustic, p.theta_live, p.delta
            ),
        }
    }
    let p = &tuned.params;
    println!(
        "TUNED w_acoustic={} w_sequence={} theta_live={} delta={} h={} h_max={} dev_errors={}/{} candidates={}",
        p.w_acoustic,
        p.w_sequence,
        p.theta_live,
        p.delta,
        p.h,
        p.h_max,
        tuned.dev_errors,
        tuned.dev_ref_len,
        tuned.candidates
    );
    let cells = sweep_test(&feed.test, &feed.lm, &tuned.params, &BUDGETS).expect("TEST sweep");
    for c in &cells {
        eprintln!(
            "CELL budget={} arm={:?} WER={:.4} peak_live={:?} peak_dormant={:?}",
            c.budget, c.arm, c.wer, c.peak_live, c.peak_dormant
        );
    }
    std::fs::write(&out, render_sweep_tsv(&cells)).expect("write TSV");
    println!("wrote {} ({} rows)", out.display(), cells.len());
}
