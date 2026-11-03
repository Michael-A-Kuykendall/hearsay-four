//! bridge_scale: time decode_lattice over lattice truncations.
//!
//! Diagnostic for bead `hearsay-four-ic1w--fabric-experiment-arms` (wave 2):
//! the bridge hangs on real lattices, so this probe runs decode_lattice with
//! default params over successively larger token prefixes to expose the
//! scaling curve. Temporary instrument; not part of the experiment.
//!
//! usage: bridge_scale --lattice <utt.lat.jsonl> --lm <lm-interpolated.json>

use fabric::decision::DecisionParams;
use hearsay::{InterpolatedScorer, decode_lattice};
use std::path::PathBuf;
use std::time::Instant;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lattice_path: Option<PathBuf> = None;
    let mut lm_path: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lattice" => {
                lattice_path = Some(PathBuf::from(
                    take_value(&args, &mut i, "--lattice").unwrap(),
                ))
            }
            "--lm" => lm_path = Some(PathBuf::from(take_value(&args, &mut i, "--lm").unwrap())),
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let text = std::fs::read_to_string(lattice_path.as_ref().unwrap()).unwrap();
    let full = lattice::read_lattice(&text).unwrap();
    eprintln!(
        "loaded {} tokens {} arcs",
        full.tokens.len(),
        full.arcs.len()
    );
    let t0 = Instant::now();
    let lm = lm::persist::load_interpolated(lm_path.as_ref().unwrap()).unwrap();
    eprintln!("lm load {:.1}s", t0.elapsed().as_secs_f64());
    let scorer = InterpolatedScorer::new(&lm);

    let mut order: Vec<usize> = (0..full.tokens.len()).collect();
    order.sort_by(|&a, &b| {
        let (ta, tb) = (&full.tokens[a], &full.tokens[b]);
        (ta.start, ta.end, ta.id).cmp(&(tb.start, tb.end, tb.id))
    });
    let mut schedule: Vec<usize> = vec![10, 25, 50, 100, 150, 200, 300, 500];
    schedule.push(full.tokens.len());
    schedule.sort_unstable();
    schedule.dedup();
    for k in schedule {
        if k > full.tokens.len() {
            continue;
        }
        let keep: std::collections::BTreeSet<u64> =
            order.iter().take(k).map(|&i| full.tokens[i].id).collect();
        let tokens: Vec<lattice::Token> = full
            .tokens
            .iter()
            .filter(|t| keep.contains(&t.id))
            .cloned()
            .collect();
        let arcs: Vec<lattice::Arc> = full
            .arcs
            .iter()
            .filter(|a| keep.contains(&a.from) && keep.contains(&a.to))
            .cloned()
            .collect();
        let lat = lattice::Lattice {
            utterance: format!("{}-top{k}", full.utterance),
            frame_shift_ms: full.frame_shift_ms,
            producer: full.producer.clone(),
            tokens,
            arcs,
        };
        let t0 = Instant::now();
        let out = decode_lattice(&lat, &scorer, DecisionParams::default()).unwrap();
        eprintln!(
            "K={k} ntok={} narc={} elapsed={:.2}s transcript_words={} commits={} events={} peak_live={} peak_total={}",
            lat.tokens.len(),
            lat.arcs.len(),
            t0.elapsed().as_secs_f64(),
            out.transcript.len(),
            out.commits.len(),
            out.events_jsonl.lines().count(),
            out.peak_live,
            out.peak_total,
        );
    }
}
