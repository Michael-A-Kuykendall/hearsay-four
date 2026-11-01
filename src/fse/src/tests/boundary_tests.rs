//! SEG-1 gate tests (bead `hearsay-four-uth`): synthetic exactness, silence /
//! continuous-tone edge cases, committed courtroom-sample plausibility, and
//! a local-only Oyez rate diagnostic (skips when the gitignored clip is
//! absent — bulk audio is staging-only, never committed).

use calibration::Frame;

use crate::{
    boundaries::{detect_boundaries, BoundaryConfig},
    extract_features, window_audio,
};

/// 16 kHz sine burst at full-ish amplitude (classical fixture, no model).
fn tone(freq_hz: f64, seconds: f64) -> Vec<i16> {
    let rate = 16_000.0;
    let n = (rate * seconds).round() as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / rate;
            (8_000.0 * (2.0 * std::f64::consts::PI * freq_hz * t).sin()) as i16
        })
        .collect()
}

fn silence(seconds: f64) -> Vec<i16> {
    vec![0i16; (16_000.0 * seconds).round() as usize]
}

fn features_of(pcm: &[i16]) -> Vec<Frame> {
    let windows = window_audio(pcm, 16_000);
    assert!(!windows.is_empty(), "synthetic pcm must window");
    extract_features(&windows, 16_000).expect("synthetic pcm extracts")
}

/// Synthetic: 0.3 s 440 Hz / 0.3 s silence / 0.3 s 660 Hz / 0.3 s silence /
/// 0.3 s 440 Hz. Gap centers sit at 450 ms and 1050 ms; the detector must
/// report exactly those two boundaries within one hangover of truth.
#[test]
fn synthetic_two_gaps_yield_exact_boundaries() {
    let mut pcm = tone(440.0, 0.3);
    pcm.extend_from_slice(&silence(0.3));
    pcm.extend_from_slice(&tone(660.0, 0.3));
    pcm.extend_from_slice(&silence(0.3));
    pcm.extend_from_slice(&tone(440.0, 0.3));
    let frames = features_of(&pcm);
    let found = detect_boundaries(&frames, &BoundaryConfig::default());
    assert_eq!(
        found.len(),
        2,
        "two silence gaps give two boundaries (got {:?})",
        found.iter().map(|b| b.time_ms).collect::<Vec<_>>()
    );
    for (boundary, expected_ms) in found.iter().zip([450.0, 1050.0]) {
        assert!(
            (boundary.time_ms - expected_ms).abs() <= 50.0,
            "boundary at {:.1} ms within ±50 ms of gap center {expected_ms} ms",
            boundary.time_ms
        );
    }
}

/// Edge: 1 s of digital silence carries no dips and no onsets.
#[test]
fn all_silence_yields_no_boundaries() {
    let frames = features_of(&silence(1.0));
    let found = detect_boundaries(&frames, &BoundaryConfig::default());
    assert!(
        found.is_empty(),
        "silence must not seed boundaries (got {found:?})"
    );
}

/// Edge: 1 s of continuous tone carries no dips and no onset peaks.
#[test]
fn continuous_tone_yields_no_boundaries() {
    let frames = features_of(&tone(440.0, 1.0));
    let found = detect_boundaries(&frames, &BoundaryConfig::default());
    assert!(
        found.is_empty(),
        "steady tone must not seed boundaries (got {found:?})"
    );
}

/// Committed courtroom speech (`data/demo/sample.wav`, 8 s): the boundary
/// count must sit in the pre-registered plausibility band — nonzero (speech
/// has pauses and onsets) and under 20/s (no per-frame chatter).
#[test]
fn courtroom_sample_boundary_count_is_speech_plausible() {
    let path = std::path::Path::new("../../data/demo/sample.wav");
    let pcm = corpus::audio::decode_to_pcm16k(path).expect("demo sample decodes");
    assert_eq!(pcm.sample_rate, 16_000, "fixture is 16 kHz");
    let windows = window_audio(&pcm.frames, pcm.sample_rate);
    let frames = extract_features(&windows, pcm.sample_rate).expect("demo sample extracts");
    let found = detect_boundaries(&frames, &BoundaryConfig::default());
    eprintln!(
        "SEG-1 diagnostic: {} boundaries over 8 s sample",
        found.len()
    );
    assert!(
        (1..=160).contains(&found.len()),
        "8 s of speech yields 1..=160 boundaries (got {})",
        found.len()
    );
    let mut times: Vec<f64> = found.iter().map(|b| b.time_ms).collect();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(times, sorted, "boundaries increase in time");
    times.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
}

/// Local-only Oyez diagnostic (non-gating): first 60 s of the gitignored
/// bulk clip, skipped where the staging audio is absent. Records the raw
/// boundary rate for the bead note; asserts only time ordering.
#[test]
fn oyez_bulk_clip_boundary_rate_diagnostic() {
    let path = std::path::Path::new("../../sessions/oyez-bulk/10-218_2011/audio/run1.wav");
    if !path.exists() {
        eprintln!("SEG-1 diagnostic: Oyez staging clip absent, skipping");
        return;
    }
    let pcm = corpus::audio::decode_to_pcm16k(path).expect("oyez clip decodes");
    let take = (pcm.sample_rate as usize * 60).min(pcm.frames.len());
    let windows = window_audio(&pcm.frames[..take], pcm.sample_rate);
    let frames = extract_features(&windows, pcm.sample_rate).expect("oyez span extracts");
    let found = detect_boundaries(&frames, &BoundaryConfig::default());
    let secs = take as f64 / pcm.sample_rate as f64;
    eprintln!(
        "SEG-1 diagnostic: {} boundaries over {secs:.1} s oyez ({:.2}/s)",
        found.len(),
        found.len() as f64 / secs
    );
    let mut prev = 0usize;
    for b in &found {
        assert!(b.frame >= prev, "boundaries increase in frame order");
        prev = b.frame;
    }
}
