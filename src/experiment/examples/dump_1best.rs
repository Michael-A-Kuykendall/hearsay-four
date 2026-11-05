//! Dump lattice acoustic-1-best transcripts + self-score vs manifest.
//! usage: dump_1best --manifest <tsv> --lat-dir <dir> --out <tsv>
//! Writes id<TAB>hyp per line; prints total errs/WER to stderr
//! (DEV-200 cross-check: 3116 errs / 0.6805).
use std::collections::HashMap;
use std::path::PathBuf;

fn take_value(args: &[String], i: &mut usize, name: &str) -> String {
    *i += 1;
    args.get(*i)
        .unwrap_or_else(|| panic!("{name} needs a value"))
        .clone()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mut man, mut lat_dir, mut out) =
        (String::new(), String::new(), String::new());
    let mut i = 0;
    while i + 1 < args.len() {
        i += 1;
        match args[i].as_str() {
            "--manifest" => man = take_value(&args, &mut i, "--manifest"),
            "--lat-dir" => lat_dir = take_value(&args, &mut i, "--lat-dir"),
            "--out" => out = take_value(&args, &mut i, "--out"),
            a => panic!("unknown arg {a}"),
        }
    }
    let mut s = String::from("id\thyp\n");
    let (mut refs, mut hyps) = (Vec::new(), Vec::new());
    for line in std::fs::read_to_string(&man).expect("read manifest").lines() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert!(f.len() >= 4, "manifest wants >= 4 cols: {line}");
        let id = f[0];
        let text = std::fs::read_to_string(
            PathBuf::from(&lat_dir).join(format!("{id}.lat.jsonl")),
        )
        .expect("read lattice");
        let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
        let by_id: HashMap<u64, &str> =
            lat.tokens.iter().map(|t| (t.id, t.word.as_str())).collect();
        let words: Vec<String> = a2::oracle::acoustic_best_path_ids(&lat)
            .iter()
            .map(|wanted| {
                by_id.get(wanted).expect("path id in lattice").to_string()
            })
            .collect();
        s.push_str(&format!("{id}\t{}\n", words.join(" ")));
        refs.push(f[3].split_whitespace().map(str::to_string).collect());
        hyps.push(words);
    }
    std::fs::write(&out, s).expect("write tsv");
    let tot = experiment::scoring::score_corpus(&refs, &hyps);
    eprintln!(
        "1-best: {} errs / {} refs = {:.4} ({} utts)",
        tot.subs + tot.dels + tot.inss,
        tot.ref_len,
        tot.wer(),
        refs.len()
    );
}
