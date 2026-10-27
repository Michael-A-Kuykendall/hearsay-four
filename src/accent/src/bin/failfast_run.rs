//! Fail-fast experiment driver (39c.5 end-to-end run on real audio).
//!
//! Loads the real session manifest and the real fail-fast MP3, scores
//! every utterance under both Condition A (fresh) and Condition B
//! (accent-signature carry-forward), and prints the bucketed stats +
//! the pre-registered verdict. This is the experiment synthesis §6
//! proposes — it has never been run end-to-end on the real audio before.

use std::env;
use std::path::PathBuf;

use accent::measure::{BucketStat, apply_rule, load_session, measure_session};
use accent::{ConditionA, ConditionB, decode_session_audio};
use calibration::FEATURE_DIM;

/// Process exit codes (bead nwu regression gate):
/// - 0: scored cleanly; with `--margin-floor` the gate PASSED.
/// - 1: `--margin-floor` gate FAILED (verdict FAIL or margin below floor).
/// - 2: usage or I/O error (missing dir, bad flag value, decode failure) —
///   always with an error line, never a silent PASS.
pub const EXIT_PASS: i32 = 0;
pub const EXIT_GATE_FAIL: i32 = 1;
pub const EXIT_ERROR: i32 = 2;

fn main() {
    std::process::exit(run(&env::args().collect::<Vec<_>>()));
}

fn run(args: &[String]) -> i32 {
    // Optional: --root <dir> scores a different session layout (default:
    // the 39c.2 fixture), a bare N limits to the first N rows, and
    // --margin-floor <f> asserts the separation margin clears a regression
    // floor (exit 1 on regress, exit 2 on error — never silent PASS).
    let mut root = PathBuf::from("/home/michael/repos/hearsay-four/data/session");
    let mut limit: Option<usize> = None;
    let mut enroll: Option<PathBuf> = None;
    let mut margin_floor: Option<f64> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--root" {
            i += 1;
            match args.get(i) {
                Some(d) => root = PathBuf::from(d),
                None => {
                    eprintln!("failfast_run: error: --root needs a directory");
                    return EXIT_ERROR;
                }
            }
        } else if args[i] == "--enroll" {
            i += 1;
            match args.get(i) {
                Some(f) => enroll = Some(PathBuf::from(f)),
                None => {
                    eprintln!("failfast_run: error: --enroll needs a WAV file");
                    return EXIT_ERROR;
                }
            }
        } else if args[i] == "--margin-floor" {
            i += 1;
            match args.get(i) {
                Some(s) => match s.parse::<f64>() {
                    Ok(f) if f.is_finite() => margin_floor = Some(f),
                    _ => {
                        eprintln!(
                            "failfast_run: error: --margin-floor needs a finite number, got {s:?}"
                        );
                        return EXIT_ERROR;
                    }
                },
                None => {
                    eprintln!("failfast_run: error: --margin-floor needs a number");
                    return EXIT_ERROR;
                }
            }
        } else if let Ok(n) = args[i].parse::<usize>() {
            limit = Some(n);
        } else {
            eprintln!("failfast_run: error: unknown arg {:?}", args[i]);
            return EXIT_ERROR;
        }
        i += 1;
    }
    let rows = match load_session(&root) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("failfast_run: error: load manifest: {e}");
            return EXIT_ERROR;
        }
    };
    let total_rows = rows.len();
    let rows: Vec<_> = if let Some(n) = limit {
        rows.into_iter().take(n).collect()
    } else {
        rows
    };
    println!("manifest: {total_rows} rows, running {}", rows.len());

    let custom_root = root.as_os_str() != "/home/michael/repos/hearsay-four/data/session";
    println!("decoding session audio...");
    let audio = if custom_root {
        // Ingested runs (bead o20): single WAV written by session-ingest.
        match corpus::audio::decode_to_pcm16k(&root.join("audio/run1.wav")) {
            Ok(audio) => audio,
            Err(e) => {
                eprintln!("failfast_run: error: decode: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        match decode_session_audio() {
            Ok(audio) => audio,
            Err(e) => {
                eprintln!("failfast_run: error: decode: {e}");
                return EXIT_ERROR;
            }
        }
    };
    println!(
        "  sample_rate={} frames={} duration={:.1}s",
        audio.sample_rate,
        audio.frames.len(),
        audio.duration_secs()
    );

    // Build a baseline from the first 100 feature frames. (In a real run
    // this would come from enrollment, but for the experiment we use a
    // baseline from the session audio itself, so Condition A and B see
    // the same initial reference.)
    // Use a TRULY GENERIC baseline: mean=0, var=1 per dimension. This is
    // what synthesis §6 calls "scored fresh against a generic reference" —
    // NOT a per-speaker fit. Fitting the baseline from the speaker's own
    // first frames would conflate per-speaker adaptation with drift
    // handling (the first 100 frames ARE the speaker's voice, so A's NLL
    // is artificially low for early utterances and the experiment loses
    // sensitivity to drift).
    let baseline = match &enroll {
        Some(path) => {
            let enroll_audio = match corpus::audio::decode_to_pcm16k(path) {
                Ok(audio) => audio,
                Err(e) => {
                    eprintln!("failfast_run: error: enroll decode {}: {e}", path.display());
                    return EXIT_ERROR;
                }
            };
            let fitted = match accent::baseline_from_frames(
                &enroll_audio.frames,
                enroll_audio.sample_rate,
            ) {
                Ok(fitted) => fitted,
                Err(e) => {
                    eprintln!("failfast_run: error: enroll baseline: {e}");
                    return EXIT_ERROR;
                }
            };
            println!(
                "baseline: ENROLLMENT ({} frames from {})",
                fitted.frame_count,
                path.display()
            );
            fitted
        }
        None => {
            let generic = calibration::Baseline {
                mean: vec![0.0; FEATURE_DIM],
                variance: vec![1.0; FEATURE_DIM],
                frame_count: 0,
            };
            println!("baseline: GENERIC (mean=0, var=1 per dim) — §6 'generic reference'");
            generic
        }
    };

    let a = ConditionA::new(baseline.clone());
    let mut b = ConditionB::new(baseline);
    println!();

    let stats: Vec<BucketStat> =
        match measure_session(&a, &mut b, &audio.frames, audio.sample_rate, &rows) {
            Ok(stats) => stats,
            Err(e) => {
                eprintln!("failfast_run: error: measure_session: {e}");
                return EXIT_ERROR;
            }
        };

    println!("=== BUCKET STATS ===");
    println!(
        "{:<6} {:>5} {:>12} {:>12} {:>12}",
        "bucket", "n", "mean_A", "mean_B", "A-B"
    );
    for s in &stats {
        println!(
            "{:<6} {:>5} {:>12.6} {:>12.6} {:>+12.6}",
            s.bucket,
            s.count,
            s.mean_a,
            s.mean_b,
            s.mean_a - s.mean_b
        );
    }

    let verdict = apply_rule(&stats);
    println!();
    println!("=== VERDICT (synthesis §6 separation rule) ===");
    println!(
        "  tail_separation (B_2nd_half < A_2nd_half): {}",
        verdict.tail_separation
    );
    println!(
        "  margin_two_pp    (A_2nd - B_2nd >= 0.02):   {}",
        verdict.margin_two_pp
    );
    println!(
        "  monotone_b       (B non-increasing, B_last <= A_last): {}",
        verdict.monotone_b
    );
    println!("  OVERALL: {}", if verdict.pass { "PASS" } else { "FAIL" });
    for c in &verdict.checks {
        println!("    - {c}");
    }

    println!();
    println!("=== B ACCENT SIGNATURE (carry-forward state at end) ===");
    let sig = b.signature();
    let nonzero: Vec<(usize, f64)> = sig
        .iter()
        .enumerate()
        .filter(|(_, v)| v.abs() > 0.001)
        .map(|(i, v)| (i, *v))
        .collect();
    println!("  non-zero dims: {} / {}", nonzero.len(), sig.len());
    if !nonzero.is_empty() {
        println!(
            "  first 5 non-zero: {:?}",
            nonzero.iter().take(5).collect::<Vec<_>>()
        );
    }
    println!("  utterance_count: {}", b.utterance_count());

    // Regression gate (bead nwu): with --margin-floor, PASS requires the
    // pre-registered verdict AND margin >= floor. Exit 1 on regress, never
    // a silent PASS; errors above already exited 2.
    if let Some(floor) = margin_floor {
        let gate_pass = verdict.passes_floor(floor);
        println!();
        println!("=== MARGIN GATE (regression floor {floor}) ===");
        println!("  observed_margin (2nd-half A-B): {:.4}", verdict.margin);
        println!("  required_floor:                 {floor:.4}");
        if gate_pass {
            println!("  GATE: PASS (margin >= floor)");
            EXIT_PASS
        } else {
            println!("  GATE: FAIL (margin < floor or verdict FAIL)");
            EXIT_GATE_FAIL
        }
    } else {
        EXIT_PASS
    }
}
