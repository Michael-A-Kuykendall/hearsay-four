//! Condition B gate tests (39c.4): accent-signature carry-forward.
//!
//! Asserts the treatment's mechanism per accent-feedback-semantics.md:
//! - signature updates utterance-over-utterance (bounded EMA, §3)
//! - signature feeds back into scoring (§4 bias)
//! - update is bounded (INV `signature_bounded`)
//! - Condition B starts identical to A (zero signature) then diverges as
//!   evidence accumulates (INV `condition_b_carry_forward`)

use crate::{
    ACCENT_EMA_ALPHA, ACCENT_FEEDBACK_BETA, ConditionA, ConditionB, FEATURE_DIM, ScoreError,
    decode_session_audio, score_session_utterance_b, slice_pcm,
};
use calibration::{Frame, fit_baseline};

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
fn condition_b_signature_starts_zero() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let b = ConditionB::new(baseline);
    assert_eq!(b.signature().len(), FEATURE_DIM);
    assert!(
        b.signature().iter().all(|&v| v == 0.0),
        "fresh signature is zero"
    );
    assert_eq!(b.utterance_count(), 0);
}

#[test]
fn condition_b_carry_forward_updates_signature() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let mut b = ConditionB::new(baseline);
    let utt = synthetic_frames(40, 1.2); // displaced +0.2 from baseline mean 1.0

    let first = b.score_utterance(&utt).unwrap();
    assert_eq!(b.utterance_count(), 1);
    // EMA: Δ ← (1−α)·0 + α·δ = 0.2·0.2 = 0.04
    let expect = ACCENT_EMA_ALPHA * 0.2;
    assert!(
        (b.signature()[0] - expect).abs() < 1e-9,
        "signature moved toward displacement, got {} want {}",
        b.signature()[0],
        expect
    );

    // Second + third identical utterances: feedback bias drives the score
    // monotonically down as the signature accumulates evidence.
    let second = b.score_utterance(&utt).unwrap();
    assert_eq!(b.utterance_count(), 2);
    let third = b.score_utterance(&utt).unwrap();
    assert_eq!(b.utterance_count(), 3);
    assert!(
        second < first,
        "feedback bias lowers score after first update: {} vs {}",
        second,
        first
    );
    assert!(
        third < second,
        "score keeps improving as signature converges: {} vs {}",
        third,
        second
    );
    // The treatment changed the model, so B's score on the final pass is
    // strictly better than its own first-pass score.
    assert!(third < first);
}

#[test]
fn condition_b_same_as_condition_a_at_zero_signature() {
    // With a zero signature, B scores identically to A: they diverge only as
    // carry-forward evidence accumulates (INV `condition_b_carry_forward`,
    // `measurement_symmetry` at equal state).
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let a = ConditionA::new(baseline.clone());
    let mut b = ConditionB::new(baseline);
    let utt = synthetic_frames(10, 1.0);
    assert_eq!(
        a.score_utterance(&utt).unwrap(),
        b.score_utterance(&utt).unwrap()
    );
}

#[test]
fn condition_b_ema_converges_toward_realized_mean() {
    // Feed a fixed displacement for many utterances: signature must converge
    // to the realized displacement (INV `feedback_converges`).
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let mut b = ConditionB::new(baseline);
    let displaced = synthetic_frames(20, 3.0); // δ = 2.0
    for _ in 0..200 {
        b.score_utterance(&displaced).unwrap();
    }
    assert!(
        (b.signature()[0] - 2.0).abs() < 1e-6,
        "EMA converges to realized displacement, got {}",
        b.signature()[0]
    );
}

#[test]
fn condition_b_signature_stays_bounded() {
    // A single outlier utterance moves the signature by at most α·|δ| (INV
    // `signature_bounded`); it cannot diverge the EMA.
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let mut b = ConditionB::new(baseline);
    let wild = synthetic_frames(5, 50.0); // δ = 49.0
    b.score_utterance(&wild).unwrap();
    for v in b.signature() {
        assert!(v.is_finite());
        assert!(
            v.abs() <= 49.0 * ACCENT_EMA_ALPHA + 1e-9,
            "single-step EMA bound"
        );
    }
}

#[test]
fn condition_b_empty_utterance_returns_err() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let mut b = ConditionB::new(baseline);
    assert_eq!(
        b.score_utterance(&[]).unwrap_err(),
        ScoreError::EmptyUtterance
    );
}

#[test]
fn condition_b_invalid_rates_rejected() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    assert!(ConditionB::with_rates(baseline.clone(), 0.0, 1.0).is_err());
    assert!(ConditionB::with_rates(baseline.clone(), 1.5, 1.0).is_err());
    assert!(ConditionB::with_rates(baseline, 0.2, 0.0).is_err());
}

#[test]
fn condition_b_scores_real_session_utterance_finite() {
    let audio = match decode_session_audio() {
        Ok(audio) => audio,
        Err(e) => {
            eprintln!("SKIP: session audio absent (gitignored data/session/audio): {e}");
            return;
        }
    };
    assert_eq!(audio.sample_rate, 16_000);
    let windows = fse::window_audio(&audio.frames, 16_000);
    let features = fse::extract_features(&windows, 16_000).expect("features");
    let baseline = fit_baseline(&features[..100]).expect("baseline");
    let mut b = ConditionB::new(baseline.clone());
    let a = ConditionA::new(baseline);
    // Same slice for A and B (first session row, bucket 0 idx 0).
    let (a_frames, a_score) =
        crate::score_session_utterance(&a, &audio.frames, 16_000, 1691.865, 1834.940).unwrap();
    let (b_frames, b_score) =
        score_session_utterance_b(&mut b, &audio.frames, 16_000, 1691.865, 1834.940).unwrap();
    assert_eq!(
        a_frames.len(),
        b_frames.len(),
        "same audio slice (symmetry)"
    );
    assert!(a_score.is_finite());
    assert!(b_score.is_finite());
}

#[test]
fn condition_b_slice_pcm_bounds_reused() {
    // Condition B reuses the shared slicing code path (INV
    // `measurement_symmetry`).
    let pcm: Vec<i16> = vec![0; 16000];
    assert!(slice_pcm(&pcm, 16_000, 1.0, 0.5).is_err());
}

#[test]
fn condition_b_signature_uses_correct_update_math() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let mut b = ConditionB::new(baseline);
    let utt = synthetic_frames(10, 1.2);
    b.score_utterance(&utt).unwrap();
    let ideal = ACCENT_EMA_ALPHA * 0.2; // α·δ
    assert!((b.signature()[0] - ideal).abs() < 1e-9);
    assert_eq!(ACCENT_FEEDBACK_BETA, 1.0); // preregistered
}
