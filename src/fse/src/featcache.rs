//! On-disk feature cache (hmm-fe2 canonical: post-CMVN 39-dim frames).
//!
//! Key: `<root>/<frontend_id>/<sha2>/<sha>.bin` where `sha` is the audio
//! file sha256 (`fse::instrument::sha256_hex`) and `frontend_id` is
//! [`crate::HMM_FRONTEND_ID`]. A frontend change bumps the id, which
//! routes to a fresh directory: no explicit invalidation, stale entries
//! are simply never read again.
//!
//! Format (`HFC1`): magic[4] + dim u32LE + nframes u32LE + rate u32LE,
//! then frames row-major f64LE. `load` returns `None` on missing file,
//! bad magic, dim/rate mismatch, or short read (caller recomputes).
//! `store` writes tmp+rename (atomic) and fsyncs the file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{FEATURE_DIM, Frame};

const MAGIC: &[u8; 4] = b"HFC1";
const HEADER_LEN: usize = 16;

/// Cache path for one utterance (sharded by sha prefix).
pub fn cache_path(root: &Path, frontend_id: &str, audio_sha: &str) -> PathBuf {
    let shard = audio_sha.get(..2).unwrap_or("00");
    root.join(frontend_id)
        .join(shard)
        .join(format!("{audio_sha}.bin"))
}

/// Load cached frames, or `None` on any miss/mismatch/corruption.
pub fn load(path: &Path, sample_rate: u32) -> Option<Vec<Frame>> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC {
        return None;
    }
    let u = |o: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bytes.get(o..o + 4)?.try_into().ok()?) as usize)
    };
    let dim = u(4)?;
    let nframes = u(8)?;
    let rate = u32::from_le_bytes(bytes[12..16].try_into().ok()?);
    if dim != FEATURE_DIM || rate != sample_rate {
        return None;
    }
    let want = HEADER_LEN + nframes * dim * 8;
    if bytes.len() != want {
        return None;
    }
    let mut frames = Vec::with_capacity(nframes);
    for i in 0..nframes {
        let mut f: Frame = [0.0; FEATURE_DIM];
        for (d, v) in f.iter_mut().enumerate() {
            let o = HEADER_LEN + (i * dim + d) * 8;
            *v = f64::from_le_bytes(bytes[o..o + 8].try_into().ok()?);
        }
        if !v_finite(&f) {
            return None;
        }
        frames.push(f);
    }
    Some(frames)
}

fn v_finite(f: &Frame) -> bool {
    f.iter().all(|x| x.is_finite())
}

/// Store frames atomically (tmp file + rename + fsync).
pub fn store(path: &Path, sample_rate: u32, frames: &[Frame]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut buf = Vec::with_capacity(HEADER_LEN + frames.len() * FEATURE_DIM * 8);
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&(FEATURE_DIM as u32).to_le_bytes());
    buf.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    for f in frames {
        for v in f.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    {
        let mut fh = fs::File::create(&tmp)?;
        fh.write_all(&buf)?;
        fh.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: usize) -> Vec<Frame> {
        (0..n)
            .map(|i| {
                let mut f: Frame = [0.0; FEATURE_DIM];
                for (d, v) in f.iter_mut().enumerate() {
                    *v = (i * 131 + d * 17) as f64 * 0.001 - 3.0;
                }
                f
            })
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("hfc_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn round_trip_is_bit_identical() {
        let root = scratch("roundtrip");
        let path = cache_path(&root, "hmm-fe2.test", &"ab".repeat(32));
        let fr = frames(50);
        store(&path, 16_000, &fr).expect("store");
        let back = load(&path, 16_000).expect("load");
        assert_eq!(back, fr);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rate_mismatch_is_miss() {
        let root = scratch("ratemiss");
        let path = cache_path(&root, "hmm-fe2.test", &"cd".repeat(32));
        store(&path, 16_000, &frames(10)).expect("store");
        assert!(load(&path, 8_000).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_magic_is_miss() {
        let root = scratch("corrupt");
        let path = cache_path(&root, "hmm-fe2.test", &"ef".repeat(32));
        store(&path, 16_000, &frames(10)).expect("store");
        let mut bytes = fs::read(&path).expect("read");
        bytes[0] = b'X';
        fs::write(&path, bytes).expect("rewrite");
        assert!(load(&path, 16_000).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_file_is_miss() {
        let root = scratch("missing");
        let path = cache_path(&root, "hmm-fe2.test", &"00".repeat(32));
        assert!(load(&path, 16_000).is_none());
    }
}
