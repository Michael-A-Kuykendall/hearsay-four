//! SCRATCH (ufe4): standalone dev5 print-run using only pre-existing API,
//! so it compiles both pre- and post-change. DELETE BEFORE COMMIT.

use fabric::decision::DecisionParams;
use hearsay::{BridgeOutcome, InterpolatedScorer, decode_lattice};
use lattice::{Lattice, read_lattice};
use lm::interpolate::InterpolatedLm;
use lm::persist::load_interpolated;
use std::path::PathBuf;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_dir() -> PathBuf {
    crate_dir().join("../..")
}

fn dev5_paths() -> Option<Vec<PathBuf>> {
    let dir = workspace_dir().join("data/lattices/dev5");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return None;
    }
    Some(paths)
}

fn decode_all(pop: &[(String, Lattice)], lm: &InterpolatedLm) -> Vec<BridgeOutcome> {
    let scorer = InterpolatedScorer::new(lm);
    pop.iter()
        .map(|(name, lat)| {
            decode_lattice(lat, &scorer, DecisionParams::default())
                .unwrap_or_else(|e| panic!("lattice {name} decodes: {e}"))
        })
        .collect()
}

#[test]
fn ufe4_scratch_dev5_real() {
    let paths = dev5_paths().expect("dev5 present");
    assert_eq!(paths.len(), 5);
    let pop: Vec<(String, Lattice)> = paths
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).expect("reads");
            let lat = read_lattice(&text).expect("parses");
            (lat.utterance.clone(), lat)
        })
        .collect();
    let lm_path = workspace_dir().join("data/lm-text/r1v2-full/lm-r1v2-interpolated.json");
    let lm = load_interpolated(&lm_path).expect("shipped LM loads");
    let outs = decode_all(&pop, &lm);
    let total_words: usize = outs.iter().map(|o| o.transcript.len()).sum();
    let total_gaps: usize = outs.iter().map(|o| o.gaps.len()).sum();
    println!("WORDS={total_words} GAPS={total_gaps}");
    for o in &outs {
        println!(
            "UTT={} N={} GAPS={:?} TX={}",
            o.utterance,
            o.transcript.len(),
            o.gaps,
            o.transcript.join(" ")
        );
    }
}
