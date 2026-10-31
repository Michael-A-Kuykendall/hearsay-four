//! Audio decode + normalize (ul2.2): MP3/WAV -> canonical PCM 16 kHz mono.
//!
//! Handles the two corpus formats (see `docs/research/corpus/`):
//! - True RIFF PCM WAV (phrase_training, bulk_tts_1k) — `hound`.
//! - MPEG audio, mislabeled `.wav` (quality_checked_1k) and CourtListener's
//!   22.05 kHz enhanced MP3s — `symphonia` (`mp3` feature only).
//!
//! Output: canonical **PCM 16 kHz mono** `i16` frames via linear resampling
//! when the source rate differs from 16 kHz.
//!
//! Both entry points — [`decode_file`]/[`decode_to_pcm16k`] (file path) and
//! [`decode_bytes`]/[`decode_bytes_to_pcm16k`] (in-memory path, e.g. piped
//! stdin) — sniff the container from magic bytes and share the same
//! mono-collapse + resample core ([`to_pcm16k`]), so the two paths produce
//! identical PCM for identical audio bytes.

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
    Ok(sniff_format_bytes(&magic[..n]))
}

/// Detect the container format from a byte prefix (magic bytes).
///
/// Shared by the file path ([`detect_format`]) and the in-memory path, so
/// piped stdin is sniffed exactly like a file: RIFF means WAV, an MPEG
/// syncword or ID3v2 preamble means MP3, anything else is [`AudioFormat::Unknown`].
pub fn sniff_format_bytes(prefix: &[u8]) -> AudioFormat {
    let n = prefix.len();
    if n >= 4 && &prefix[..4] == b"RIFF" {
        return AudioFormat::Wav;
    }
    // MPEG audio syncword is 11 bits of 1s (0xFFEx for Layer III); ID3v2
    // preamble also implies MP3.
    if (n >= 2 && prefix[0] == 0xFF && prefix[1] & 0xE0 == 0xE0)
        || (n >= 3 && prefix[..3] == *b"ID3")
    {
        return AudioFormat::Mp3;
    }
    AudioFormat::Unknown
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

/// Probe an MP3 [`MediaSourceStream`](symphonia::core::io::MediaSourceStream)
/// and decode every packet to interleaved `i16` via the shared packet loop.
fn decode_mp3_stream(
    mss: symphonia::core::io::MediaSourceStream,
    display: &str,
) -> Result<PcmAudio, String> {
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("{display}: MP3 probe error: {e}"))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| format!("{display}: no audio track found"))?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(
            &track.codec_params,
            &symphonia::core::codecs::DecoderOptions::default(),
        )
        .map_err(|e| format!("{display}: MP3 decoder init error: {e}"))?;

    decode_mp3_packets(&mut format, &mut decoder, display)
}

/// Shared symphonia packet loop: decode every packet to interleaved `i16`,
/// skipping corrupt packets but failing the whole clip on demux errors and
/// on a mid-stream sample-rate change.
fn decode_mp3_packets(
    format: &mut Box<dyn symphonia::core::formats::FormatReader>,
    decoder: &mut Box<dyn symphonia::core::codecs::Decoder>,
    display: &str,
) -> Result<PcmAudio, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::errors::Error as SymError;

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
                return Err(format!("{display}: MP3 demux error: {e}"));
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
                            "{display}: sample rate changes mid-stream ({r} -> {rate})"
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
                return Err(format!("{display}: MP3 decode error: {e}"));
            }
        }
    }
    if samples.is_empty() {
        return Err(format!("{display}: decoded zero frames"));
    }
    Ok(PcmAudio {
        sample_rate: sample_rate.unwrap_or(0),
        channels: channels.max(1),
        samples,
    })
}

/// Decode an MPEG audio file (MP1/2/2.5 Layer III, incl. ADTS-ish streams and
/// mislabeled `.wav` files) via `symphonia`.
pub fn decode_mp3(path: &Path) -> Result<PcmAudio, String> {
    use symphonia::core::io::MediaSourceStream;

    let display = path.display().to_string();
    let file = File::open(path).map_err(|e| format!("{display}: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    decode_mp3_stream(mss, &display)
}

/// Decode MPEG audio from an in-memory buffer via `symphonia`, sharing the
/// exact packet loop the file path uses, so identical bytes decode to
/// identical PCM.
pub fn decode_mp3_bytes(bytes: &[u8]) -> Result<PcmAudio, String> {
    use symphonia::core::io::MediaSourceStream;

    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    decode_mp3_stream(mss, "stdin")
}

/// Decode a 16-bit PCM WAV from an in-memory buffer via `hound`.
pub fn decode_wav_bytes(bytes: &[u8]) -> Result<PcmAudio, String> {
    let reader =
        hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|e| format!("stdin: {e}"))?;
    let spec = reader.spec();
    if spec.bits_per_sample != 16 {
        return Err(format!(
            "stdin: unsupported bits_per_sample {} (must be 16-bit PCM)",
            spec.bits_per_sample
        ));
    }
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| format!("stdin sample error: {e}"))?;
    Ok(PcmAudio {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        samples,
    })
}

/// Decode in-memory audio bytes, dispatching on the sniffed container format
/// exactly like [`decode_file`]: RIFF bytes decode as WAV, MPEG-sync/ID3
/// bytes decode as MP3, anything else is an error (never guessed).
pub fn decode_bytes(bytes: &[u8]) -> Result<PcmAudio, String> {
    match sniff_format_bytes(bytes) {
        AudioFormat::Wav => decode_wav_bytes(bytes),
        AudioFormat::Mp3 => decode_mp3_bytes(bytes),
        AudioFormat::Unknown => {
            Err("stdin: unrecognized audio container (not RIFF/WAV, not MPEG sync)".to_string())
        }
    }
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
/// Probe a WAV header without decoding samples: `(spec, frames)`.
/// The single header-reading helper for non-corpus crates (RS-1 §5: all WAV
/// reads live in corpus, so the CI grep for `hound::WavReader` stays green
/// outside this crate).
pub fn wav_probe(path: &Path) -> Result<(hound::WavSpec, u32), String> {
    let r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((r.spec(), r.len()))
}

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

/// Shared canonicalization core: collapse to mono, resample to 16 kHz, and
/// reject empty decodes. Used by BOTH the file path and the in-memory path,
/// so identical audio bytes produce identical PCM whichever way they arrive.
pub fn to_pcm16k(audio: &PcmAudio) -> Result<Pcm16kAudio, String> {
    let mono = to_mono(audio);
    let frames = resample_linear(&mono, audio.sample_rate, TARGET_SAMPLE_RATE);
    if frames.is_empty() {
        return Err("decoded zero frames".to_string());
    }
    Ok(Pcm16kAudio {
        sample_rate: TARGET_SAMPLE_RATE,
        channels: TARGET_CHANNELS,
        frames,
    })
}

/// Decode in-memory audio bytes (MP3 or WAV, sniffed) into canonical
/// **PCM 16 kHz mono** frames — the in-memory twin of [`decode_to_pcm16k`].
/// Enforces `decode_produces_pcm_16k`.
pub fn decode_bytes_to_pcm16k(bytes: &[u8]) -> Result<Pcm16kAudio, String> {
    let audio = decode_bytes(bytes)?;
    let pcm = to_pcm16k(&audio).map_err(|e| format!("stdin: {e}"))?;
    assert_invariant(
        audio.sample_rate > 0,
        DECODE_PRODUCES_PCM_16K,
        Some("stdin"),
    );
    Ok(pcm)
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
