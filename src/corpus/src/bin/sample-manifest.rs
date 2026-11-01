//! Stratified LibriSpeech TSV sampler: <in.tsv> <out.tsv> <per_speaker> <seed>.
use corpus::manifest::{parse_librispeech_tsv, sample_stratified};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 5,
        "usage: sample-manifest <in.tsv> <out.tsv> <per_speaker> <seed>"
    );
    let (inp, outp) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let per: usize = args[3].parse().expect("per_speaker");
    let seed: u64 = args[4].parse().expect("seed");
    let text = std::fs::read_to_string(&inp).expect("read input");
    let rows = parse_librispeech_tsv(&text);
    let picked = sample_stratified(&rows, per, seed);
    let mut s = String::new();
    for r in &picked {
        s.push_str(&format!("{}\t{}\t{}\t{}\n", r.id, r.rel, r.sec, r.text));
    }
    std::fs::write(&outp, s).expect("write output");
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &picked {
        *counts
            .entry(r.id.split('-').next().unwrap_or(""))
            .or_default() += 1;
    }
    eprintln!(
        "sample_manifest in_rows={} speakers={} per_speaker={} seed={} out_rows={}",
        rows.len(),
        counts.len(),
        per,
        seed,
        picked.len()
    );
    let short: Vec<(&&str, &usize)> = counts.iter().filter(|(_, c)| **c < per).collect();
    if !short.is_empty() {
        eprintln!("sample_manifest SHORT speakers (< {per}): {short:?}");
    }
}
