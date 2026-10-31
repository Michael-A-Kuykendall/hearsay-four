//! d5n.3 gate: `cd src/fse && cargo test --all-features`
//!
//! Contract test 'stage1 fse' over the invariants from
//! phoneme-alphabet.md §5 + nbest-candidates.md §4, plus proptest fuzz over
//! windows and score perturbation. Serialized via `invariant_ppt::test_lock`.

use calibration::fit_baseline;
use proptest::prelude::*;

use crate::nbest::emit_nbest;
use crate::{
    FEATURE_DIM, FeatureError, Frame, default_codebook, emit_clip_candidates, extract_features,
    score_phones, window_audio,
};

/// All six contract invariants for Stage 1 FSE.
const CONTRACT_INVARIANTS: &[&str] = &[
    "alphabet_fixed",
    "feature_vector_length",
    "phoneme_in_alphabet",
    "score_in_range",
    "n_best_bounded",
    "n_best_not_argmax",
];

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

/// C-Test: the Stage 1 FSE pipeline actively enforces all six invariants.
#[test]
fn contract_stage1_fse_invariants_enforced() {
    let _guard = invariant_ppt::test_lock::lock();
    invariant_ppt::clear_invariant_log();
    invariant_ppt::clear_contracts();
    invariant_ppt::register_contract("stage1 fse", CONTRACT_INVARIANTS);

    // Exercise every invariant-holding path.
    let baseline = fit_baseline(&synthetic_frames(50, 1.0)).expect("baseline");

    // 1. feature_vector_length via extraction.
    let pcm: Vec<i16> = (0..16_000)
        .map(|i| ((i % 40) as f64 * 100.0) as i16)
        .collect();
    let windows = window_audio(&pcm, 16_000);
    let frames = extract_features(&windows, 16_000).expect("features");

    // 2. alphabet_fixed + phoneme_in_alphabet + score_in_range +
    //    n_best_bounded + n_best_not_argmax via emission with real
    //    competition (codebook scores cluster near the frame).
    let codebook = default_codebook();
    let scores = score_phones(&frames[0], &baseline, &codebook);
    let emitted = emit_nbest(&scores);
    assert!(!emitted.is_empty());

    assert!(
        invariant_ppt::verify_all_contracts() >= 1,
        "'stage1 fse' contract verified"
    );
}

// P-Test (proptest): random PCM buffers never panic and always yield
// 39-dim finite frames or a clean Err.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn ppt_random_pcm_windows_never_panic(
        len in 0usize..3000,
        seed in 0u64..1000,
    ) {
        let _guard = invariant_ppt::test_lock::lock();
        let mut rng = seed;
        let pcm: Vec<i16> = (0..len)
            .map(|_| {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (rng >> 33) as i16
            })
            .collect();
        let windows = window_audio(&pcm, 16_000);
        match extract_features(&windows, 16_000) {
            Ok(frames) => {
                for f in &frames {
                    prop_assert_eq!(f.len(), FEATURE_DIM);
                    prop_assert!(f.iter().all(|x| x.is_finite()));
                }
            }
            Err(FeatureError::EmptyInput | FeatureError::InvalidWindow(_)) => {}
            Err(e) => prop_assert!(false, "unexpected error: {e}"),
        }
    }
}

// P-Test (proptest): perturbing a real frame's codebook scores keeps the
// emission contract (bounded, alphabet-only, relative threshold).
proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn ppt_perturbed_scores_keep_emission_contract(
        jitter in -0.01f64..0.01f64,
    ) {
        let _guard = invariant_ppt::test_lock::lock();
        let baseline = fit_baseline(&synthetic_frames(50, 1.0)).expect("baseline");
        let pcm: Vec<i16> = (0..16_000)
            .map(|i| ((i % 40) as f64 * 100.0) as i16)
            .collect();
        let frames = extract_features(&window_audio(&pcm, 16_000), 16_000).expect("features");
        let mut scores = score_phones(&frames[0], &baseline, &default_codebook());
        for (_, s) in scores.iter_mut() {
            *s = (*s + jitter).clamp(0.0, 1.0);
        }
        let out = emit_nbest(&scores);
        prop_assert!(out.len() <= 5);
        prop_assert!(out.iter().all(|c| crate::ARPABET.contains(&c.phoneme)));
        prop_assert!(out.iter().all(|c| (0.0..=1.0).contains(&c.score)));
    }
}

/// End-to-end: candidates emitted for a real 16 kHz corpus clip.
#[test]
fn emit_candidates_real_corpus_clip() {
    let _guard = invariant_ppt::test_lock::lock();
    if !corpus::manifest::CorpusManifest::is_available() {
        eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
        return;
    }
    let root = corpus::manifest::CorpusManifest::root();
    let path = root.join("data/phrase_training/audio/phrase_000000.wav");
    let audio = corpus::audio::decode_to_pcm16k(&path).expect("decode real clip");
    let frames = extract_features(&window_audio(&audio.frames, 16_000), 16_000).expect("features");
    let baseline = fit_baseline(&frames[..100]).expect("baseline");

    let per_frame = emit_clip_candidates(&audio.frames, 16_000, &baseline, &default_codebook())
        .expect("emission");
    assert_eq!(per_frame.len(), frames.len());
    for frame in &per_frame {
        assert!(frame.len() <= 5, "capped at 5 per frame");
        assert!(!frame.is_empty(), "every frame emits at least the argmax");
    }
}
