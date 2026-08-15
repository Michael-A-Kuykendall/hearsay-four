//! Commit-window width sweep (aem.1) — the empirical dial.
//!
//! Synthesis §4.4/§6: window width is not a settled constant. Too narrow
//! reintroduces global hard-threshold brittleness (a recoverable hypothesis
//! is discarded before it can win); too wide reintroduces the combinatorial
//! load the fabric exists to eliminate.
//!
//! This harness sweeps window widths against an **error curve** — a list of
//! positions carrying the rank of the *true* candidate among the ordered
//! competition emitted by Stage 1 (the fail-fast measurement, 39c.5, reports
//! the prototype's per-position error). For each width it reports:
//! - **error** — the fraction of true candidates NOT offered/kept because
//!   the window was full (brittleness);
//! - **load** — the peak number of live candidates the window admitted
//!   (combinatorial cost).
//!
//! The empirical optimum is the narrowest width whose error stays under a
//! target floor. The tradeoff is characterized numerically — NOT decided
//! on paper (synthesis §6).

use invariant_ppt::assert_invariant;

use crate::window::CommitWindow;

/// One position in the simulated stream.
///
/// `true_rank` = the 0-based rank (by Stage-1 score) of the *true* phone
/// among the emitted N-best candidates. `competitors` = how many candidates
/// vie at this position.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    /// Rank of the true candidate (0 = acoustic argmax).
    pub true_rank: usize,
    /// Total competing candidates at this position.
    pub competitors: usize,
}

/// One (width, error, load) datum.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepPoint {
    pub width: usize,
    /// Fraction of true positions lost (0..=1).
    pub error: f64,
    /// Peak simultaneous live candidates (the combinatorial cost).
    pub max_load: usize,
}

/// The sweep's empirical conclusion.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepReport {
    pub points: Vec<SweepPoint>,
    /// Narrowest width achieving error <= `error_floor` and its stats.
    pub optimum: Option<SweepPoint>,
    /// Human-readable tradeoff characterization.
    pub tradeoff: String,
}

/// Run the real commit window at one width against the error curve.
///
/// The window retains at most `width` candidates per position (offered in
/// Stage-1 rank order), so the *true* candidate is kept iff
/// `true_rank < width`. A position is an error when the true candidate falls
/// beyond the retained set (brittleness). `max_load` is the peak live count
/// (load).
pub fn run_width(width: usize, positions: &[Position]) -> (f64, usize) {
    let mut max_load = 0usize;
    let mut errors = 0usize;

    for (idx, pos) in positions.iter().enumerate() {
        let mut window = CommitWindow::new(width);
        // Offer competitors in rank order until the window is full. Anything
        // beyond the retained set that holds the true candidate is an error.
        let offers = pos.competitors.min(width).min(pos.true_rank + 1);
        for _ in 0..offers {
            let ev = window.fabric.intern();
            window.fabric.assert_assumption(ev);
            let mut ev_set = std::collections::BTreeSet::new();
            ev_set.insert(ev);
            let ok = window.offer(idx, ev_set, &[], 1.0).is_ok();
            if !ok {
                break;
            }
        }
        // The true candidate is retained only if it fit in the window.
        if pos.true_rank >= width {
            errors += 1;
        }
        max_load = max_load.max(window.candidates().len());
        if !window.is_empty() {
            let _ = window.close();
        }
    }
    let total = positions.len().max(1);
    (errors as f64 / total as f64, max_load)
}

/// Sweep widths against the error curve and pick the empirical optimum.
pub fn sweep(widths: &[usize], positions: &[Position], target_error: f64) -> SweepReport {
    assert_invariant(!widths.is_empty(), "sweep_widths_nonempty", Some("aem.1"));
    let mut points = Vec::new();
    let mut optimum: Option<SweepPoint> = None;
    for &width in widths {
        let (error, max_load) = run_width(width, positions);
        assert_invariant(
            (0.0..=1.0).contains(&error),
            "sweep_error_in_unit_interval",
            Some("aem.1"),
        );
        if error <= target_error {
            match &optimum {
                None => {
                    optimum = Some(SweepPoint {
                        width,
                        error,
                        max_load,
                    })
                }
                Some(o) if width < o.width => {
                    optimum = Some(SweepPoint {
                        width,
                        error,
                        max_load,
                    });
                }
                Some(_) => {}
            }
        }
        points.push(SweepPoint {
            width,
            error,
            max_load,
        });
    }

    let tradeoff = match &optimum {
        Some(o) => format!(
            "optimum width={} (error {:.3} ≤ {target_error:.2}, load {}) — narrower loses recoverable candidates, wider admits {} candidates per position",
            o.width, o.error, o.max_load, o.max_load
        ),
        None => format!("no width in {:?} met error floor {target_error}", widths),
    };
    SweepReport {
        points,
        optimum,
        tradeoff,
    }
}
