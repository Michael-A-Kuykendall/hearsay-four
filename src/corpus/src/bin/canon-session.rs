//! `canon-session <raw-dir> <out-dir>` — RS-1 canonicalizer (RS-1 §4).
//!
//! Validates a raw owner session, resamples every take to 16 kHz mono with
//! the pinned windowed-sinc polyphase resampler, endpoints, re-pads to
//! exactly 250 ms each side, and writes takes + manifest + provenance + QC.
//! Deterministic: same raw input gives byte-identical output.

use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("usage: canon-session <raw-dir> <out-dir>");
    std::process::exit(2);
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        usage();
    }
    let (raw, out) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let report = corpus::canonical::canon_session(&raw, &out)?;
    println!(
        "canon-session: OK flavor={:?} takes={} accepted={} rejected={} version={} resampler={}",
        report.flavor,
        report.takes_total,
        report.takes_accepted,
        report.rejected.len(),
        corpus::canonical::CANON_VERSION,
        corpus::canonical::RESAMPLER_ID,
    );
    for (take, reason) in &report.rejected {
        println!("canon-session: REJECTED {take}: {reason}");
    }
    println!("canon-session: wrote {}", report.out_dir.display());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("canon-session: error: {e}");
        std::process::exit(1);
    }
}
