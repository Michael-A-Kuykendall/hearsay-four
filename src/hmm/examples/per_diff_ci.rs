//! Paired bootstrap verdict between two --per-utt files.
//!
//! usage: per_diff_ci <a.perutt> <b.perutt>
//! Prints mean paired difference (a-b), 95% CI, and which side wins.
//! Exit 0 always; the verdict line is the machine-readable result.
use std::collections::HashMap;

use hmm::paired_ci;

fn load(path: &str) -> HashMap<String, f64> {
    let text = std::fs::read_to_string(path).expect("read per-utt");
    let mut m = HashMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(2, '\t');
        let id = cols.next().unwrap_or("").to_string();
        let v: f64 = cols.next().unwrap_or("nan").parse().expect("parse per");
        assert!(!id.is_empty() && v.is_finite(), "bad per-utt row");
        m.insert(id, v);
    }
    m
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 3,
        "usage: per_diff_ci <a.perutt> <b.perutt>"
    );
    let a = load(&args[1]);
    let b = load(&args[2]);
    let mut diffs = Vec::new();
    let mut n_only_a = 0usize;
    let mut n_only_b = 0usize;
    for (id, va) in &a {
        match b.get(id) {
            Some(vb) => diffs.push(va - vb),
            None => n_only_a += 1,
        }
    }
    for id in b.keys() {
        if !a.contains_key(id) {
            n_only_b += 1;
        }
    }
    assert!(!diffs.is_empty(), "no paired utterances");
    let (mean, lo, hi) = paired_ci(&diffs);
    println!("paired_n={}", diffs.len());
    println!("only_a={n_only_a} only_b={n_only_b}");
    println!("mean_diff_a_minus_b={mean:.6}");
    println!("ci95_lo={lo:.6} ci95_hi={hi:.6}");
    let verdict = if hi < 0.0 {
        "A_WINS"
    } else if lo > 0.0 {
        "B_WINS"
    } else {
        "TIE"
    };
    println!("verdict={verdict}");
}
