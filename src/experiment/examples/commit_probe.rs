//! Commit probe: replicate the bridge intake through the PUBLIC fabric
//! API for one lattice, then print winner/runner scores, margins, and
//! unary-sum samples. Diagnoses why real-lattice commit margins are
//! uniformly 0.0 without touching fabric/hearsay sources.
//!
//! usage: commit_probe --lat <file> --lm <json> [--theta 0] [--wseq 6]

use fabric::decision::{CommitOutcome, DecisionFabric, DecisionParams, SourceClass, TokenKind};
use hearsay::{Arm as FabricArm, InterpolatedScorer, SequenceScorer};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn take_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lat_path: Option<PathBuf> = None;
    let mut lm_path: Option<PathBuf> = None;
    let mut theta = 0.0f64;
    let mut wseq = 6.0f64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lat" => lat_path = Some(PathBuf::from(take_value(&args, &mut i, "--lat").unwrap())),
            "--lm" => lm_path = Some(PathBuf::from(take_value(&args, &mut i, "--lm").unwrap())),
            "--theta" => {
                theta = take_value(&args, &mut i, "--theta")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            "--wseq" => {
                wseq = take_value(&args, &mut i, "--wseq")
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            other => panic!("unknown argument {other:?}"),
        }
        i += 1;
    }
    let text = std::fs::read_to_string(lat_path.expect("--lat")).expect("read lattice");
    let lat: lattice::Lattice = lattice::read_lattice(&text).expect("parse lattice");
    let lm = lm::persist::load_interpolated(&lm_path.expect("--lm")).expect("load lm");
    let scorer = InterpolatedScorer::new(&lm);

    let params = DecisionParams {
        w_acoustic: 1.0,
        w_sequence: wseq,
        theta_live: theta,
        live_budget: usize::MAX,
        dormant_budget: usize::MAX,
        combined_budget: None,
        h: 10,
        h_max: 20,
        delta: 0.0,
        j: 3,
        arm: FabricArm::C,
        ..DecisionParams::default()
    };
    let mut fabric = DecisionFabric::new(params);
    let mut by_id: BTreeMap<u64, usize> = BTreeMap::new();
    for (ix, t) in lat.tokens.iter().enumerate() {
        by_id.insert(t.id, ix);
    }
    let mut order: Vec<usize> = (0..lat.tokens.len()).collect();
    order.sort_by(|&a, &b| {
        let (ta, tb) = (&lat.tokens[a], &lat.tokens[b]);
        (ta.start, ta.end, ta.id).cmp(&(tb.start, tb.end, tb.id))
    });
    let mut incoming: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for a in &lat.arcs {
        incoming.entry(a.to).or_default().push(a.from);
    }
    let mut fact_of: BTreeMap<u64, fabric::FactId> = BTreeMap::new();
    let mut meta_of: BTreeMap<fabric::FactId, (u64, String, u32, u32)> = BTreeMap::new();
    // Duplicate-token census: lattice ids sharing (word, start, end).
    let mut spans: BTreeMap<(String, u32, u32), Vec<u64>> = BTreeMap::new();
    for t in &lat.tokens {
        spans
            .entry((t.word.clone(), t.start, t.end))
            .or_default()
            .push(t.id);
    }
    let dup_groups = spans.values().filter(|v| v.len() > 1).count();
    let dup_toks: usize = spans
        .values()
        .filter(|v| v.len() > 1)
        .map(|v| v.len())
        .sum();
    println!(
        "lattice: {} tokens, {} distinct (word,start,end), {dup_groups} dup groups holding {dup_toks} tokens",
        lat.tokens.len(),
        spans.len(),
    );
    for &ix in &order {
        let t = &lat.tokens[ix];
        let fact = fabric
            .mint_token(t.word.clone(), TokenKind::Word, t.start, t.end)
            .expect("mint");
        fabric
            .add_unary(fact, SourceClass::Acoustic, t.acoustic_llr, BTreeSet::new())
            .expect("unary");
        for (class, llr) in [
            (SourceClass::Lexical, t.lexical_llr),
            (SourceClass::Speaker, t.speaker_llr),
            (SourceClass::Verify, t.verify_llr),
        ] {
            if let Some(llr) = llr {
                fabric
                    .add_unary(fact, class, llr, BTreeSet::new())
                    .expect("unary");
            }
        }
        fact_of.insert(t.id, fact);
        meta_of.insert(fact, (t.id, t.word.clone(), t.start, t.end));
        fabric.attach_context_for_new_token(fact, &|from_tok, to_tok| {
            scorer.sequence_llr(Some(from_tok.label.as_str()), to_tok.label.as_str())
        });
        match incoming.get(&t.id) {
            None => {
                let llr = scorer.sequence_llr(None, &t.word);
                fabric.add_boundary_sequence(fact, llr).expect("boundary");
            }
            Some(froms) => {
                for f in froms {
                    let from_fact = fact_of[f];
                    let from_word = &lat.tokens[by_id[f]].word;
                    let llr = scorer.sequence_llr(Some(from_word), &t.word);
                    let mut premises = BTreeSet::new();
                    premises.insert(from_fact);
                    premises.insert(fact);
                    fabric
                        .add_sequence(from_fact, fact, llr, premises)
                        .expect("pair");
                }
            }
        }
        fabric.enforce_budgets();
    }
    println!("intake done: {} tokens", lat.tokens.len());
    // Sample fabric-side liveness + unary sums (tests evidence visibility).
    let mut n = 0;
    let (mut liv_min, mut liv_max, mut usum_min, mut usum_max) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for t in lat.tokens.iter().step_by(lat.tokens.len() / 200 + 1) {
        let f = fact_of[&t.id];
        let (l, u) = (fabric.liveness(f), fabric.unary_sum(f));
        liv_min = liv_min.min(l);
        liv_max = liv_max.max(l);
        usum_min = usum_min.min(u);
        usum_max = usum_max.max(u);
        if n < 5 {
            println!(
                "  tok {} {:?} acoustic={:.2} liveness={:.2} usum={:.2}",
                t.id, t.word, t.acoustic_llr, l, u
            );
        }
        n += 1;
    }
    println!(
        "sampled {n}: liveness [{liv_min:.2}, {liv_max:.2}] usum [{usum_min:.2}, {usum_max:.2}]"
    );
    let s = lat.tokens.iter().map(|t| t.start).min().unwrap();
    let e = lat.tokens.iter().map(|t| t.end).max().unwrap();
    let (winner, runner) = fabric.best_tilings((s, e));
    match (&winner, &runner) {
        (Some(w), Some(r)) => {
            let show = |toks: &[fabric::FactId]| {
                toks.iter()
                    .map(|f| {
                        let (id, word, s, e) = &meta_of[f];
                        format!("{word}#{id}[{s},{e}]")
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!(
                "winner score={:.4} ntok={} {}",
                w.score,
                w.tokens.len(),
                show(&w.tokens)
            );
            println!(
                "runner score={:.4} ntok={} {}",
                r.score,
                r.tokens.len(),
                show(&r.tokens)
            );
            println!("margin = {:.6}", w.score - r.score);
        }
        (Some(w), None) => println!("unopposed winner score={:.4}", w.score),
        _ => println!("NO TILING"),
    }
    match fabric.commit_region((s, e), e) {
        CommitOutcome::Committed {
            margin, defeated, ..
        } => {
            println!(
                "commit_region: margin={margin:?} defeated={}",
                defeated.len()
            )
        }
        CommitOutcome::MarginTooSmall { margin } => {
            println!("commit_region: MarginTooSmall {margin}")
        }
        CommitOutcome::NotEligible => println!("commit_region: NotEligible"),
        CommitOutcome::NoTiling => println!("commit_region: NoTiling"),
    }
}
