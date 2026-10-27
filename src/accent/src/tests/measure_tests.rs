//! 39c.5 gate tests: measure error rate A vs B and check separation.
//!
//! Asserts the INV contracts of the fail-fast measurement:
//! - `measurement_symmetry`: A and B consume the same audio + same feature
//!   code; they differ only in Condition-B's carried accent signature.
//! - `buckets_exhaustive`: every session row lands in exactly one bucket and
//!   buckets partition the session.
//! - `verdict_pre_registered`: applying the §4 rule to synthetic bucket
//!   stats yields exactly the documented PASS/FAIL decision.

use crate::measure::{BucketStat, SessionRow, apply_rule, load_session, measure_session};
use crate::{ConditionA, ConditionB, FEATURE_DIM, decode_session_audio, score_session_utterance};
use calibration::{Frame, fit_baseline};

const SESSION_ROOT: &str = "/home/michael/repos/hearsay-four/data/session";

fn synthetic_frames(n: usize, dim_value: f64) -> Vec<Frame> {
    (0..n)
        .map(|_| {
            let mut f = [0.0; FEATURE_DIM];
            for x in f.iter_mut() {
                *x = dim_value;
            }
            f
        })
        .collect()
}

#[test]
fn measure_session_loads_manifest() {
    let rows = load_session(std::path::Path::new(SESSION_ROOT)).expect("load session");
    assert!(!rows.is_empty());
    for r in &rows {
        assert!(r.bucket <= 1, "bucket {} present", r.bucket);
        assert!(r.start_sec > 0.0, "nonzero start");
    }
}

#[test]
fn measure_session_buckets_exhaustive_invariant() {
    let rows = vec![
        SessionRow {
            bucket: 0,
            idx: 0,
            start_sec: 1.0,
            stop_sec: 2.0,
            text: "word".to_string(),
        },
        SessionRow {
            bucket: 0,
            idx: 1,
            start_sec: 2.0,
            stop_sec: 3.0,
            text: "two words".to_string(),
        },
        SessionRow {
            bucket: 1,
            idx: 0,
            start_sec: 3.0,
            stop_sec: 4.0,
            text: "three".to_string(),
        },
    ];
    // 4 s of 16 kHz PCM with a signal that yields finite, distinct scores.
    let pcm: Vec<i16> = (0..(4 * 16_000)).map(|i| ((i as i16) % 7) - 3).collect();

    let enroll = synthetic_frames(100, 1.0);
    let baseline = fit_baseline(&enroll).expect("baseline");
    let a = ConditionA::new(baseline.clone());
    let mut b = ConditionB::new(baseline);

    let stats = measure_session(&a, &mut b, &pcm, 16_000, &rows).expect("measure");
    let occupied: Vec<usize> = stats.iter().map(|s| s.bucket).collect();
    assert_eq!(occupied, vec![0, 1], "buckets partition the session");
    for s in &stats {
        assert!(s.mean_a.is_finite());
        assert!(s.mean_b.is_finite());
        assert!(s.count > 0, "bucket {} populated", s.bucket);
    }
}

/// D1 enrollment-as-baseline consumption (bead hearsay-four-cz0): the wired
/// enrollment fact's payload builds the ConditionA/B pair that
/// `measure_session` scores. Synthetic PCM + StubCapture only.
#[test]
fn enrolled_baseline_feeds_measure_session() {
    use calibration::arena::{baseline_fact_active, enroll_baseline};
    use calibration::enrollment::{StubCapture, capture_enrollment};

    let enrollment = capture_enrollment(&mut StubCapture::default());
    let mut fabric = fabric::Fabric::new();
    let enrolled = enroll_baseline(&mut fabric, &enrollment).expect("enrollment wires");
    assert!(baseline_fact_active(&fabric, enrolled.fact));

    let a = ConditionA::new(enrolled.baseline.clone());
    let mut b = ConditionB::new(enrolled.baseline);

    let rows = vec![
        SessionRow {
            bucket: 0,
            idx: 0,
            start_sec: 1.0,
            stop_sec: 2.0,
            text: "word".to_string(),
        },
        SessionRow {
            bucket: 1,
            idx: 0,
            start_sec: 2.0,
            stop_sec: 3.0,
            text: "two words".to_string(),
        },
    ];
    // 4 s of 16 kHz PCM with a signal that yields finite scores.
    let pcm: Vec<i16> = (0..(4 * 16_000)).map(|i| ((i as i16) % 7) - 3).collect();

    let stats = measure_session(&a, &mut b, &pcm, 16_000, &rows).expect("measure");
    assert_eq!(stats.len(), 2, "both buckets measured");
    for s in &stats {
        assert!(s.mean_a.is_finite());
        assert!(s.mean_b.is_finite());
    }
    assert!(
        b.utterance_count() > 0,
        "B carried the wired baseline forward"
    );
}

#[test]
fn apply_rule_clear_b_separation_passes() {
    let stats = vec![bstat(0, 0.5000, 0.4800, 4), bstat(1, 0.5500, 0.4500, 4)];
    let v = apply_rule(&stats);
    assert!(v.tail_separation, "mean(B,2nd) < mean(A,2nd)");
    assert!(v.margin_two_pp, "margin >= 2pp");
    assert!(v.monotone_b, "B non-increasing, B(final) <= A(final)");
    assert!(v.pass, "verdicts: {:?}", v.checks);
}

#[test]
fn apply_rule_no_separation_fails() {
    let stats = vec![bstat(0, 0.5000, 0.5000, 4), bstat(1, 0.5000, 0.5000, 4)];
    let v = apply_rule(&stats);
    assert!(!v.pass, "identity conditions must FAIL");
    assert!(!v.tail_separation);
    assert!(!v.margin_two_pp);
}

#[test]
fn apply_rule_margin_below_two_pp_fails() {
    let stats = vec![bstat(0, 0.5000, 0.4950, 4), bstat(1, 0.5500, 0.5350, 4)];
    let v = apply_rule(&stats);
    assert!(v.tail_separation, "still separated");
    assert!(!v.margin_two_pp, "0.015 < 0.02 -> FAIL");
    assert!(!v.pass);
}

#[test]
fn apply_rule_b_not_monotone_fails() {
    let stats = vec![bstat(0, 0.5000, 0.4500, 4), bstat(1, 0.5500, 0.4900, 4)];
    let v = apply_rule(&stats);
    assert!(v.tail_separation, "0.49 < 0.55");
    assert!(v.margin_two_pp, "0.06 >= 0.02");
    assert!(!v.monotone_b, "0.49 > 0.45 -> B increased");
    assert!(!v.pass);
}

#[test]
fn measure_symmetry_b_uses_same_audio_frames() {
    let rows = load_session(std::path::Path::new(SESSION_ROOT)).expect("session");
    let audio = decode_session_audio().expect("decode");
    assert_eq!(audio.sample_rate, 16_000);

    let windows = fse::window_audio(&audio.frames, 16_000);
    let features = fse::extract_features(&windows, 16_000).expect("features");
    let baseline = fit_baseline(&features[..100]).expect("baseline");

    // Run A alone over the same slices -> ground-truth A numbers.
    let a1 = ConditionA::new(baseline.clone());
    let stats_a1 = first_bucket_a(&a1, &audio.frames, &rows);

    // Run A + B together over the same slices; A's numbers must be identical.
    let a2 = ConditionA::new(baseline);
    let mut b = ConditionB::new(a1.baseline().clone());
    let stats = measure_session(&a2, &mut b, &audio.frames, 16_000, &rows).expect("measure");
    let bucket0 = stats.iter().find(|s| s.bucket == 0).expect("bucket 0");
    assert!(
        (bucket0.mean_a - stats_a1).abs() < 1e-9,
        "A identical under symmetry, got {} vs {}",
        bucket0.mean_a,
        stats_a1
    );
    assert!(stats.iter().all(|s| s.mean_b.is_finite()));
}

fn bstat(bucket: usize, mean_a: f64, mean_b: f64, count: usize) -> BucketStat {
    BucketStat {
        bucket,
        mean_a,
        mean_b,
        count,
    }
}

fn first_bucket_a(a: &ConditionA, pcm: &[i16], rows: &[SessionRow]) -> f64 {
    let mut sum = 0.0;
    let mut n = 0;
    for r in rows.iter().filter(|r| r.bucket == 0) {
        sum += score_session_utterance(a, pcm, 16_000, r.start_sec, r.stop_sec)
            .unwrap()
            .1;
        n += 1;
    }
    sum / n as f64
}
