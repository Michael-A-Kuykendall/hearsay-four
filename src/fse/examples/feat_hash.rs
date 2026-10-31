//! Config-drift detector: hash feature matrices per tuning-grid config.
//! Usage: cargo run -p fse --example feat_hash -- <canon-dir> [take-index]
//! If two configs produce identical hashes, the config is not affecting
//! features on this path (investigate before trusting a grid).
use std::path::PathBuf;

fn fnv1a(frames: &[Vec<f64>]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= frames.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for row in frames {
        for &x in row {
            h ^= x.to_bits();
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: feat_hash <canon-dir> [take-index]");
    let idx: usize = std::env::args()
        .nth(2)
        .map(|s| s.parse().expect("take-index"))
        .unwrap_or(0);
    let session =
        corpus::canonical::load_session(&PathBuf::from(&dir)).expect("load canon session");
    let take = &session.takes[idx];
    println!(
        "take={} prompt={} pcm16k={}",
        take.id,
        take.prompt,
        take.pcm.len()
    );
    for (name, cfg) in fse::frontend::FrontendConfig::tuning_grid() {
        let feats = fse::frontend::extract_with_config(
            &take.pcm,
            corpus::audio::TARGET_SAMPLE_RATE,
            &cfg,
        )
        .expect("extract");
        println!(
            "{name} frames={} dim={} fnv={:016x}",
            feats.len(),
            feats.first().map(|r| r.len()).unwrap_or(0),
            fnv1a(&feats)
        );
    }
}
