//! Shared evaluation statistics (STAT-1).
//!
//! Paired bootstrap confidence intervals over per-utterance paired
//! correctness/error diffs. This replaces the normal-approximation
//! (`hmm::paired_ci`, mean +/- 1.96 standard errors) shortcut everywhere it
//! was used: bootstrap percentile intervals make no normality assumption and
//! stay valid for skewed per-utterance error diffs.
//!
//! Method: for `B` resamples, draw `n` utterance indices with replacement,
//! recompute the statistic on each resample, and report the two-sided
//! percentile interval at the configured confidence level. Resampling uses a
//! seed-pinned splitmix64 stream, so identical inputs plus identical seed
//! give bit-identical output on every platform.
//!
//! Two statistics are supported:
//! - [`paired_bootstrap_ci`]: mean of per-utterance diffs (each utterance
//!   contributes one paired difference, e.g. adapted errors minus base
//!   errors on that utterance).
//! - [`corpus_delta_ci`]: corpus-level rate delta, i.e. (total adapted
//!   errors - total base errors) / total denominator (words/phones) over the
//!   resampled utterances. This is the WER/PER delta quoted in result docs.

/// Default resample count: 10k per STAT-1.
pub const DEFAULT_RESAMPLES: usize = 10_000;
/// Default seed. Pinned so every gate recomputes identical intervals.
pub const DEFAULT_SEED: u64 = 0x51ab_3c7d_9e20_4f11;
/// Default confidence level: two-sided 95 pct.
pub const DEFAULT_CONFIDENCE: f64 = 0.95;

/// Configuration for a bootstrap confidence interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BootstrapConfig {
    /// Number of bootstrap resamples (`B`). Must be >= 1.
    pub resamples: usize,
    /// Seed for the resampling stream. Identical seed plus identical data
    /// yields bit-identical output.
    pub seed: u64,
    /// Confidence level in (0, 1); 0.95 gives the two-sided 95 pct interval.
    pub confidence: f64,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            resamples: DEFAULT_RESAMPLES,
            seed: DEFAULT_SEED,
            confidence: DEFAULT_CONFIDENCE,
        }
    }
}

/// A two-sided bootstrap confidence interval plus its point estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BootstrapCi {
    /// Point estimate on the full sample (mean of diffs, or corpus delta).
    pub estimate: f64,
    /// Lower percentile bound.
    pub lo: f64,
    /// Upper percentile bound.
    pub hi: f64,
    /// Confidence level the bounds were computed at.
    pub confidence: f64,
    /// Resample count used.
    pub resamples: usize,
    /// Seed used.
    pub seed: u64,
}

impl BootstrapCi {
    /// True when `value` lies strictly outside `[lo, hi]`.
    ///
    /// Gates use `ci.excludes(0.0)` for "paired CI excludes 0" and
    /// `ci.hi <= epsilon` for the S4.3 non-inferiority rule.
    pub fn excludes(&self, value: f64) -> bool {
        value < self.lo || value > self.hi
    }

    /// Interval width (`hi - lo`).
    pub fn width(&self) -> f64 {
        self.hi - self.lo
    }
}

/// Minimal splitmix64 stream (Steele et al. 2014).
///
/// Hand-rolled like `hmm::Rng` so this crate stays dependency-free; the
/// algorithm is fixed, so the resampling stream never changes under us
/// (unlike external RNG crates, whose streams may vary across versions).
/// The same seed always yields the same `u64` sequence on every platform.
#[derive(Debug, Clone)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform index in `[0, n)`. Requires `n >= 1`.
    fn next_index(&mut self, n: usize) -> usize {
        debug_assert!(n >= 1);
        (self.next_u64() % n as u64) as usize
    }
}

/// Linear-interpolation quantile of a slice sorted ascending (`q` in
/// `[0, 1]`; R-7 / numpy-default definition). Requires non-empty input.
fn quantile_sorted(sorted: &[f64], q: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    debug_assert!((0.0..=1.0).contains(&q));
    if sorted.len() == 1 {
        return sorted[0];
    }
    if q <= 0.0 {
        return sorted[0];
    }
    if q >= 1.0 {
        return sorted[sorted.len() - 1];
    }
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let frac = pos - lo as f64;
    sorted[lo] + frac * (sorted[lo + 1] - sorted[lo])
}

/// Shared percentile-interval engine: `replicates` holds one statistic value
/// per resample; returns `(lo, hi)` at the configured two-sided level.
fn percentile_interval(replicates: &mut [f64], confidence: f64) -> (f64, f64) {
    replicates.sort_by(|a, b| a.total_cmp(b));
    let alpha = 1.0 - confidence;
    (
        quantile_sorted(replicates, alpha / 2.0),
        quantile_sorted(replicates, 1.0 - alpha / 2.0),
    )
}

/// Paired bootstrap CI for the mean of per-utterance diffs.
///
/// Each element of `diffs` is one utterance's paired difference (adapted
/// errors minus base errors, or any per-utterance paired quantity). The
/// point estimate is the plain sample mean; the bounds are the two-sided
/// percentile interval over `config.resamples` utterance-level resamples.
///
/// Edge cases (mirroring `hmm::paired_ci` conventions):
/// - empty input yields `(estimate, lo, hi) = (0, 0, 0)`;
/// - a single utterance, or zero-variance diffs, yields a zero-width
///   interval at the observed value (every resample reproduces it).
///
/// Panics if `config.resamples` is 0 or `config.confidence` is not in
/// `(0, 1)`. NaN inputs give deterministic but unspecified output; callers
/// must not pass NaN diffs.
pub fn paired_bootstrap_ci(diffs: &[f64], config: BootstrapConfig) -> BootstrapCi {
    assert!(config.resamples >= 1, "resamples must be >= 1");
    assert!(
        config.confidence > 0.0 && config.confidence < 1.0,
        "confidence must be in (0, 1)"
    );
    if diffs.is_empty() {
        return BootstrapCi {
            estimate: 0.0,
            lo: 0.0,
            hi: 0.0,
            confidence: config.confidence,
            resamples: config.resamples,
            seed: config.seed,
        };
    }
    let n = diffs.len();
    let estimate = diffs.iter().sum::<f64>() / n as f64;
    let mut rng = SplitMix64::new(config.seed);
    let mut replicates = Vec::with_capacity(config.resamples);
    for _ in 0..config.resamples {
        let mut acc = 0.0;
        for _ in 0..n {
            acc += diffs[rng.next_index(n)];
        }
        replicates.push(acc / n as f64);
    }
    let (lo, hi) = percentile_interval(&mut replicates, config.confidence);
    BootstrapCi {
        estimate,
        lo,
        hi,
        confidence: config.confidence,
        resamples: config.resamples,
        seed: config.seed,
    }
}

/// Paired bootstrap CI for a corpus-level rate delta (WER/PER style).
///
/// `base_errors[i]` / `adapt_errors[i]` are utterance `i`'s error counts
/// under the two systems and `denoms[i]` its denominator (reference words
/// or phones). Each resample recomputes
/// `(sum adapt - sum base) / sum denom` over the resampled utterances, so
/// long utterances carry proportional weight, exactly as in the reported
/// corpus rate. The point estimate is the same ratio on the full sample.
///
/// Edge cases:
/// - empty inputs yield `(0, 0, 0)`;
/// - a resample (or the full sample) with total denominator 0 contributes
///   a 0.0 replicate (documented; no-division-by-zero by construction).
///
/// Panics if the three slices have unequal lengths, if
/// `config.resamples` is 0, or if `config.confidence` is not in `(0, 1)`.
pub fn corpus_delta_ci(
    base_errors: &[u32],
    adapt_errors: &[u32],
    denoms: &[u32],
    config: BootstrapConfig,
) -> BootstrapCi {
    assert!(
        base_errors.len() == adapt_errors.len() && base_errors.len() == denoms.len(),
        "base_errors, adapt_errors, denoms must have equal lengths"
    );
    assert!(config.resamples >= 1, "resamples must be >= 1");
    assert!(
        config.confidence > 0.0 && config.confidence < 1.0,
        "confidence must be in (0, 1)"
    );
    if base_errors.is_empty() {
        return BootstrapCi {
            estimate: 0.0,
            lo: 0.0,
            hi: 0.0,
            confidence: config.confidence,
            resamples: config.resamples,
            seed: config.seed,
        };
    }
    let n = base_errors.len();
    let estimate = corpus_delta(base_errors, adapt_errors, denoms, 0..n);
    let mut rng = SplitMix64::new(config.seed);
    let mut replicates = Vec::with_capacity(config.resamples);
    // Resample counts per utterance, then accumulate once (equivalent to
    // summing over n draws with replacement, without materializing them).
    let mut counts = vec![0u64; n];
    for _ in 0..config.resamples {
        counts.fill(0);
        for _ in 0..n {
            counts[rng.next_index(n)] += 1;
        }
        let mut be = 0u64;
        let mut ae = 0u64;
        let mut dn = 0u64;
        for (i, &c) in counts.iter().enumerate() {
            be += c * base_errors[i] as u64;
            ae += c * adapt_errors[i] as u64;
            dn += c * denoms[i] as u64;
        }
        replicates.push(if dn == 0 {
            0.0
        } else {
            (ae as f64 - be as f64) / dn as f64
        });
    }
    let (lo, hi) = percentile_interval(&mut replicates, config.confidence);
    BootstrapCi {
        estimate,
        lo,
        hi,
        confidence: config.confidence,
        resamples: config.resamples,
        seed: config.seed,
    }
}

/// Corpus delta over an index range (full-sample point estimate helper).
fn corpus_delta(
    base_errors: &[u32],
    adapt_errors: &[u32],
    denoms: &[u32],
    range: std::ops::Range<usize>,
) -> f64 {
    let mut be = 0u64;
    let mut ae = 0u64;
    let mut dn = 0u64;
    for i in range {
        be += base_errors[i] as u64;
        ae += adapt_errors[i] as u64;
        dn += denoms[i] as u64;
    }
    if dn == 0 {
        0.0
    } else {
        (ae as f64 - be as f64) / dn as f64
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn quantile_endpoints_and_midpoint() {
        let v = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(quantile_sorted(&v, 0.0), 1.0);
        assert_eq!(quantile_sorted(&v, 1.0), 4.0);
        assert_eq!(quantile_sorted(&v, 0.5), 2.5);
    }

    #[test]
    fn quantile_single_element() {
        assert_eq!(quantile_sorted(&[7.0], 0.025), 7.0);
        assert_eq!(quantile_sorted(&[7.0], 0.975), 7.0);
    }

    #[test]
    fn splitmix_matches_reference_vector() {
        // Published splitmix64 test vector (Steele et al. 2014): seed 0
        // yields 0xe220a8397b1dcdaf first. This pins the resampling stream
        // against an external reference, not against itself.
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xe220_a839_7b1d_cdaf);
    }

    #[test]
    fn excludes_and_width() {
        let ci = BootstrapCi {
            estimate: -0.5,
            lo: -0.7,
            hi: -0.3,
            confidence: 0.95,
            resamples: 10_000,
            seed: DEFAULT_SEED,
        };
        assert!(ci.excludes(0.0));
        assert!(!ci.excludes(-0.5));
        assert!(!ci.excludes(-0.7));
        assert!(!ci.excludes(-0.3));
        assert!((ci.width() - 0.4).abs() < 1e-12);
    }
}
