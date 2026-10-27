//! mp3-to-wav: decode any `decode_to_pcm16k`-supported audio file to a
//! mono 16-bit WAV (bead zl3 pilot; reused by bulk ingest).
//!
//! Usage: mp3-to-wav <input.mp3|wav|...> <output.wav>

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let input: PathBuf = args.next().map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: mp3-to-wav <input> <output.wav>");
        std::process::exit(2);
    });
    let output: PathBuf = args.next().map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: mp3-to-wav <input> <output.wav>");
        std::process::exit(2);
    });

    let pcm = match corpus::audio::decode_to_pcm16k(&input) {
        Ok(pcm) => pcm,
        Err(e) => {
            eprintln!("mp3-to-wav: decode failed for {}: {e}", input.display());
            std::process::exit(1);
        }
    };
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: pcm.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = match hound::WavWriter::create(&output, spec) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("mp3-to-wav: cannot write {}: {e}", output.display());
            std::process::exit(1);
        }
    };
    for &s in &pcm.frames {
        if writer.write_sample(s).is_err() {
            eprintln!("mp3-to-wav: write failed for {}", output.display());
            std::process::exit(1);
        }
    }
    if writer.finalize().is_err() {
        eprintln!("mp3-to-wav: finalize failed for {}", output.display());
        std::process::exit(1);
    }
    println!(
        "mp3-to-wav: OK ({} samples @ {} Hz -> {})",
        pcm.frames.len(),
        pcm.sample_rate,
        output.display()
    );
}
