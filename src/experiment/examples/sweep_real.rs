//! sweep_real: oracle WER sweep over saved real lattices.
//!
//! Bead `hearsay-four-ic1w--fabric-experiment-arms` (wave 2): runs the exact
//! tested oracle (`experiment::oracle_sweep`, i.e. `a2::oracle::sweep_k`)
//! over a manifest's saved `<utt>.lat.jsonl` files at K=3/5/10, plus a
//! per-utterance table with 1-best errors and generation-miss attribution
//! (reference words absent from the lattice at any rank).
//!
//! usage: sweep_real --manifest <tsv> --lat-dir <dir> --out <report.md>
//!   --per-utt <tsv> [--ks 3,5,10]

use experiment::oracle_sweep::{render_sweep_markdown, sweep_corpus, sweep_utterance};
use experiment::scoring::one_best_path;
use std::collections::BTreeSet;
use std::path::PathBuf;

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
    let mut out: Option<PathBuf> = None;
    let mut per_utt: Option<PathBuf> = None;
    let mut ks: Vec<usize> = vec![3, 5, 10];
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
            "--out" => out = Some(PathBuf::from(take_value(&args, &mut i, "--out").unwrap())),
            "--per-utt" => {
                per_utt = Some(PathBuf::from(
                    take_value(&args, &mut i, "--per-utt").unwrap(),
                ))
            }
            "--ks" => {
                ks = take_value(&args, &mut i, "--ks")
                    .unwrap()
                    .split(',')
                    .map(|s| s.parse::<usize>().unwrap())
                    .collect();
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let (manifest, lat_dir, out, per_utt) = (
        manifest.expect("--manifest"),
        lat_dir.expect("--lat-dir"),
        out.expect("--out"),
        per_utt.expect("--per-utt"),
    );
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
    eprintln!("utterances {}", ids.len());
    let mut lat_texts: Vec<String> = Vec::with_capacity(ids.len());
    for id in &ids {
        let p = lat_dir.join(format!("{id}.lat.jsonl"));
        lat_texts.push(std::fs::read_to_string(&p).expect("read lattice"));
    }
    let items: Vec<(&str, &[String])> = lat_texts
        .iter()
        .map(String::as_str)
        .zip(refs.iter().map(Vec::as_slice))
        .collect();
    let rows = sweep_corpus(&items, &ks).expect("sweep");
    let mut report = String::from("# Oracle sweep (real lattices)\n\n");
    report.push_str(&format!(
        "Manifest: {} ({} utterances). Lattices: {}. K = {ks:?}.\n\n",
        manifest.display(),
        ids.len(),
        lat_dir.display()
    ));
    report.push_str(&render_sweep_markdown(&rows));
    report.push('\n');
    std::fs::write(&out, &report).expect("write report");

    // Per-utterance detail: 1-best errors, oracle errors per K, token/arc
    // counts, and generation-miss words (ref words absent from the lattice
    // token set at any rank — fixable only at generation, not by search).
    let mut detail = String::from("id\tref_len\tbest1_errs");
    for k in &ks {
        detail.push_str(&format!("\toracle{k}_errs"));
    }
    detail.push_str("\ttokens\tarcs\tgenmiss\tgenmiss_words\n");
    for ((id, r), lt) in ids.iter().zip(refs.iter()).zip(lat_texts.iter()) {
        let lat = lattice::read_lattice(lt).expect("parse lattice");
        let hyp = one_best_path(&lat);
        let s = output::wer(r, &hyp);
        let best1 = s.substitutions + s.deletions + s.insertions;
        let sw = sweep_utterance(lt, r, &ks).expect("sweep utterance");
        let vocab: BTreeSet<&str> = lat.tokens.iter().map(|t| t.word.as_str()).collect();
        let missing: Vec<&str> = r
            .iter()
            .map(String::as_str)
            .filter(|w| !vocab.contains(w))
            .collect();
        detail.push_str(&format!("{id}\t{}\t{best1}", r.len()));
        let (mut nt, mut na) = (0, 0);
        for (_, o, t, a) in &sw {
            detail.push_str(&format!("\t{}", o.errors));
            nt = *t;
            na = *a;
        }
        detail.push_str(&format!(
            "\t{nt}\t{na}\t{}\t{}\n",
            missing.len(),
            missing.join(" ")
        ));
    }
    std::fs::write(&per_utt, &detail).expect("write per-utt");
    eprintln!("wrote {} and {}", out.display(), per_utt.display());
}
