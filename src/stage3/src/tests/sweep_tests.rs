//! aem.1 gate tests: commit-window width sweep against the error curve.
//!
//! The empirical dial (synthesis §6): a sweep harness runs a range of window
//! widths against an error curve, identifies an empirical optimum, and
//! characterizes the brittleness-vs-blowup tradeoff numerically.

use crate::sweep::Position;

/// A benign curve: the true candidate is almost always the Stage-1 argmax.
fn benign_curve() -> Vec<Position> {
    (0..10)
        .map(|_| Position {
            true_rank: 0,
            competitors: 5,
        })
        .collect()
}

/// A hard curve: the true candidate is often rank 2+ (recoverable only if
/// the window holds enough alternatives).
fn hard_curve() -> Vec<Position> {
    let mut positions = Vec::new();
    for i in 0..10 {
        positions.push(Position {
            true_rank: if i < 5 { 2 } else { 0 },
            competitors: 5,
        });
    }
    positions
}

#[test]
fn sweep_finds_narrow_width_as_optimum_when_curve_is_easy() {
    let report = crate::sweep::sweep(&[1, 2, 3, 4, 5], &benign_curve(), 0.0);
    let opt = report.optimum.expect("optimum exists");
    // True candidate is always rank 0 -> width 1 already suffices.
    assert_eq!(opt.width, 1);
    assert_eq!(opt.error, 0.0);
    assert!(report.tradeoff.contains("optimum width"));
}

#[test]
fn sweep_hard_curve_needs_wider_window() {
    let report = crate::sweep::sweep(&[1, 2, 3, 4, 5], &hard_curve(), 0.0);
    // 5/10 positions have true_rank=2 -> need width >= 3 to keep them.
    let opt = report.optimum.expect("optimum exists");
    assert!(opt.width >= 3, "must widen to keep rank-2 candidates");
    assert_eq!(opt.error, 0.0);
}

#[test]
fn sweep_error_decreases_with_width() {
    let positions = hard_curve();
    let mut prev_error = 1.0f64;
    for width in 1..=5 {
        let (error, _) = crate::sweep::run_width(width, &positions);
        assert!(error <= prev_error, "error monotone into width");
        prev_error = error;
    }
}

#[test]
fn sweep_load_grows_with_width() {
    let positions = hard_curve();
    let (_, load1) = crate::sweep::run_width(1, &positions);
    let (_, load5) = crate::sweep::run_width(5, &positions);
    assert!(load5 > load1, "wider window holds more live candidates");
}

#[test]
fn sweep_reports_tradeoff_string() {
    let report = crate::sweep::sweep(&[1, 2, 3], &hard_curve(), 0.0);
    assert!(!report.tradeoff.is_empty());
    assert!(!report.points.is_empty());
}
