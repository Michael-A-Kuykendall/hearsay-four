//! J-horizon study: how the dormant successor window `j` moves budget
//! binding, evictions, and transcripts (spec gap: default `j=3` seals
//! tokens almost immediately, so budgets may never bind).
//!
//! usage:
//!   j_horizon --manifest <tsv> --lat-dir <dir> --lm <lm.json>
//!     --cell <armA.tsv> --ids <file> --theta <f> --out-tsv <tsv>
//!     [--wseq 6] [--budget 500] [--js 3,20,100]
//!
//! Each `(utt, j, arm)` cell calls [`hearsay::decode_lattice`] directly
//! with an explicit `j` (the bridge default is untouched). One TSV row
//! per cell; the per-J summary table goes to stdout.

use experiment::calibrate::{harvest_margins, percentile, scan_events, unsealed_windows};
use fabric::decision::DecisionParams;
use hearsay::{Arm as FabricArm, InterpolatedScorer};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn wer_errors(r: &[String], h: &[String]) -> (u32, u32) {
    let s = output::wer(r, h);
    (
        (s.substitutions + s.deletions + s.insertions) as u32,
        r.len() as u32,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut manifest: Option<PathBuf> = None;
    let mut lat_dir: Option<PathBuf> = None;
    let mut lm_path: Option<PathBuf> = None;
    let mut cell: Option<PathBuf> = None;
    let mut ids_path: Option<PathBuf> = None;
    let mut theta: Option<f64> = None;
    let mut wseq = 6.0f64;
    let mut budget = 500usize;
    let mut js = vec![3u32, 20, 100];
    let mut out_tsv: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--manifest" => {
                manifest = Some(PathBuf::from(
                    take_value(&args, &mut i, "--manifest").unwrap(),
                ))
            }
            "--lat-dir" => {
                lat_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--lat-dir").unwrap(),
                ))
            }
            "--lm" => lm_path = Some(PathBuf::from(take_value(&args, &mut i, "--lm").unwrap())),
            "--cell" => cell = Some(PathBuf::from(take_value(&args, &mut i, "--cell").unwrap())),
            "--ids" => ids_path = Some(PathBuf::from(take_value(&args, &mut i, "--ids").unwrap())),
            "--theta" => {
                theta = Some(
                    take_value(&args, &mut i, "--theta")
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
            "--wseq" => {
                wseq = take_value(&args, &mut i, "--wseq")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--budget" => {
                budget = take_value(&args, &mut i, "--budget")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--js" => {
                js = take_value(&args, &mut i, "--js")
                    .unwrap()
                    .split(',')
                    .map(|s| s.parse().unwrap())
                    .collect()
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
    let (manifest, lat_dir, lm_path, cell, ids_path, theta, out_tsv) = (
        manifest.expect("--manifest"),
        lat_dir.expect("--lat-dir"),
        lm_path.expect("--lm"),
        cell.expect("--cell"),
        ids_path.expect("--ids"),
        theta.expect("--theta"),
        out_tsv.expect("--out-tsv"),
    );
    let ids: Vec<String> = std::fs::read_to_string(&ids_path)
        .expect("read ids")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    let wanted: BTreeMap<&str, ()> = ids.iter().map(|s| (s.as_str(), ())).collect();
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in std::fs::read_to_string(&manifest)
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
        if wanted.contains_key(id) {
            refs.insert(
                id.to_string(),
                t.split_whitespace().map(str::to_string).collect(),
            );
        }
    }
    let mut arm_a: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in std::fs::read_to_string(&cell)
        .expect("read cell")
        .lines()
        .skip(1)
    {
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        if wanted.contains_key(c[0]) {
            arm_a.insert(
                c[0].to_string(),
                c.get(6)
                    .unwrap_or(&"")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
            );
        }
    }
    assert_eq!(refs.len(), ids.len(), "manifest missing study ids");
    assert_eq!(arm_a.len(), ids.len(), "cell.tsv missing study ids");

    let lm = lm::persist::load_interpolated(&lm_path).expect("load lm");
    let scorer = InterpolatedScorer::new(&lm);
    const HEADER: &str = "utt\tj\tarm\terrors\tref_len\tdiv_errors\tarmA_len\tpeak_live\tpeak_dormant\tpeak_total\tpruned\tevicted\tevict_budgets\trevived\tcommitted\tn_margins\tmargin_p50\twin_med\twin_max\telapsed_sec\thyp\n";
    // Resumable: rows append per utterance; completed (utt, j, arm) cells
    // in an existing output are skipped on restart.
    let mut done: BTreeSet<(String, u32, String)> = BTreeSet::new();
    if out_tsv.exists() {
        let prior = std::fs::read_to_string(&out_tsv).expect("read prior tsv");
        for line in prior.lines().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            done.insert((c[0].to_string(), c[1].parse().unwrap(), c[2].to_string()));
        }
        eprintln!("resume: {} cells already done", done.len());
    } else {
        std::fs::write(&out_tsv, HEADER).expect("write header");
    }
    let mut failures: Vec<String> = Vec::new();
    for id in &ids {
        let text = std::fs::read_to_string(lat_dir.join(format!("{id}.lat.jsonl")))
            .unwrap_or_else(|_| panic!("read lattice {id}"));
        let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
        let mut rows = String::new();
        for &j in &js {
            // Unsealed window is arm-independent (pure frame geometry).
            let wins = unsealed_windows(&lat, j);
            let wins_f: Vec<f64> = wins.iter().map(|w| *w as f64).collect();
            let (win_med, win_max) = (
                percentile(&wins_f, 50.0),
                wins.iter().max().copied().unwrap_or(0),
            );
            for (arm_s, farm, dormant) in [("B", FabricArm::B, 0), ("C", FabricArm::C, budget)] {
                if done.contains(&(id.clone(), j, arm_s.to_string())) {
                    continue;
                }
                let params = DecisionParams {
                    w_acoustic: 1.0,
                    w_sequence: wseq,
                    theta_live: theta,
                    live_budget: budget,
                    dormant_budget: dormant,
                    combined_budget: Some(budget),
                    h: 10,
                    h_max: 20,
                    delta: 0.0,
                    j,
                    arm: farm,
                    ..DecisionParams::default()
                };
                let t0 = std::time::Instant::now();
                let out = match hearsay::decode_lattice(&lat, &scorer, params) {
                    Ok(o) => o,
                    Err(e) => {
                        failures.push(format!("{id} j={j} arm={arm_s}: {e}"));
                        continue;
                    }
                };
                let elapsed = t0.elapsed().as_secs_f64();
                let (counts, _) = scan_events(&out.events_jsonl);
                let margins = harvest_margins(&out.events_jsonl);
                let eb: Vec<String> = counts
                    .evicted_by_budget
                    .iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect();
                let (errs, rl) = wer_errors(&refs[id], &out.transcript);
                let (div, al) = wer_errors(&arm_a[id], &out.transcript);
                rows.push_str(&format!(
                    "{id}\t{j}\t{arm_s}\t{errs}\t{rl}\t{div}\t{al}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.4}\t{win_med:.0}\t{win_max}\t{elapsed:.3}\t{}\n",
                    out.peak_live,
                    out.peak_dormant,
                    out.peak_total,
                    counts.pruned,
                    counts.evicted,
                    if eb.is_empty() { "-".to_string() } else { eb.join(",") },
                    counts.revived,
                    counts.committed,
                    margins.len(),
                    if margins.is_empty() { f64::NAN } else { percentile(&margins, 50.0) },
                    out.transcript.join(" "),
                ));
                done.insert((id.clone(), j, arm_s.to_string()));
            }
        }
        if !rows.is_empty() {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&out_tsv)
                .expect("open tsv")
                .write_all(rows.as_bytes())
                .expect("append rows");
        }
        eprintln!("{id} done");
    }
    // Per-J summary to stdout, rebuilt from the file (resume-safe).
    let tsv = std::fs::read_to_string(&out_tsv).expect("read tsv");
    let mut hyps: BTreeMap<(String, u32), (Vec<String>, Vec<String>)> = BTreeMap::new();
    for line in tsv.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        let hyp = c
            .get(21)
            .unwrap_or(&"")
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let e = hyps
            .entry((c[0].to_string(), c[1].parse().unwrap()))
            .or_insert((Vec::new(), Vec::new()));
        if c[2] == "B" {
            e.0 = hyp;
        } else {
            e.1 = hyp;
        }
    }
    println!(
        "j\tn_utts\twin_med_med\twin_max_max\tevicted_C\tpruned_B\tcapped_C\twerB\twerC\tdivC\tbc_diverge"
    );
    for &j in &js {
        let mut wmed: Vec<f64> = Vec::new();
        let mut wmax = 0usize;
        let (mut ev_c, mut pr_b, mut capped) = (0usize, 0usize, 0usize);
        let (mut eb, mut rb, mut ec, mut rc) = (0u32, 0u32, 0u32, 0u32);
        let (mut dc, mut ac) = (0u32, 0u32);
        let mut seen: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
        for line in tsv.lines().skip(1) {
            let c: Vec<&str> = line.split('\t').collect();
            if c[1].parse::<u32>().unwrap() != j {
                continue;
            }
            seen.entry(c[0])
                .or_insert((c[17].parse().unwrap(), c[18].parse().unwrap()));
            match c[2] {
                "B" => {
                    eb += c[3].parse::<u32>().unwrap();
                    rb += c[4].parse::<u32>().unwrap();
                    pr_b += c[10].parse::<usize>().unwrap();
                }
                "C" => {
                    ec += c[3].parse::<u32>().unwrap();
                    rc += c[4].parse::<u32>().unwrap();
                    dc += c[5].parse::<u32>().unwrap();
                    ac += c[6].parse::<u32>().unwrap();
                    ev_c += c[11].parse::<usize>().unwrap();
                    if c[9].parse::<usize>().unwrap() >= budget {
                        capped += 1;
                    }
                }
                _ => unreachable!(),
            }
        }
        for (m, x) in seen.values() {
            wmed.push(*m);
            wmax = wmax.max(*x);
        }
        let diverge = hyps
            .iter()
            .filter(|((_, jj), (b, c))| *jj == j && b != c)
            .count();
        println!(
            "{j}\t{}\t{:.0}\t{wmax}\t{ev_c}\t{pr_b}\t{capped}\t{:.4}\t{:.4}\t{:.4}\t{diverge}",
            seen.len(),
            if wmed.is_empty() {
                f64::NAN
            } else {
                percentile(&wmed, 50.0)
            },
            eb as f64 / rb.max(1) as f64,
            ec as f64 / rc.max(1) as f64,
            dc as f64 / ac.max(1) as f64,
        );
    }
    if !failures.is_empty() {
        eprintln!("FAILURES ({}):", failures.len());
        for f in &failures {
            eprintln!("  {f}");
        }
        std::process::exit(1);
    }
}
