//! Audio decode + normalize (ul2.2): MP3/WAV -> canonical PCM 16 kHz mono.
//!
//! Handles the two corpus formats (see `docs/research/corpus/`):
//! - True RIFF PCM WAV (phrase_training, bulk_tts_1k) — `hound`.
//! - MPEG audio, mislabeled `.wav` (quality_checked_1k) and CourtListener's
//!   22.05 kHz enhanced MP3s — `minimp3`.
//!
//! Output: canonical **PCM 16 kHz mono** `i16` frames via linear resampling
//! when the source rate differs from 16 kHz.

use std::fs::File;
use std::path::Path;

use invariant_ppt::assert_invariant;

use crate::invariants::DECODE_PRODUCES_PCM_16K;

/// The canonical corpus sample rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
/// The canonical channel count.
pub const TARGET_CHANNELS: u16 = 1;

/// Decoded audio with its source format metadata.
#[derive(Debug, Clone)]
pub struct PcmAudio {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples in source channel order.
    pub samples: Vec<i16>,
}

impl PcmAudio {
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

/// Container format detected from magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Unknown,
}

/// Detect the container format from the first bytes of the file.
pub fn detect_format(path: &Path) -> Result<AudioFormat, String> {
    let mut magic = [0u8; 4];
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    use std::io::Read;
    let n = file.read(&mut magic).unwrap_or(0);
    if n >= 4 && &magic[..4] == b"RIFF" {
        return Ok(AudioFormat::Wav);
    }
    // MPEG audio syncword is 11 bits of 1s (0xFFEx for Layer III); ID3v2
    // preamble also implies MP3.
    if (n >= 2 && magic[0] == 0xFF && magic[1] & 0xE0 == 0xE0) || (n >= 3 && magic[..3] == *b"ID3")
    {
        return Ok(AudioFormat::Mp3);
    }
    Ok(AudioFormat::Unknown)
}

/// Decode a WAV file to PCM via `hound` (16-bit PCM formats).
pub fn decode_wav(path: &Path) -> Result<PcmAudio, String> {
    let reader = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.bits_per_sample != 16 {
        return Err(format!(
            "{}: unsupported bits_per_sample {} (corpus is 16-bit PCM)",
            path.display(),
            spec.bits_per_sample
        ));
    }
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(PcmAudio {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        samples,
    })
}

/// Decode an MPEG audio file (MP1/2/2.5 Layer III, incl. ADTS-ish streams and
/// mislabeled `.wav` files) via `symphonia`.
pub fn decode_mp3(path: &Path) -> Result<PcmAudio, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    use symphonia::core::errors::Error as SymError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("{}: MP3 probe error: {e}", path.display()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| format!("{}: no audio track found", path.display()))?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(
            &track.codec_params,
            &symphonia::core::codecs::DecoderOptions::default(),
        )
        .map_err(|e| format!("{}: MP3 decoder init error: {e}", path.display()))?;

    let mut samples: Vec<i16> = Vec::new();
    let mut sample_rate: Option<u32> = None;
    let mut channels: u16 = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // Clean end-of-stream.
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => {
                return Err(format!("{}: MP3 demux error: {e}", path.display()));
            }
        };
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let rate = spec.rate;
                match sample_rate {
                    None => sample_rate = Some(rate),
                    Some(r) if r != rate => {
                        return Err(format!(
                            "{}: sample rate changes mid-stream ({} -> {})",
                            path.display(),
                            r,
                            rate
                        ));
                    }
                    _ => {}
                }
                channels = channels.max(spec.channels.count() as u16);
                let mut sbuf = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
                sbuf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(sbuf.samples());
            }
            // Skip corrupt packets instead of failing the whole clip.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => {
                return Err(format!("{}: MP3 decode error: {e}", path.display()));
            }
        }
    }
    if samples.is_empty() {
        return Err(format!("{}: decoded zero frames", path.display()));
    }
    Ok(PcmAudio {
        sample_rate: sample_rate.unwrap_or(0),
        channels: channels.max(1),
        samples,
    })
}

/// Decode a file, dispatching on its detected container format.
pub fn decode_file(path: &Path) -> Result<PcmAudio, String> {
    match detect_format(path)? {
        AudioFormat::Wav => decode_wav(path),
        AudioFormat::Mp3 => decode_mp3(path),
        AudioFormat::Unknown => Err(format!(
            "{}: unrecognized audio container (not RIFF/WAV, not MPEG sync)",
            path.display()
        )),
    }
}

/// Collapse interleaved multi-channel samples to mono by averaging.
pub fn to_mono(audio: &PcmAudio) -> Vec<i16> {
    let ch = audio.channels as usize;
    if ch <= 1 {
        return audio.samples.clone();
    }
    audio
        .samples
        .chunks_exact(ch)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / ch as i32) as i16
        })
        .collect()
}

/// Linear-interpolation resampler. Identity when `src_rate == dst_rate`.
pub fn resample_linear(samples: &[i16], src_rate: u32, dst_rate: u32) -> Vec<i16> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = dst_rate as f64 / src_rate as f64;
    let dst_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(dst_len);
    for i in 0..dst_len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let a = samples[idx.min(samples.len() - 1)] as f64;
        let b = samples[(idx + 1).min(samples.len() - 1)] as f64;
        out.push(
            (a + (b - a) * frac)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16,
        );
    }
    out
}

/// Decode any corpus audio file into canonical **PCM 16 kHz mono** frames
/// (ul2.2 output). Enforces `decode_produces_pcm_16k`.
///
/// Output is mono by construction ([`to_mono`]) and 16 kHz by construction
/// ([`resample_linear`]); the invariant guards the precondition that the
/// source rate is known, so the resample ratio is well-defined.
pub fn decode_to_pcm_16k_mono(path: &Path) -> Result<Vec<i16>, String> {
    let audio = decode_file(path)?;
    let mono = to_mono(&audio);
    let frames = resample_linear(&mono, audio.sample_rate, TARGET_SAMPLE_RATE);
    assert_invariant(
        audio.sample_rate > 0,
        DECODE_PRODUCES_PCM_16K,
        Some(path.to_string_lossy().as_ref()),
    );
    Ok(frames)
}

/// Full decoded form of [`decode_to_pcm_16k_mono`] with rate/channel proof.
pub fn decode_to_pcm16k(path: &Path) -> Result<Pcm16kAudio, String> {
    let frames = decode_to_pcm_16k_mono(path)?;
    Ok(Pcm16kAudio {
        sample_rate: TARGET_SAMPLE_RATE,
        channels: TARGET_CHANNELS,
        frames,
    })
}

/// Canonical corpus audio: PCM 16 kHz mono `i16` frames.
#[derive(Debug, Clone)]
pub struct Pcm16kAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: Vec<i16>,
}

impl Pcm16kAudio {
    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.frames.len() as f64 / self.sample_rate as f64
    }
}
