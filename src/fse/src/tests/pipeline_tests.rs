//! Stage 1 FSE gate tests (d5n.1): windowing, 39-dim features, scoring,
//! empty-input safety, and a real 16 kHz PCM corpus clip end to end.

use crate::{
    ARPABET, CLOSED_VOCABULARY, FEATURE_DIM, FeatureError, SHIFT_MS, WINDOW_MS, extract_features,
    process_clip, score_frame, window_audio, window_len,
};
use calibration::{Frame, fit_baseline};

/// Synthetic PCM: a constant-amplitude tone-ish buffer at a given rate.
fn synth_pcm(rate: u32, seconds: f64) -> Vec<i16> {
    let n = (rate as f64 * seconds).round() as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / rate as f64;
            (0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() * i16::MAX as f64) as i16
        })
        .collect()
}

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
fn window_audio_splits_25ms_at_10ms_shift() {
    let rate = 16_000u32;
    let samples = synth_pcm(rate, 1.0);
    let frames = window_audio(&samples, rate);
    let window = window_len(rate);
    let shift = (rate as f64 * SHIFT_MS / 1000.0).round() as usize;
    let expected = (samples.len() - window) / shift + 1;
    assert_eq!(frames.len(), expected, "frame count from window/shift math");
    assert!(
        frames.iter().all(|f| f.len() == window),
        "every window is 25 ms"
    );
}

#[test]
fn window_audio_applies_hamming() {
    let rate = 16_000u32;
    // Constant DC signal; Hamming attenuates the window edges.
    let dc: Vec<i16> = vec![i16::MAX / 2; window_len(rate)];
    let frames = window_audio(&dc, rate);
    let f = &frames[0];
    let center = f[f.len() / 2].abs();
    let edge = f[0].abs();
    assert!(edge > 0.0, "edge sample non-zero");
    assert!(
        center > edge * 1.5,
        "Hamming center much stronger than edge"
    );
}

#[test]
fn extract_features_produces_39dim_finite() {
    let rate = 16_000u32;
    let samples = synth_pcm(rate, 0.5);
    let windows = window_audio(&samples, rate);
    let frames = extract_features(&windows, rate).expect("features");
    assert_eq!(frames.len(), windows.len());
    for f in &frames {
        assert_eq!(f.len(), FEATURE_DIM, "INV: feature_vector_length");
        assert!(f.iter().all(|x| x.is_finite()), "no NaN/inf components");
    }
}

#[test]
fn extract_features_empty_returns_err_no_panic() {
    let err = extract_features(&[], 16_000).unwrap_err();
    assert_eq!(err, FeatureError::EmptyInput);
    let err = extract_features(&[vec![0.0; 0]], 16_000).unwrap_err();
    assert_eq!(err, FeatureError::InvalidWindow(0));
}

#[test]
fn window_audio_empty_returns_empty_no_panic() {
    assert!(window_audio(&[], 16_000).is_empty());
    // Too-short input also yields no frames, no panic.
    assert!(window_audio(&[0i16; 10], 16_000).is_empty());
}

#[test]
fn score_frame_returns_finite_nll_against_calibration_baseline() {
    let baseline = fit_baseline(&synthetic_frames(100, 1.5)).expect("baseline");
    let same = synthetic_frames(1, 1.5)[0];
    let far = synthetic_frames(1, 7.5)[0];
    let same_nll = score_frame(&baseline, &same);
    let far_nll = score_frame(&baseline, &far);
    assert!(same_nll.is_finite() && far_nll.is_finite());
    assert!(
        same_nll < far_nll,
        "same distribution scores lower than far"
    );
}

#[test]
fn process_clip_real_16khz_corpus_clip() {
    if !corpus::manifest::CorpusManifest::is_available() {
        eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
        return;
    }
    let root = corpus::manifest::CorpusManifest::root();
    let path = root.join("data/phrase_training/audio/phrase_000000.wav");
    let audio = corpus::audio::decode_to_pcm16k(&path).expect("decode real clip");
    assert_eq!(audio.sample_rate, 16_000);

    let windows = window_audio(&audio.frames, 16_000);
    let features = extract_features(&windows, 16_000).expect("features");
    assert!(!features.is_empty());

    // Enrollment on the first 100 frames; score the whole clip.
    let baseline = fit_baseline(&features[..100]).expect("baseline");
    let scored = process_clip(&audio.frames, 16_000, &baseline).expect("pipeline");
    assert_eq!(scored.len(), features.len());
    assert!(scored.iter().all(|s| s.nll.is_finite()));
    assert!(scored.iter().all(|s| s.features.len() == FEATURE_DIM));
}

#[test]
fn alphabet_is_fixed_39_phone_arpabet() {
    assert_eq!(ARPABET.len(), 39);
    assert_eq!(ARPABET[0], "AA");
    assert_eq!(ARPABET[38], "ZH");
    let mut sorted = ARPABET.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 39, "no duplicate phones");
}

#[test]
fn window_ms_and_shift_ms_match_spec() {
    assert_eq!(WINDOW_MS, 25.0);
    assert_eq!(SHIFT_MS, 10.0);
    assert_eq!(window_len(16_000), 400, "25 ms at 16 kHz = 400 samples");
}

#[test]
fn closed_vocabulary_is_30_words_subsetting_phrase_training() {
    // ~30-word closed vocabulary, every word present in the phrase_training
    // transcripts (train_phrases.tsv), lower-case, no duplicates.
    assert_eq!(CLOSED_VOCABULARY.len(), 30);
    let mut sorted = CLOSED_VOCABULARY.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 30, "no duplicate words");
    assert!(
        CLOSED_VOCABULARY
            .iter()
            .all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase()))
    );

    if !corpus::manifest::CorpusManifest::is_available() {
        eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
        return;
    }
    let manifest = corpus::manifest::CorpusManifest::load().expect("manifest");
    let rows = manifest.rows_from(corpus::manifest::CorpusSource::PhraseTraining, None);
    let transcript: String = rows.iter().map(|r| format!("{} ", r.text)).collect();
    for word in CLOSED_VOCABULARY {
        assert!(
            transcript.split_whitespace().any(|w| w == word),
            "vocabulary word '{word}' must appear in phrase_training transcripts"
        );
    }
}
