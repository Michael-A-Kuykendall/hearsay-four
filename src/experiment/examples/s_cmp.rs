//! Score-split probe: S(oracle acoustic-best path) vs S(fabric winner)
//! under the fabric's own objective. If oracle-path scores higher, the
//! tiling search fails to find the max (search bug); if lower, the
//! objective itself prefers garbage (score misrank).
//!
//! usage: s_cmp --lat <utt.lat.jsonl> --lm <lm.json> --wseq <f>

use fabric::decision::DecisionParams;
use hearsay::{Arm as FabricArm, InterpolatedScorer, SequenceScorer};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut lat, mut lm_path, mut wseq) = (None, None, 6.0f64);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lat" => {
                i += 1;
                lat = Some(PathBuf::from(args[i].clone()))
            }
            "--lm" => {
                i += 1;
                lm_path = Some(PathBuf::from(args[i].clone()))
            }
            "--wseq" => {
                i += 1;
                wseq = args[i].parse().unwrap()
            }
            other => panic!("unknown {other:?}"),
        }
        i += 1;
    }
    let (lat, lm_path) = (lat.expect("--lat"), lm_path.expect("--lm"));
    let text = std::fs::read_to_string(&lat).expect("read lattice");
    let lattice: lattice::Lattice = lattice::read_lattice(&text).expect("parse");
    let lm = lm::persist::load_interpolated(&lm_path).expect("load lm");
    let scorer = InterpolatedScorer::new(&lm);
    // Oracle acoustic-best path, scored under fabric S(T).
    let oids = a2::oracle::acoustic_best_path_ids(&lattice);
    let toks: BTreeMap<u64, &lattice::Token> = lattice.tokens.iter().map(|t| (t.id, t)).collect();
    let mut s_oracle = 0.0;
    let mut prev: Option<&str> = None;
    let mut owords: Vec<String> = Vec::new();
    for id in &oids {
        let t = toks[id];
        s_oracle += t.acoustic_llr;
        s_oracle += wseq * scorer.sequence_llr(prev, &t.word);
        prev = Some(&t.word);
        owords.push(t.word.clone());
    }
    // Fabric winner under the same objective.
    let params = DecisionParams {
        w_acoustic: 1.0,
        w_sequence: wseq,
        theta_live: 0.0,
        live_budget: usize::MAX,
        dormant_budget: usize::MAX,
        combined_budget: None,
        arm: FabricArm::C,
        ..DecisionParams::default()
    };
    let out = hearsay::decode_lattice(&lattice, &scorer, params).expect("decode");
    let s_fabric: f64 = out.commits.iter().map(|c| c.winner_score).sum();
    println!("oracle_ids={} S_oracle={s_oracle:.2}", oids.len());
    println!(
        "fabric_ntokens={} S_fabric={s_fabric:.2} n_commits={} gaps={:?}",
        out.transcript.len(),
        out.commits.len(),
        out.gaps
    );
    println!("delta_oracle_minus_fabric={:.2}", s_oracle - s_fabric);
    println!("oracle_words={}", owords.join(" "));
    println!("fabric_words={}", out.transcript.join(" "));
}
