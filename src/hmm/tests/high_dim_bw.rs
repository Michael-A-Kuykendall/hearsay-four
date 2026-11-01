//! Regression gate (hearsay-four-bwf--baum-welch-underflow-fix): BW must learn
//! at both dim 2 (R2a regime) and dim 39 (real-feature regime). Compares
//! forced-align train LL before/after one BW iteration from flat start; the
//! dim-39 case caught linear-domain underflow (forward LL collapsed to 0.0).
use hmm::{HmmSet, TrainConfig};

fn synth(dim: usize, n_utt: usize, frames_per_phone: usize) -> Vec<(Vec<String>, Vec<Vec<f64>>)> {
    // Deterministic zigzag "noise": no RNG, phones well separated by construction.
    let mut data = Vec::new();
    for u in 0..n_utt {
        let mut seq = vec!["SIL".to_string()];
        let mut obs = Vec::new();
        for (pi, base) in [0.0f64, 5.0f64].iter().enumerate() {
            let ph = if pi == 0 { "AA" } else { "BB" };
            seq.push(ph.to_string());
            for t in 0..frames_per_phone {
                let mut f = Vec::with_capacity(dim);
                for d in 0..dim {
                    let wob = (((u * 131 + t * 17 + d * 7 + pi * 101) % 13) as f64 - 6.0) / 6.0;
                    f.push(base + wob);
                }
                obs.push(f);
            }
        }
        seq.push("SIL".to_string());
        // SIL frames near zero-mean filler
        for t in 0..6 {
            let mut f = Vec::with_capacity(dim);
            for d in 0..dim {
                f.push((((u * 131 + t * 17 + d * 7) % 13) as f64 - 6.0) / 6.0 * 0.5);
            }
            obs.insert(t.min(obs.len()), f);
        }
        data.push((seq, obs));
    }
    data
}

fn viterbi_ll(m: &HmmSet, data: &[(Vec<String>, Vec<Vec<f64>>)]) -> f64 {
    data.iter()
        .map(|(s, o)| {
            m.forced_align(s, o)
                .map(|(_, ll)| ll)
                .unwrap_or(f64::NEG_INFINITY)
        })
        .sum()
}

fn run_case(dim: usize) {
    let data = synth(dim, 20, 30);
    let obs_only: Vec<Vec<Vec<f64>>> = data.iter().map(|(_, o)| o.clone()).collect();
    let mut m = HmmSet::flat_start(&["AA", "BB"], &obs_only, 0.01).unwrap();
    let ll0 = viterbi_ll(&m, &data);
    let cfg = TrainConfig {
        bw_iterations: 1,
        ..Default::default()
    };
    let with_ids: Vec<(String, Vec<String>, Vec<Vec<f64>>)> = data
        .iter()
        .enumerate()
        .map(|(i, (s, o))| (format!("u{i}"), s.clone(), o.clone()))
        .collect();
    m.baum_welch_ids(&with_ids, &cfg).unwrap();
    let ll1 = viterbi_ll(&m, &data);
    eprintln!(
        "DIM {dim}: viterbi train LL iter0={ll0:.1} iter1={ll1:.1} delta={:.1}",
        ll1 - ll0
    );
    assert!(
        ll1 > ll0,
        "BW must improve train LL in one iteration (dim {dim}): {ll0:.1} -> {ll1:.1}"
    );
    // Also show the BW forward LL the trainer itself reports.
    let mut m2 = HmmSet::flat_start(&["AA", "BB"], &obs_only, 0.01).unwrap();
    let fwd0 = m2.baum_welch_ids(&with_ids, &cfg).unwrap();
    let fwd1 = m2.baum_welch_ids(&with_ids, &cfg).unwrap();
    eprintln!(
        "DIM {dim}: forward train LL bw1={fwd0:.1} bw2={fwd1:.1} delta={:.1}",
        fwd1 - fwd0
    );
}

#[test]
fn probe_dim2_learns() {
    run_case(2);
}

#[test]
fn probe_dim39_learns() {
    run_case(39);
}

fn diagnose(dim: usize) {
    let data = synth(dim, 20, 30);
    let obs_only: Vec<Vec<Vec<f64>>> = data.iter().map(|(_, o)| o.clone()).collect();
    let mut m = HmmSet::flat_start(&["AA", "BB"], &obs_only, 0.01).unwrap();
    let cfg = TrainConfig {
        bw_iterations: 1,
        ..Default::default()
    };
    let with_ids: Vec<(String, Vec<String>, Vec<Vec<f64>>)> = data
        .iter()
        .enumerate()
        .map(|(i, (s, o))| (format!("u{i}"), s.clone(), o.clone()))
        .collect();
    m.baum_welch_ids(&with_ids, &cfg).unwrap();
    // Inspect what the M-step actually wrote.
    let aa0 = &m.phones["AA"].emissions[0].components[0].gaussian;
    eprintln!(
        "DIM {dim}: AA/s0 mean[0..3]={:.2?} var[0..3]={:.4?}",
        &aa0.mean[..3.min(dim)],
        &aa0.var[..3.min(dim)]
    );
    // Best-state log_prob per frame + underflow census.
    let mut worst_best = f64::INFINITY;
    let mut zero_rows = 0usize;
    let mut frames = 0usize;
    for (_, obs) in data.iter().take(4) {
        for o in obs {
            frames += 1;
            let mut best = f64::NEG_INFINITY;
            let mut allzero = true;
            for h in m.phones.values() {
                if h.tied_to.is_some() {
                    continue;
                }
                for mix in &h.emissions {
                    let lp = mix.log_prob(o);
                    if lp > best {
                        best = lp;
                    }
                    if lp > -745.0 {
                        allzero = false;
                    }
                }
            }
            if best < worst_best {
                worst_best = best;
            }
            if allzero {
                zero_rows += 1;
            }
        }
    }
    eprintln!(
        "DIM {dim}: frames={frames} allzero_rows={zero_rows} worst_best_logprob={worst_best:.1}"
    );
}

#[test]
fn probe_diagnose_both() {
    diagnose(2);
    diagnose(39);
}
#[test]
fn parallel_estep_is_deterministic() {
    // Ordered merge: the parallel E-step must be bit-identical across runs
    // (LL and full model), independent of thread scheduling.
    let data = synth(13, 20, 30);
    let obs_only: Vec<Vec<Vec<f64>>> = data.iter().map(|(_, o)| o.clone()).collect();
    let with_ids: Vec<(String, Vec<String>, Vec<Vec<f64>>)> = data
        .iter()
        .enumerate()
        .map(|(i, (s, o))| (format!("u{i}"), s.clone(), o.clone()))
        .collect();
    let cfg = TrainConfig {
        bw_iterations: 1,
        ..Default::default()
    };
    let mut a = HmmSet::flat_start(&["AA", "BB"], &obs_only, 0.01).unwrap();
    let mut b = a.clone();
    let ll_a1 = a.baum_welch_ids(&with_ids, &cfg).unwrap();
    let ll_b1 = b.baum_welch_ids(&with_ids, &cfg).unwrap();
    assert!(
        ll_a1 == ll_b1,
        "iter1 LL must be bit-identical: {ll_a1} vs {ll_b1}"
    );
    assert!(a == b, "iter1 models must be identical");
    let ll_a2 = a.baum_welch_ids(&with_ids, &cfg).unwrap();
    let ll_b2 = b.baum_welch_ids(&with_ids, &cfg).unwrap();
    assert!(
        ll_a2 == ll_b2,
        "iter2 LL must be bit-identical: {ll_a2} vs {ll_b2}"
    );
    assert!(a == b, "iter2 models must be identical");
}
#[test]
fn estep_chunking_matches_single_chunk_within_tolerance() {
    // d7qe: bounded-memory chunked merge must not change numerics beyond
    // 1e-9 (many small chunks vs one single chunk on the same fixture).
    let data = synth(13, 20, 10);
    let obs_only: Vec<Vec<Vec<f64>>> = data.iter().map(|(_, o)| o.clone()).collect();
    let with_ids: Vec<(String, Vec<String>, Vec<Vec<f64>>)> = data
        .iter()
        .enumerate()
        .map(|(i, (s, o))| (format!("u{i}"), s.clone(), o.clone()))
        .collect();
    let tiny = TrainConfig {
        bw_iterations: 1,
        estep_chunk: 3,
        ..Default::default()
    };
    let huge = TrainConfig {
        bw_iterations: 1,
        estep_chunk: usize::MAX,
        ..Default::default()
    };
    let mut a = HmmSet::flat_start(&["AA", "BB"], &obs_only, 0.01).unwrap();
    let mut b = a.clone();
    let ll_a = a.baum_welch_ids(&with_ids, &tiny).unwrap();
    let ll_b = b.baum_welch_ids(&with_ids, &huge).unwrap();
    assert!(
        ll_a == ll_b,
        "chunked LL must be bit-identical: {ll_a} vs {ll_b}"
    );
    for ((name_a, ha), (name_b, hb)) in a.phones.iter().zip(b.phones.iter()) {
        assert_eq!(name_a, name_b, "phone inventory must match");
        for s in 0..ha.n_states {
            for (ca, cb) in ha.emissions[s]
                .components
                .iter()
                .zip(hb.emissions[s].components.iter())
            {
                assert!(
                    (ca.weight - cb.weight).abs() < 1e-9,
                    "{name_a} s{s} weight: {} vs {}",
                    ca.weight,
                    cb.weight
                );
                for d in 0..ca.gaussian.mean.len() {
                    assert!(
                        (ca.gaussian.mean[d] - cb.gaussian.mean[d]).abs() < 1e-9,
                        "{name_a} s{s} mean[{d}]: {} vs {}",
                        ca.gaussian.mean[d],
                        cb.gaussian.mean[d]
                    );
                    assert!(
                        (ca.gaussian.var[d] - cb.gaussian.var[d]).abs() < 1e-9,
                        "{name_a} s{s} var[{d}]: {} vs {}",
                        ca.gaussian.var[d],
                        cb.gaussian.var[d]
                    );
                }
            }
        }
    }
}
