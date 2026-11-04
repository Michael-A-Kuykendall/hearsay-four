//! sweep_collect: assemble queue task files into tuning/sweep results.
//!
//! The queue (`cell_decode` shards under xargs) writes one JSON per
//! (utt, budget, arm) task; this binary scores them exactly once through
//! the SWEEP scorer (`score_corpus`, `delta_ci`, `count_late_rescues`,
//! `assemble_cell` — the same fns `sweep_test` uses).
//!
//! usage:
//!   sweep_collect --mode tune --manifest <tsv> --params-file <grid>
//!     --task-dir <tune_cells/> --budgets 50,100,250,500 --out <frozen.json>
//!   sweep_collect --mode sweep --manifest <tsv> --cell <cell.tsv>
//!     --task-dir <sweep_cells/> --budgets 50,100,250,500 --out-tsv <tsv>
//!     [--per-utt <tsv>]
//!
//! Tune mode: per-candidate total DEV errors summed over the tuning
//! budgets (arm B); lowest wins, ties to the earliest grid candidate
//! (matching `tune_arm_b`). A candidate with any missing/failed task
//! scores `None` and is skipped; all-failed is `Err`. Missing task files
//! otherwise abort loudly (re-run the queue to resume).

use experiment::arms::Arm;
use experiment::scoring::{delta_ci, per_utterance_errors, score_corpus};
use experiment::sweep::{
    ArmABaseline, CellRun, assemble_arm_a_cell, assemble_cell, count_late_rescues, render_sweep_tsv,
};
use std::path::{Path, PathBuf};

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn read_manifest(path: &Path) -> (Vec<String>, Vec<Vec<String>>) {
    let text = std::fs::read_to_string(path).expect("read manifest");
    let mut ids = Vec::new();
    let mut refs = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(id), Some(_), Some(_), Some(t)) => {
                ids.push(id.to_string());
                refs.push(t.split_whitespace().map(str::to_string).collect());
            }
            _ => panic!("manifest line {} malformed", lineno + 1),
        }
    }
    assert!(!ids.is_empty(), "empty manifest {}", path.display());
    (ids, refs)
}

fn read_cell(path: &Path, ids: &[String]) -> Vec<ArmABaseline> {
    let text = std::fs::read_to_string(path).expect("read cell.tsv");
    let mut out = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        if lineno == 0 {
            assert!(line.starts_with("utt\terrors"), "cell header: {line}");
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 7, "cell line {} has 7 cols", lineno + 1);
        assert_eq!(cols[0], ids[out.len()], "cell order matches manifest");
        out.push(ArmABaseline {
            hyp: cols[6].split_whitespace().map(str::to_string).collect(),
            audio_sec: cols[4].parse().unwrap(),
            decode_sec: cols[5].parse().unwrap(),
        });
    }
    assert_eq!(out.len(), ids.len(), "cell covers the manifest");
    out
}

fn read_task(dir: &Path, utt: &str, budget: usize, arm: &str) -> Option<CellRun> {
    let p = dir.join(format!("{utt}.{budget}.{arm}.json"));
    if !p.exists() {
        return None;
    }
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).expect("read task")).expect("parse task");
    let str_list = |k: &str| {
        v[k].as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    Some(CellRun {
        hyp: str_list("hyp"),
        pruned: str_list("pruned"),
        evicted: str_list("evicted"),
        peak_live: v["peak_live"].as_u64().unwrap() as usize,
        peak_dormant: v["peak_dormant"].as_u64().unwrap() as usize,
        elapsed: v["elapsed_sec"].as_f64().unwrap(),
        span: v["span_sec"].as_f64().unwrap(),
    })
}

fn parse_budgets(s: &str) -> Vec<usize> {
    s.split(',').map(|b| b.parse().unwrap()).collect()
}

fn run_tune(manifest: &Path, params_file: &Path, task_dir: &Path, budgets: &[usize], out: &Path) {
    let (ids, refs) = read_manifest(manifest);
    let grid_text = std::fs::read_to_string(params_file).expect("read params-file");
    let mut cands: Vec<(usize, Vec<f64>)> = Vec::new();
    for line in grid_text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 7, "params-file line wants 7 fields: {line}");
        cands.push((
            f[0].parse().unwrap(),
            f[1..].iter().map(|x| x.parse().unwrap()).collect(),
        ));
    }
    assert!(!cands.is_empty(), "empty params-file");
    let ref_len: u32 = refs.iter().map(|r| r.len() as u32).sum::<u32>() * budgets.len() as u32;
    let mut board: Vec<(usize, Vec<f64>, Option<u32>)> = Vec::new();
    for (k, p) in &cands {
        let dir = task_dir.join(format!("cand{k}"));
        let mut errs = 0u32;
        let mut failed = false;
        'utt: for (id, r) in ids.iter().zip(refs.iter()) {
            for &b in budgets {
                match read_task(&dir, id, b, "B") {
                    Some(run) => {
                        let s = output::wer(r, &run.hyp);
                        errs += (s.substitutions + s.deletions + s.insertions) as u32;
                    }
                    None => {
                        failed = true;
                        break 'utt;
                    }
                }
            }
        }
        board.push((*k, p.clone(), if failed { None } else { Some(errs) }));
    }
    for (k, p, e) in &board {
        match e {
            Some(v) => println!(
                "TUNE cand={k} w_acoustic={} w_sequence={} theta_live={} delta={} h={} h_max={} dev_errors={v}",
                p[0], p[1], p[2], p[3], p[4], p[5]
            ),
            None => println!("TUNE cand={k} FAILED (missing/failed tasks)"),
        }
    }
    let mut best: Option<(usize, u32)> = None;
    for (i, (_, _, e)) in board.iter().enumerate() {
        if let Some(v) = e
            && best.is_none_or(|(_, b)| *v < b)
        {
            best = Some((i, *v));
        }
    }
    let (bi, be) = best.expect("tune: every candidate failed");
    let (bk, bp, _) = &board[bi];
    println!(
        "TUNED cand={bk} w_acoustic={} w_sequence={} theta_live={} delta={} h={} h_max={} dev_errors={be}/{ref_len} candidates={}",
        bp[0],
        bp[1],
        bp[2],
        bp[3],
        bp[4],
        bp[5],
        board.len()
    );
    let scoreboard: Vec<serde_json::Value> = board
        .iter()
        .map(|(k, p, e)| {
            serde_json::json!({
                "cand": k,
                "w_acoustic": p[0], "w_sequence": p[1], "theta_live": p[2],
                "delta": p[3], "h": p[4] as u32, "h_max": p[5] as u32,
                "dev_errors": e,
            })
        })
        .collect();
    let frozen = serde_json::json!({
        "params": {
            "budget": 0,
            "w_acoustic": bp[0], "w_sequence": bp[1], "theta_live": bp[2],
            "delta": bp[3], "h": bp[4] as u32, "h_max": bp[5] as u32,
        },
        "winner_cand": bk,
        "dev_errors": be,
        "dev_ref_len": ref_len,
        "candidates": board.len(),
        "grid": "theta_live x w_sequence x delta coarse (w_acoustic pinned 1.0)",
        "budgets": budgets,
        "scoreboard": scoreboard,
    });
    std::fs::write(out, serde_json::to_string_pretty(&frozen).unwrap()).expect("write frozen");
    println!("wrote {}", out.display());
}

fn run_sweep(
    manifest: &Path,
    cell: &Path,
    task_dir: &Path,
    budgets: &[usize],
    out_tsv: &Path,
    per_utt: &Option<PathBuf>,
) {
    let (ids, refs) = read_manifest(manifest);
    let base = read_cell(cell, &ids);
    let denoms: Vec<u32> = refs.iter().map(|r| r.len() as u32).collect();
    let mut cells = Vec::new();
    let mut detail = String::from(
        "id\tbudget\tref_len\ta_errs\tb_errs\tc_errs\tb_peak_live\tb_peak_dorm\tc_peak_live\tc_peak_dorm\tb_hyp\tc_hyp\n",
    );
    for &budget in budgets {
        let mut b_runs = Vec::new();
        let mut c_runs = Vec::new();
        for id in &ids {
            b_runs.push(
                read_task(task_dir, id, budget, "B").unwrap_or_else(|| {
                    panic!("missing task {id}.{budget}.B.json (re-run the queue)")
                }),
            );
            c_runs.push(
                read_task(task_dir, id, budget, "C").unwrap_or_else(|| {
                    panic!("missing task {id}.{budget}.C.json (re-run the queue)")
                }),
            );
        }
        let b_hyps: Vec<Vec<String>> = b_runs.iter().map(|r| r.hyp.clone()).collect();
        let c_hyps: Vec<Vec<String>> = c_runs.iter().map(|r| r.hyp.clone()).collect();
        let b_tot = score_corpus(&refs, &b_hyps);
        let c_tot = score_corpus(&refs, &c_hyps);
        let (b_errs, _) = per_utterance_errors(&refs, &b_hyps);
        let (c_errs, _) = per_utterance_errors(&refs, &c_hyps);
        let (a_errs, _) = per_utterance_errors(
            &refs,
            &base.iter().map(|b| b.hyp.clone()).collect::<Vec<_>>(),
        );
        let rescues: usize = c_runs
            .iter()
            .zip(b_runs.iter())
            .map(|(c, b)| count_late_rescues(&c.hyp, &b.pruned, &b.evicted))
            .sum();
        let ci = delta_ci(&b_errs, &c_errs, &denoms);
        eprintln!(
            "CELL budget={budget} B WER={:.4} C WER={:.4} rescues={rescues}",
            b_tot.wer(),
            c_tot.wer()
        );
        for (k, id) in ids.iter().enumerate() {
            detail.push_str(&format!(
                "{id}\t{budget}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                refs[k].len(),
                a_errs[k],
                b_errs[k],
                c_errs[k],
                b_runs[k].peak_live,
                b_runs[k].peak_dormant,
                c_runs[k].peak_live,
                c_runs[k].peak_dormant,
                b_runs[k].hyp.join(" "),
                c_runs[k].hyp.join(" "),
            ));
        }
        cells.push(assemble_arm_a_cell(budget, &refs, &base));
        cells.push(assemble_cell(budget, Arm::B, &b_tot, &b_runs, None, None));
        cells.push(assemble_cell(
            budget,
            Arm::C,
            &c_tot,
            &c_runs,
            Some(rescues),
            Some(ci),
        ));
    }
    std::fs::write(out_tsv, render_sweep_tsv(&cells)).expect("write TSV");
    println!("wrote {} ({} rows)", out_tsv.display(), cells.len());
    if let Some(p) = per_utt {
        std::fs::write(p, &detail).expect("write per-utt");
        println!("wrote {}", p.display());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode: Option<String> = None;
    let mut manifest: Option<PathBuf> = None;
    let mut params_file: Option<PathBuf> = None;
    let mut cell: Option<PathBuf> = None;
    let mut task_dir: Option<PathBuf> = None;
    let mut budgets: Vec<usize> = vec![50, 100, 250, 500];
    let mut out: Option<PathBuf> = None;
    let mut out_tsv: Option<PathBuf> = None;
    let mut per_utt: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => mode = Some(take_value(&args, &mut i, "--mode").unwrap()),
            "--manifest" => {
                manifest = Some(PathBuf::from(
                    take_value(&args, &mut i, "--manifest").unwrap(),
                ))
            }
            "--params-file" => {
                params_file = Some(PathBuf::from(
                    take_value(&args, &mut i, "--params-file").unwrap(),
                ))
            }
            "--cell" => cell = Some(PathBuf::from(take_value(&args, &mut i, "--cell").unwrap())),
            "--task-dir" => {
                task_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--task-dir").unwrap(),
                ))
            }
            "--budgets" => {
                budgets = parse_budgets(&take_value(&args, &mut i, "--budgets").unwrap())
            }
            "--out" => out = Some(PathBuf::from(take_value(&args, &mut i, "--out").unwrap())),
            "--out-tsv" => {
                out_tsv = Some(PathBuf::from(
                    take_value(&args, &mut i, "--out-tsv").unwrap(),
                ))
            }
            "--per-utt" => {
                per_utt = Some(PathBuf::from(
                    take_value(&args, &mut i, "--per-utt").unwrap(),
                ))
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    match mode.as_deref() {
        Some("tune") => run_tune(
            &manifest.expect("--manifest"),
            &params_file.expect("--params-file"),
            &task_dir.expect("--task-dir"),
            &budgets,
            &out.expect("--out"),
        ),
        Some("sweep") => run_sweep(
            &manifest.expect("--manifest"),
            &cell.expect("--cell"),
            &task_dir.expect("--task-dir"),
            &budgets,
            &out_tsv.expect("--out-tsv"),
            &per_utt,
        ),
        other => panic!("--mode tune|sweep, got {other:?}"),
    }
}
