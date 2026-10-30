//! Live microphone capture via cpal (feature `mic`).
//!
//! Implements `calibration::enrollment::CaptureBackend` against the default
//! input device. Construction is fallible (device/config acquisition);
//! capture itself records a fixed-duration window, mixes down to mono i16,
//! and runs the PCM through the FSE front end.

use crate::pcm;
use calibration::enrollment::CaptureBackend;
use calibration::{FEATURE_DIM, Frame};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Errors from acquiring/running the microphone.
#[derive(Debug)]
pub enum CaptureError {
    /// No default input device present.
    NoInputDevice,
    /// Device configuration or stream setup failed.
    Setup(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NoInputDevice => write!(f, "no default input device"),
            CaptureError::Setup(msg) => write!(f, "capture setup failed: {msg}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Live mic backend. Construct once, capture repeatedly.
pub struct MicCapture {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    /// Recording duration per `capture` call.
    pub duration_ms: u64,
}

impl MicCapture {
    /// Acquire the default input device with its default config.
    pub fn new(duration_ms: u64) -> Result<Self, CaptureError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(CaptureError::NoInputDevice)?;
        let config = device
            .default_input_config()
            .map_err(|e| CaptureError::Setup(e.to_string()))?;
        Ok(MicCapture {
            device,
            config,
            duration_ms,
        })
    }
    /// Capture at an explicitly requested mono rate (e.g. 16000 Hz for the
    /// sealed instrument) instead of the device default. Fails closed when
    /// no mono input config covers `rate_hz`.
    pub fn new_with_rate(duration_ms: u64, rate_hz: u32) -> Result<Self, CaptureError> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(CaptureError::NoInputDevice)?;
        let wanted = cpal::SampleRate(rate_hz);
        let mut best: Option<cpal::SupportedStreamConfig> = None;
        let mut best_rank = u32::MAX;
        let configs = device
            .supported_input_configs()
            .map_err(|e| CaptureError::Setup(e.to_string()))?;
        for range in configs {
            if range.channels() != 1
                || range.min_sample_rate() > wanted
                || range.max_sample_rate() < wanted
            {
                continue;
            }
            let rank = match range.sample_format() {
                cpal::SampleFormat::I16 => 0,
                cpal::SampleFormat::F32 => 1,
                cpal::SampleFormat::U16 => 2,
                _ => continue,
            };
            if rank < best_rank {
                best_rank = rank;
                best = Some(range.with_sample_rate(wanted));
            }
        }
        let config = best.ok_or_else(|| {
            CaptureError::Setup(format!("no mono input config covers {rate_hz} Hz"))
        })?;
        Ok(MicCapture {
            device,
            config,
            duration_ms,
        })
    }
    /// Record one utterance and return MFCC frames (empty on stream error —
    /// the downstream `Enrollment::is_valid` invariant catches that).
    pub fn record(&mut self) -> Vec<Frame> {
        let (mono, sample_rate) = self.record_raw();
        pcm::to_frames(&mono, sample_rate).unwrap_or_default()
    }

    /// Record one utterance and return raw mono i16 PCM plus the device
    /// sample rate (empty vec when no stream could run). Powers the
    /// `record-session` bin's WAV output; `record()` is this plus the
    /// FSE front end.
    pub fn record_raw(&mut self) -> (Vec<i16>, u32) {
        let sample_format = self.config.sample_format();
        let cfg: cpal::StreamConfig = self.config.clone().into();
        let sample_rate = cfg.sample_rate.0;
        let channels = cfg.channels as usize;

        let (tx, rx) = std::sync::mpsc::channel::<Vec<i16>>();
        let err_fn = |e| eprintln!("capture stream error: {e}");

        let mut stream = match sample_format {
            cpal::SampleFormat::I16 => self
                .device
                .build_input_stream(
                    &cfg,
                    move |data: &[i16], _: &_| {
                        let _ = tx.send(data.to_vec());
                    },
                    err_fn,
                    None,
                )
                .ok(),
            cpal::SampleFormat::U16 => self
                .device
                .build_input_stream(
                    &cfg,
                    move |data: &[u16], _: &_| {
                        let _ = tx.send(data.iter().map(|&s| (s as i32 - 32768) as i16).collect());
                    },
                    err_fn,
                    None,
                )
                .ok(),
            cpal::SampleFormat::F32 => self
                .device
                .build_input_stream(
                    &cfg,
                    move |data: &[f32], _: &_| {
                        let _ = tx.send(
                            data.iter()
                                .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                                .collect(),
                        );
                    },
                    err_fn,
                    None,
                )
                .ok(),
            other => {
                eprintln!("unsupported sample format {other:?}");
                None
            }
        };

        let Some(stream) = stream.as_mut() else {
            return (Vec::new(), sample_rate);
        };
        if stream.play().is_err() {
            return (Vec::new(), sample_rate);
        }

        // Collect chunks for the recording window, mixing to mono.
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(self.duration_ms);
        let mut mono: Vec<i16> = Vec::new();
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(chunk) => mixdown(&chunk, channels, &mut mono),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // Stop capture; dropping the stream releases the device.
        let _ = stream.pause();

        (mono, sample_rate)
    }
    /// `record_raw` truncated to exactly `duration_ms` worth of frames.
    /// Bursty sources (RDP/Pulse) can deliver more samples than the wall
    /// window asks for; the excess is dropped and reported. Short takes are
    /// returned short (fail-closed retries live in the caller).
    pub fn record_exact(&mut self) -> (Vec<i16>, u32) {
        let (mut mono, rate) = self.record_raw();
        let expected = (u64::from(rate) * self.duration_ms / 1000) as usize;
        if mono.len() > expected {
            eprintln!(
                "trimmed {} excess frames ({} captured, {} expected)",
                mono.len() - expected,
                mono.len(),
                expected
            );
            mono.truncate(expected);
        }
        (mono, rate)
    }
}

impl CaptureBackend for MicCapture {
    fn capture(&mut self, _prompt: &str) -> Vec<[f64; FEATURE_DIM]> {
        self.record()
    }
}

/// Mix an interleaved multi-channel chunk down to mono, appending to `out`.
/// Incomplete trailing frames (len != channels) are skipped.
fn mixdown(chunk: &[i16], channels: usize, out: &mut Vec<i16>) {
    if channels <= 1 {
        out.extend_from_slice(chunk);
        return;
    }
    for frame in chunk.chunks(channels) {
        if frame.len() != channels {
            continue; // truncated final frame — no defensible value
        }
        let sum: i64 = frame.iter().map(|&s| s as i64).sum();
        out.push((sum / channels as i64) as i16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixdown_mono_passthrough() {
        let mut out = Vec::new();
        mixdown(&[1, 2, 3], 1, &mut out);
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn mixdown_stereo_averages_pairs() {
        let mut out = Vec::new();
        mixdown(&[0, 10, 10, 20], 2, &mut out);
        assert_eq!(out, vec![5, 15]);
    }

    #[test]
    fn mixdown_ignores_trailing_partial_frame() {
        let mut out = Vec::new();
        mixdown(&[0, 10, 99], 2, &mut out);
        assert_eq!(out, vec![5]);
    }

    #[test]
    fn live_capture_produces_valid_frames_when_device_present() {
        // Soft-skip on headless machines: no input device is an expected
        // environment, not a failure. With a device present this records
        // ~1s and asserts the full enrollment contract.
        let Ok(mut backend) = super::MicCapture::new(1000) else {
            eprintln!("SKIP: no default input device; live capture untested here");
            return;
        };
        let frames = backend.capture("unused-here-prompt-contract-is-upstream");
        assert!(pcm::frames_valid(&frames), "live frames must be valid");
    }
}
