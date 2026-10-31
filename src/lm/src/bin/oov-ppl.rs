//! oov-ppl: OOV rates + OOV-excluded perplexities on dev-clean.
//!
//! Loads a general KN model + book KN model, builds the interpolated model at
//! a given book weight, and scores dev-clean transcripts three ways:
//!   - own-vocab exclusion: each model skips tokens OOV w.r.t. its own vocab
//!     (interpolated uses the union vocab — a word seen by either component
//!     is predictable without the UNK floor dominating).
//!   - common-subset exclusion: all three models scored on the general-vocab
//!     in-vocab subset, for apples-to-apples comparison.
//!   - standard (OOV-included, UNK-mapped) perplexities, to verify round-trip
//!     against the training log.
//!
//! usage: oov-ppl <general_json> <book_json> <dev_clean_dir> <book_weight>

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lm::persist;

fn collect_trans_txt(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut subs: Vec<PathBuf> = Vec::new();
    for e in entries {
        let p = e.unwrap_or_else(|e| panic!("dir entry: {e}")).path();
        if p.is_dir() {
            subs.push(p);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".trans.txt"))
        {
            out.push(p);
        }
    }
    subs.sort();
    for s in subs {
        collect_trans_txt(&s, out);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: oov-ppl <general_json> <book_json> <dev_clean_dir> <book_weight>");
        std::process::exit(2);
    }
    let weight: f64 = args[4]
        .parse()
        .unwrap_or_else(|_| panic!("bad book_weight {:?}", args[4]));

    let general = persist::load_kn(Path::new(&args[1])).expect("load general");
    let book = persist::load_kn(Path::new(&args[2])).expect("load book");
    let interp =
        lm::InterpolatedLm::new(general.clone(), book.clone(), weight).expect("interp lm");

    let mut trans_paths = Vec::new();
    collect_trans_txt(Path::new(&args[3]), &mut trans_paths);
    trans_paths.sort();
    assert!(!trans_paths.is_empty(), "no *.trans.txt");
    let mut sents: Vec<Vec<String>> = Vec::new();
    for p in &trans_paths {
        let text =
            std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        for line in text.lines() {
            let Some((_, transcript)) = line.split_once([' ', '\t']) else {
                continue;
            };
            sents.extend(lm::normalize_text(transcript));
        }
    }

    let vg = general.vocabulary();
    let vb = book.vocabulary();
    let union: BTreeSet<String> = vg.union(vb).cloned().collect();
    println!(
        "vocab\tgeneral={} book={} union={}",
        vg.len(),
        vb.len(),
        union.len()
    );

    let mut total: u64 = 0;
    let (mut oov_g, mut oov_b, mut oov_u) = (0u64, 0u64, 0u64);
    // own-vocab exclusion accumulators
    let (mut lp_g, mut n_g) = (0.0f64, 0u64);
    let (mut lp_b, mut n_b) = (0.0f64, 0u64);
    let (mut lp_i, mut n_i) = (0.0f64, 0u64);
    // common-subset (general in-vocab) accumulators for book + interp
    // (general common == general own, reused)
    let (mut lp_b_common, mut lp_i_common) = (0.0f64, 0.0f64);
    for s in &sents {
        let mut padded: Vec<String> =
            vec![lm::BOS_TOKEN.into(), lm::BOS_TOKEN.into()];
        padded.extend(s.iter().cloned());
        padded.push(lm::EOS_TOKEN.into());
        for t in padded.windows(3) {
            total += 1;
            let (w1, w2, w) = (&t[0], &t[1], &t[2]);
            let g_oov = !vg.contains(w.as_str());
            let b_oov = !vb.contains(w.as_str());
            let u_oov = !union.contains(w.as_str());
            if g_oov {
                oov_g += 1;
            } else {
                lp_g += general.prob(w1, w2, w).max(1e-12).ln();
                n_g += 1;
                // common subset: general in-vocab, score all three
                lp_b_common += book.prob(w1, w2, w).max(1e-12).ln();
                lp_i_common += interp.prob(w1, w2, w).max(1e-12).ln();
            }
            if b_oov {
                oov_b += 1;
            } else {
                lp_b += book.prob(w1, w2, w).max(1e-12).ln();
                n_b += 1;
            }
            if u_oov {
                oov_u += 1;
            } else {
                lp_i += interp.prob(w1, w2, w).max(1e-12).ln();
                n_i += 1;
            }
        }
    }
    let ppl = |lp: f64, n: u64| {
        if n == 0 {
            f64::INFINITY
        } else {
            (-lp / n as f64).exp()
        }
    };
    println!(
        "dev\tsents={} predicted_tokens={total} oov_general={oov_g} oov_rate_general={:.6} oov_book={oov_b} oov_rate_book={:.6} oov_union={oov_u} oov_rate_union={:.6}",
        sents.len(),
        oov_g as f64 / total as f64,
        oov_b as f64 / total as f64,
        oov_u as f64 / total as f64,
    );
    println!(
        "ppl-own\tgeneral={:.4}\ttokens={n_g}\tbook={:.4}\ttokens={n_b}\tinterp={:.4}\ttokens={n_i}",
        ppl(lp_g, n_g),
        ppl(lp_b, n_b),
        ppl(lp_i, n_i),
    );
    println!(
        "ppl-common-general-vocab\tgeneral={:.4}\tbook={:.4}\tinterp={:.4}\ttokens={n_g}",
        ppl(lp_g, n_g),
        ppl(lp_b_common, n_g),
        ppl(lp_i_common, n_g),
    );
    let std_g = general.perplexity(&sents).perplexity;
    let std_b = book.perplexity(&sents).perplexity;
    let std_i = interp.perplexity(&sents).perplexity;
    println!("ppl-standard\tgeneral={std_g:.4}\tbook={std_b:.4}\tinterp={std_i:.4}");
}
