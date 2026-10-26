//! record-session: live recording tool for the human fail-fast run.
//!
//! Behind the `mic` feature (cpal). Two modes:
//!
//! - `--list-devices` — print input devices, no recording.
//! - default — capture enrollment (fixed prompt) then one WAV per line of
//!   `--script`, plus a `manifest.tsv` mapping file → expected transcript.
//!
//! Layout: `<out>/<utterance>.wav` at the device sample rate, mono i16.
//! The manifest's third/fourth columns (rate, samples) let the scorer
//! verify provenance without trusting filenames.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use capture::mic::MicCapture;

fn usage() -> ! {
    eprintln!(
        "usage: record-session [--list-devices] [--out DIR] [--script FILE] [--enroll-only] [--utterances N]"
    );
    std::process::exit(2);
}

fn write_wav(path: &PathBuf, mono: &[i16], sample_rate: u32) -> hound::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in mono {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Prompt on stdout, wait for Enter, then record and save under `stem`.
fn capture_one(cap: &mut MicCapture, out: &Path, stem: &str, say: &str) -> (u32, usize) {
    println!("---");
    println!("Say:  {say}");
    print!("Press Enter to record {stem} ... ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    println!("Recording 3s ...");
    let (mono, rate) = cap.record_raw();
    let n = mono.len();
    let path = out.join(format!("{stem}.wav"));
    match write_wav(&path, &mono, rate) {
        Ok(()) => println!("saved {} ({n} samples @ {rate} Hz)", path.display()),
        Err(e) => eprintln!("WAV write failed for {}: {e}", path.display()),
    }
    (rate, n)
}

fn main() {
    let mut list_only = false;
    let mut out = PathBuf::from("sessions/run1");
    let mut script: Option<PathBuf> = None;
    let mut enroll_only = false;
    let mut max_utterances: Option<usize> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list-devices" => list_only = true,
            "--out" => out = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--script" => script = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--enroll-only" => enroll_only = true,
            "--utterances" => {
                max_utterances = args.next().unwrap_or_else(|| usage()).parse().ok();
            }
            _ => usage(),
        }
    }

    if list_only {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let mut any = false;
        match host.input_devices() {
            Ok(devs) => {
                for d in devs {
                    println!("{}", d.name().unwrap_or_else(|_| "<unnamed>".into()));
                    any = true;
                }
            }
            Err(e) => {
                eprintln!("device query failed: {e}");
                std::process::exit(1);
            }
        }
        if !any {
            println!("no input devices found");
        }
        return;
    }

    std::fs::create_dir_all(&out).unwrap_or_else(|e| {
        eprintln!("cannot create {}: {e}", out.display());
        std::process::exit(1);
    });

    let mut cap = MicCapture::new(3000).unwrap_or_else(|e| {
        eprintln!("no usable input device: {e}");
        std::process::exit(1);
    });

    let manifest_path = out.join("manifest.tsv");
    let mut manifest = File::create(&manifest_path).unwrap_or_else(|e| {
        eprintln!("cannot create {}: {e}", manifest_path.display());
        std::process::exit(1);
    });

    // Enrollment first: the fixed prompt, always `enroll.wav`.
    let (rate, n) = capture_one(&mut cap, &out, "enroll", calibration::ENROLLMENT_PROMPT);
    writeln!(
        manifest,
        "enroll.wav\t{}\t{rate}\t{n}",
        calibration::ENROLLMENT_PROMPT
    )
    .ok();
    if enroll_only {
        println!("enrollment only; manifest at {}", manifest_path.display());
        return;
    }

    // Utterance loop: one line = one expected transcript.
    let lines: Vec<String> = match script {
        Some(path) => {
            let f = File::open(&path).unwrap_or_else(|e| {
                eprintln!("cannot open {}: {e}", path.display());
                std::process::exit(1);
            });
            BufReader::new(f)
                .lines()
                .map_while(Result::ok)
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        }
        None => vec!["the cat sat on the mat".to_string()],
    };
    let take = max_utterances.unwrap_or(lines.len());
    for (i, expected) in lines.into_iter().take(take).enumerate() {
        let stem = format!("utt{i:03}");
        let (rate, n) = capture_one(&mut cap, &out, &stem, &expected);
        writeln!(manifest, "{stem}.wav\t{expected}\t{rate}\t{n}").ok();
    }
    println!("done; manifest at {}", manifest_path.display());
}
