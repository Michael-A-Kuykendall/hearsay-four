//! Fail-fast experiment driver (39c.5 end-to-end run on real audio).
//!
//! Loads the real session manifest and the real fail-fast MP3, scores
//! every utterance under both Condition A (fresh) and Condition B
//! (accent-signature carry-forward), and prints the bucketed stats +
//! the pre-registered verdict. This is the experiment synthesis §6
//! proposes — it has never been run end-to-end on the real audio before.

use std::env;
use std::path::Path;

use accent::measure::{apply_rule, load_session, measure_session, BucketStat};
use accent::{decode_session_audio, ConditionA, ConditionB};
use calibration::{fit_baseline, Frame};
use fse::{extract_features, window_audio};

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    // Optional: limit to first N rows for a smoke test.
    let limit = args.get(1).and_then(|s| s.parse::<usize>().ok());

    let root = Path::new("/home/michael/repos/hearsay-four/data/session");
    let rows = load_session(root).map_err(|e| format!("load manifest: {e}"))?;
    let total_rows = rows.len();
    let rows: Vec<_> = if let Some(n) = limit {
        rows.into_iter().take(n).collect()
    } else {
        rows
    };
    println!("manifest: {total_rows} rows, running {}", rows.len());

    println!("decoding MP3...");
    let audio = decode_session_audio().map_err(|e| format!("decode: {e}"))?;
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
    let windows = window_audio(&audio.frames, audio.sample_rate);
    let features = extract_features(&windows, audio.sample_rate)
        .map_err(|e| format!("extract_features: {e}"))?;
    if features.len() < 100 {
        return Err(format!(
            "only {} frames — need >=100 for baseline",
            features.len()
        ));
    }
    let baseline = fit_baseline(&features[..100]).ok_or("fit_baseline failed")?;
    println!(
        "baseline fitted from {} frames (mean[0]={:.4})",
        baseline.frame_count,
        baseline.mean.first().copied().unwrap_or(0.0)
    );

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
