//! record-session: live recording tool for the human fail-fast run.
//!
//! Behind the `mic` feature (cpal). Modes:
//!
//! - `--list-devices` — print input devices, no recording.
//! - default (legacy) — capture enrollment (fixed prompt) then one WAV per
//!   line of `--script`, plus a `manifest.tsv` mapping file → expected
//!   transcript. Every take offers `[r]`edo.
//! - `--sealed` — precommitted-order session (order.txt hash first, then
//!   takes, then manifest + seal files). Every take offers `[r]`edo: the
//!   redo replaces the take in place, is logged in session.log, and never
//!   changes the precommitted order, so the seal stays valid.
//!
//! Both modes capture at the device NATIVE rate (RS-1 section 1: no
//! capture-time resampling; the canonicalizer resamples once downstream).
//! Layout: `<out>/<utterance>.wav` mono i16. The manifest's third/fourth
//! columns (rate, samples) let the scorer verify provenance without
//! trusting filenames.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use capture::mic::MicCapture;

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!(
        "  record-session --script LINES --out DIR            legacy takes (enroll + native rate)"
    );
    eprintln!("  record-session --script LINES --out DIR --sealed [--dur SECS]");
    eprintln!("  record-session --out DIR --continuous SECS");
    eprintln!(
        "  record-session --validate DIR [--labels N --dur S]  check a sealed session (VALID/REJECT)"
    );
    eprintln!("  record-session --list-devices");
    eprintln!("flags: --enroll-only --resume-at N --utterances N --no-enroll --dur SECS");
    eprintln!("(all modes capture at device native rate; --dur applies to both modes; sealed implies --no-enroll)");
    eprintln!("(new sealed sessions validate via --labels N --dur S; bare --validate is the frozen 30-word 16kHz seal)");
    std::process::exit(2);
}
fn utterance_stem(index: usize) -> String {
    format!("utt{index:03}")
}
/// Script lines: trimmed, blanks and full-line `#` comments dropped.
/// Section labels and provenance headers live in comments so one hashed
/// script file can carry labeled sets (prep beads) without becoming prompts.
fn script_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
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
/// RS1-a: 250 ms click guard after Enter (keystroke never inside the
/// window), peak print + `[r]`edo loop, SILENT TAKE auto-retry below
/// -40 dBFS. Every attempt is appended to `session.log`. Returns the
/// accepted take's (rate, samples); the manifest write stays single.
fn capture_one(
    cap: &mut MicCapture,
    out: &Path,
    stem: &str,
    say: &str,
    dur_secs: f64,
    log: &mut File,
) -> (u32, usize) {
    println!("---");
    println!("Say:  {say}");
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        print!("Press Enter to record {stem} ... ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok();
        // RS1-a click guard: wait out the keystroke before opening the window.
        std::thread::sleep(std::time::Duration::from_millis(250));
        println!("SPEAK NOW - Recording {dur_secs}s ...");
        let (mono, rate) = cap.record_raw();
        let n = mono.len();
        let peak_db = peak_dbfs(&mono);
        let path = out.join(format!("{stem}.wav"));
        match write_wav(&path, &mono, rate) {
            Ok(()) => println!("saved {} ({n} samples @ {rate} Hz)", path.display()),
            Err(e) => eprintln!("WAV write failed for {}: {e}", path.display()),
        }
        writeln!(log, "take {stem} attempt {attempt} peak_dbfs={peak_db:.1}").ok();
        if take_is_silent(&mono) {
            println!(
                "SILENT TAKE: peak {peak_db:.1} dBFS below floor -40.0 dBFS - retrying same prompt"
            );
            writeln!(log, "take {stem} attempt {attempt} SILENT auto-retry").ok();
            continue;
        }
        println!("peak {peak_db:.1} dBFS  [Enter]=next  [r]=redo");
        std::io::stdout().flush().ok();
        let mut choice = String::new();
        std::io::stdin().read_line(&mut choice).ok();
        if choice.trim().eq_ignore_ascii_case("r") {
            writeln!(log, "take {stem} attempt {attempt} REDO requested").ok();
            continue;
        }
        return (rate, n);
    }
}
fn peak_dbfs(mono: &[i16]) -> f32 {
    let peak = mono
        .iter()
        .map(|s| s.unsigned_abs() as f32 / 32768.0)
        .fold(0.0f32, f32::max);
    if peak <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}
/// Record-time silence gate, sharing the single floor predicate in
/// `fse::instrument` (same floor the sealed-session validators enforce).
fn take_is_silent(mono: &[i16]) -> bool {
    fse::instrument::take_is_silent(mono)
}
/// Sealed take: Enter, exact-frame capture, then `[r]`edo. A redo
/// re-captures the SAME position in place (same stem, same order slot);
/// the order never changes, so the seal stays valid. Every attempt is
/// appended to `session.log`. Short/silent takes auto-retry; a device
/// rate change mid-session fails closed (partial sessions never validate).
/// Returns the accepted take's (rate, samples).
#[allow(clippy::too_many_arguments)]
fn capture_one_sealed(
    cap: &mut MicCapture,
    out: &Path,
    stem: &str,
    say: &str,
    position: usize,
    total: usize,
    dur_secs: f64,
    expected_frames: usize,
    expected_rate: u32,
    log: &mut File,
) -> (u32, usize) {
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        println!("=== [{position}/{total}] Say:  {say}   (Enter = record {dur_secs}s) ===");
        print!("Press Enter ... ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok();
        // Same click guard as legacy: the keystroke never lands in the window.
        std::thread::sleep(std::time::Duration::from_millis(250));
        println!("Recording {dur_secs}s ... speak NOW:  {say}");
        let (mono, rate) = cap.record_exact();
        if rate != expected_rate {
            eprintln!(
                "RATE CHANGED: device gave {rate} Hz, session runs at {expected_rate} Hz — abandoning (partial sessions never validate)"
            );
            std::process::exit(1);
        }
        if mono.len() < expected_frames {
            eprintln!(
                "SHORT TAKE: captured {} frames, need {expected_frames} — retrying same prompt (Ctrl-C abandons the session)",
                mono.len()
            );
            writeln!(log, "take {stem} attempt {attempt} SHORT {}/{} auto-retry", mono.len(), expected_frames).ok();
            continue;
        }
        if take_is_silent(&mono) {
            eprintln!(
                "SILENT TAKE: peak {:.1} dBFS below floor -40.0 dBFS (muted input?) — retrying same prompt (Ctrl-C abandons the session)",
                peak_dbfs(&mono)
            );
            writeln!(log, "take {stem} attempt {attempt} SILENT auto-retry").ok();
            continue;
        }
        let n = mono.len();
        let peak_db = peak_dbfs(&mono);
        let path = out.join(format!("{stem}.wav"));
        match write_wav(&path, &mono, rate) {
            Ok(()) => println!(
                "wrote {stem}.wav: {n} frames @ {rate}Hz mono, peak {peak_db:.1} dBFS [OK]"
            ),
            Err(e) => {
                eprintln!("WAV write failed for {}: {e}", path.display());
                writeln!(log, "take {stem} attempt {attempt} WRITE-FAIL auto-retry").ok();
                continue;
            }
        }
        writeln!(
            log,
            "take {stem} attempt {attempt} peak_dbfs={peak_db:.1} frames={n}"
        )
        .ok();
        println!("peak {peak_db:.1} dBFS  [Enter]=next  [r]=redo (replaces this take in place)");
        std::io::stdout().flush().ok();
        let mut choice = String::new();
        std::io::stdin().read_line(&mut choice).ok();
        if choice.trim().eq_ignore_ascii_case("r") {
            writeln!(log, "take {stem} attempt {attempt} REDO requested").ok();
            continue;
        }
        return (rate, n);
    }
}
/// Sealed session: guards, precommit, takes with redo, seal files.
/// Prompts are recorded in script order (the prep beads own the order).
/// Captures at the device NATIVE rate (RS-1 section 1); the declared rate
/// is queried up front and every take must match it. Layout validates via
/// `fse::instrument::validate_session_general` (the frozen 30-word seal
/// pins 16 kHz and only re-checks the October evidence).
fn run_sealed(out: &Path, lines: &[String], dur_secs: f64) {
    use cpal::traits::{DeviceTrait, HostTrait};
    if lines.is_empty() {
        eprintln!("sealed session needs a non-empty --script");
        std::process::exit(1);
    }
    if !(0.5..=600.0).contains(&dur_secs) {
        eprintln!("--dur must be within 0.5..=600s, got {dur_secs}");
        std::process::exit(1);
    }
    let total = lines.len();
    let seed = {
        use std::hash::{BuildHasher, Hasher};
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    };
    let host = cpal::default_host();
    let device = host.default_input_device().unwrap_or_else(|| {
        eprintln!("no default input device");
        std::process::exit(1);
    });
    let device_name = device.name().unwrap_or_else(|_| "<unnamed>".to_string());
    // Native rate, queried once: no capture-time resampling, ever.
    let native_rate = device
        .default_input_config()
        .map(|c| c.sample_rate().0)
        .unwrap_or_else(|e| {
            eprintln!("cannot query native input rate: {e}");
            std::process::exit(1);
        });
    if !fse::instrument::is_plausible_rate(native_rate) {
        eprintln!("native input rate {native_rate} Hz is not a plausible device rate — abandoning");
        std::process::exit(1);
    }
    let duration_ms = (dur_secs * 1000.0).round() as u64;
    let expected_frames = (u64::from(native_rate) * duration_ms / 1000) as usize;

    // Order is frozen before the first take: order.txt predates every wav.
    let order_txt = capture::seal::build_order_txt(seed, lines);
    let order_sha = fse::instrument::sha256_hex(order_txt.as_bytes());
    println!("guards OK: {total} prompts, DUR={dur_secs}, RATE={native_rate} (native)");
    println!("PRECOMMIT seed={seed} order_sha256={order_sha}");
    println!("  (record this hash in the session bead notes BEFORE capturing)");
    std::fs::write(out.join("order.txt"), &order_txt).unwrap_or_else(|e| {
        eprintln!("cannot write order.txt: {e}");
        std::process::exit(1);
    });
    let mut session_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join("session.log"))
        .unwrap_or_else(|e| {
            eprintln!("cannot open session.log: {e}");
            std::process::exit(1);
        });
    writeln!(
        session_log,
        "precommit seed={seed} order_sha256={order_sha} device={device_name} rate_hz={native_rate} dur_s={dur_secs}"
    )
    .ok();
    println!("--- session dir: {}", out.display());
    println!("Mic: {device_name} @ {native_rate}Hz mono native | {total} prompts x {dur_secs}s");
    println!(
        "Lead/trail silence is KEPT (do not trim). [r] redoes a take in place (logged, order unchanged). Ctrl-C abandons the session (a partial session will NOT validate)."
    );

    let mut cap = MicCapture::new(duration_ms).unwrap_or_else(|e| {
        eprintln!("no usable input device: {e}");
        std::process::exit(1);
    });

    let mut rows: Vec<(String, String, u32, u32)> = Vec::new();
    for (position, prompt) in lines.iter().enumerate() {
        let stem = format!("utt{:03}", position);
        let (rate, n) = capture_one_sealed(
            &mut cap,
            out,
            &stem,
            prompt,
            position + 1,
            total,
            dur_secs,
            expected_frames,
            native_rate,
            &mut session_log,
        );
        rows.push((format!("{stem}.wav"), prompt.clone(), rate, n as u32));
    }

    let manifest_tsv = capture::seal::build_manifest_tsv(&rows);
    let manifest_sha = fse::instrument::sha256_hex(manifest_tsv.as_bytes());
    std::fs::write(out.join("manifest.tsv"), &manifest_tsv).unwrap_or_else(|e| {
        eprintln!("cannot write manifest.tsv: {e}");
        std::process::exit(1);
    });
    let meta = capture::seal::build_session_meta(
        &device_name,
        native_rate,
        &capture::seal::utc_now_string(),
        seed,
        &order_sha,
        &manifest_sha,
        &capture::seal::recorder_commit(),
        rows.len(),
    );
    std::fs::write(out.join("session.meta"), &meta).unwrap_or_else(|e| {
        eprintln!("cannot write session.meta: {e}");
        std::process::exit(1);
    });
    let mut names: Vec<String> = rows.iter().map(|(f, _, _, _)| f.clone()).collect();
    names.push("manifest.tsv".to_string());
    names.push("order.txt".to_string());
    if let Err(e) = capture::seal::write_sums_file(out, &names) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    println!("done; sealed {total} takes in {}", out.display());
    println!("verify: record-session --validate {}", out.display());
}
fn main() {
    let mut list_only = false;
    let mut out = PathBuf::from("sessions/run1");
    let mut script: Option<PathBuf> = None;
    let mut enroll_only = false;
    let mut resume_at: Option<usize> = None;
    let mut max_utterances: Option<usize> = None;
    let mut dur_secs: f64 = 3.0;
    let mut no_enroll = false;
    let mut sealed = false;
    let mut validate: Option<PathBuf> = None;
    let mut labels: Option<usize> = None;
    let mut continuous: Option<f64> = None;
    let mut dur_given = false;

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
            "--dur" => {
                dur_secs = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .unwrap_or_else(|_| usage());
                dur_given = true;
            }
            "--continuous" => {
                continuous = args
                    .next()
                    .unwrap_or_else(|| usage())
                    .parse()
                    .ok()
                    .filter(|s: &f64| s.is_finite() && *s > 0.0);
                if continuous.is_none() {
                    usage();
                }
            }
            "--no-enroll" => no_enroll = true,
            "--sealed" => sealed = true,
            "--validate" => {
                validate = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())));
            }
            "--labels" => {
                labels = args.next().unwrap_or_else(|| usage()).parse().ok();
                if labels.is_none() {
                    usage();
                }
            }
            _ => usage(),
        }
    }
    if enroll_only && resume_at.is_some() {
        usage();
    }
    if sealed && (enroll_only || resume_at.is_some() || max_utterances.is_some()) {
        usage();
    }
    if let Some(secs) = continuous {
        if sealed || enroll_only || resume_at.is_some() || max_utterances.is_some() {
            eprintln!("--continuous takes no takes-mode flags");
            usage();
        }
        if script.is_some() || dur_given {
            eprintln!("--continuous takes no --script/--dur (window is SECS)");
            usage();
        }
        if !(1.0..=600.0).contains(&secs) {
            eprintln!("--continuous must be within 1..=600s, got {secs}");
            std::process::exit(1);
        }
    }
    if !(0.5..=600.0).contains(&dur_secs) {
        eprintln!("--dur must be within 0.5..=600s, got {dur_secs}");
        std::process::exit(1);
    }

    if labels.is_some() && validate.is_none() {
        eprintln!("--labels needs --validate DIR");
        usage();
    }
    if let Some(dir) = validate {
        let result = match labels {
            Some(n) => fse::instrument::validate_session_general(&dir, n, dur_secs),
            None => fse::instrument::validate_session(&dir),
        };
        match result {
            Ok(_) => println!("VALID: {}", dir.display()),
            Err(e) => {
                println!("{e}");
                std::process::exit(1);
            }
        }
        return;
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

    if let Some(secs) = continuous {
        run_sealed(&out, &[format!("continuous-{secs}s")], secs);
        return;
    }

    // Utterance loop: one line = one expected transcript.
    let lines: Vec<String> = if enroll_only {
        Vec::new()
    } else {
        match &script {
            Some(path) => {
                let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
                    eprintln!("cannot open {}: {e}", path.display());
                    std::process::exit(1);
                });
                script_lines(&text)
            }
            None => vec!["the cat sat on the mat".to_string()],
        }
    };

    if sealed {
        if script.is_none() {
            eprintln!("sealed session requires --script");
            std::process::exit(1);
        }
        run_sealed(&out, &lines, dur_secs);
        return;
    }

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

    let mut cap = MicCapture::new((dur_secs * 1000.0).round() as u64).unwrap_or_else(|e| {
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

    // RS1-a: per-attempt log (redos + silent retries) and guard metadata.
    let mut session_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join("session.log"))
        .unwrap_or_else(|e| {
            eprintln!("cannot open session.log: {e}");
            std::process::exit(1);
        });
    if resume_at.is_none() {
        std::fs::write(
            out.join("session.meta"),
            format!(
                "host={}\nclick_guard_ms=250\ndur_s={dur_secs}\n",
                std::env::consts::OS
            ),
        )
        .ok();
    }

    if resume_at.is_none() && !no_enroll {
        // Enrollment first: the fixed prompt, always `enroll.wav`.
        let (rate, n) = capture_one(
            &mut cap,
            &out,
            "enroll",
            calibration::ENROLLMENT_PROMPT,
            dur_secs,
            &mut session_log,
        );
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
        let (rate, n) = capture_one(
            &mut cap,
            &out,
            &stem,
            &expected,
            dur_secs,
            &mut session_log,
        );
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
#[test]
fn script_lines_skips_blanks_and_hash_comments() {
    let lines = script_lines("# provenance: test\n\n  hello world  \n# section\n\tsecond line\n");
    assert_eq!(
        lines,
        vec!["hello world".to_string(), "second line".to_string()]
    );
}
#[test]
fn silent_take_gate_rejects_dithered_silence_accepts_speech() {
    // Muted-stream signature measured 2026-10-30/31: full-length takes with
    // +/-1 LSB dither only (peak 0.00003..0.00034), no speech.
    let mut silent = vec![0i16; 48_000];
    for (i, s) in silent.iter_mut().enumerate() {
        if i % 6 == 0 {
            *s = 1;
        } else if i % 11 == 0 {
            *s = -1;
        }
    }
    assert!(take_is_silent(&silent));
    assert!(take_is_silent(&vec![0i16; 48_000]));
    // Quietest real speech measured: peak 0.1096. Synthetic speech-level tone.
    let speech: Vec<i16> = (0..48_000)
        .map(|i| ((i as f64 * 0.05).sin() * 0.1 * 32768.0) as i16)
        .collect();
    assert!(!take_is_silent(&speech));
}
