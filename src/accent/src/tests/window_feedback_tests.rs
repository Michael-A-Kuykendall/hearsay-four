//! s3t gate tests: stage3 commit-window close → accent signature update →
//! feedback bias into the next window's Stage 1 scoring.
//!
//! DoD (bead hearsay-four-s3t):
//! - a real stage3 `CommitWindow` closes over real session audio (data/session)
//! - the winner's phoneme observation updates the accent signature exactly
//!   per `ema_update` (INV `feedback_converges`)
//! - the next window's Stage 1 per-phone scores shift by `Δ·β` (INV
//!   `signature_bounded`, §4 bias)
//! - sustained drift takes the `retrain_after_drift` path and the Tier 2
//!   signature fact stays live (INV `recascade_automatic`)

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::derive::PHONEME_COUNT;
use crate::drift::DEFAULT_TOLERANCE;
use crate::feedback::{EMA_ALPHA, FEEDBACK_BETA};
use crate::measure::{SessionRow, load_session};
use crate::window_feedback::WindowFeedback;
use crate::{DEFAULT_SESSION_ROOT, FEATURE_DIM, SESSION_ROOT_ENV, decode_session_audio};
use calibration::{Frame, fit_baseline};
use stage3::window::CommitWindow;

fn session_root() -> PathBuf {
    std::env::var(SESSION_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SESSION_ROOT))
}

/// Feature frames for one manifest row, or `None` when the slice is unusable.
fn window_frames(pcm: &[i16], row: &SessionRow) -> Option<Vec<Frame>> {
    let sliced = crate::slice_pcm(pcm, 16_000, row.start_sec, row.stop_sec).ok()?;
    let windows = fse::window_audio(&sliced, 16_000);
    if windows.is_empty() {
        return None;
    }
    fse::extract_features(&windows, 16_000).ok()
}

/// Mean Stage-1 per-phone score across a window's frames (ARPABET order).
fn mean_phone_scores(
    frames: &[Frame],
    baseline: &calibration::Baseline,
    codebook: &[fse::PhoneTemplate],
) -> Vec<f64> {
    let mut means = vec![0.0; PHONEME_COUNT];
    for frame in frames {
        for (acc, &(_, score)) in means
            .iter_mut()
            .zip(fse::score_phones(frame, baseline, codebook).iter())
        {
            *acc += score;
        }
    }
    for acc in means.iter_mut() {
        *acc /= frames.len() as f64;
    }
    means
}

/// Scalar displacement of a window's mean frame from a phone prototype.
fn displacement_of(frames: &[Frame], prototype: &Frame) -> f64 {
    let mut mean = [0.0; FEATURE_DIM];
    for frame in frames {
        for (acc, &x) in mean.iter_mut().zip(frame.iter()) {
            *acc += x;
        }
    }
    mean.iter()
        .zip(prototype.iter())
        .map(|(m, p)| m / frames.len() as f64 - p)
        .sum::<f64>()
        / FEATURE_DIM as f64
}

/// Offer the top-`n` phones of a window at one commit position; returns
/// `(fact, phoneme_ix)` pairs so the close winner maps back to a phoneme.
fn offer_top_n(window: &mut CommitWindow, means: &[f64], n: usize) -> Vec<(fabric::FactId, usize)> {
    let mut order: Vec<usize> = (0..means.len()).collect();
    order.sort_by(|&a, &b| {
        means[b]
            .partial_cmp(&means[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    order
        .into_iter()
        .take(n)
        .map(|ix| {
            let evidence = window.fabric.intern();
            window.fabric.assert_assumption(evidence);
            let mut local = BTreeSet::new();
            local.insert(evidence);
            let candidate = window
                .offer(0, local, &[], means[ix])
                .expect("offer top phone");
            (candidate.fact, ix)
        })
        .collect()
}

#[test]
fn window_feedback_window_close_updates_signature_and_biases_next_window() {
    // Real session audio (data/session mp3 + manifest, same pair the 39c
    // fail-fast tests decode).
    let audio = decode_session_audio().expect("decode session mp3");
    assert_eq!(audio.sample_rate, 16_000);
    let rows = load_session(&session_root()).expect("session manifest");
    let usable: Vec<&SessionRow> = rows
        .iter()
        .filter(|r| window_frames(&audio.frames, r).is_some())
        .take(2)
        .collect();
    assert!(usable.len() == 2, "need two scorable windows");

    // Baseline from the clip head (same convention as condition_b).
    let windows = fse::window_audio(&audio.frames, 16_000);
    let features = fse::extract_features(&windows, 16_000).expect("features");
    let baseline = fit_baseline(&features[..100]).expect("baseline");
    let codebook = fse::default_codebook();

    let mut feedback = WindowFeedback::new(DEFAULT_TOLERANCE);
    assert!(
        feedback.signature().displacements.iter().all(|&v| v == 0.0),
        "fresh loop carries no signature"
    );
    assert!(
        feedback.signature_fact_is_active(),
        "Tier 2 signature fact live from genesis"
    );

    // --- Window 1: Stage-1 scores feed a real commit window; close commits
    // --- the winner.
    let frames_a = window_frames(&audio.frames, usable[0]).expect("window 1 frames");
    let means_a = mean_phone_scores(&frames_a, &baseline, &codebook);
    let mut window_a = CommitWindow::new(8);
    let offered_a = offer_top_n(&mut window_a, &means_a, 3);
    let (winner_a, _) = window_a.close().expect("window 1 closes");
    let winner_ix = offered_a
        .iter()
        .find(|(fact, _)| *fact == winner_a.fact)
        .expect("winner is an offered candidate")
        .1;
    assert!(window_a.fabric.is_active(winner_a.fact));

    // --- Window-1 close folds into the signature exactly per ema_update.
    let disp_a = displacement_of(&frames_a, &codebook[winner_ix].mean);
    assert!(disp_a.is_finite(), "real-audio displacement finite");
    assert!(disp_a != 0.0, "real audio moves the signature");
    let fired_a = feedback.observe_commit(winner_ix, disp_a);
    assert!(!fired_a, "one window cannot trip the 3-of-5 drift guard");
    assert!(
        (feedback.signature().delta(winner_ix) - EMA_ALPHA * disp_a).abs() < 1e-12,
        "signature EMA-updates toward the committed winner"
    );
    for (ix, &d) in feedback.signature().displacements.iter().enumerate() {
        if ix != winner_ix {
            assert_eq!(d, 0.0, "unobserved phonemes do not move");
        }
    }

    // --- Window 2: the next window's Stage-1 scores shift by Δ·β.
    let frames_b = window_frames(&audio.frames, usable[1]).expect("window 2 frames");
    let raw_b = fse::score_phones(&frames_b[0], &baseline, &codebook);
    let biased_b = feedback.score_next_window(&raw_b);
    assert_eq!(raw_b.len(), biased_b.len());
    let phone_name = fse::ARPABET[winner_ix];
    let raw_w = raw_b
        .iter()
        .find(|(p, _)| *p == phone_name)
        .expect("winner phone scored")
        .1;
    let biased_w = biased_b
        .iter()
        .find(|(p, _)| *p == phone_name)
        .expect("winner phone biased")
        .1;
    assert!(
        (biased_w - (raw_w + feedback.signature().delta(winner_ix) * FEEDBACK_BETA)).abs() < 1e-12,
        "next-window score = raw + Δ·β"
    );
    assert!(
        (biased_w - raw_w).abs() > 0.0,
        "feedback shifts the next window's scores"
    );

    // --- Window-2 close: repeated evidence converges the signature
    // --- monotonically toward the realized mean (INV `feedback_converges`).
    let means_b = mean_phone_scores(&frames_b, &baseline, &codebook);
    let mut window_b = CommitWindow::new(8);
    let offered_b = offer_top_n(&mut window_b, &means_b, 3);
    let (winner_b, _) = window_b.close().expect("window 2 closes");
    let winner_ix_b = offered_b
        .iter()
        .find(|(fact, _)| *fact == winner_b.fact)
        .expect("winner is an offered candidate")
        .1;
    let disp_b = displacement_of(&frames_b, &codebook[winner_ix_b].mean);
    let dist_before = (disp_b - feedback.signature().delta(winner_ix_b)).abs();
    let fired_b = feedback.observe_commit(winner_ix_b, disp_b);
    assert!(!fired_b, "two windows cannot trip the 3-of-5 guard");
    let dist_after = (disp_b - feedback.signature().delta(winner_ix_b)).abs();
    assert!(
        dist_after <= dist_before + 1e-12,
        "signature converges toward repeated evidence"
    );
    assert!(
        feedback
            .signature()
            .displacements
            .iter()
            .all(|v| v.is_finite()),
        "signature_bounded"
    );
    assert!(
        feedback.signature_fact_is_active(),
        "Tier 2 signature fact stays live"
    );
}

#[test]
fn window_feedback_drift_triggers_retrain_path() {
    // Sustained deviating evidence trips the 3-of-5 guard; the signature is
    // retrained through the §5 retract-and-recascade path.
    let mut feedback = WindowFeedback::new(DEFAULT_TOLERANCE);
    let mut ever_fired = false;
    for _ in 0..5 {
        ever_fired |= feedback.observe_commit(7, 9.0);
    }
    assert!(ever_fired, "sustained deviation fires the drift trigger");
    // Re-derived from the recent committed phonemes: mean of identical 9.0
    // observations on phoneme 7, zero elsewhere.
    assert!(
        (feedback.signature().delta(7) - 9.0).abs() < 1e-9,
        "retrain re-derives from recent committed facts"
    );
    for (ix, &d) in feedback.signature().displacements.iter().enumerate() {
        if ix != 7 {
            assert_eq!(d, 0.0, "unobserved phonemes stay zero");
        }
    }
    assert!(
        feedback.signature_fact_is_active(),
        "re-derived signature fact live (recascade_automatic)"
    );
}
