//! Scorer self-check binary: exits 0 with a report, 1 on failure.
//!
//! Usage: scorer_selfcheck (no arguments). Checks the paired-bootstrap
//! contract: identical transcripts give delta 0 with a CI covering 0, and
//! swapped arm labels flip the sign.

use experiment::sweep::selfcheck_scorer;

fn main() {
    match selfcheck_scorer() {
        Ok(report) => println!("SELFCHECK PASS: {report}"),
        Err(e) => {
            eprintln!("SELFCHECK FAIL: {e}");
            std::process::exit(1);
        }
    }
}
