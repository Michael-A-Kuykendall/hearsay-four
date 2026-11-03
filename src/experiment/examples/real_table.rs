//! real_table: arms A/B/C over saved real lattices at all budgets.
//!
//! Bead `hearsay-four-ic1w--fabric-experiment-arms` (wave 2): the real-data
//! counterpart of `fixture_table`. Arm A hypotheses come from a `wer_devclean`
//! cell.tsv (the 1-best baseline, scored with its own decode seconds for
//! RTF); arms B/C run through `experiment::arms::run_fabric_arm` with the
//! real interpolated LM; totals/CI/rescues reuse the fixture-table
//! definitions. Also reports 1-best agreement between the cell hyps and
//! `one_best_path` over the saved kbest lattices (same search, so any
//! mismatch is a pipeline defect).
//!
//! usage: real_table --manifest <tsv> --lat-dir <dir> --lm <lm.json>
//!   --cell <cell.tsv> --out <report.md> --per-utt <tsv>
//!   --params <w_ac,w_seq,theta,delta,h,hmax>

use experiment::arms::{Arm, ArmParams, run_fabric_arm};
use experiment::scoring::{delta_ci, one_best_path, per_utterance_errors, score_corpus};
use experiment::table::{ArmCell, BUDGETS, BudgetRow, BudgetTable, render_markdown};
use hearsay::{InterpolatedScorer, SequenceScorer};
use std::path::PathBuf;
use std::time::Instant;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut manifest: Option<PathBuf> = None;
    let mut lat_dir: Option<PathBuf> = None;
    let mut lm_path: Option<PathBuf> = None;
    let mut cell: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut per_utt: Option<PathBuf> = None;
    let mut params: Option<String> = None;
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
            "--out" => out = Some(PathBuf::from(take_value(&args, &mut i, "--out").unwrap())),
            "--per-utt" => {
                per_utt = Some(PathBuf::from(
                    take_value(&args, &mut i, "--per-utt").unwrap(),
                ))
            }
            "--params" => params = Some(take_value(&args, &mut i, "--params").unwrap()),
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let (manifest, lat_dir, lm_path, cell, out, per_utt) = (
        manifest.expect("--manifest"),
        lat_dir.expect("--lat-dir"),
        lm_path.expect("--lm"),
        cell.expect("--cell"),
        out.expect("--out"),
        per_utt.expect("--per-utt"),
    );
    let pv: Vec<f64> = params
        .expect("--params")
        .split(',')
        .map(|s| s.parse::<f64>().unwrap())
        .collect();
    assert_eq!(pv.len(), 6, "--params wants w_ac,w_seq,theta,delta,h,hmax");
    let (w_ac, w_seq, theta, delta, h, hmax) =
        (pv[0], pv[1], pv[2], pv[3], pv[4] as u32, pv[5] as u32);

    // Manifest refs (same extraction as wer_devclean).
    let text = std::fs::read_to_string(&manifest).expect("read manifest");
    let mut ids: Vec<String> = Vec::new();
    let mut refs: Vec<Vec<String>> = Vec::new();
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
    // Arm A hyps + timing from the wer_devclean cell.tsv.
    let cell_text = std::fs::read_to_string(&cell).expect("read cell.tsv");
    let mut a_hyps: Vec<Vec<String>> = Vec::new();
    let mut a_audio = 0.0f64;
    let mut a_decode = 0.0f64;
    for (lineno, line) in cell_text.lines().enumerate() {
        if lineno == 0 {
            assert!(line.starts_with("utt\terrors"), "cell.tsv header: {line}");
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 7, "cell.tsv line {} has 7 cols", lineno + 1);
        assert_eq!(cols[0], ids[a_hyps.len()], "cell order matches manifest");
        a_audio += cols[4].parse::<f64>().unwrap();
        a_decode += cols[5].parse::<f64>().unwrap();
        a_hyps.push(cols[6].split_whitespace().map(str::to_string).collect());
    }
    assert_eq!(a_hyps.len(), ids.len(), "cell covers the manifest");
    eprintln!(
        "utterances {} arm-A audio_hours {:.2} decode_hours {:.2} RTF {:.3}",
        ids.len(),
        a_audio / 3600.0,
        a_decode / 3600.0,
        a_decode / a_audio.max(1e-9)
    );

    // Lattices.
    let t0 = Instant::now();
    let mut lats: Vec<lattice::Lattice> = Vec::with_capacity(ids.len());
    for id in &ids {
        let p = lat_dir.join(format!("{id}.lat.jsonl"));
        let lt = std::fs::read_to_string(&p).expect("read lattice");
        lats.push(lattice::read_lattice(&lt).expect("parse lattice"));
    }
    eprintln!("lattices load {:.1}s", t0.elapsed().as_secs_f64());

    // 1-best agreement: cell hyp vs one_best_path over the kbest lattice.
    let mut agree = 0usize;
    for (lat, ah) in lats.iter().zip(a_hyps.iter()) {
        if &one_best_path(lat) == ah {
            agree += 1;
        }
    }
    eprintln!("1-best agreement cell-vs-lattice {agree}/{}", ids.len());

    // Real LM scorer (ClosureScorer maps utterance-start to "<s>").
    let t0 = Instant::now();
    let lm = lm::persist::load_interpolated(&lm_path).expect("load lm");
    eprintln!("lm load {:.1}s", t0.elapsed().as_secs_f64());
    let scorer = InterpolatedScorer::new(&lm);
    let lm_fn = |prev: &str, word: &str| {
        scorer.sequence_llr(if prev == "<s>" { None } else { Some(prev) }, word)
    };

    let a_tot = score_corpus(&refs, &a_hyps);
    let (a_errs, denoms) = per_utterance_errors(&refs, &a_hyps);
    let a_cell = ArmCell {
        wer: a_tot.wer(),
        subs: a_tot.subs,
        dels: a_tot.dels,
        inss: a_tot.inss,
        ref_len: a_tot.ref_len,
        rtf: a_decode / a_audio.max(1e-9),
        peak_live: None,
        peak_dormant: None,
        peak_total: None,
        hyps: a_hyps.clone(),
    };
    let mut detail = String::from(
        "id\tbudget\tref_len\ta_errs\tb_errs\tc_errs\tb_peak_live\tb_peak_dorm\tc_peak_live\tc_peak_dorm\tb_hyp\tc_hyp\n",
    );
    let mut rows = Vec::new();
    for &budget in &BUDGETS {
        let params = ArmParams {
            budget,
            w_acoustic: w_ac,
            w_sequence: w_seq,
            theta_live: theta,
            delta,
            h,
            h_max: hmax,
        };
        let t0 = Instant::now();
        let mut b_hyps = Vec::new();
        let mut c_hyps = Vec::new();
        let (mut bl, mut bd, mut bt, mut cl, mut cd, mut ct) =
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        let (mut be, mut bs, mut ce, mut cs) = (0.0, 0.0, 0.0, 0.0);
        let mut rescues = 0usize;
        for (((id, lat), r), ae) in ids
            .iter()
            .zip(lats.iter())
            .zip(refs.iter())
            .zip(a_errs.iter())
        {
            let bo = run_fabric_arm(lat, Arm::B, &params, &lm_fn).expect("arm B run");
            let co = run_fabric_arm(lat, Arm::C, &params, &lm_fn).expect("arm C run");
            bl = bl.max(bo.peak_live);
            bd = bd.max(bo.peak_dormant);
            bt = bt.max(bo.peak_total);
            cl = cl.max(co.peak_live);
            cd = cd.max(co.peak_dormant);
            ct = ct.max(co.peak_total);
            be += bo.elapsed_sec;
            bs += bo.span_sec;
            ce += co.elapsed_sec;
            cs += co.span_sec;
            for w in &co.hypothesis {
                if bo.pruned.iter().any(|e| e == w) || bo.evicted.iter().any(|e| e == w) {
                    rescues += 1;
                }
            }
            let sb = output::wer(r, &bo.hypothesis);
            let sc = output::wer(r, &co.hypothesis);
            detail.push_str(&format!(
                "{id}\t{budget}\t{}\t{ae}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                r.len(),
                sb.substitutions + sb.deletions + sb.insertions,
                sc.substitutions + sc.deletions + sc.insertions,
                bo.peak_live,
                bo.peak_dormant,
                co.peak_live,
                co.peak_dormant,
                bo.hypothesis.join(" "),
                co.hypothesis.join(" "),
            ));
            b_hyps.push(bo.hypothesis);
            c_hyps.push(co.hypothesis);
        }
        let b_tot = score_corpus(&refs, &b_hyps);
        let c_tot = score_corpus(&refs, &c_hyps);
        let (b_errs, _) = per_utterance_errors(&refs, &b_hyps);
        let (c_errs, _) = per_utterance_errors(&refs, &c_hyps);
        eprintln!(
            "budget {budget}: B WER {:.4} C WER {:.4} rescues {rescues} wall {:.1}s",
            b_tot.wer(),
            c_tot.wer(),
            t0.elapsed().as_secs_f64()
        );
        rows.push(BudgetRow {
            budget,
            a: a_cell.clone(),
            b: ArmCell {
                wer: b_tot.wer(),
                subs: b_tot.subs,
                dels: b_tot.dels,
                inss: b_tot.inss,
                ref_len: b_tot.ref_len,
                rtf: if bs > 0.0 { be / bs } else { 0.0 },
                peak_live: Some(bl),
                peak_dormant: Some(bd),
                peak_total: Some(bt),
                hyps: b_hyps,
            },
            c: ArmCell {
                wer: c_tot.wer(),
                subs: c_tot.subs,
                dels: c_tot.dels,
                inss: c_tot.inss,
                ref_len: c_tot.ref_len,
                rtf: if cs > 0.0 { ce / cs } else { 0.0 },
                peak_live: Some(cl),
                peak_dormant: Some(cd),
                peak_total: Some(ct),
                hyps: c_hyps,
            },
            rescues,
            delta_c_minus_b: delta_ci(&b_errs, &c_errs, &denoms),
        });
    }
    let table = BudgetTable { rows };
    let mut report = String::from("# A/B/C budget table (real lattices)\n\n");
    report.push_str(&format!(
        "Manifest: {} ({} utterances). Lattices: {}. Cell: {}. Params: w_ac={w_ac} w_seq={w_seq} theta={theta} delta={delta} h={h} hmax={hmax}.\n\n",
        manifest.display(),
        ids.len(),
        lat_dir.display(),
        cell.display()
    ));
    report.push_str(&render_markdown(&table));
    report.push('\n');
    std::fs::write(&out, &report).expect("write report");
    std::fs::write(&per_utt, &detail).expect("write per-utt");
    eprintln!("wrote {} and {}", out.display(), per_utt.display());
}
