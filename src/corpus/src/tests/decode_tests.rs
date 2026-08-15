//! ul2.2 gate: `cd src/corpus && cargo test decode`
//!
//! Uses real corpus samples by reference (never copied):
//! - `phrase_training/audio/phrase_000000.wav` — true 16 kHz mono PCM WAV
//! - `quality_checked_1k/abating_0707.wav` — mislabeled MPEG Layer III (24 kHz)
//! - `bulk_tts_1k/*.wav` — true 16 kHz PCM

use std::path::PathBuf;

use crate::audio::{
    AudioFormat, TARGET_CHANNELS, TARGET_SAMPLE_RATE, decode_file, decode_mp3,
    decode_to_pcm_16k_mono, decode_to_pcm16k, decode_wav, detect_format, resample_linear, to_mono,
};
use crate::manifest::CorpusManifest;

fn sample(rel: &str) -> PathBuf {
    CorpusManifest::root().join(rel)
}

#[test]
fn decode_reads_true_16khz_pcm_wav() {
    let path = sample("data/phrase_training/audio/phrase_000000.wav");
    assert_eq!(detect_format(&path).unwrap(), AudioFormat::Wav);
    let audio = decode_wav(&path).expect("hound reads the real PCM WAV");
    assert_eq!(audio.sample_rate, 16_000);
    assert_eq!(audio.channels, 1);
    assert!(audio.frame_count() > 0);
}

#[test]
fn decode_identifies_mislabeled_mp3() {
    let path = sample("data/quality_checked_1k/abating_0707.wav");
    assert_eq!(
        detect_format(&path).unwrap(),
        AudioFormat::Mp3,
        "must detect MPEG sync despite .wav extension"
    );
    let audio = decode_mp3(&path).expect("minimp3 decodes the mislabeled MP3");
    assert!(audio.sample_rate > 0, "source rate known (24 kHz)");
    assert!(audio.frame_count() > 0);
}

#[test]
fn decode_normalizes_mp3_to_16khz_mono() {
    let path = sample("data/quality_checked_1k/abating_0707.wav");
    let canonical = decode_to_pcm16k(&path).expect("canonical decode");
    assert_eq!(canonical.sample_rate, TARGET_SAMPLE_RATE);
    assert_eq!(canonical.channels, TARGET_CHANNELS);
    assert!(!canonical.frames.is_empty());
    // 24 kHz source -> 16 kHz target shrinks duration-scaled sample count.
    let src = decode_mp3(&path).unwrap();
    let ratio = TARGET_SAMPLE_RATE as f64 / src.sample_rate as f64;
    let expected = (src.frame_count() as f64 * ratio).round() as usize;
    assert!(
        (canonical.frames.len() as i64 - expected as i64).abs() <= 1,
        "frame count matches resample expectation"
    );
}

#[test]
fn decode_to_pcm_16k_mono_handles_both_formats() {
    let wav = sample("data/phrase_training/audio/phrase_000000.wav");
    let mp3 = sample("data/quality_checked_1k/abating_0707.wav");
    for p in [&wav, &mp3] {
        let frames = decode_to_pcm_16k_mono(p).expect("canonical decode");
        assert!(!frames.is_empty());
    }
}

#[test]
fn decode_audio_transcript_paired_samples_decode() {
    // A slice of the closed-vocab corpus: every sampled train row decodes.
    let m = CorpusManifest::load().expect("manifest load");
    let mut rows: Vec<_> = m
        .rows_from(crate::manifest::CorpusSource::QualityChecked, None)
        .into_iter()
        .take(5)
        .collect();
    rows.extend(
        m.rows_from(crate::manifest::CorpusSource::PhraseTraining, None)
            .into_iter()
            .take(2),
    );
    for row in rows {
        let audio = decode_file(&row.audio_path)
            .unwrap_or_else(|e| panic!("{}: {e}", row.audio_path.display()));
        assert!(
            audio.frame_count() > 0,
            "{}: non-empty audio",
            row.audio_path.display()
        );
    }
}

#[test]
fn decode_resampler_linear_identity_and_ratio() {
    let src = vec![0i16, 100, 200, 300, 400, 500, 600, 700];
    assert_eq!(
        resample_linear(&src, 16_000, 16_000),
        src,
        "identity when rates match"
    );
    let down = resample_linear(&src, 24_000, 16_000);
    let expected_len = (8.0f64 * 16_000.0f64 / 24_000.0f64).round() as usize;
    assert_eq!(down.len(), expected_len);
}

#[test]
fn decode_to_mono_averages_channels() {
    // 2-channel synthetic: L = 100, R = 200 -> mono 150.
    let stereo = crate::audio::PcmAudio {
        sample_rate: 16_000,
        channels: 2,
        samples: vec![100, 200, 300, 400],
    };
    assert_eq!(to_mono(&stereo), vec![150, 350]);
}
