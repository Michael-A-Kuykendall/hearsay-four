//! Gate collector: SANITY / SENSITIVITY / DORMANCY verdicts over
//! `cell_decode` task JSONs.
//!
//! usage:
//!   gate_collect --mode sanity --manifest <tsv> --cell <armA.tsv>
//!     --task-dir <dir> --budget <n> --arm <B|C> [--bound 0.10]
//!     [--lat-dir <dir>] --out-tsv <tsv>
//!   gate_collect --mode sensitivity --manifest <tsv> --task-dir-a <dir>
//!     --task-dir-b <dir> --name-a <s> --name-b <s> [--min-changed 10]
//!     --out-tsv <tsv>
//!   gate_collect --mode dormancy --task-dir <dir> --budgets 50,100,250,500
//!     --out-tsv <tsv>
//!
//! Prints the verdict line (`GATE <name> PASS|FAIL ...`) to stdout and
//! exits nonzero on FAIL so drivers stop loudly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn read_manifest(path: &Path) -> BTreeMap<String, Vec<String>> {
    let mut refs = BTreeMap::new();
    for line in std::fs::read_to_string(path)
        .expect("read manifest")
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let (id, _, _, t) = (
            parts.next().unwrap(),
            parts.next(),
            parts.next(),
            parts.next().unwrap(),
        );
        refs.insert(
            id.to_string(),
            t.split_whitespace().map(str::to_string).collect(),
        );
    }
    refs
}

fn read_cell(path: &Path) -> BTreeMap<String, (Vec<String>, u32)> {
    let mut hyps = BTreeMap::new();
    for line in std::fs::read_to_string(path)
        .expect("read cell")
        .lines()
        .skip(1)
    {
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        hyps.insert(
            c[0].to_string(),
            (
                c.get(6)
                    .unwrap_or(&"")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                c[1].parse().unwrap(),
            ),
        );
    }
    hyps
}

#[derive(Debug, Clone)]
struct Task {
    hyp: Vec<String>,
    peak_dormant: usize,
    peak_total: usize,
    n_pruned: usize,
    n_evicted: usize,
}

fn read_task(path: &Path) -> Task {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read task"))
            .expect("parse task");
    Task {
        hyp: v["hyp"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w.as_str().unwrap().to_string())
            .collect(),
        peak_dormant: v["peak_dormant"].as_u64().unwrap() as usize,
        peak_total: v["peak_total"].as_u64().unwrap() as usize,
        n_pruned: v["pruned"].as_array().unwrap().len(),
        n_evicted: v["evicted"].as_array().unwrap().len(),
    }
}

fn wer_errors(r: &[String], h: &[String]) -> u32 {
    let s = output::wer(r, h);
    (s.substitutions + s.deletions + s.insertions) as u32
}

#[allow(clippy::too_many_arguments)]
fn run_sanity(
    manifest: &Path,
    cell: &Path,
    task_dir: &Path,
    budget: usize,
    arm: &str,
    bound: f64,
    lat_dir: Option<&Path>,
    out_tsv: &Path,
) {
    let refs = read_manifest(manifest);
    let arm_a = read_cell(cell);
    let mut tsv = String::from("utt\tref_len\terrors_arm\terrors_A\terrors_lat1\n");
    let (mut e_arm, mut e_a, mut e_lat1, mut nref) = (0u32, 0u32, 0u32, 0u32);
    let mut drift = 0usize;
    for (id, r) in &refs {
        let t = read_task(&task_dir.join(format!("{id}.{budget}.{arm}.json")));
        let ea = wer_errors(r, &t.hyp);
        let (ha, recorded) = &arm_a[id];
        let eb = wer_errors(r, ha);
        if eb != *recorded {
            drift += 1;
        }
        let el = match lat_dir {
            None => 0,
            Some(dir) => {
                let text = std::fs::read_to_string(dir.join(format!("{id}.lat.jsonl")))
                    .unwrap_or_else(|_| panic!("read lattice {id}"));
                let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
                wer_errors(r, &experiment::scoring::one_best_path(&lat))
            }
        };
        tsv.push_str(&format!("{id}\t{}\t{ea}\t{eb}\t{el}\n", r.len()));
        e_arm += ea;
        e_a += eb;
        e_lat1 += el;
        nref += r.len() as u32;
    }
    std::fs::write(out_tsv, &tsv).expect("write tsv");
    let (wa, wb) = (e_arm as f64 / nref as f64, e_a as f64 / nref as f64);
    let d = (wa - wb).abs();
    let verdict = if d <= bound { "PASS" } else { "FAIL" };
    println!(
        "GATE SANITY {verdict}: arm{arm} WER={wa:.4} ({e_arm}/{nref}) armA WER={wb:.4} ({e_a}/{nref}) |d|={d:.4} bound={bound} cell_drift={drift}"
    );
    if lat_dir.is_some() {
        let wl = e_lat1 as f64 / nref as f64;
        println!(
            "GATE SANITY diagnostic: lattice-1best WER={wl:.4} ({e_lat1}/{nref}) |arm-lat1|={:.4}",
            (wa - wl).abs()
        );
    }
    if verdict == "FAIL" {
        std::process::exit(1);
    }
}

fn collect_hyps(dir: &Path) -> BTreeMap<String, Vec<String>> {
    let mut m = BTreeMap::new();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read task dir")
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    for n in names {
        let t = read_task(&dir.join(&n));
        m.insert(n, t.hyp);
    }
    m
}

fn run_sensitivity(
    task_a: &Path,
    task_b: &Path,
    name_a: &str,
    name_b: &str,
    min_changed: usize,
    out_tsv: &Path,
) {
    let (a, b) = (collect_hyps(task_a), collect_hyps(task_b));
    assert_eq!(a.len(), b.len(), "task-set size mismatch");
    let mut tsv = String::from("task\tchanged\n");
    let mut changed = 0usize;
    for (k, ha) in &a {
        let hb = b
            .get(k)
            .unwrap_or_else(|| panic!("{k} missing in {name_b}"));
        let c = ha != hb;
        changed += usize::from(c);
        tsv.push_str(&format!("{k}\t{c}\n"));
    }
    std::fs::write(out_tsv, &tsv).expect("write tsv");
    let verdict = if changed >= min_changed {
        "PASS"
    } else {
        "FAIL"
    };
    println!(
        "GATE SENSITIVITY {verdict}: {changed}/{} transcripts differ ({name_a} vs {name_b}) min_changed={min_changed}",
        a.len()
    );
    if verdict == "FAIL" {
        std::process::exit(1);
    }
}

fn run_dormancy(task_dir: &Path, budgets: &[usize], out_tsv: &Path) {
    let mut tsv = String::from(
        "budget\tarm\tn_utts\tn_peak_dormant_gt0\ttotal_prunes\tn_utts_prunes_gt0\ttotal_evicted\tpeak_total_max\n",
    );
    let mut fail = false;
    for &bd in budgets {
        for arm in ["B", "C"] {
            let mut n = 0usize;
            let (mut nd, mut tp, mut np, mut te, mut pmax) = (0, 0, 0, 0, 0);
            let mut names: Vec<String> = std::fs::read_dir(task_dir)
                .expect("read task dir")
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .filter(|x| x.ends_with(&format!(".{bd}.{arm}.json")))
                .collect();
            names.sort();
            for x in names {
                let t = read_task(&task_dir.join(&x));
                n += 1;
                nd += usize::from(t.peak_dormant > 0);
                tp += t.n_pruned;
                np += usize::from(t.n_pruned > 0);
                te += t.n_evicted;
                pmax = pmax.max(t.peak_total);
            }
            tsv.push_str(&format!(
                "{bd}\t{arm}\t{n}\t{nd}\t{tp}\t{np}\t{te}\t{pmax}\n"
            ));
            if n == 0 {
                fail = true;
            }
            if arm == "C" && nd == 0 {
                fail = true;
            }
            if arm == "B" && tp == 0 {
                fail = true;
            }
        }
    }
    std::fs::write(out_tsv, &tsv).expect("write tsv");
    print!("{tsv}");
    let verdict = if fail { "FAIL" } else { "PASS" };
    println!("GATE DORMANCY {verdict}: every budget has C peak_dormant>0 and B prunes>0");
    if fail {
        std::process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode: Option<String> = None;
    let mut manifest: Option<PathBuf> = None;
    let mut cell: Option<PathBuf> = None;
    let mut lat_dir: Option<PathBuf> = None;
    let mut task_dir: Option<PathBuf> = None;
    let mut task_a: Option<PathBuf> = None;
    let mut task_b: Option<PathBuf> = None;
    let mut name_a = String::from("A");
    let mut name_b = String::from("B");
    let mut budget: Option<usize> = None;
    let mut arm: Option<String> = None;
    let mut budgets: Vec<usize> = vec![50, 100, 250, 500];
    let mut bound = 0.10f64;
    let mut min_changed = 10usize;
    let mut out_tsv: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => mode = Some(take_value(&args, &mut i, "--mode").unwrap()),
            "--manifest" => {
                manifest = Some(PathBuf::from(
                    take_value(&args, &mut i, "--manifest").unwrap(),
                ))
            }
            "--cell" => cell = Some(PathBuf::from(take_value(&args, &mut i, "--cell").unwrap())),
            "--lat-dir" => {
                lat_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--lat-dir").unwrap(),
                ))
            }
            "--task-dir" => {
                task_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--task-dir").unwrap(),
                ))
            }
            "--task-dir-a" => {
                task_a = Some(PathBuf::from(
                    take_value(&args, &mut i, "--task-dir-a").unwrap(),
                ))
            }
            "--task-dir-b" => {
                task_b = Some(PathBuf::from(
                    take_value(&args, &mut i, "--task-dir-b").unwrap(),
                ))
            }
            "--name-a" => name_a = take_value(&args, &mut i, "--name-a").unwrap(),
            "--name-b" => name_b = take_value(&args, &mut i, "--name-b").unwrap(),
            "--budget" => {
                budget = Some(
                    take_value(&args, &mut i, "--budget")
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
            "--arm" => arm = Some(take_value(&args, &mut i, "--arm").unwrap()),
            "--budgets" => {
                budgets = take_value(&args, &mut i, "--budgets")
                    .unwrap()
                    .split(',')
                    .map(|s| s.parse().unwrap())
                    .collect()
            }
            "--bound" => {
                bound = take_value(&args, &mut i, "--bound")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--min-changed" => {
                min_changed = take_value(&args, &mut i, "--min-changed")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--out-tsv" => {
                out_tsv = Some(PathBuf::from(
                    take_value(&args, &mut i, "--out-tsv").unwrap(),
                ))
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let mode = mode.expect("--mode");
    let out = out_tsv.expect("--out-tsv");
    match mode.as_str() {
        "sanity" => run_sanity(
            &manifest.expect("--manifest"),
            &cell.expect("--cell"),
            &task_dir.expect("--task-dir"),
            budget.expect("--budget"),
            &arm.expect("--arm"),
            bound,
            lat_dir.as_deref(),
            &out,
        ),
        "sensitivity" => run_sensitivity(
            &task_a.expect("--task-dir-a"),
            &task_b.expect("--task-dir-b"),
            &name_a,
            &name_b,
            min_changed,
            &out,
        ),
        "dormancy" => run_dormancy(&task_dir.expect("--task-dir"), &budgets, &out),
        other => panic!("unknown mode {other:?}"),
    }
}
