//! CX1 cage (bead nlvs rewrite item 7): growth-exponent gate over
//! bridge-like synthetic workloads at {1k, 10k, 100k} tokens.
//!
//! Workload per token (mirrors the bridge phase-2 feed): mint, two
//! premise-free unary rows (acoustic + lexical), up to three `{from,to}`
//! -premised sequence edges to recent predecessors, and the §9 context
//! hook with a constant LM. Periodic `live_dormant_counts` plus a region
//! commit every 500 tokens (fixed region size, so commit work is linear
//! in N; every scale commits, so per-path fits stay honest). Budgets are
//! infinite (default params): this cage measures the
//! saturation engine's scaling, not the pre-existing per-call budget-sort
//! (see the bead note: that sort is a separate, bit-identical follow-up).
//!
//! Gate: `cargo test -p fabric cx1` (debug, k ≤ 1.5) and the release
//! profile (k ≤ 1.4).

use fabric::decision::{DecisionFabric, DecisionParams, SourceClass, TokenKind};
use fabric::FactId;
use std::collections::BTreeSet;
use std::time::Instant;

/// Least-squares growth exponent: time = c·n^k ⇒ k = log-log slope.
/// Same estimator shape as the mikedb-core cage.
fn growth_exponent(times: &[(u64, f64)]) -> f64 {
    let n = times.len() as f64;
    let sum_ln: f64 = times.iter().map(|(r, _)| (*r as f64).ln()).sum();
    let sum_lnt: f64 = times.iter().map(|(_, t)| t.ln()).sum();
    let sum_ln2: f64 = times.iter().map(|(r, _)| (*r as f64).ln().powi(2)).sum();
    let sum_ln_lnt: f64 = times
        .iter()
        .map(|(r, t)| (*r as f64).ln() * t.ln())
        .sum();
    (n * sum_ln_lnt - sum_ln * sum_lnt) / (n * sum_ln2 - sum_ln * sum_ln)
}

/// One bridge-like feed of `n` tokens. Returns (wall seconds, peak open,
/// stats snapshot values, per-phase seconds [ingest, attach, query,
/// enforce, commit]) for the report lines and per-path k fits.
fn feed_scale(n: u64) -> (f64, usize, [u64; 9], [f64; 5]) {
    let params = DecisionParams::default();
    let h = params.h;
    let mut d = DecisionFabric::new(params);
    let mut toks: Vec<FactId> = Vec::with_capacity(n as usize);
    let mut cursor_idx: u64 = 0;
    let (mut t_ingest, mut t_attach, mut t_query, mut t_enforce, mut t_commit) =
        (0.0, 0.0, 0.0, 0.0, 0.0);
    let t0 = Instant::now();
    for i in 0..n {
        let s = i as u32;
        let p0 = Instant::now();
        let t = d
            .mint_token(format!("w{i}"), TokenKind::Word, s, s + 5)
            .unwrap();
        toks.push(t);
        // Mixed population: every 5th token strong (live), the rest weak
        // (dormant), so the context hook and revival path fire.
        let live = i % 5 == 0;
        let (a, l) = if live { (2.0, 1.0) } else { (-1.0, -0.5) };
        d.add_unary(t, SourceClass::Acoustic, a, BTreeSet::new())
            .unwrap();
        d.add_unary(t, SourceClass::Lexical, l, BTreeSet::new())
            .unwrap();
        // Sequence edges to up to three predecessors, premised on both
        // endpoints exactly like the bridge.
        for back in 1..=3u64 {
            if back > i {
                continue;
            }
            let f = toks[(i - back) as usize];
            let mut premises = BTreeSet::new();
            premises.insert(f);
            premises.insert(t);
            d.add_sequence(f, t, 0.3, premises).unwrap();
        }
        if i == 0 {
            d.add_boundary_sequence(t, 0.1).unwrap();
        }
        t_ingest += p0.elapsed().as_secs_f64();
        let p1 = Instant::now();
        d.attach_context_for_new_token(t, &|_, _| 0.5);
        t_attach += p1.elapsed().as_secs_f64();
        // Counts every 10 tokens (bridge counts per intake; this keeps the
        // phase measurable at every scale). Enforcement stays sparse: with
        // infinite budgets it early-returns, and the finite-budget sort is
        // a separate, pre-existing cost (see the bead note).
        if (i + 1) % 10 == 0 {
            let p2 = Instant::now();
            let _ = d.live_dormant_counts();
            t_query += p2.elapsed().as_secs_f64();
        }
        if (i + 1) % 200 == 0 {
            let p3 = Instant::now();
            d.enforce_budgets();
            t_enforce += p3.elapsed().as_secs_f64();
        }
        if (i + 1) % 500 == 0 {
            // Region end aligns with a live token's end frame (region
            // starts stay multiples of 5, all live), so a tiling exists
            // and the commit fires every cadence step, at every scale.
            let s = cursor_idx as u32;
            let e = (cursor_idx + 495) as u32 + 5;
            let frontier = e + h + 5;
            let p4 = Instant::now();
            let _ = d.commit_region((s, e), frontier);
            t_commit += p4.elapsed().as_secs_f64();
            cursor_idx += 500;
        }
    }
    let wall = t0.elapsed().as_secs_f64();
    let st = d.stats_snapshot();
    let peak = d.peak_open_set();
    (
        wall,
        peak,
        [
            st.liveness_calls,
            st.unary_lookups,
            st.seq_pair_lookups,
            st.heap_pushes,
            st.heap_pops,
            st.evicted,
            st.pruned,
            st.defeated,
            st.committed,
        ],
        [t_ingest, t_attach, t_query, t_enforce, t_commit],
    )
}

#[test]
fn cx1_growth_exponent_within_ceiling() {
    let ceiling = if cfg!(debug_assertions) { 1.5 } else { 1.4 };
    let mut points: Vec<(u64, f64)> = Vec::new();
    let mut phase_points: [Vec<(u64, f64)>; 5] = Default::default();
    for n in [1_000u64, 10_000, 100_000] {
        let (wall, peak, st, phases) = feed_scale(n);
        assert!(
            wall > 0.0,
            "scale {n}: zero wall time voids the log-log fit"
        );
        points.push((n, wall));
        println!(
            "CX1 scale={n} wall_s={wall:.3} peak_open={peak} \
             liv={} unary={} seqpair={} hpush={} hpop={} evict={} pruned={} def={} cmt={}",
            st[0], st[1], st[2], st[3], st[4], st[5], st[6], st[7], st[8]
        );
        println!(
            "CX1 phases scale={n} ingest={:.3} attach={:.3} query={:.3} enforce={:.3} commit={:.3}",
            phases[0], phases[1], phases[2], phases[3], phases[4]
        );
        for (slot, ph) in phase_points.iter_mut().zip(phases) {
            // Clamp sub-microsecond phases so the log fit stays finite;
            // a phase that fast is flat (k ~= 0) by inspection.
            slot.push((n, ph.max(1e-6)));
        }
    }
    let k = growth_exponent(&points);
    println!("CX1 k={k:.4} ceiling={ceiling} points={points:?}");
    assert!(
        k <= ceiling,
        "CX1 growth exponent k={k:.4} exceeds ceiling {ceiling}"
    );
    // Per-path gates with a materiality void (cage methodology): a path
    // under 1% of the 100k wall cannot drive scaling, and fitting k on
    // microsecond noise is statistically void. Void paths still print
    // their k (committed evidence); only material paths assert.
    let wall_100k = points[2].1;
    for (name, slot) in ["ingest", "attach", "query", "enforce", "commit"]
        .into_iter()
        .zip(phase_points.iter())
    {
        let pk = growth_exponent(slot);
        let material = slot[2].1 >= 0.01 * wall_100k;
        println!(
            "CX1 path={name} k={pk:.4} ceiling={ceiling} {} points={slot:?}",
            if material { "gated" } else { "VOID-immaterial" }
        );
        assert!(
            !material || pk <= ceiling,
            "CX1 path {name}: k={pk:.4} exceeds ceiling {ceiling}"
        );
    }
}
