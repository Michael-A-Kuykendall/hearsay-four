//! cell_decode: one 16-worker queue shard (bead hearsay-four-7i2o).
//!
//! Loads the real interpolated LM once, then runs every task line in a
//! shard file through `experiment::arms::run_fabric_arm`, writing one JSON
//! per (utt, budget, arm) task. Existing outputs are skipped (resume).
//!
//! Task line: `utt budget arm w_ac w_seq theta delta h hmax` (arm B|C).
//! Output: `{out-dir}/{utt}.{budget}.{arm}.json`.
//!
//! usage: cell_decode --lm <lm.json> --lat-dir <dir> --shard <file>
//!   --out-dir <dir>

use experiment::arms::{Arm, ArmParams, run_fabric_arm};
use hearsay::{InterpolatedScorer, SequenceScorer};
use std::path::PathBuf;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lm_path: Option<PathBuf> = None;
    let mut lat_dir: Option<PathBuf> = None;
    let mut shard: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lm" => lm_path = Some(PathBuf::from(take_value(&args, &mut i, "--lm").unwrap())),
            "--lat-dir" => {
                lat_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--lat-dir").unwrap(),
                ))
            }
            "--shard" => shard = Some(PathBuf::from(take_value(&args, &mut i, "--shard").unwrap())),
            "--out-dir" => {
                out_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--out-dir").unwrap(),
                ))
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let (lm_path, lat_dir, shard, out_dir) = (
        lm_path.expect("--lm"),
        lat_dir.expect("--lat-dir"),
        shard.expect("--shard"),
        out_dir.expect("--out-dir"),
    );
    std::fs::create_dir_all(&out_dir).expect("create out-dir");
    let lm = lm::persist::load_interpolated(&lm_path).expect("load lm");
    let scorer = InterpolatedScorer::new(&lm);
    let lm_fn = |a: &str, b: &str| scorer.sequence_llr(if a == "<s>" { None } else { Some(a) }, b);
    let text = std::fs::read_to_string(&shard).expect("read shard");
    // Last-utterance lattice cache: shard lines group tasks by utt, so
    // consecutive lines usually share one lattice parse.
    let mut cached: Option<(String, lattice::Lattice)> = None;
    let (mut ran, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 9, "shard line {} wants 9 fields", lineno + 1);
        let (utt, budget, arm_s) = (f[0], f[1].parse::<usize>().unwrap(), f[2]);
        let arm = match arm_s {
            "B" => Arm::B,
            "C" => Arm::C,
            _ => panic!("shard line {} bad arm {arm_s:?}", lineno + 1),
        };
        let params = ArmParams {
            budget,
            w_acoustic: f[3].parse().unwrap(),
            w_sequence: f[4].parse().unwrap(),
            theta_live: f[5].parse().unwrap(),
            delta: f[6].parse().unwrap(),
            h: f[7].parse().unwrap(),
            h_max: f[8].parse().unwrap(),
        };
        let dest = out_dir.join(format!("{utt}.{budget}.{arm_s}.json"));
        if dest.exists() {
            skipped += 1;
            continue;
        }
        let lat = match &cached {
            Some((id, lat)) if id == utt => lat.clone(),
            _ => {
                let lt = std::fs::read_to_string(lat_dir.join(format!("{utt}.lat.jsonl")))
                    .unwrap_or_else(|_| panic!("read lattice {utt}"));
                let lat: lattice::Lattice = lattice::read_lattice(&lt).expect("parse lattice");
                cached = Some((utt.to_string(), lat.clone()));
                lat
            }
        };
        let o = match run_fabric_arm(&lat, arm, &params, &lm_fn) {
            Ok(o) => o,
            Err(e) => {
                // Loud but non-fatal: leave a FAIL marker (retried on
                // resume — only `.json` outputs skip) and continue the
                // shard so one bad task cannot starve the other 15
                // workers. The shard still exits nonzero.
                let fail = out_dir.join(format!("{utt}.{budget}.{arm_s}.FAIL"));
                std::fs::write(&fail, format!("{e}\n")).expect("write FAIL marker");
                eprintln!("FAIL {utt} budget {budget} arm {arm_s}: {e}");
                failed += 1;
                continue;
            }
        };
        let v = serde_json::json!({
            "utt": utt,
            "budget": budget,
            "arm": arm_s,
            "hyp": o.hypothesis,
            "committed": o.committed,
            "peak_live": o.peak_live,
            "peak_dormant": o.peak_dormant,
            "peak_total": o.peak_total,
            "revivals": o.revivals,
            "pruned": o.pruned,
            "evicted": o.evicted,
            "elapsed_sec": o.elapsed_sec,
            "span_sec": o.span_sec,
        });
        let tmp = dest.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(&v).unwrap()).expect("write tmp");
        std::fs::rename(&tmp, &dest).expect("publish task output");
        ran += 1;
    }
    eprintln!(
        "shard {} done: ran {ran} skipped {skipped} failed {failed}",
        shard.display()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
