//! PCM → MFCC frames: the offline-testable half of the capture pipeline.
//!
//! Pure composition of the Stage-1 FSE front end (`window_audio` +
//! `extract_features`) over mono i16 PCM. No hardware, no features required.

use calibration::Frame;

/// Convert mono i16 PCM into 39-dim MFCC frames via the FSE front end.
///
/// Errors propagate (`FeatureError`) rather than panicking; empty/short
/// input yields zero frames by design (windowing rule, INV `enrollment_nonempty`
/// catches it downstream).
pub fn to_frames(samples: &[i16], sample_rate: u32) -> Result<Vec<Frame>, fse::FeatureError> {
    let windows = fse::window_audio(samples, sample_rate);
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    fse::extract_features(&windows, sample_rate)
}

/// Validate frames against the enrollment contract invariants
/// (`enrollment_nonempty`, `feature_vector_length`): non-empty, every frame
/// exactly [`FEATURE_DIM`] wide (static for [`Frame`]), all components finite.
pub fn frames_valid(frames: &[Frame]) -> bool {
    !frames.is_empty() && frames.iter().all(|f| f.iter().all(|x| x.is_finite()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sine wave at 440 Hz, 1 second, 16 kHz — enough frames for MFCC.
    fn sine(rate: u32, hz: f64, seconds: f64) -> Vec<i16> {
        let n = (rate as f64 * seconds) as usize;
        (0..n)
            .map(|i| {
                ((2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() * 8000.0) as i16
            })
            .collect()
    }

    #[test]
    fn sine_yields_valid_frames() {
        let rate = 16_000u32;
        let frames = to_frames(&sine(rate, 440.0, 1.0), rate).expect("extraction");
        assert!(
            frames_valid(&frames),
            "sine capture must satisfy enrollment frame invariants"
        );
        // 25 ms window (400 samples) / 10 ms shift (160) over 16000 samples.
        assert_eq!(frames.len(), (rate as usize - 400) / 160 + 1);
    }

    #[test]
    fn empty_pcm_gives_zero_frames_no_error() {
        assert!(to_frames(&[], 16_000).unwrap().is_empty());
    }

    #[test]
    fn short_pcm_below_window_gives_zero_frames() {
        assert!(to_frames(&[100; 100], 16_000).unwrap().is_empty());
    }

    #[test]
    fn silence_produces_finite_frames() {
        // Log-energy of digital silence must still be finite (clamped).
        let rate = 16_000u32;
        let frames = to_frames(&vec![0i16; rate as usize], rate).expect("extraction");
        assert!(!frames.is_empty());
        assert!(frames.iter().all(|f| f.iter().all(|x| x.is_finite())));
    }
}
