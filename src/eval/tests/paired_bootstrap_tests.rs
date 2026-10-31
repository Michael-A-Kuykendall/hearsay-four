//! STAT-1 acceptance: coverage on synthetic known-diff fixtures plus
//! determinism under seed, for both `paired_bootstrap_ci` and
//! `corpus_delta_ci`.
//!
//! All fixtures are fixed arrays with hand-computed means (independent of
//! the crate under test); the Monte-Carlo checks use a test-local LCG, so
//! every assertion here is deterministic and rerunnable.

use eval::{BootstrapCi, BootstrapConfig, corpus_delta_ci, paired_bootstrap_ci};

fn default_config() -> BootstrapConfig {
    BootstrapConfig::default()
}

fn assert_covers(ci: &BootstrapCi, truth: f64, fixture: &str) {
    assert!(
        ci.lo <= truth && truth <= ci.hi,
        "{fixture}: CI [{}, {}] misses known diff {truth}",
        ci.lo,
        ci.hi
    );
}

// ---------------------------------------------------------------------------
// Config defaults (10k resamples, seed-pinned, two-sided 95 pct)
// ---------------------------------------------------------------------------

#[test]
fn defaults_match_stat1_spec() {
    let cfg = BootstrapConfig::default();
    assert_eq!(cfg.resamples, 10_000);
    assert_eq!(cfg.confidence, 0.95);
    assert_eq!(cfg.seed, eval::DEFAULT_SEED);
}

// ---------------------------------------------------------------------------
// Determinism under seed
// ---------------------------------------------------------------------------

#[test]
fn paired_ci_deterministic_under_seed() {
    let diffs: Vec<f64> = (0..37).map(|i| (i as f64 * 0.37).sin() - 0.2).collect();
    let a = paired_bootstrap_ci(&diffs, default_config());
    let b = paired_bootstrap_ci(&diffs, default_config());
    assert_eq!(a, b, "same seed + same data must be bit-identical");
}

#[test]
fn corpus_ci_deterministic_under_seed() {
    let base = vec![3, 0, 5, 1, 2, 4, 0, 1];
    let adapt = vec![1, 0, 5, 0, 1, 3, 0, 2];
    let denoms = vec![10, 5, 20, 8, 6, 12, 4, 9];
    let a = corpus_delta_ci(&base, &adapt, &denoms, default_config());
    let b = corpus_delta_ci(&base, &adapt, &denoms, default_config());
    assert_eq!(a, b, "same seed + same data must be bit-identical");
}

#[test]
fn different_seeds_move_the_interval() {
    let diffs: Vec<f64> = (0..50).map(|i| (i as f64 * 0.61).cos()).collect();
    let a = paired_bootstrap_ci(&diffs, default_config());
    let other = BootstrapConfig {
        seed: 12345,
        ..default_config()
    };
    let b = paired_bootstrap_ci(&diffs, other);
    assert!(
        a.lo != b.lo || a.hi != b.hi,
        "seed must feed the resampling stream"
    );
}

// ---------------------------------------------------------------------------
// Exact fixtures (zero-width intervals, hand-verifiable)
// ---------------------------------------------------------------------------

#[test]
fn all_zero_diffs_give_zero_point_interval() {
    let ci = paired_bootstrap_ci(&[0.0; 25], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (0.0, 0.0, 0.0));
}

#[test]
fn constant_diffs_give_point_interval_at_value() {
    let ci = paired_bootstrap_ci(&vec![-0.25; 30], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (-0.25, -0.25, -0.25));
}

#[test]
fn single_utterance_gives_point_interval() {
    let ci = paired_bootstrap_ci(&[1.5], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (1.5, 1.5, 1.5));
}

#[test]
fn empty_input_gives_zeros() {
    let ci = paired_bootstrap_ci(&[], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (0.0, 0.0, 0.0));
    let ci = corpus_delta_ci(&[], &[], &[], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (0.0, 0.0, 0.0));
}

// ---------------------------------------------------------------------------
// Known-diff coverage fixtures (paired)
// ---------------------------------------------------------------------------

#[test]
fn covers_known_negative_shift() {
    // 40 utterances alternating -0.4 / -0.6: true mean exactly -0.5.
    let diffs: Vec<f64> = (0..40)
        .map(|i| if i % 2 == 0 { -0.4 } else { -0.6 })
        .collect();
    let ci = paired_bootstrap_ci(&diffs, default_config());
    assert_eq!(ci.estimate, -0.5);
    assert_covers(&ci, -0.5, "neg-shift");
    assert!(ci.excludes(0.0), "strong effect must exclude 0: {ci:?}");
}

#[test]
fn covers_known_skewed_diff() {
    // Skewed error-count-like diffs: 40 zeros + 10 spikes of 1.5.
    // True mean exactly 0.3; a symmetric Wald interval would mislead here.
    let mut diffs = vec![0.0; 40];
    diffs.extend(vec![1.5; 10]);
    let ci = paired_bootstrap_ci(&diffs, default_config());
    assert!((ci.estimate - 0.3).abs() < 1e-12);
    assert_covers(&ci, 0.3, "skewed");
}

#[test]
fn null_fixture_includes_zero() {
    // Symmetric +/- pairs summing to exactly 0.0.
    let mut diffs = Vec::new();
    for m in [0.1, 0.2, 0.3, 0.15, 0.25, 0.05] {
        for _ in 0..5 {
            diffs.push(m);
            diffs.push(-m);
        }
    }
    let ci = paired_bootstrap_ci(&diffs, default_config());
    assert_eq!(ci.estimate, 0.0);
    assert_covers(&ci, 0.0, "null");
    assert!(!ci.excludes(0.0), "null fixture must include 0: {ci:?}");
}

#[test]
fn covers_known_sparse_error_reduction() {
    // Word-error-like: 80 unchanged utterances, 20 with one fewer error.
    let mut diffs = vec![0.0; 80];
    diffs.extend(vec![-1.0; 20]);
    let ci = paired_bootstrap_ci(&diffs, default_config());
    assert!((ci.estimate - (-0.2)).abs() < 1e-12);
    assert_covers(&ci, -0.2, "sparse");
    assert!(ci.excludes(0.0), "20/100 improved must exclude 0: {ci:?}");
}

// ---------------------------------------------------------------------------
// Independent analytic cross-check: agrees with Wald on large-n Gaussian data
// ---------------------------------------------------------------------------

/// Test-local deterministic Gaussian source (LCG + Box-Muller), fully
/// independent of the crate's internal RNG.
struct TestRng(u64);

impl TestRng {
    fn next_f64(&mut self) -> f64 {
        // Numerical Recipes LCG.
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn next_gaussian(&mut self, mean: f64, sd: f64) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        mean + sd * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

#[test]
fn agrees_with_wald_on_large_gaussian_sample() {
    let mut rng = TestRng(0x1234_5678);
    let diffs: Vec<f64> = (0..2000).map(|_| rng.next_gaussian(0.5, 2.0)).collect();
    let n = diffs.len() as f64;
    let mean = diffs.iter().sum::<f64>() / n;
    let var = diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let wald_half = 1.96 * (var / n).sqrt();

    let ci = paired_bootstrap_ci(&diffs, default_config());
    assert_eq!(ci.estimate, mean);
    assert_covers(&ci, 0.5, "gaussian-truth");
    let boot_half = ci.width() / 2.0;
    let rel_err = (boot_half - wald_half).abs() / wald_half;
    assert!(
        rel_err < 0.10,
        "bootstrap half-width {boot_half} vs Wald {wald_half} (rel err {rel_err})"
    );
}

// ---------------------------------------------------------------------------
// Monte-Carlo coverage: nominal 95 pct intervals cover ~95 pct of the time
// ---------------------------------------------------------------------------

#[test]
fn monte_carlo_coverage_near_nominal() {
    // 300 synthetic DEV sets, 60 utterances each, true diff 0.4.
    // Threshold 0.90 sits well below nominal 0.95 (sampling std over 300
    // sets is ~1.2 pct) but far above what a broken resampler yields.
    let mut covered = 0;
    let sets = 300;
    for s in 0..sets {
        let mut rng = TestRng(1000 + s as u64);
        let diffs: Vec<f64> = (0..60).map(|_| rng.next_gaussian(0.4, 1.0)).collect();
        let ci = paired_bootstrap_ci(&diffs, default_config());
        if ci.lo <= 0.4 && 0.4 <= ci.hi {
            covered += 1;
        }
    }
    let rate = covered as f64 / sets as f64;
    assert!(
        rate >= 0.90,
        "coverage {covered}/{sets} = {rate}, want >= 0.90"
    );
}

// ---------------------------------------------------------------------------
// Corpus-level delta tests
// ---------------------------------------------------------------------------

#[test]
fn corpus_constant_rate_gives_point_interval() {
    // Every utterance: base 2/10, adapted 1/10 -> delta exactly -0.1.
    let ci = corpus_delta_ci(&[2; 12], &[1; 12], &[10; 12], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (-0.1, -0.1, -0.1));
}

#[test]
fn corpus_estimate_is_hand_computed_ratio() {
    // Totals: base 8, adapted 6, denom 35 -> delta exactly -2/35.
    let base = vec![3, 0, 5];
    let adapt = vec![1, 0, 5];
    let denoms = vec![10, 5, 20];
    let ci = corpus_delta_ci(&base, &adapt, &denoms, default_config());
    assert_eq!(ci.estimate, -2.0 / 35.0);
    assert_covers(&ci, -2.0 / 35.0, "corpus-ratio");
}

#[test]
fn corpus_weights_by_denominator_not_by_utterance() {
    // One 10-word utterance at +1.0 rate, one 90-word utterance at 0.0:
    // corpus delta is 0.1 while the mean of per-utterance rates is 0.5.
    let ci = corpus_delta_ci(&[0, 0], &[10, 0], &[10, 90], default_config());
    assert_eq!(ci.estimate, 0.1);
    assert_covers(&ci, 0.1, "corpus-weighting");
}

#[test]
fn corpus_ignores_zero_denominator_utterances() {
    let ci = corpus_delta_ci(&[1, 2], &[0, 2], &[0, 4], default_config());
    assert_eq!(ci.estimate, -0.25);
    assert_covers(&ci, -0.25, "zero-denom-utt");
}

#[test]
fn corpus_all_zero_denominators_give_zeros() {
    let ci = corpus_delta_ci(&[1, 2], &[0, 1], &[0, 0], default_config());
    assert_eq!((ci.estimate, ci.lo, ci.hi), (0.0, 0.0, 0.0));
}

#[test]
fn corpus_deterministic_known_answer_shape() {
    // WER-style fixture: 50 utterances, 10 words each; adapted fixes one
    // error on 15 of them. True corpus delta exactly -15/500 = -0.03.
    let base = vec![2; 50];
    let mut adapt = vec![2; 50];
    for a in adapt.iter_mut().take(15) {
        *a = 1;
    }
    let denoms = vec![10; 50];
    let ci = corpus_delta_ci(&base, &adapt, &denoms, default_config());
    assert_eq!(ci.estimate, -0.03);
    assert_covers(&ci, -0.03, "wer-style");
    assert!(ci.excludes(0.0), "15/50 fixed must exclude 0: {ci:?}");
}

// ---------------------------------------------------------------------------
// Wider variance -> wider interval (same seed, paired comparison)
// ---------------------------------------------------------------------------

#[test]
fn wider_data_gives_wider_interval() {
    let tight: Vec<f64> = (0..40).map(|i| (i % 2) as f64 * 0.02 - 0.01).collect();
    let wide: Vec<f64> = (0..40).map(|i| (i % 2) as f64 * 2.0 - 1.0).collect();
    let a = paired_bootstrap_ci(&tight, default_config());
    let b = paired_bootstrap_ci(&wide, default_config());
    assert!(b.width() > 10.0 * a.width(), "{:?} vs {a:?}", b);
}

// ---------------------------------------------------------------------------
// Golden known-answer test (regression pin)
// ---------------------------------------------------------------------------

#[test]
fn known_answer_small_fixture() {
    // Fixed 5-utterance fixture (same values as hmm's checkpoint paired-CI
    // check, so the old Wald shortcut and the new bootstrap can be compared
    // by eye). Bounds below are pinned outputs: correctness is established
    // by the independent checks above (reference-vector RNG, exact
    // fixtures, Wald agreement, Monte-Carlo coverage); this test pins the
    // algorithm against silent drift. Re-derive, never hand-tune.
    let diffs = vec![-0.5, -0.4, -0.6, -0.5, -0.45];
    let ci = paired_bootstrap_ci(&diffs, default_config());
    assert!((ci.estimate - (-0.49)).abs() < 1e-12);
    assert_eq!((ci.lo, ci.hi), (-0.55, -0.43000000000000005));
}

// ---------------------------------------------------------------------------
// Contract violations panic with a message (documented, not silent)
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "equal lengths")]
fn corpus_mismatched_lengths_panic() {
    corpus_delta_ci(&[1, 2], &[1], &[5, 5], default_config());
}

#[test]
#[should_panic(expected = "resamples")]
fn zero_resamples_panics() {
    paired_bootstrap_ci(
        &[1.0],
        BootstrapConfig {
            resamples: 0,
            ..default_config()
        },
    );
}

#[test]
#[should_panic(expected = "confidence")]
fn bad_confidence_panics() {
    paired_bootstrap_ci(
        &[1.0],
        BootstrapConfig {
            confidence: 1.5,
            ..default_config()
        },
    );
}
