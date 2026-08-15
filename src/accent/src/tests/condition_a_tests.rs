//! Condition A gate tests: stateless, no cross-utterance memory.

use crate::{
    ConditionA, FEATURE_DIM, ScoreError, decode_session_audio, score_session_utterance, slice_pcm,
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
fn condition_a_scores_are_stateless_and_order_independent() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let a = ConditionA::new(baseline);
    let utt_x = synthetic_frames(40, 1.2);
    let utt_y = synthetic_frames(40, 6.0);

    let x_first = a.score_utterance(&utt_x).unwrap();
    // Interleave another utterance; X's score must not change.
    let _y = a.score_utterance(&utt_y).unwrap();
    let x_again = a.score_utterance(&utt_x).unwrap();
    assert_eq!(
        x_first, x_again,
        "no cross-utterance state (INV: condition_a_stateless)"
    );

    // Reordering the whole session changes nothing.
    let y_after = a.score_utterance(&utt_y).unwrap();
    assert_eq!(_y, y_after);

    // Different utterances get different scores (fresh, generic reference).
    assert_ne!(x_first, _y);
}

#[test]
fn condition_a_immutable_across_sessions() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let a1 = ConditionA::new(baseline.clone());
    let a2 = ConditionA::new(baseline);
    let utt = synthetic_frames(30, 2.0);
    assert_eq!(
        a1.score_utterance(&utt).unwrap(),
        a2.score_utterance(&utt).unwrap()
    );
}

#[test]
fn condition_a_empty_utterance_returns_err() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.0)).expect("baseline");
    let a = ConditionA::new(baseline);
    assert_eq!(
        a.score_utterance(&[]).unwrap_err(),
        ScoreError::EmptyUtterance
    );
}

#[test]
fn slice_pcm_bounds() {
    let pcm: Vec<i16> = vec![0; 16000];
    let s = slice_pcm(&pcm, 16_000, 0.5, 1.0).unwrap();
    assert_eq!(s.len(), 8000, "0.5s at 16 kHz");
    assert!(
        slice_pcm(&pcm, 16_000, 1.0, 0.5).is_err(),
        "stop <= start rejected"
    );
    assert!(
        slice_pcm(&pcm, 16_000, 100.0, 101.0).is_err(),
        "out of range rejected"
    );
}

#[test]
fn condition_a_scores_real_session_utterance_finite() {
    let audio = decode_session_audio().expect("decode session mp3");
    assert_eq!(audio.sample_rate, 16_000);
    assert!(audio.frames.len() > 1_000_000, "full argument decoded");

    // Enroll the generic baseline from the first 100 frames of the clip.
    let windows = fse::window_audio(&audio.frames, 16_000);
    let features = fse::extract_features(&windows, 16_000).expect("features");
    let baseline = fit_baseline(&features[..100]).expect("baseline");
    let a = ConditionA::new(baseline);

    // First session row (bucket 0, idx 0): 1691.865..1834.940s.
    let (frames, score) =
        score_session_utterance(&a, &audio.frames, 16_000, 1691.865, 1834.940).unwrap();
    assert!(!frames.is_empty());
    assert!(score.is_finite());
    // Scoring the same utterance again yields the identical score.
    let (_, score2) =
        score_session_utterance(&a, &audio.frames, 16_000, 1691.865, 1834.940).unwrap();
    assert_eq!(score, score2);
}
