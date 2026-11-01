//! build_featcache: precompute hmm-fe2 (post-CMVN) features for a manifest.
//!
//! usage: build_featcache <manifest.tsv> <wav_root> <cache_root>
//!
//! Rows are `id<TAB>rel<TAB>sec<TAB>text` (subset.tsv or any manifest with
//! the same columns; text is ignored). Each utterance: wav bytes ->
//! sha256 -> skip if cached, else decode/mono/window/CMVN-extract/store.
//! Existing entries are never recomputed (content key). Parallel over
//! utterances (rayon); progress every 1000 rows.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use corpus::audio::{decode_file, to_mono};
use fse::featcache::{cache_path, store};
use fse::instrument::sha256_hex;
use fse::{FEATURE_DIM, HMM_FRONTEND_ID, extract_features_cmvn, window_audio};
use rayon::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 4,
        "usage: build_featcache <manifest.tsv> <wav_root> <cache_root>"
    );
    let (manifest_path, wav_root, cache_root) =
        (PathBuf::from(&args[1]), PathBuf::from(&args[2]), PathBuf::from(&args[3]));
    let text = std::fs::read_to_string(&manifest_path).expect("read manifest");
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(4, '\t');
        let (id, rel) = (
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("").to_string(),
        );
        if id.is_empty() || rel.is_empty() {
            continue;
        }
        rows.push((id, rel));
    }
    println!(
        "build_featcache rows={} frontend={HMM_FRONTEND_ID} threads={}",
        rows.len(),
        rayon::current_num_threads()
    );
    let t0 = Instant::now();
    let done = AtomicUsize::new(0);
    let filled = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    rows.par_iter().for_each(|(id, rel)| {
        let wav_path = wav_root.join(rel);
        let bytes = match std::fs::read(&wav_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("build_featcache SKIP {id} read: {e}");
                return;
            }
        };
        let sha = sha256_hex(&bytes);
        let path = cache_path(&cache_root, HMM_FRONTEND_ID, &sha);
        if path.is_file() {
            skipped.fetch_add(1, Ordering::Relaxed);
        } else {
            let pcm = match decode_file(&wav_path) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("build_featcache SKIP {id} decode: {e}");
                    return;
                }
            };
            let mono = to_mono(&pcm);
            let windows = window_audio(&mono, pcm.sample_rate);
            if windows.is_empty() {
                eprintln!("build_featcache SKIP {id} no windows");
                return;
            }
            match extract_features_cmvn(&windows, pcm.sample_rate) {
                Ok(frames) if !frames.is_empty() && frames[0].len() == FEATURE_DIM => {
                    if let Err(e) = store(&path, pcm.sample_rate, &frames) {
                        eprintln!("build_featcache SKIP {id} store: {e}");
                        return;
                    }
                    filled.fetch_add(1, Ordering::Relaxed);
                }
                _ => {
                    eprintln!("build_featcache SKIP {id} extract");
                    return;
                }
            }
        }
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(1000) {
            eprintln!("build_featcache progress {n}/{} s={:.0}", rows.len(), t0.elapsed().as_secs_f64());
        }
    });
    println!(
        "build_featcache DONE rows={} filled={} cached={} s={:.0}",
        rows.len(),
        filled.load(Ordering::Relaxed),
        skipped.load(Ordering::Relaxed),
        t0.elapsed().as_secs_f64()
    );
}
