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

/// Resolve a better-voice audio path, or `None` when the checkout is absent
/// (fresh clone): callers skip via `let-else return`, never fail.
fn sample(rel: &str) -> Option<PathBuf> {
    if !CorpusManifest::is_available() {
        eprintln!(
            "SKIP: better-voice checkout absent at {} (set $BETTER_VOICE_ROOT)",
            CorpusManifest::root().display()
        );
        return None;
    }
    Some(CorpusManifest::root().join(rel))
}

#[test]
fn decode_reads_true_16khz_pcm_wav() {
    let Some(path) = sample("data/phrase_training/audio/phrase_000000.wav") else {
        return;
    };
    assert_eq!(detect_format(&path).unwrap(), AudioFormat::Wav);
    let audio = decode_wav(&path).expect("hound reads the real PCM WAV");
    assert_eq!(audio.sample_rate, 16_000);
    assert_eq!(audio.channels, 1);
    assert!(audio.frame_count() > 0);
}

#[test]
fn decode_identifies_mislabeled_mp3() {
    let Some(path) = sample("data/quality_checked_1k/abating_0707.wav") else {
        return;
    };
    assert_eq!(
        detect_format(&path).unwrap(),
        AudioFormat::Mp3,
        "must detect MPEG sync despite .wav extension"
    );
    let audio = decode_mp3(&path).expect("symphonia decodes the mislabeled MP3");
    assert!(audio.sample_rate > 0, "source rate known (24 kHz)");
    assert!(audio.frame_count() > 0);
}

#[test]
fn decode_normalizes_mp3_to_16khz_mono() {
    let Some(path) = sample("data/quality_checked_1k/abating_0707.wav") else {
        return;
    };
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
    let Some(wav) = sample("data/phrase_training/audio/phrase_000000.wav") else {
        return;
    };
    let Some(mp3) = sample("data/quality_checked_1k/abating_0707.wav") else {
        return;
    };
    for p in [&wav, &mp3] {
        let frames = decode_to_pcm_16k_mono(p).expect("canonical decode");
        assert!(!frames.is_empty());
    }
}

#[test]
fn decode_audio_transcript_paired_samples_decode() {
    // A slice of the closed-vocab corpus: every sampled train row decodes.
    if !CorpusManifest::is_available() {
        eprintln!("SKIP: better-voice checkout absent (set $BETTER_VOICE_ROOT)");
        return;
    }
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

/// MP3-1 (bead hearsay-four-gmv): the in-memory decode path produces
/// byte-identical canonical PCM to the file path for identical bytes —
/// for both the Oyez MP3 clip and its WAV twin.
#[test]
fn decode_bytes_path_matches_file_path() {
    use crate::audio::{decode_bytes_to_pcm16k, sniff_format_bytes};
    use std::path::PathBuf;

    // Repo-relative fixtures (cargo test cwd is the corpus package root).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/demo");
    for name in ["serve-mp3.mp3", "serve-mp3-twin.wav"] {
        let path = root.join(name);
        let bytes = std::fs::read(&path).expect("fixture present");
        // Sniff agrees with the on-disk probe.
        let via_prefix = sniff_format_bytes(&bytes);
        let via_file = detect_format(&path).expect("probe file");
        assert_eq!(
            via_prefix, via_file,
            "{name}: stdin sniff matches file probe"
        );
        let from_bytes = decode_bytes_to_pcm16k(&bytes).expect("decode bytes");
        let from_file = decode_to_pcm16k(&path).expect("decode file");
        assert_eq!(from_bytes.sample_rate, from_file.sample_rate);
        assert_eq!(from_bytes.channels, from_file.channels);
        assert_eq!(
            from_bytes.frames, from_file.frames,
            "{name}: byte path and file path decode to identical PCM"
        );
        assert!(!from_bytes.frames.is_empty(), "{name}: non-empty PCM");
    }
    // Unknown bytes are an error on both paths, never a guess.
    assert_eq!(
        sniff_format_bytes(b"\x00\x01\x02\x03"),
        AudioFormat::Unknown
    );
    assert!(decode_bytes_to_pcm16k(b"\x00\x01\x02\x03").is_err());
}

/// MP3-1: the synthetic corrupt fixture sniffs as MP3 but never decodes.
#[test]
fn decode_rejects_corrupt_mp3_fixture() {
    use crate::audio::decode_bytes_to_pcm16k;
    use std::path::PathBuf;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/demo/corrupt.mp3");
    assert_eq!(
        detect_format(&path).unwrap(),
        AudioFormat::Mp3,
        "corrupt fixture carries an MP3 preamble"
    );
    assert!(
        decode_mp3(&path).is_err(),
        "corrupt fixture must not decode via file"
    );
    let bytes = std::fs::read(&path).expect("fixture present");
    assert!(
        decode_bytes_to_pcm16k(&bytes).is_err(),
        "corrupt fixture must not decode via bytes"
    );
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
