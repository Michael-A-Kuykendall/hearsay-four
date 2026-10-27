//! Separation regression gate tests (bead hearsay-four-nwu, A3).
//!
//! - synthetic two-condition fixture reproduces the verdict incl. margin;
//! - missing session dir exits 2 with an error line (never silent PASS);
//! - a floor above the observed margin exits 1 FAIL (gate is load-bearing).
//!
//! Model-free: synthetic PCM + WAV fixtures only (plus the lib's real front
//! end). No network.

use accent::measure::{BucketStat, apply_rule, measure_session, separation_margin};
use accent::{ConditionA, ConditionB};
use calibration::fit_baseline;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_failfast_run"))
}

fn bstat(bucket: usize, mean_a: f64, mean_b: f64, count: usize) -> BucketStat {
    BucketStat {
        bucket,
        mean_a,
        mean_b,
        count,
    }
}

/// Synthetic two-condition fixture reproduces the verdict incl. margin:
/// stats-level exact margin plus a real ConditionA-vs-ConditionB run over
/// synthetic PCM through the real front end.
#[test]
fn synthetic_two_condition_fixture_reproduces_verdict_incl_margin() {
    // Stats level: second-half A−B is exactly 0.55 − 0.45 = 0.10.
    let stats = vec![bstat(0, 0.5000, 0.4800, 4), bstat(1, 0.5500, 0.4500, 4)];
    let v = apply_rule(&stats);
    assert!(v.pass, "fixture verdict: {:?}", v.checks);
    assert!(
        (v.margin - 0.10).abs() < 1e-12,
        "margin is 2nd-half A−B, got {}",
        v.margin
    );
    assert!(
        (separation_margin(&stats) - v.margin).abs() < 1e-12,
        "helper agrees with verdict field"
    );
    assert!(v.passes_floor(0.05), "0.10 clears 0.05");
    assert!(!v.passes_floor(0.15), "0.10 does not clear 0.15");

    // Both conditions over synthetic PCM: the margin the verdict carries is
    // exactly the helper's margin over the measured buckets.
    let pcm: Vec<i16> = (0..(4 * 16_000)).map(|i| ((i as i16) % 7) - 3).collect();
    let frames: Vec<_> = (0..100).map(|_| [1.0; calibration::FEATURE_DIM]).collect();
    let baseline = fit_baseline(&frames).expect("baseline");
    let a = ConditionA::new(baseline.clone());
    let mut b = ConditionB::new(baseline);
    let rows = vec![
        accent::measure::SessionRow {
            bucket: 0,
            idx: 0,
            start_sec: 0.0,
            stop_sec: 1.0,
            text: "congealed".to_string(),
        },
        accent::measure::SessionRow {
            bucket: 1,
            idx: 0,
            start_sec: 1.0,
            stop_sec: 2.0,
            text: "survival".to_string(),
        },
    ];
    let measured = measure_session(&a, &mut b, &pcm, 16_000, &rows).expect("measure");
    let mv = apply_rule(&measured);
    assert!(mv.margin.is_finite(), "measured margin is finite");
    assert!(
        (mv.margin - separation_margin(&measured)).abs() < 1e-12,
        "verdict margin matches helper on measured stats"
    );
    assert_eq!(
        mv.pass,
        mv.tail_separation && mv.margin_two_pp && mv.monotone_b,
        "pass is the conjunction of the three checks"
    );
}

/// Missing session dir exits 2 with an error line — never a silent PASS.
#[test]
fn missing_dir_exits_2_with_error_line() {
    let out = Command::new(bin())
        .args([
            "--root",
            "/nonexistent-separation-gate-dir-xyz",
            "--margin-floor",
            "1.5",
        ])
        .output()
        .expect("run gate binary");
    assert_eq!(out.status.code(), Some(2), "missing dir -> exit 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stderr.contains("failfast_run: error:"),
        "error line on stderr, got: {stderr}"
    );
    assert!(
        !stdout.contains("PASS") && !stdout.contains("GATE"),
        "no PASS/GATE line on error, got: {stdout}"
    );
}

/// A floor above the observed margin exits 1 FAIL: the floor is load-bearing.
#[test]
fn floor_above_observed_margin_exits_1_fail() {
    let dir = fixture_session_dir("floor");
    let baseline = Command::new(bin())
        .args(["--root", dir.to_str().unwrap(), "--margin-floor", "0"])
        .output()
        .expect("run gate binary");
    assert_eq!(baseline.status.code(), Some(0), "floor-0 run exits 0");
    let stdout = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        stdout.contains("GATE: PASS"),
        "floor-0 PASS line, got: {stdout}"
    );
    let observed = parse_observed_margin(&stdout).expect("observed margin line");

    let above = Command::new(bin())
        .args([
            "--root",
            dir.to_str().unwrap(),
            "--margin-floor",
            &format!("{:.4}", observed + 1.0),
        ])
        .output()
        .expect("run gate binary");
    assert_eq!(above.status.code(), Some(1), "floor above margin -> exit 1");
    let above_out = String::from_utf8_lossy(&above.stdout);
    assert!(
        above_out.contains("GATE: FAIL"),
        "FAIL line, got: {above_out}"
    );

    let below = Command::new(bin())
        .args([
            "--root",
            dir.to_str().unwrap(),
            "--margin-floor",
            &format!("{:.4}", observed - 1.0),
        ])
        .output()
        .expect("run gate binary");
    assert_eq!(below.status.code(), Some(0), "floor below margin -> exit 0");
    let below_out = String::from_utf8_lossy(&below.stdout);
    assert!(
        below_out.contains("GATE: PASS"),
        "PASS line, got: {below_out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn parse_observed_margin(stdout: &str) -> Option<f64> {
    stdout
        .lines()
        .find(|l| l.contains("observed_margin"))
        .and_then(|l| l.rsplit(':').next())
        .and_then(|v| v.trim().parse::<f64>().ok())
}

/// Synthetic 2-bucket × 2-utterance session: 4 s of 16 kHz tone PCM plus a
/// `session.tsv` manifest (generic baseline scores it PASS, margin ≈ 23.1).
fn fixture_session_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("separation-gate-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("audio")).unwrap();
    let samples: Vec<i16> = (0..(4 * 16_000))
        .map(|i| ((((i % 97) * 173) % 32767) as i16) - 16383)
        .collect();
    write_wav(&dir.join("audio/run1.wav"), &samples, 16_000);
    std::fs::write(
        dir.join("session.tsv"),
        "bucket\tidx\tstart_sec\tstop_sec\tspeaker\tdrift_tags\ttext\n\
         0\t0\t0.000\t1.000\ttest\tfatigue:test\tcongealed\n\
         0\t1\t1.000\t2.000\ttest\tfatigue:test\tsurvival\n\
         1\t0\t2.000\t3.000\ttest\tfatigue:test\tripple\n\
         1\t1\t3.000\t4.000\ttest\tfatigue:test\tprocess\n",
    )
    .unwrap();
    dir
}

/// Minimal mono 16-bit PCM WAV writer (std only; keeps the gate test
/// dependency-free).
fn write_wav(path: &Path, samples: &[i16], rate: u32) {
    let mut buf = Vec::with_capacity(44 + samples.len() * 2);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&((36 + samples.len() * 2) as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&rate.to_le_bytes());
    buf.extend_from_slice(&(rate * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&((samples.len() * 2) as u32).to_le_bytes());
    for s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, buf).unwrap();
}
