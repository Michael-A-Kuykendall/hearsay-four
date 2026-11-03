//! Performance proof (PERF nlvs): a bridge-cadence synthetic workload over
//! both arms. The indexed arm must agree exactly with the reference and
//! complete a 10k-token lattice under a wall bound; the reference arm is
//! timed at moderate scale only (its cost is super-quadratic by design).

#[allow(dead_code)]
#[path = "support/reference_decision.rs"]
mod reference_decision;

use fabric::FactId;
use fabric::decision as new;
use reference_decision as old;
use std::collections::BTreeSet;
use std::time::Instant;

/// Bridge-cadence workload, stamped per arm: phase-1 mint+unary in frame
/// order, phase-2 attach+sequence+budgets+peaks per token, then greedy
/// commit regions. Returns (events, tilings, transcript) for comparison.
macro_rules! run_workload {
    ($m:ident, $n:expr, $params:expr) => {{
        let mut d = $m::DecisionFabric::new($params);
        let mut toks: Vec<FactId> = Vec::new();
        let mut prev: Option<FactId> = None;
        // Phase 1: chains with competitors (every 3rd token doubles).
        for i in 0..$n {
            let s = (i * 10) as u32;
            let reps = if i % 3 == 2 { 2 } else { 1 };
            for r in 0..reps {
                let label = format!("w{}", i % 11);
                let t = d
                    .mint_token(
                        label,
                        $m::TokenKind::Word,
                        s,
                        s + 10,
                    )
                    .expect("mint");
                let acoustic = 2.0 - r as f64 * 3.0 - (i % 5) as f64 * 0.2;
                d.add_unary(t, $m::SourceClass::Acoustic, acoustic, BTreeSet::new())
                    .expect("acoustic");
                if i % 2 == 0 {
                    d.add_unary(t, $m::SourceClass::Lexical, 0.5, BTreeSet::new())
                        .expect("lexical");
                }
                toks.push(t);
            }
        }
        // Phase 2: attach + chain sequence + budgets + peaks, in order.
        let mut peak = 0usize;
        for (k, t) in toks.iter().enumerate() {
            d.attach_context_for_new_token(*t, &|a, b| {
                if a.label == b.label { 1.0 } else { -0.5 }
            });
            match prev {
                None => {
                    d.add_boundary_sequence(*t, 0.3).expect("boundary");
                }
                Some(f) => {
                    let mut ps = BTreeSet::new();
                    ps.insert(f);
                    ps.insert(*t);
                    d.add_sequence(f, *t, 0.7, ps).expect("seq");
                }
            }
            prev = Some(*t);
            d.enforce_budgets();
            peak = peak.max(d.live_tokens().len() + d.dormant_tokens().len());
            let _ = k;
        }
        // Commit phase: greedy maximal tileable regions (bridge policy).
        let utter_end = ($n * 10) as u32;
        let mut cursor = 0u32;
        let mut transcript: Vec<String> = Vec::new();
        let mut tilings: Vec<(Vec<FactId>, u64)> = Vec::new();
        loop {
            let mut ends: Vec<u32> = toks
                .iter()
                .map(|t| d.token(*t).unwrap().end_frame)
                .filter(|e| *e > cursor)
                .collect();
            ends.sort_unstable();
            ends.dedup();
            let mut region: Option<(u32, u32)> = None;
            for e in ends.iter().rev() {
                if d.best_tilings((cursor, *e)).0.is_some() {
                    region = Some((cursor, *e));
                    break;
                }
            }
            match region {
                Some((s, e)) => {
                    d.enforce_budgets();
                    match d.commit_region((s, e), utter_end) {
                        $m::CommitOutcome::Committed { winner, .. } => {
                            tilings.push((
                                winner.tokens.clone(),
                                winner.score.to_bits(),
                            ));
                            for f in &winner.tokens {
                                transcript.push(d.token(*f).unwrap().label.clone());
                            }
                        }
                        _ => break,
                    }
                    cursor = e;
                    if cursor >= utter_end {
                        break;
                    }
                }
                None => {
                    match toks
                        .iter()
                        .map(|t| d.token(*t).unwrap().start_frame)
                        .filter(|s| *s > cursor)
                        .min()
                    {
                        Some(next) => cursor = next,
                        None => break,
                    }
                }
            }
        }
        (
            format!("{:?}", d.events()),
            format!("{tilings:?}"),
            transcript,
            peak,
        )
    }};
}

fn finite_budgets_n() -> new::DecisionParams {
    let mut p = new::DecisionParams::default();
    p.live_budget = 50;
    p.dormant_budget = 50;
    p.combined_budget = Some(120);
    p
}

fn finite_budgets_o() -> old::DecisionParams {
    let mut p = old::DecisionParams::default();
    p.live_budget = 50;
    p.dormant_budget = 50;
    p.combined_budget = Some(120);
    p
}

#[test]
fn reference_vs_indexed_agreement_and_speedup() {
    // Moderate scale, finite budgets (heap path): identical outputs, and
    // the indexed arm is dramatically faster.
    let n = 150usize;
    let t0 = Instant::now();
    let o = run_workload!(old, n, finite_budgets_o());
    let ref_wall = t0.elapsed();
    let t1 = Instant::now();
    let w = run_workload!(new, n, finite_budgets_n());
    let idx_wall = t1.elapsed();
    assert_eq!(w.0, o.0, "event logs match");
    assert_eq!(w.1, o.1, "tilings match");
    assert_eq!(w.2, o.2, "transcripts match");
    assert_eq!(w.3, o.3, "peak pool match");
    eprintln!(
        "perf n={n}: reference={:.3}s indexed={:.3}s speedup={:.1}x peak={}",
        ref_wall.as_secs_f64(),
        idx_wall.as_secs_f64(),
        ref_wall.as_secs_f64() / idx_wall.as_secs_f64().max(1e-9),
        w.3,
    );
}

/// Before-profile point (ignored from the gate: the reference arm is
/// super-quadratic by design). Run explicitly for before/after reports:
/// `cargo test --release -p fabric --test decision_perf before_profile -- --ignored --nocapture`.
#[test]
#[ignore]
fn before_profile_reference_400() {
    let n = 400usize;
    let t0 = Instant::now();
    let o = run_workload!(old, n, finite_budgets_o());
    let wall = t0.elapsed();
    eprintln!(
        "before-profile n={n}: reference={:.3}s peak={} transcript_words={}",
        wall.as_secs_f64(),
        o.3,
        o.2.len(),
    );
}

/// Bridge-faithful 10k workload (new arm): phase-1 mint+unary, phase-2
/// attach+sequence+budgets+peaks per token with item-8 interleaved commits
/// (maximal eligible end per step), then a final greedy drain. Returns
/// (wall, phase walls, peak open, transcript words, commits, stats).
fn run_10k_workload(n: usize) -> (f64, (f64, f64, f64), usize, usize, usize, new::FabricStatsSnapshot)
{
    let mut d = new::DecisionFabric::new(new::DecisionParams::default());
    let h = d.params().h;
    // Realistic dense lattice: `words` word slots with `comp` competitors
    // each (n = words * comp tokens, ~words winners — like a real K-best
    // lattice, where winners are O(words) and competitors dominate).
    let words = 100usize;
    let comp = n / words;
    assert!(words * comp == n, "n must split into words*comp");
    let mut toks: Vec<FactId> = Vec::new();
    let t_mint = Instant::now();
    for w in 0..words {
        let s = (w * 100) as u32;
        for c in 0..comp {
            let label = format!("w{}", (w + c) % 11);
            let t = d
                .mint_token(label, new::TokenKind::Word, s, s + 100)
                .expect("mint");
            let acoustic = 2.0 - c as f64 * 0.05 - (w % 5) as f64 * 0.1;
            d.add_unary(t, new::SourceClass::Acoustic, acoustic, BTreeSet::new())
                .expect("acoustic");
            if (w + c) % 2 == 0 {
                d.add_unary(t, new::SourceClass::Lexical, 0.5, BTreeSet::new())
                    .expect("lexical");
            }
            toks.push(t);
        }
    }
    let mut ends_asc: Vec<u32> = toks
        .iter()
        .map(|t| d.token(*t).unwrap().end_frame)
        .collect();
    ends_asc.sort_unstable();
    ends_asc.dedup();
    let utter_end = (words * 100) as u32;
    let t_mint = t_mint.elapsed().as_secs_f64();
    // Phase 2 with interleaved commits.
    let mut cursor = 0u32;
    let mut frontier = 0u32;
    let mut transcript: Vec<String> = Vec::new();
    let mut commits = 0usize;
    let mut peak_open = 0usize;
    let mut prev: Option<FactId> = None;
    let t_intake = Instant::now();
    for t in &toks {
        d.attach_context_for_new_token(*t, &|a, b| {
            if a.label == b.label { 1.0 } else { -0.5 }
        });
        match prev {
            None => {
                d.add_boundary_sequence(*t, 0.3).expect("boundary");
            }
            Some(f) => {
                let mut ps = BTreeSet::new();
                ps.insert(f);
                ps.insert(*t);
                d.add_sequence(f, *t, 0.7, ps).expect("seq");
            }
        }
        prev = Some(*t);
        d.enforce_budgets();
        let (nl, nd) = d.live_dormant_counts();
        peak_open = peak_open.max(nl + nd).max(d.open_set_len());
        frontier = frontier.max(d.token(*t).unwrap().end_frame);
        loop {
            let end = ends_asc
                .iter()
                .rev()
                .filter(|e| **e > cursor)
                .find(|e| **e <= frontier.saturating_sub(h))
                .copied();
            let Some(e) = end else { break };
            if d.best_tilings((cursor, e)).0.is_none() {
                break;
            }
            d.enforce_budgets();
            match d.commit_region((cursor, e), frontier) {
                new::CommitOutcome::Committed { winner, .. } => {
                    for f in &winner.tokens {
                        transcript.push(d.token(*f).unwrap().label.clone());
                    }
                    commits += 1;
                    cursor = e;
                }
                _ => break,
            }
        }
    }
    let t_intake = t_intake.elapsed().as_secs_f64();
    // Final greedy drain from the advanced cursor.
    let t_commit = Instant::now();
    loop {
        let mut region: Option<(u32, u32)> = None;
        for e in ends_asc.iter().rev().filter(|e| **e > cursor) {
            if d.best_tilings((cursor, *e)).0.is_some() {
                region = Some((cursor, *e));
                break;
            }
        }
        match region {
            Some((s, e)) => {
                d.enforce_budgets();
                match d.commit_region((s, e), utter_end) {
                    new::CommitOutcome::Committed { winner, .. } => {
                        for f in &winner.tokens {
                            transcript.push(d.token(*f).unwrap().label.clone());
                        }
                        commits += 1;
                    }
                    _ => break,
                }
                cursor = e;
                if cursor >= utter_end {
                    break;
                }
            }
            None => {
                match toks
                    .iter()
                    .map(|t| d.token(*t).unwrap().start_frame)
                    .filter(|s| *s > cursor)
                    .min()
                {
                    Some(next) => cursor = next,
                    None => break,
                }
            }
        }
    }
    let t_commit = t_commit.elapsed().as_secs_f64();
    let stats = d.stats_snapshot();
    let wall = t_mint + t_intake + t_commit;
    (wall, (t_mint, t_intake, t_commit), peak_open, transcript.len(), commits, stats)
}

#[test]
fn indexed_10k_synthetic_completes_under_bound() {
    // The bead's perf gate at synthetic scale: 10k-token lattice through
    // the indexed arm with the bridge cadence (interleaved commits),
    // default (gate) params, under a wall bound.
    let (wall, (t_mint, t_intake, t_commit), peak, words, commits, st) =
        run_10k_workload(10_000);
    assert!(words > 0, "10k lattice yields a transcript");
    assert!(commits > 0, "10k lattice commits");
    eprintln!(
        "perf 10k: wall={wall:.3}s (mint={t_mint:.3}s intake={t_intake:.3}s commit={t_commit:.3}s) \
         peak_open={peak} words={words} commits={commits} \
         liveness={} unary={} seqpair={} pushed={} popped={} evicted={} pruned={} defeated={} committed={}",
        st.liveness_calls,
        st.unary_lookups,
        st.seq_pair_lookups,
        st.heap_pushes,
        st.heap_pops,
        st.evicted,
        st.pruned,
        st.defeated,
        st.committed,
    );
    assert!(wall < 120.0, "10k synthetic completed in {wall:.3}s (bound 120s)");
}

