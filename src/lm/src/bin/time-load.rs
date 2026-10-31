//! Time LM archive loads repeatedly (R1-v2 gate verification).
//!
//! usage: time-load <archive.json>...
//! Kind is decided by filename: paths containing "interpolated" load as
//! interpolated archives, everything else as KN trigram archives.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(args.len() >= 2, "usage: time-load <archive>...");
    for path in &args[1..] {
        let is_interp = path.contains("interpolated");
        for i in 0..3 {
            let t0 = Instant::now();
            if is_interp {
                let _m = lm::persist::load_interpolated(std::path::Path::new(path))
                    .expect("load interpolated");
            } else {
                let _m = lm::persist::load_kn(std::path::Path::new(path)).expect("load kn");
            }
            println!(
                "load\t{path}\trep={i}\tsecs={:.3}",
                t0.elapsed().as_secs_f64()
            );
        }
    }
}
