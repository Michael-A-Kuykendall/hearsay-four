//! Stage 0 enrollment — fixed prompt + capture (task 6lx.1).
//!
//! Implements the enrollment protocol from `docs/design/calibration-utterance.md`
//! (aem.11): a fixed well-known pangram is captured per speaker, with the
//! invariant that the *same* prompt is used every time (so signatures are
//! comparable across speakers).
//!
//! The actual audio capture (mic, framing) is a backend seam — this module
//! provides the prompt contract, a capture result type, and a stub backend
//! for headless/prototype use. The real backend plugs in at the same
//! interface without changing the prompt invariant.

use crate::FEATURE_DIM;

// The fixed enrollment prompt comes from the calibration crate root
// (`docs/design/calibration-utterance.md`, aem.11). Referenced via the crate
// root to keep a single source of truth.
use crate::ENROLLMENT_PROMPT;

/// A captured enrollment: a sequence of 39-dim feature frames for one
/// utterance of the fixed prompt.
#[derive(Debug, Clone)]
pub struct Enrollment {
    /// The prompt that was spoken (must equal `ENROLLMENT_PROMPT`).
    pub prompt: String,
    /// The captured feature frames (39-dim each).
    pub frames: Vec<[f64; FEATURE_DIM]>,
    /// Whether the prompt matched the fixed one (comparability invariant).
    pub prompt_matched: bool,
}

impl Enrollment {
    /// Verify the prompt is the fixed pangram (invariant `fixed_prompt`).
    pub fn prompt_is_fixed(&self) -> bool {
        self.prompt.trim() == ENROLLMENT_PROMPT
    }

    /// Verify the enrollment is non-empty and every frame is the right dim
    /// (invariant `enrollment_nonempty`).
    pub fn is_valid(&self) -> bool {
        !self.frames.is_empty() && self.frames.iter().all(|f| f.len() == FEATURE_DIM)
    }
}

/// A capture backend seam. The prototype uses a stub that synthesizes frames;
/// the real system implements this with the actual mic + FSE front end.
pub trait CaptureBackend {
    /// Capture one utterance of the given prompt, returning feature frames.
    fn capture(&mut self, prompt: &str) -> Vec<[f64; FEATURE_DIM]>;
}

/// Stub backend for headless/prototype use: synthesizes deterministic frames
/// so the enrollment flow is exercisable without hardware.
pub struct StubCapture {
    pub frame_count: usize,
    pub base_value: f64,
}

impl Default for StubCapture {
    fn default() -> Self {
        StubCapture {
            frame_count: 50,
            base_value: 1.0,
        }
    }
}

impl CaptureBackend for StubCapture {
    fn capture(&mut self, _prompt: &str) -> Vec<[f64; FEATURE_DIM]> {
        (0..self.frame_count)
            .map(|_| {
                let mut f = [0.0; FEATURE_DIM];
                for x in f.iter_mut() {
                    *x = self.base_value;
                }
                f
            })
            .collect()
    }
}

/// Run an enrollment capture: capture the fixed prompt and validate it.
pub fn capture_enrollment(backend: &mut dyn CaptureBackend) -> Enrollment {
    let frames = backend.capture(ENROLLMENT_PROMPT);
    Enrollment {
        prompt: ENROLLMENT_PROMPT.to_string(),
        prompt_matched: true,
        frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_fixed_pangram() {
        assert_eq!(
            ENROLLMENT_PROMPT,
            "The quick brown fox jumps over the lazy dog"
        );
    }

    #[test]
    fn stub_capture_produces_valid_enrollment() {
        let mut backend = StubCapture::default();
        let e = capture_enrollment(&mut backend);
        assert!(e.prompt_is_fixed());
        assert!(e.prompt_matched);
        assert!(e.is_valid());
        assert_eq!(e.frames.len(), 50);
    }

    #[test]
    fn enrollment_flow_is_repeatable() {
        let mut backend = StubCapture::default();
        let e1 = capture_enrollment(&mut backend);
        let e2 = capture_enrollment(&mut backend);
        // Same prompt both times -> comparable signatures (invariant).
        assert_eq!(e1.prompt, e2.prompt);
        assert_eq!(e1.frames.len(), e2.frames.len());
    }
}
