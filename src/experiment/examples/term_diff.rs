//! Term-by-term S(T) table: fabric winner vs acoustic 1-best, one utt.
//! usage: term_diff --utt <id> --lat-dir <dir> --lm <lm.json>
//! SANITY config: arm C, infinite budgets, w_ac 1, w_seq 6, theta -1e15.
//! Per-token terms use the same scorer fns the bridge feeds the fabric;
//! seqmax = max over incoming arcs AND the boundary pair (assumption:
//! boundary competes in active_sequence_max; the totals row validates it
//! empirically against the committed winner).
use std::collections::HashMap;

fn take_value(args: &[String], i: &mut usize, name: &str) -> String {
    *i += 1;
    args.get(*i)
        .unwrap_or_else(|| panic!("{name} needs a value"))
        .clone()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mut utt, mut lat_dir, mut lm_path) =
        (String::new(), String::new(), String::new());
    let mut i = 0;
    while i + 1 < args.len() {
        i += 1;
        match args[i].as_str() {
            "--utt" => utt = take_value(&args, &mut i, "--utt"),
            "--lat-dir" => lat_dir = take_value(&args, &mut i, "--lat-dir"),
            "--lm" => lm_path = take_value(&args, &mut i, "--lm"),
            a => panic!("unknown arg {a}"),
        }
    }
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(&lat_dir).join(format!("{utt}.lat.jsonl")),
    )
    .expect("read lattice");
    let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
    let lm = lm::persist::load_interpolated(std::path::Path::new(&lm_path)).expect("load lm");
    let scorer = hearsay::InterpolatedScorer::new(&lm);
    let params = fabric::decision::DecisionParams {
        w_acoustic: 1.0,
        w_sequence: 6.0,
        theta_live: -1e15,
        live_budget: usize::MAX,
        dormant_budget: usize::MAX,
        combined_budget: None,
        h: 10,
        h_max: 20,
        delta: 0.0,
        arm: fabric::decision::Arm::C,
        ..Default::default()
    };
    let out = hearsay::decode_lattice(&lat, &scorer, params).expect("decode");
    let mut winner_ids: Vec<u64> = Vec::new();
    for line in out.events_jsonl.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("event json");
        if v.get("event").and_then(|k| k.as_str()) == Some("committed") {
            if let Some(ws) = v.get("winner").and_then(|w| w.as_array()) {
                for w in ws {
                    if let Some(id) = w.get("id").and_then(|x| x.as_u64()) {
                        winner_ids.push(id);
                    }
                }
            }
        }
    }
    let best_ids = a2::oracle::acoustic_best_path_ids(&lat);
    let by_id: HashMap<u64, &lattice::Token> =
        lat.tokens.iter().map(|t| (t.id, t)).collect();
    let incoming: HashMap<u64, Vec<u64>> = {
        let mut m: HashMap<u64, Vec<u64>> = HashMap::new();
        for a in &lat.arcs {
            m.entry(a.to).or_default().push(a.from);
        }
        m
    };
    println!("fabric hyp: {}", out.transcript.join(" "));
    for (tag, ids) in [("FABRIC", &winner_ids), ("1BEST", &best_ids)] {
        println!("--- {tag} ({} tokens) ---", ids.len());
        println!("word\tacous\tseqmax\tlex\twsum");
        let (mut ta, mut ts, mut tl) = (0.0, 0.0, 0.0);
        for id in ids {
            let t = by_id.get(id).expect("path id in lattice");
            let ac = t.acoustic_llr;
            let mut sq = hearsay::SequenceScorer::sequence_llr(&scorer, None, &t.word);
            if let Some(preds) = incoming.get(id) {
                for p in preds {
                    if let Some(pt) = by_id.get(p) {
                        let v = hearsay::SequenceScorer::sequence_llr(
                            &scorer,
                            Some(&pt.word),
                            &t.word,
                        );
                        if v > sq {
                            sq = v;
                        }
                    }
                }
            }
            let lx = if t.word == "SIL" {
                0.0
            } else {
                scorer.lexical_llr(&t.word)
            };
            ta += ac;
            ts += sq;
            tl += lx;
            println!(
                "{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
                t.word,
                ac,
                6.0 * sq,
                6.0 * lx,
                ac + 6.0 * sq + 6.0 * lx
            );
        }
        println!(
            "TOTAL\t{:.3}\t{:.3}\t{:.3}\tS={:.3}",
            ta,
            6.0 * ts,
            6.0 * tl,
            ta + 6.0 * ts + 6.0 * tl
        );
    }
}
