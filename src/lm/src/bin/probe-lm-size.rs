//! Size probe for R1-v2 (bead hearsay-four-247--adapted-decode-r5a).
//!
//! Trains the full general KN trigram, prints counts-of-counts histograms
//! (with suffix-kept entries per cutoff), then times serialize/deserialize
//! at several count cutoffs. No entropy pruning: this run only measures
//! the entries -> archive-bytes -> load-seconds curve so the final
//! `train-r1v2` run can pick thresholds that load in under 2 s.
//!
//! usage: probe-lm-size <general_txt>

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use lm::persist;

fn print_hist(name: &str, hist: &BTreeMap<u64, u64>, total: u64) {
    // suffix[v] = entries with count >= v (kept entries at cutoff v).
    let mut suffix = total;
    let mut printed_tail = false;
    for (&v, &n) in hist.iter() {
        if v <= 20 {
            println!("hist\t{name}\tcount={v}\tentries={n}\tkept_at_cutoff={suffix}");
        } else if !printed_tail {
            println!("hist\t{name}\tcount=21+\tentries=TAIL\tkept_at_cutoff={suffix}");
            printed_tail = true;
        }
        suffix -= n;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: probe-lm-size <general_txt>");
        std::process::exit(2);
    }
    let t0 = Instant::now();
    let raw = std::fs::read_to_string(Path::new(&args[1])).expect("read general_txt");
    let mut sents = Vec::new();
    let mut lines = 0u64;
    for line in raw.lines() {
        lines += 1;
        sents.extend(lm::normalize_text(line));
    }
    drop(raw);
    let tokens: u64 = sents.iter().map(|s| s.len() as u64).sum();
    println!(
        "input\tgeneral\tlines={lines} sentences={} tokens={tokens} ingest_secs={:.1}",
        sents.len(),
        t0.elapsed().as_secs_f64()
    );
    let t0 = Instant::now();
    let full = lm::KnTrigram::train(&sents);
    drop(sents);
    println!(
        "trained\tgeneral\tbigrams={} trigrams={} train_secs={:.1}",
        full.bigram_entry_count(),
        full.trigram_entry_count(),
        t0.elapsed().as_secs_f64()
    );
    let (bi_hist, tri_hist) = full.count_histograms();
    let bi_total: u64 = bi_hist.values().sum();
    let tri_total: u64 = tri_hist.values().sum();
    print_hist("bigram", &bi_hist, bi_total);
    print_hist("trigram", &tri_hist, tri_total);

    for (min_bi, min_tri) in [(2u64, 2u64), (3, 2), (3, 3), (5, 3), (5, 5), (8, 5)] {
        let t0 = Instant::now();
        let cut = full.count_cutoff_pruned(min_bi, min_tri);
        let cut_secs = t0.elapsed().as_secs_f64();
        let (b, t) = (cut.bigram_entry_count(), cut.trigram_entry_count());
        let t0 = Instant::now();
        let bytes = persist::kn_to_bytes(&cut);
        let ser_secs = t0.elapsed().as_secs_f64();
        let t0 = Instant::now();
        let _rt = persist::kn_from_bytes(&bytes).expect("cut round trip");
        let deser_secs = t0.elapsed().as_secs_f64();
        println!(
            "cutoff-candidate\tmin_bi={min_bi}\tmin_tri={min_tri}\tbigrams={b}\ttrigrams={t}\tarchive_bytes={}\tcut_secs={cut_secs:.1}\tser_secs={ser_secs:.1}\tdeser_secs={deser_secs:.3}\tunder_2s={}",
            bytes.len(),
            deser_secs < 2.0
        );
    }
}
