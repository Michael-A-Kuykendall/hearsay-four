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
        "usage: record-session [--list-devices] [--out DIR] [--script FILE] [--enroll-only] [--resume-at N] [--utterances N]"
    );
    std::process::exit(2);
}
fn utterance_stem(index: usize) -> String {
    format!("utt{index:03}")
}
fn select_utterances(
    lines: &[String],
    start_index: usize,
    max_utterances: Option<usize>,
) -> Vec<(usize, String)> {
    lines
        .iter()
        .enumerate()
        .skip(start_index)
        .take(max_utterances.unwrap_or(usize::MAX))
        .map(|(index, expected)| (index, expected.clone()))
        .collect()
}
fn validate_resume_manifest_text(
    text: &str,
    lines: &[String],
    start_index: usize,
) -> Result<(), String> {
    let mut scripted: Vec<(&str, &str)> = Vec::new();
    for line in text.lines() {
        let mut columns = line.split('\t');
        let file = columns.next().unwrap_or_default();
        if file == "enroll.wav" {
            continue;
        }
        if !file.starts_with("utt") || !file.ends_with(".wav") {
            return Err(format!("unexpected manifest row: {line}"));
        }
        scripted.push((file, columns.next().unwrap_or_default()));
    }
    if scripted.len() != start_index {
        return Err(format!(
            "manifest has {} scripted rows, not resume index {start_index}",
            scripted.len()
        ));
    }
    if start_index > lines.len() {
        return Err(format!(
            "resume index {start_index} exceeds script length {}",
            lines.len()
        ));
    }
    for (index, (file, expected)) in scripted.iter().enumerate() {
        if *file != format!("{}.wav", utterance_stem(index)) {
            return Err(format!(
                "manifest row {index} is {file}, not {}",
                utterance_stem(index)
            ));
        }
        if *expected != lines[index] {
            return Err(format!(
                "manifest row {index} expected '{expected}', script has '{}'",
                lines[index]
            ));
        }
    }
    Ok(())
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
    let mut resume_at: Option<usize> = None;
    let mut max_utterances: Option<usize> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list-devices" => list_only = true,
            "--out" => out = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--script" => script = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--enroll-only" => enroll_only = true,
            "--resume-at" => {
                let value = args.next().unwrap_or_else(|| usage());
                resume_at = Some(value.parse().unwrap_or_else(|_| usage()));
            }
            "--utterances" => {
                max_utterances = args.next().unwrap_or_else(|| usage()).parse().ok();
            }
            _ => usage(),
        }
    }
    if enroll_only && resume_at.is_some() {
        usage();
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

    // Utterance loop: one line = one expected transcript.
    let lines: Vec<String> = if enroll_only {
        Vec::new()
    } else {
        match script {
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
        }
    };
    let start_index = resume_at.unwrap_or(0);
    let selected = select_utterances(&lines, start_index, max_utterances);
    let manifest_path = out.join("manifest.tsv");

    if let Some(start) = resume_at {
        if selected.is_empty() {
            eprintln!("resume index {start} selects no script lines");
            std::process::exit(1);
        }
        let manifest_text = std::fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
            eprintln!("cannot read existing {}: {e}", manifest_path.display());
            std::process::exit(1);
        });
        if let Err(e) = validate_resume_manifest_text(&manifest_text, &lines, start) {
            eprintln!("cannot resume: {e}");
            std::process::exit(1);
        }
        for (index, _) in &selected {
            let wav_path = out.join(format!("{}.wav", utterance_stem(*index)));
            if wav_path.exists() {
                eprintln!("cannot resume: {} already exists", wav_path.display());
                std::process::exit(1);
            }
        }
    }

    let mut cap = MicCapture::new(3000).unwrap_or_else(|e| {
        eprintln!("no usable input device: {e}");
        std::process::exit(1);
    });

    let mut manifest = if resume_at.is_some() {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&manifest_path)
            .unwrap_or_else(|e| {
                eprintln!("cannot open {}: {e}", manifest_path.display());
                std::process::exit(1);
            })
    } else {
        File::create(&manifest_path).unwrap_or_else(|e| {
            eprintln!("cannot create {}: {e}", manifest_path.display());
            std::process::exit(1);
        })
    };

    if resume_at.is_none() {
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
    }

    for (index, expected) in selected {
        let stem = utterance_stem(index);
        let (rate, n) = capture_one(&mut cap, &out, &stem, &expected);
        writeln!(manifest, "{stem}.wav\t{expected}\t{rate}\t{n}").ok();
    }
    println!("done; manifest at {}", manifest_path.display());
}
#[test]
fn resume_selection_uses_global_indices() {
    let lines: Vec<String> = (0..120).map(|i| format!("word-{i}")).collect();
    let selected = select_utterances(&lines, 64, Some(26));
    assert_eq!(selected.len(), 26);
    assert_eq!(selected.first().map(|item| item.0), Some(64));
    assert_eq!(selected.last().map(|item| item.0), Some(89));
    assert_eq!(utterance_stem(64), "utt064");
}
#[test]
fn resume_manifest_must_match_script_prefix() {
    let lines: Vec<String> = vec!["alpha".into(), "beta".into(), "gamma".into()];
    let manifest = "enroll.wav\tprompt\t48000\t144000\nutt000.wav\talpha\t48000\t144000\nutt001.wav\tbeta\t48000\t144000\n";
    assert!(validate_resume_manifest_text(manifest, &lines, 2).is_ok());
    assert!(validate_resume_manifest_text(manifest, &lines, 1).is_err());
    assert!(validate_resume_manifest_text(manifest, &["alpha".into(), "wrong".into()], 2).is_err());
}
