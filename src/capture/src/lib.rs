//! Real audio capture backend (bead hearsay-four-tbn, 6lx.1 remediation).
//!
//! Replaces the `StubCapture` seam in `calibration::enrollment` with an
//! actual microphone path: raw PCM from the default input device (behind the
//! `mic` feature) is windowed and MFCC-extracted through the Stage-1 FSE
//! front end, producing exactly the `Frame` shape the enrollment contract
//! requires.
//!
//! Two layers, so the non-hardware logic is testable anywhere:
//! - [`pcm`] — pure function: i16 mono PCM + sample rate → frames
//!   (offline-testable, no features needed).
//! - [`mic::MicCapture`] — the `CaptureBackend` impl that records live audio
//!   (feature `mic`, requires a real input device at runtime).

pub mod pcm;

#[cfg(feature = "mic")]
pub mod mic;
