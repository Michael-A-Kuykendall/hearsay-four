//! viterbi_reestimate tests (CMVN retrain init): means track aligned data,
// weights/floors hold, runs are deterministic.

use hmm::{HmmSet, UttData};

const DIM: usize = 4;

/// Deterministic pseudo-frames centered near `shift`.
fn frames(n: usize, shift: f64) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| {
            (0..DIM)
                .map(|d| (((i * 7 + d * 13) % 11) as f64 - 5.0) * 0.4 + shift)
                .collect()
        })
        .collect()
}

fn seq() -> Vec<String> {
    vec!["AA".to_string(), "IY".to_string()]
}

fn model_on(shift: f64) -> HmmSet {
    let utts = vec![frames(30, shift), frames(30, shift)];
    let mut m = HmmSet::flat_start(&["AA", "IY"], &utts, 0.01).expect("flat");
    m.split_mixtures_to(2);
    m
}

fn data_on(shift: f64) -> Vec<UttData> {
    vec![
        ("u0".to_string(), seq(), frames(30, shift)),
        ("u1".to_string(), seq(), frames(30, shift)),
    ]
}

/// Aligned-frame mean for one composite state (test oracle).
fn aligned_mean(data: &[UttData], paths: &[Vec<usize>], want: usize) -> Vec<f64> {
    let mut sum = vec![0.0; DIM];
    let mut n = 0usize;
    for (u, path) in paths.iter().enumerate() {
        for (t, &s) in path.iter().enumerate() {
            if s == want {
                for d in 0..DIM {
                    sum[d] += data[u].2[t][d];
                }
                n += 1;
            }
        }
    }
    assert!(n > 0, "state {want} must own frames");
    sum.iter().map(|v| v / n as f64).collect()
}

fn dist(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

#[test]
fn reestimate_tracks_aligned_data() {
    let mut m = model_on(0.0);
    let data = data_on(5.0);
    let paths: Vec<Vec<usize>> = data
        .iter()
        .map(|(_, s, o)| m.forced_align(s, o).expect("align").0)
        .collect();
    // Composite state 0 == first state of AA; mixture mean must move toward it.
    let target = aligned_mean(&data, &paths, 0);
    let before: f64 = m.phones["AA"].emissions[0]
        .components
        .iter()
        .map(|c| dist(&c.gaussian.mean, &target))
        .sum();
    let used = m.viterbi_reestimate(&data, &paths, 0.01).expect("reest");
    assert_eq!(used, 60);
    let after: f64 = m.phones["AA"].emissions[0]
        .components
        .iter()
        .map(|c| dist(&c.gaussian.mean, &target))
        .sum();
    assert!(
        after < before,
        "means must move toward aligned data: {before} -> {after}"
    );
}

#[test]
fn reestimate_weights_and_floors_hold() {
    let mut m = model_on(0.0);
    let data = data_on(5.0);
    let paths: Vec<Vec<usize>> = data
        .iter()
        .map(|(_, s, o)| m.forced_align(s, o).expect("align").0)
        .collect();
    m.viterbi_reestimate(&data, &paths, 0.01).expect("reest");
    for h in m.phones.values() {
        for mix in &h.emissions {
            let w: f64 = mix.components.iter().map(|c| c.weight).sum();
            assert!((w - 1.0).abs() < 1e-9, "weights sum to 1, got {w}");
            for c in &mix.components {
                for d in 0..DIM {
                    assert!(
                        c.gaussian.var[d] >= m.global_var[d] * 0.01 - 1e-12,
                        "var floor holds"
                    );
                    assert!(c.gaussian.mean[d].is_finite() && c.gaussian.var[d].is_finite());
                }
            }
        }
    }
}

#[test]
fn reestimate_is_deterministic() {
    let data = data_on(5.0);
    let mk = || {
        let mut m = model_on(0.0);
        let paths: Vec<Vec<usize>> = data
            .iter()
            .map(|(_, s, o)| m.forced_align(s, o).expect("align").0)
            .collect();
        m.viterbi_reestimate(&data, &paths, 0.01).expect("reest");
        m
    };
    assert_eq!(mk(), mk());
}
