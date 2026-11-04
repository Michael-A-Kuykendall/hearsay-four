//! Commit-margin harvest: the `delta` grid needs the 25th percentile
//! of DEV commit margins, which only exist inside a decode. Runs one
//! fixed configuration over a shard of utterances and writes every
//! non-null committed/defeated margin plus the transcript per utt.
//!
//! usage:
//!   margin_harvest --manifest <tsv> --lat-dir <dir> --lm <lm.json>
//!     --ids <file> --theta <f> --wseq <f> --budget <n> --arm <B|C>
//!     --margins-out <txt> --utt-out <tsv>
//!
//! Margin rows are long-form (`utt margin`, one per margin); utt rows
//! (`utt n_margins margin_p50 peak_total hyp`) carry the transcript for
//! cross-checking against the `cell_decode` path.

use experiment::calibrate::{harvest_margins, percentile};
use fabric::decision::DecisionParams;
use hearsay::{Arm as FabricArm, InterpolatedScorer};
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
    let mut lm_path: Option<PathBuf> = None;
    let mut ids_path: Option<PathBuf> = None;
    let mut theta: Option<f64> = None;
    let mut wseq = 6.0f64;
    let mut budget = 500usize;
    let mut arm_s = String::from("C");
    let mut margins_out: Option<PathBuf> = None;
    let mut utt_out: Option<PathBuf> = None;
    let mut events_dir: Option<PathBuf> = None;
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
            "--arm" => arm_s = take_value(&args, &mut i, "--arm").unwrap(),
            "--margins-out" => {
                margins_out = Some(PathBuf::from(
                    take_value(&args, &mut i, "--margins-out").unwrap(),
                ))
            }
            "--utt-out" => {
                utt_out = Some(PathBuf::from(
                    take_value(&args, &mut i, "--utt-out").unwrap(),
                ))
            }
            "--events-dir" => {
                events_dir = Some(PathBuf::from(
                    take_value(&args, &mut i, "--events-dir").unwrap(),
                ))
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let (manifest, lat_dir, lm_path, ids_path, theta, margins_out, utt_out) = (
        manifest.expect("--manifest"),
        lat_dir.expect("--lat-dir"),
        lm_path.expect("--lm"),
        ids_path.expect("--ids"),
        theta.expect("--theta"),
        margins_out.expect("--margins-out"),
        utt_out.expect("--utt-out"),
    );
    let _ = manifest;
    let farm = match arm_s.as_str() {
        "B" => FabricArm::B,
        "C" => FabricArm::C,
        other => panic!("bad arm {other:?}"),
    };
    let dormant = if arm_s == "B" { 0 } else { budget };
    let ids: Vec<String> = std::fs::read_to_string(&ids_path)
        .expect("read ids")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    let lm = lm::persist::load_interpolated(&lm_path).expect("load lm");
    let scorer = InterpolatedScorer::new(&lm);
    let mut margins = String::new();
    let mut utts = String::from("utt\tn_margins\tmargin_p50\tpeak_total\thyp\n");
    for id in &ids {
        let text = std::fs::read_to_string(lat_dir.join(format!("{id}.lat.jsonl")))
            .unwrap_or_else(|_| panic!("read lattice {id}"));
        let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
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
            j: 3,
            arm: farm,
            ..DecisionParams::default()
        };
        let out = hearsay::decode_lattice(&lat, &scorer, params)
            .unwrap_or_else(|e| panic!("decode {id}: {e}"));
        if let Some(dir) = &events_dir {
            std::fs::write(dir.join(format!("{id}.events.jsonl")), &out.events_jsonl)
                .expect("write events");
        }
        let ms = harvest_margins(&out.events_jsonl);
        for m in &ms {
            margins.push_str(&format!("{id}\t{m}\n"));
        }
        utts.push_str(&format!(
            "{id}\t{}\t{:.4}\t{}\t{}\n",
            ms.len(),
            if ms.is_empty() {
                f64::NAN
            } else {
                percentile(&ms, 50.0)
            },
            out.peak_total,
            out.transcript.join(" "),
        ));
        eprintln!("{id} done ({} margins)", ms.len());
    }
    std::fs::write(&margins_out, margins).expect("write margins");
    std::fs::write(&utt_out, utts).expect("write utts");
    eprintln!("wrote {} and {}", margins_out.display(), utt_out.display());
}
