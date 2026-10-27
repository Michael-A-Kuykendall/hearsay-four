//! D3 front-end feed tests (bead hearsay-four-7eh): real Stage-1 FSE
//! features into [`crate::fit_baseline_checked`] (`frames in -> Gaussian
//! baseline out`).
//!
//! Data rule: in-repo Oyez-derived clip (`data/demo/sample.wav`, SCOTUS oral
//! argument, public domain) + synthetic PCM only. No network, no hardware.
//! Model-free: classical MFCC + closed-form Gaussian fit, no pretrained
//! anything.

use super::*;
use std::path::PathBuf;

/// In-repo Oyez-derived fixture clip (see `data/demo/README.md`).
fn oyez_clip_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/demo/sample.wav")
}

/// Real front-end frames for the fixture clip: decode -> window -> 39-dim
/// MFCC, exactly the Stage-1 path production enrollment uses.
fn oyez_frames() -> Vec<Frame> {
    let audio =
        corpus::audio::decode_to_pcm16k(&oyez_clip_path()).expect("decode in-repo Oyez clip");
    assert_eq!(audio.sample_rate, 16_000, "fixture is canonical 16 kHz");
    let windows = fse::window_audio(&audio.frames, 16_000);
    assert!(!windows.is_empty(), "clip yields windows");
    fse::extract_features(&windows, 16_000).expect("real FSE features")
}

/// Golden mean: `fit_baseline` over the fixture clip's FSE frames (8 s mono
/// 16 kHz -> 798 frames). Deterministic classical fit; any front-end drift
/// changes these bits.
const GOLDEN_MEAN: [f64; FEATURE_DIM] = [
    7.390360039760217,
    -0.8422362100460798,
    1.3599037226082498,
    -1.0403176770348639,
    -2.8954916878569303,
    -2.5264308616004394,
    -0.2897238073330265,
    -0.8259018144748778,
    0.19608459623879598,
    0.14364807765522866,
    -0.9290252191800115,
    -0.5864663456738586,
    -1.0697292209120712,
    0.0015236468067391304,
    0.006897002554850661,
    0.010005708150031743,
    0.007782668159412407,
    -0.00553550294163007,
    -0.004359694705691211,
    5.959403849904247e-5,
    -0.0021318855495845715,
    -0.0020687111906723655,
    -0.0016914495128826587,
    0.0023706304524845827,
    0.0014739326259332964,
    -0.0026564279612108013,
    -0.0006935081001255646,
    -0.001280217810505708,
    0.0001361750237155637,
    0.0012295973864633798,
    0.0001268674617471716,
    -0.00046138601342286177,
    -0.000205175955154181,
    0.0007625406236550138,
    5.927926895791552e-5,
    -0.0004662053589764951,
    -0.0002532581268436626,
    -0.0001471966082562553,
    -0.00024975328394028423,
];

/// Golden variance: same fit as [`GOLDEN_MEAN`]. Minimum 0.0138 — orders of
/// magnitude above [`crate::DEGENERATE_VARIANCE_CEIL`] (1e-6), so real speech
/// never trips the silence reject.
const GOLDEN_VARIANCE: [f64; FEATURE_DIM] = [
    28.899216860919456,
    8.074892516822601,
    8.024590902832559,
    3.71000283734028,
    4.240340717407637,
    2.1934000348614138,
    2.0094916718692217,
    1.7463957849014895,
    1.2000273091711482,
    1.03995288344842,
    0.8399825421210404,
    1.0148042794222858,
    6.195959127588025,
    1.1822511076165652,
    0.511372177511565,
    0.30318549395087313,
    0.2122601381602982,
    0.21390727195650144,
    0.13957860343243267,
    0.1211578362667571,
    0.1180060249142296,
    0.09599680324615659,
    0.08308367654815099,
    0.07653577397321029,
    0.0687573026036541,
    0.2037808578209741,
    0.16960117803899377,
    0.0769947028171337,
    0.0473826878633248,
    0.03631660968009641,
    0.03575159691341152,
    0.02743457528400326,
    0.02206598312459685,
    0.021780552667868394,
    0.01909533581194597,
    0.015461995425235787,
    0.014993950946054293,
    0.013877619574168143,
    0.03359625801722342,
];

/// (1) Recorded Oyez-clip frames produce a valid baseline via the strict
/// front-end entry point — the D3 wiring itself.
#[test]
fn frontend_oyez_clip_fits_valid_baseline() {
    let frames = oyez_frames();
    // 8 s @ 16 kHz, 25 ms window / 10 ms shift: (128000 - 400) / 160 + 1.
    assert_eq!(frames.len(), 798, "fixture clip yields 798 FSE frames");
    let baseline = fit_baseline_checked(&frames).expect("real speech fits");
    assert!(baseline.is_valid());
    assert_eq!(baseline.frame_count, frames.len());
    assert!(baseline.variance.iter().all(|&v| v > 0.0));
}

/// (1a) Golden mean fixture: the classical fit over the Oyez clip is
/// bit-stable — front-end drift fails here, not downstream.
#[test]
fn frontend_oyez_golden_mean_matches_fixture() {
    let frames = oyez_frames();
    let baseline = fit_baseline_checked(&frames).expect("real speech fits");
    assert_eq!(baseline.mean.len(), GOLDEN_MEAN.len());
    for (i, (&got, &want)) in baseline.mean.iter().zip(GOLDEN_MEAN.iter()).enumerate() {
        assert!(
            (got - want).abs() < 1e-9,
            "mean[{i}] drifted: got {got}, golden {want}"
        );
    }
}

/// (1b) Golden variance fixture: same stability contract for the spread.
#[test]
fn frontend_oyez_golden_variance_matches_fixture() {
    let frames = oyez_frames();
    let baseline = fit_baseline_checked(&frames).expect("real speech fits");
    assert_eq!(baseline.variance.len(), GOLDEN_VARIANCE.len());
    for (i, (&got, &want)) in baseline
        .variance
        .iter()
        .zip(GOLDEN_VARIANCE.iter())
        .enumerate()
    {
        assert!(
            (got - want).abs() < 1e-9,
            "variance[{i}] drifted: got {got}, golden {want}"
        );
    }
}

/// (3) All-silence input is an explicit invalid-baseline error — never a
/// silent degenerate fit. (Silence FSE frames are finite and non-empty, so
/// only the zero-variance reject catches them.)
#[test]
fn frontend_silence_is_explicit_invalid_baseline_error() {
    let rate = 16_000u32;
    let silence = vec![0i16; rate as usize];
    let windows = fse::window_audio(&silence, rate);
    let frames = fse::extract_features(&windows, rate).expect("silence extracts");
    assert!(!frames.is_empty(), "silence still yields frames");
    assert!(
        frames.iter().all(|f| f.iter().all(|x| x.is_finite())),
        "silence frames are finite (log-energy clamped) — degeneracy is zero variance"
    );
    assert_eq!(
        fit_baseline_checked(&frames).unwrap_err(),
        BaselineError::DegenerateFrames
    );
}

/// Empty input is an explicit empty error on the strict path (and `None` on
/// the legacy raw core).
#[test]
fn frontend_empty_is_explicit_empty_error() {
    assert_eq!(
        fit_baseline_checked(&[]).unwrap_err(),
        BaselineError::EmptyInput
    );
    assert!(fit_baseline(&[]).is_none());
}

/// (2) Stub path under cfg still passes its old test: `StubCapture` remains
/// available to hermetic tests and produces the same valid enrollment.
#[test]
fn frontend_stub_path_under_test_utils_passes_old_test() {
    use crate::enrollment::{StubCapture, capture_enrollment};

    let mut backend = StubCapture::default();
    let enrollment = capture_enrollment(&mut backend);
    assert!(enrollment.prompt_is_fixed());
    assert!(enrollment.prompt_matched);
    assert!(enrollment.is_valid());
    assert_eq!(enrollment.frames.len(), 50);
    assert!(
        enrollment
            .frames
            .iter()
            .all(|f| f.iter().all(|&x| x == backend.base_value)),
        "stub still synthesizes its deterministic constant frames"
    );
}
