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

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    // Optional: --root <dir> scores a different session layout (default:
    // the 39c.2 fixture), and a bare N limits to the first N rows.
    let mut root = PathBuf::from("/home/michael/repos/hearsay-four/data/session");
    let mut limit: Option<usize> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--root" {
            i += 1;
            root = args
                .get(i)
                .map(PathBuf::from)
                .ok_or_else(|| "--root needs a directory".to_string())?;
        } else if let Ok(n) = args[i].parse::<usize>() {
            limit = Some(n);
        } else {
            return Err(format!("unknown arg {:?}", args[i]));
        }
        i += 1;
    }
    let rows = load_session(&root).map_err(|e| format!("load manifest: {e}"))?;
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
        corpus::audio::decode_to_pcm16k(&root.join("audio/run1.wav"))
            .map_err(|e| format!("decode: {e}"))?
    } else {
        decode_session_audio().map_err(|e| format!("decode: {e}"))?
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
    let baseline = calibration::Baseline {
        mean: vec![0.0; FEATURE_DIM],
        variance: vec![1.0; FEATURE_DIM],
        frame_count: 0,
    };
    println!("baseline: GENERIC (mean=0, var=1 per dim) — §6 'generic reference'");

    let a = ConditionA::new(baseline.clone());
    let mut b = ConditionB::new(baseline);
    println!();

    let stats: Vec<BucketStat> =
        measure_session(&a, &mut b, &audio.frames, audio.sample_rate, &rows)
            .map_err(|e| format!("measure_session: {e}"))?;

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

    Ok(())
}
