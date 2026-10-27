//! Fail-fast experiment measurement (39c.5).
//!
//! Scores the session under BOTH conditions from the SAME audio and the SAME
//! feature code (INV `measurement_symmetry`), buckets the session
//! chronologically (INV `buckets_exhaustive`), and applies the pre-registered
//! separation rule (failfast-separation-criterion.md §4) to emit a single
//! PASS/FAIL verdict (INV `verdict_pre_registered`).
//!
//! There is no word-level decoder yet — the per-utterance condition score is
//! the observed error statistic; the bucket mean is the per-bucket WER proxy
//! that the separation rule operates on.

use std::collections::HashSet;
use std::path::Path;

use invariant_ppt::assert_invariant;

use crate::{ConditionA, ConditionB, ScoreError};

/// One row of the 39c.2 session manifest (data/session/session.tsv).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub bucket: usize,
    pub idx: usize,
    pub start_sec: f64,
    pub stop_sec: f64,
    pub text: String,
}

/// Session manifest path: `SESSION_ROOT`/`session.tsv`.
pub fn session_manifest_path(root: &Path) -> std::path::PathBuf {
    root.join("session.tsv")
}

/// Load the 39c.2 session manifest.
///
/// Skips comment (`#`) and header (`bucket<...>`) lines. Every data row must
/// carry a numeric bucket, an idx, and numeric start/stop seconds. A
/// malformed row aborts with the offending line so the manifest is never
/// silently truncated (keeps `buckets_exhaustive` meaningful).
pub fn load_session(root: &Path) -> Result<Vec<SessionRow>, String> {
    let path = session_manifest_path(root);
    let content = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.first().map(|c| c.trim()) == Some("bucket") {
            continue; // header row
        }
        if cols.len() < 7 {
            return Err(format!("{}: malformed row: {line:?}", path.display()));
        }
        let bucket: usize = cols[0]
            .trim()
            .parse()
            .map_err(|_| format!("{}: bad bucket in {line:?}", path.display()))?;
        let idx: usize = cols[1]
            .trim()
            .parse()
            .map_err(|_| format!("{}: bad idx in {line:?}", path.display()))?;
        let start_sec: f64 = cols[2]
            .trim()
            .parse()
            .map_err(|_| format!("{}: bad start_sec in {line:?}", path.display()))?;
        let stop_sec: f64 = cols[3]
            .trim()
            .parse()
            .map_err(|_| format!("{}: bad stop_sec in {line:?}", path.display()))?;
        rows.push(SessionRow {
            bucket,
            idx,
            start_sec,
            stop_sec,
            text: cols[6].trim().to_string(),
        });
    }
    assert_invariant(!rows.is_empty(), "session_rows_nonempty", Some("39c.5"));
    Ok(rows)
}

/// One bucket's aggregated condition statistics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BucketStat {
    pub bucket: usize,
    /// Mean Condition-A score over the bucket's utterances.
    pub mean_a: f64,
    /// Mean Condition-B score over the bucket's utterances.
    pub mean_b: f64,
    /// Number of utterances in this bucket.
    pub count: usize,
}

/// The pre-registered separation verdict (failfast-separation-criterion.md
/// §4). Every field is filled in before the verdict is read.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// Overall PASS (all three sub-checks hold).
    pub pass: bool,
    /// Check 1: mean(B, 2nd half) < mean(A, 2nd half).
    pub tail_separation: bool,
    /// Check 2: mean(A, 2nd half) − mean(B, 2nd half) ≥ 0.02.
    pub margin_two_pp: bool,
    /// Check 3: B non-increasing from first to last bucket, and
    /// B(final) ≤ A(final).
    pub monotone_b: bool,
    /// Separation margin: mean(A, 2nd half) − mean(B, 2nd half). Powers the
    /// `--margin-floor` regression gate (bead nwu): the gate passes only when
    /// the pre-registered verdict holds AND `margin >= floor`.
    pub margin: f64,
    /// Per-check detail strings for the report (INV
    /// `verdict_pre_registered` — the rule is fixed before analysis).
    pub checks: Vec<String>,
}

impl Verdict {
    /// Regression-gate predicate (bead nwu): overall PASS requires the
    /// pre-registered verdict AND the separation margin to clear the floor.
    pub fn passes_floor(&self, floor: f64) -> bool {
        self.pass && self.margin >= floor
    }
}

/// Separation margin of a bucketed session: mean(A, 2nd half) − mean(B, 2nd
/// half), with the same first/second-half split as [`apply_rule`] (the lower-
/// indexed half is first; the rest is second).
pub fn separation_margin(stats: &[BucketStat]) -> f64 {
    let n = stats.len();
    let half = n / 2;
    let second = &stats[half..];
    let mean = |pick: fn(&BucketStat) -> f64| {
        if second.is_empty() {
            f64::NAN
        } else {
            second.iter().map(pick).sum::<f64>() / second.len() as f64
        }
    };
    mean(|b| b.mean_a) - mean(|b| b.mean_b)
}

/// Score every session row under both A and B, aggregating per bucket.
///
/// The SAME PCM slice and SAME feature extraction feed both conditions
/// (INV `measurement_symmetry`): the conditions differ only in the
/// accent-signature state that Condition B carries (INV
/// `condition_b_carry_forward`). Rows are processed in manifest order, so B's
/// EMA sees utterances chronologically (INV `buckets_exhaustive` — every row
/// lands in exactly one bucket).
pub fn measure_session(
    a: &ConditionA,
    b: &mut ConditionB,
    pcm: &[i16],
    sample_rate: u32,
    rows: &[SessionRow],
) -> Result<Vec<BucketStat>, ScoreError> {
    let max_bucket = rows.iter().map(|r| r.bucket).max().unwrap_or(0);
    let mut sums: Vec<(f64, f64, usize)> = vec![(0.0, 0.0, 0); max_bucket + 1];

    let mut prior_start: Option<f64> = None;
    for row in rows {
        assert_invariant(
            prior_start.is_none_or(|p| p < row.start_sec),
            "session_chronological",
            Some("39c.5"),
        );
        prior_start = Some(row.start_sec);

        let (a_frames, a_score) =
            crate::score_session_utterance(a, pcm, sample_rate, row.start_sec, row.stop_sec)?;
        let (b_frames, b_score) =
            crate::score_session_utterance_b(b, pcm, sample_rate, row.start_sec, row.stop_sec)?;
        assert_invariant(a_frames == b_frames, "measurement_symmetry", Some("39c.5"));

        let entry = &mut sums[row.bucket];
        entry.0 += a_score;
        entry.1 += b_score;
        entry.2 += 1;
    }

    let mut stats: Vec<BucketStat> = sums
        .iter()
        .enumerate()
        .filter(|&(_, (_, _, count))| *count > 0)
        .map(|(bucket, (s_a, s_b, count))| BucketStat {
            bucket,
            mean_a: s_a / *count as f64,
            mean_b: s_b / *count as f64,
            count: *count,
        })
        .collect();
    stats.sort_by_key(|s| s.bucket);

    let occupied: HashSet<usize> = rows.iter().map(|r| r.bucket).collect();
    assert_invariant(
        stats.len() == occupied.len(),
        "buckets_exhaustive",
        Some("39c.5"),
    );
    Ok(stats)
}

fn mean_of(stats: &[BucketStat], pick: fn(&BucketStat) -> f64) -> f64 {
    if stats.is_empty() {
        return f64::NAN;
    }
    stats.iter().map(pick).sum::<f64>() / stats.len() as f64
}

/// Apply the pre-registered decision rule (fail-spec §4) to the bucketed
/// stats.
///
/// - first half = the lower-indexed half of the session, second half = the
///   rest (for a 2-bucket session that is exactly bucket 0 then bucket 1).
/// - Check 1: `mean(B, 2nd half) < mean(A, 2nd half)`.
/// - Check 2: `mean(A, 2nd half) − mean(B, 2nd half) ≥ 0.02`.
/// - Check 3: `mean(B, first bucket) ≥ mean(B, last bucket)` (B non-increasing)
///   and `mean(B, last) ≤ mean(A, last)`.
pub fn apply_rule(stats: &[BucketStat]) -> Verdict {
    let n = stats.len();
    let half = n / 2;
    let (first, second) = (&stats[..half], &stats[half..]);

    let mean_a = |s: &[BucketStat]| mean_of(s, |b| b.mean_a);
    let mean_b = |s: &[BucketStat]| mean_of(s, |b| b.mean_b);

    let second_a = mean_a(second);
    let second_b = mean_b(second);
    let first_b = mean_b(first);
    let margin = separation_margin(stats);

    let tail_separation = second_b < second_a;
    let margin_two_pp = second_a - second_b >= 0.02;
    let last_a = stats[n - 1].mean_a;
    let last_b = stats[n - 1].mean_b;
    // Check 3: B non-increasing (last <= first) and B(final) <= A(final).
    let monotone_b = last_b <= first_b && last_b <= last_a;

    let pass = tail_separation && margin_two_pp && monotone_b;

    let mut checks = Vec::new();
    checks.push(format!(
        "check1 tail_separation: mean(B)=[{first_b:.4}] 2nd-half B={second_b:.4} A={second_a:.4} -> {}",
        if tail_separation { "PASS" } else { "FAIL" }
    ));
    checks.push(format!(
        "check2 margin: 2nd-half A−B={:.4} (need ≥0.02) -> {}",
        second_a - second_b,
        if margin_two_pp { "PASS" } else { "FAIL" }
    ));
    checks.push(format!(
        "check3 monotone B: first_B={first_b:.4} last_B={last_b:.4} last_A={last_a:.4} -> {}",
        if monotone_b { "PASS" } else { "FAIL" }
    ));
    assert_invariant(
        second_a.is_finite() && second_b.is_finite() && last_a.is_finite() && last_b.is_finite(),
        "verdict_pre_registered",
        Some("39c.5"),
    );

    Verdict {
        pass,
        tail_separation,
        margin_two_pp,
        monotone_b,
        margin,
        checks,
    }
}
