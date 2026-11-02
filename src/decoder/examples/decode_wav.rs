//! Decode one audio file with the R5a skeleton and print the 1-best words
//! plus the A5 lattice JSONL.
//!
//! Usage: `decode_wav <audio> [am.json] [lm-file]`
//! Defaults: `models/si_base/model.json`, no LM, 30-word closed vocabulary.

use std::path::PathBuf;
use std::time::Instant;

use decoder::{DecoderConfig, MooreDecoder, Recognizer};

fn default_am() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/si_base/model.json")
}

fn resample_to_16k(mono: &[i16], from_rate: u32) -> Vec<i16> {
    if from_rate == 16_000 {
        return mono.to_vec();
    }
    let ratio = 16_000.0 / from_rate as f64;
    let out_len = ((mono.len() as f64 * ratio).round() as usize).max(1);
    (0..out_len)
        .map(|i| {
            let pos = i as f64 / ratio;
            let lo = pos.floor() as usize;
            let hi = (lo + 1).min(mono.len() - 1);
            let frac = pos - lo as f64;
            (mono[lo] as f64 * (1.0 - frac) + mono[hi] as f64 * frac).round() as i16
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let no_cmvn = args.iter().any(|a| a == "--no-cmvn");
    let positional: Vec<&str> = args[1..]
        .iter()
        .filter(|a| a.as_str() != "--no-cmvn")
        .map(String::as_str)
        .collect();
    if positional.is_empty() {
        eprintln!("usage: decode_wav <audio> [am.json] [lm-file] [--no-cmvn]");
        std::process::exit(2);
    }
    let audio_path = PathBuf::from(positional[0]);
    let am_path = positional
        .get(1)
        .map(|s| PathBuf::from(*s))
        .unwrap_or_else(default_am);
    let lm_path = positional.get(2).map(|s| PathBuf::from(*s));

    let audio = corpus::audio::decode_file(&audio_path).expect("decode audio");
    let pcm16k = resample_to_16k(&corpus::audio::to_mono(&audio), audio.sample_rate);
    let mut cfg = DecoderConfig::default();
    if no_cmvn {
        cfg.cmvn = false;
    }
    let decoder = MooreDecoder::load(
        &am_path,
        lm_path.as_deref(),
        lexicon::CLOSED_VOCABULARY_CMUDICT,
        cfg,
    )
    .expect("load decoder");
    eprintln!(
        "vocab {} words, skipped {}, lm {}",
        decoder.vocab_size(),
        decoder.skipped_words().len(),
        decoder.has_lm()
    );
    let started = Instant::now();
    let lattice = decoder.decode(&pcm16k);
    let elapsed = started.elapsed();
    let audio_sec = pcm16k.len() as f64 / 16_000.0;
    let words: Vec<&str> = lattice.tokens.iter().map(|t| t.word.as_str()).collect();
    eprintln!("words: {}", words.join(" "));
    eprintln!(
        "audio {audio_sec:.2}s decode {:.2}s RTF {:.3}",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() / audio_sec
    );
    println!(
        "{}",
        lattice::write_lattice(&lattice).expect("write lattice")
    );
}
