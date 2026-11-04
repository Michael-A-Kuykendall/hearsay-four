//! DEV scale calibration: static lattice statistics for the fabric grid.
//!
//! usage (shard step, one process per id-shard under xargs):
//!   scale_calibrate --manifest <tsv> --lat-dir <dir> --lm <lm.json>
//!     --ids <file|-> --tokens-out <tsv> --seq-out <txt> --counts-out <tsv>
//! usage (merge step):
//!   scale_calibrate --merge --tokens <f>... --seq <f>... --counts <f>...
//!     --summary-out <json>
//!
//! Shard token rows (`utt id acoustic other_unary in_max out_max`, empty
//! for absent terms) carry everything the merge needs to rebuild the
//! liveness distribution at any `w_sequence`:
//! `acoustic + other + ws * (in_max? + out_max?)`, mirroring
//! [`experiment::calibrate::static_liveness`].

use experiment::calibrate::percentile;
use hearsay::{InterpolatedScorer, SequenceScorer};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn read_ids(path: &str, manifest: &std::path::Path) -> Vec<String> {
    if path == "-" {
        let text = std::fs::read_to_string(manifest).expect("read manifest");
        return text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('\t').next().unwrap().to_string())
            .collect();
    }
    std::fs::read_to_string(path)
        .expect("read ids")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect()
}

fn run_shard(
    manifest: &std::path::Path,
    lat_dir: &std::path::Path,
    lm_path: &std::path::Path,
    ids_path: &str,
    tokens_out: &std::path::Path,
    seq_out: &std::path::Path,
    counts_out: &std::path::Path,
) {
    let ids = read_ids(ids_path, manifest);
    let lm = lm::persist::load_interpolated(lm_path).expect("load lm");
    let scorer = InterpolatedScorer::new(&lm);
    let seq = |prev: Option<&str>, word: &str| scorer.sequence_llr(prev, word);
    let mut tokens = String::new();
    let mut seqs = String::new();
    let mut counts = String::from("utt\tntokens\tnarcs\n");
    for id in &ids {
        let text = std::fs::read_to_string(lat_dir.join(format!("{id}.lat.jsonl")))
            .unwrap_or_else(|_| panic!("read lattice {id}"));
        let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
        counts.push_str(&format!("{id}\t{}\t{}\n", lat.tokens.len(), lat.arcs.len()));
        let sides = experiment::calibrate::sequence_sides(&lat, &seq);
        for t in &lat.tokens {
            let mut other = 0.0;
            for v in [t.lexical_llr, t.speaker_llr, t.verify_llr]
                .into_iter()
                .flatten()
            {
                other += v;
            }
            let s = sides[&t.id];
            let fmt = |o: Option<f64>| o.map_or_else(String::new, |v| v.to_string());
            tokens.push_str(&format!(
                "{id}\t{}\t{}\t{other}\t{}\t{}\n",
                t.id,
                t.acoustic_llr,
                fmt(s.in_max),
                fmt(s.out_max)
            ));
        }
        for v in experiment::calibrate::sequence_terms(&lat, &seq) {
            seqs.push_str(&format!("{v}\n"));
        }
    }
    std::fs::write(tokens_out, tokens).expect("write tokens");
    std::fs::write(seq_out, seqs).expect("write seqs");
    std::fs::write(counts_out, counts).expect("write counts");
    eprintln!("shard {ids_path}: {} utterances", ids.len());
}

fn parse_opt(s: &str) -> Option<f64> {
    if s.is_empty() {
        None
    } else {
        Some(s.parse().unwrap())
    }
}

fn run_merge(
    token_files: &[String],
    seq_files: &[String],
    count_files: &[String],
    summary_out: &std::path::Path,
) {
    let mut acoustic: Vec<f64> = Vec::new();
    let mut live_ws: BTreeMap<u32, Vec<f64>> = [1, 4, 6, 8, 10]
        .into_iter()
        .map(|w| (w, Vec::new()))
        .collect();
    let mut in_present = 0usize;
    let mut out_present = 0usize;
    let mut ntok = 0usize;
    for f in token_files {
        let text = std::fs::read_to_string(f).expect("read tokens shard");
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(c.len(), 6, "token row wants 6 fields: {line}");
            let (a, o, i, oo) = (
                c[2].parse::<f64>().unwrap(),
                c[3].parse::<f64>().unwrap(),
                parse_opt(c[4]),
                parse_opt(c[5]),
            );
            acoustic.push(a);
            ntok += 1;
            if i.is_some() {
                in_present += 1;
            }
            if oo.is_some() {
                out_present += 1;
            }
            for (w, v) in live_ws.iter_mut() {
                let mut live = a + o;
                if let Some(x) = i {
                    live += *w as f64 * x;
                }
                if let Some(x) = oo {
                    live += *w as f64 * x;
                }
                v.push(live);
            }
        }
    }
    let mut seqs: Vec<f64> = Vec::new();
    for f in seq_files {
        let text = std::fs::read_to_string(f).expect("read seq shard");
        for line in text.lines() {
            if !line.trim().is_empty() {
                seqs.push(line.trim().parse().unwrap());
            }
        }
    }
    let mut utts: BTreeSet<String> = BTreeSet::new();
    let mut density: Vec<(String, usize, usize)> = Vec::new();
    let mut narcs = 0usize;
    for f in count_files {
        let text = std::fs::read_to_string(f).expect("read counts shard");
        for line in text.lines().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            let (nt, na) = (
                c[1].parse::<usize>().unwrap(),
                c[2].parse::<usize>().unwrap(),
            );
            utts.insert(c[0].to_string());
            density.push((c[0].to_string(), nt, na));
            narcs += na;
        }
    }
    density.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    assert_eq!(
        ntok,
        density.iter().map(|d| d.1).sum::<usize>(),
        "token count mismatch"
    );

    let pcts = [5.0, 25.0, 50.0, 75.0, 95.0];
    let row = |name: &str, v: &[f64]| {
        let ps: Vec<f64> = pcts.iter().map(|p| percentile(v, *p)).collect();
        println!(
            "{name}\tn={}\tp5={:.4}\tp25={:.4}\tp50={:.4}\tp75={:.4}\tp95={:.4}",
            v.len(),
            ps[0],
            ps[1],
            ps[2],
            ps[3],
            ps[4]
        );
        ps
    };
    println!("quantity\tn\tp5\tp25\tp50\tp75\tp95");
    let mut summary = serde_json::json!({
        "n_utts": utts.len(),
        "n_tokens": ntok,
        "n_seq_terms": seqs.len(),
        "n_arcs": narcs,
        "tokens_with_in_max": in_present,
        "tokens_with_out_max": out_present,
    });
    summary["acoustic_llr"] = serde_json::json!(row("acoustic_llr", &acoustic));
    for (w, v) in &live_ws {
        summary[format!("liveness_ws{w}")] = serde_json::json!(row(&format!("liveness_ws{w}"), v));
    }
    summary["sequence_llr"] = serde_json::json!(row("sequence_llr", &seqs));
    summary["density_top25"] = density
        .iter()
        .take(25)
        .map(|(id, nt, na)| serde_json::json!({"utt": id, "ntokens": nt, "narcs": na}))
        .collect();
    summary["density_all"] = density
        .iter()
        .map(|(id, nt, na)| serde_json::json!({"utt": id, "ntokens": nt, "narcs": na}))
        .collect();
    std::fs::write(summary_out, serde_json::to_string_pretty(&summary).unwrap())
        .expect("write summary");
    eprintln!("wrote {}", summary_out.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--merge") {
        let mut tokens: Vec<String> = Vec::new();
        let mut seqs: Vec<String> = Vec::new();
        let mut counts: Vec<String> = Vec::new();
        let mut summary: Option<PathBuf> = None;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--tokens" => tokens.push(take_value(&args, &mut i, "--tokens").unwrap()),
                "--seq" => seqs.push(take_value(&args, &mut i, "--seq").unwrap()),
                "--counts" => counts.push(take_value(&args, &mut i, "--counts").unwrap()),
                "--summary-out" => {
                    summary = Some(PathBuf::from(
                        take_value(&args, &mut i, "--summary-out").unwrap(),
                    ))
                }
                other => panic!("unknown argument {other:?}"),
            }
            i += 1;
        }
        assert!(!tokens.is_empty(), "--tokens required");
        run_merge(&tokens, &seqs, &counts, &summary.expect("--summary-out"));
        return;
    }
    let mut manifest: Option<PathBuf> = None;
    let mut lat_dir: Option<PathBuf> = None;
    let mut lm_path: Option<PathBuf> = None;
    let mut ids = String::from("-");
    let mut tokens_out: Option<PathBuf> = None;
    let mut seq_out: Option<PathBuf> = None;
    let mut counts_out: Option<PathBuf> = None;
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
            "--ids" => ids = take_value(&args, &mut i, "--ids").unwrap(),
            "--tokens-out" => {
                tokens_out = Some(PathBuf::from(
                    take_value(&args, &mut i, "--tokens-out").unwrap(),
                ))
            }
            "--seq-out" => {
                seq_out = Some(PathBuf::from(
                    take_value(&args, &mut i, "--seq-out").unwrap(),
                ))
            }
            "--counts-out" => {
                counts_out = Some(PathBuf::from(
                    take_value(&args, &mut i, "--counts-out").unwrap(),
                ))
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    run_shard(
        &manifest.expect("--manifest"),
        &lat_dir.expect("--lat-dir"),
        &lm_path.expect("--lm"),
        &ids,
        &tokens_out.expect("--tokens-out"),
        &seq_out.expect("--seq-out"),
        &counts_out.expect("--counts-out"),
    );
}
