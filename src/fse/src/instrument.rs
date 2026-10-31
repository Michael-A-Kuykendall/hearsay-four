//! Sealed phase-test measurement instrument (bead kyz).
//!
//! A formal phase-test session directory must contain:
//! - `manifest.tsv` (4-col: file, word, rate_hz, frames; header line 1)
//! - `order.txt` (`seed <N>` line 1, then the 30 prompt words in order)
//! - `session.meta` (KEY=VALUE: device, rate_hz, date_utc, seed,
//!   order_sha256, manifest_sha256, recorder_commit, kept)
//! - `SHA256SUMS` (sidecar: `<hash>  <filename>` per wav + manifest +
//!   order.txt, sha256sum-compatible)
//! - 30 WAV files (PCM16 mono 16 kHz, 2.5-3.5 s each)
//!
//! Vocabulary: `sessions/script-wave3.txt`, baked in via `include_str!`
//! (single source of truth; `instrument_vocab_has_thirty_words` pins the
//! shape). Full protocol: `docs/design/phase-test-instrument.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::dtw::DtwVariant;

/// Expected vocabulary, single-sourced from sessions/script-wave3.txt.
const VOCAB_TXT: &str = include_str!("../../../sessions/script-wave3.txt");

pub const EXPECTED_LABELS: usize = 30;
pub const RATE_HZ: u32 = 16_000;
pub const MIN_SECS: f64 = 2.5;
pub const MAX_SECS: f64 = 3.5;

/// Base config name the frozen string must start with (cross-checked against
/// tuning_grid()[0] at the phase-test use site).
pub const FROZEN_BASE: &str = "base_cmn_d_dd_lifter22_pre097";

pub fn vocab_words() -> Vec<&'static str> {
    VOCAB_TXT
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// SHA256 (FIPS 180-4), std-only; pinned by instrument_sha256_vectors.
pub fn sha256_hex(bytes: &[u8]) -> String {
    #[rustfmt::skip]
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[4 * i..4 * i + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[derive(Debug)]
pub struct AcceptSummary {
    pub labels: usize,
    pub seed: u64,
    pub order_sha256: String,
}

/// Validate a formal phase-test session directory: all-or-nothing, first
/// violation reported. Check order (documented in
/// docs/design/phase-test-instrument.md): metadata -> order hash -> manifest
/// hash -> vocab multiset (unknown, duplicate, missing) -> positional order
/// -> sidecar -> headers -> durations.
pub fn validate_session(dir: &Path) -> Result<AcceptSummary, String> {
    // 1. session.meta exists + parses with all required keys.
    let meta_raw = std::fs::read_to_string(dir.join("session.meta")).map_err(|_| {
        format!(
            "REJECT: session.meta missing/unreadable in {}",
            dir.display()
        )
    })?;
    let mut meta = BTreeMap::new();
    for line in meta_raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| format!("REJECT: session.meta malformed line: {line}"))?;
        meta.insert(k.trim().to_string(), v.trim().to_string());
    }
    let need = |k: &str| {
        meta.get(k)
            .cloned()
            .ok_or_else(|| format!("REJECT: session.meta missing key: {k}"))
    };
    let _device = need("device")?;
    let meta_rate: u32 = need("rate_hz")?
        .parse()
        .map_err(|_| "REJECT: session.meta rate_hz not a number".to_string())?;
    let _date = need("date_utc")?;
    let meta_seed: u64 = need("seed")?
        .parse()
        .map_err(|_| "REJECT: session.meta seed not a number".to_string())?;
    let meta_order_hash = need("order_sha256")?;
    let meta_manifest_hash = need("manifest_sha256")?;
    let _commit = need("recorder_commit")?;
    let meta_kept: usize = need("kept")?
        .parse()
        .map_err(|_| "REJECT: session.meta kept not a number".to_string())?;
    // 2. Rate claim.
    if meta_rate != RATE_HZ {
        return Err(format!(
            "REJECT: session.meta rate_hz={meta_rate}, need {RATE_HZ}"
        ));
    }
    // 3. order.txt exists, hash matches, seed line matches, 30 words.
    let order_raw = std::fs::read(dir.join("order.txt"))
        .map_err(|_| format!("REJECT: order.txt missing in {}", dir.display()))?;
    let order_hash = sha256_hex(&order_raw);
    if order_hash != meta_order_hash {
        return Err(format!(
            "REJECT: order.txt hash {order_hash} != session.meta order_sha256 {meta_order_hash} (tampered order?)"
        ));
    }
    let order_text =
        String::from_utf8(order_raw).map_err(|_| "REJECT: order.txt not UTF-8".to_string())?;
    let mut olines = order_text.lines();
    let seed_line = olines.next().ok_or("REJECT: order.txt empty".to_string())?;
    let seed_here: u64 = seed_line
        .strip_prefix("seed ")
        .ok_or_else(|| format!("REJECT: order.txt line 1 must be 'seed <N>', got: {seed_line}"))?
        .parse()
        .map_err(|_| format!("REJECT: order.txt bad seed line: {seed_line}"))?;
    if seed_here != meta_seed {
        return Err(format!(
            "REJECT: order.txt seed {seed_here} != session.meta seed {meta_seed}"
        ));
    }
    let order: Vec<&str> = olines.map(str::trim).filter(|l| !l.is_empty()).collect();
    if order.len() != EXPECTED_LABELS {
        return Err(format!(
            "REJECT: order.txt has {} words, need {}",
            order.len(),
            EXPECTED_LABELS
        ));
    }
    // 4. manifest.tsv exists, hash matches, rows parse (skip header).
    let man_raw = std::fs::read(dir.join("manifest.tsv"))
        .map_err(|_| format!("REJECT: manifest.tsv missing in {}", dir.display()))?;
    let man_hash = sha256_hex(&man_raw);
    if man_hash != meta_manifest_hash {
        return Err(format!(
            "REJECT: manifest.tsv hash {man_hash} != session.meta manifest_sha256 {meta_manifest_hash} (tampered manifest?)"
        ));
    }
    let man_text =
        String::from_utf8(man_raw).map_err(|_| "REJECT: manifest.tsv not UTF-8".to_string())?;
    let mut rows: Vec<(String, String, u32, u32)> = Vec::new();
    for (i, line) in man_text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let mut f = line.split('\t');
        let file = f.next();
        let word = f.next();
        let rate = f.next();
        let frames = f.next();
        let (file, word, rate, frames) = match (file, word, rate, frames) {
            (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
            _ => return Err(format!("REJECT: manifest line {} short: {line}", i + 1)),
        };
        rows.push((
            file.to_string(),
            word.to_string(),
            rate.parse()
                .map_err(|_| format!("REJECT: manifest line {} bad rate: {line}", i + 1))?,
            frames
                .parse()
                .map_err(|_| format!("REJECT: manifest line {} bad frames: {line}", i + 1))?,
        ));
    }
    if rows.len() != meta_kept {
        return Err(format!(
            "REJECT: session.meta kept={meta_kept} disagrees with manifest rows={}",
            rows.len()
        ));
    }
    // 5. Vocab multiset: unknown, then duplicate, then missing.
    let vocab: BTreeSet<&str> = vocab_words().into_iter().collect();
    for (_, w, _, _) in &rows {
        if !vocab.contains(w.as_str()) {
            return Err(format!("REJECT: unknown label in manifest: {w}"));
        }
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, w, _, _) in &rows {
        *counts.entry(w.as_str()).or_insert(0) += 1;
    }
    for (w, c) in &counts {
        if *c > 1 {
            return Err(format!("REJECT: duplicate label in manifest: {w} (x{c})"));
        }
    }
    if rows.len() != EXPECTED_LABELS {
        let have: BTreeSet<&str> = rows.iter().map(|(_, w, _, _)| w.as_str()).collect();
        let missing: Vec<String> = vocab.difference(&have).map(|s| s.to_string()).collect();
        return Err(format!(
            "REJECT: manifest has {} rows, need {}; missing label: {}",
            rows.len(),
            EXPECTED_LABELS,
            missing.join(",")
        ));
    }
    // 6. Positional: manifest row i word == order[i].
    for (i, (_, w, _, _)) in rows.iter().enumerate() {
        if w != order[i] {
            return Err(format!(
                "REJECT: manifest row {} word {w} != order.txt position {} word {}",
                i + 1,
                i + 1,
                order[i]
            ));
        }
    }
    // 7. Sidecar covers every expected file with a matching hash (extras
    // ignored: forward-compatible with recorder additions).
    let sums_raw = std::fs::read_to_string(dir.join("SHA256SUMS"))
        .map_err(|_| format!("REJECT: SHA256SUMS missing in {}", dir.display()))?;
    let mut sums = BTreeMap::new();
    for line in sums_raw.lines() {
        let mut p = line.split_whitespace();
        if let (Some(h), Some(n)) = (p.next(), p.next()) {
            sums.insert(n.to_string(), h.to_string());
        }
    }
    let mut expected: Vec<String> = rows.iter().map(|(f, _, _, _)| f.clone()).collect();
    expected.push("manifest.tsv".to_string());
    expected.push("order.txt".to_string());
    for n in &expected {
        let bytes =
            std::fs::read(dir.join(n)).map_err(|_| format!("REJECT: session file missing: {n}"))?;
        let recomputed = sha256_hex(&bytes);
        match sums.get(n) {
            Some(h) if *h == recomputed => {}
            Some(h) => {
                return Err(format!(
                    "REJECT: SHA256SUMS mismatch for {n} (sidecar {h} != recomputed {recomputed})"
                ));
            }
            None => return Err(format!("REJECT: SHA256SUMS has no entry for {n}")),
        }
    }
    // 8+9. Headers (PCM16 mono 16k, manifest cross-check) + durations.
    for (file, _, mrate, mframes) in &rows {
        let (spec, n_probed) = corpus::audio::wav_probe(&dir.join(file))
            .map_err(|e| format!("REJECT: {file} unreadable WAV: {e}"))?;
        if spec.channels != 1
            || spec.sample_rate != RATE_HZ
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Err(format!(
                "REJECT: {file} must be PCM16 mono {RATE_HZ}Hz, got ch={} rate={} bits={} fmt={:?}",
                spec.channels, spec.sample_rate, spec.bits_per_sample, spec.sample_format
            ));
        }
        if *mrate != spec.sample_rate {
            return Err(format!(
                "REJECT: {file} manifest rate {mrate} != header {}",
                spec.sample_rate
            ));
        }
        let n = n_probed;
        if *mframes != n {
            return Err(format!(
                "REJECT: {file} manifest frames {mframes} != header {n}"
            ));
        }
        let secs = n as f64 / spec.sample_rate as f64;
        if !(MIN_SECS..=MAX_SECS).contains(&secs) {
            return Err(format!(
                "REJECT: {file} duration {secs:.2}s outside {MIN_SECS}-{MAX_SECS}s"
            ));
        }
    }
    Ok(AcceptSummary {
        labels: rows.len(),
        seed: meta_seed,
        order_sha256: order_hash,
    })
}
/// General sealed-session validation for non-30-word sessions (phrases,
/// sentences, corpora). `validate_session` is the FROZEN 30-word seal and is
/// never relaxed; this second function keeps its structural checks (meta,
/// rate, order hash, seed, manifest hash, kept count, positional match,
/// sidecar coverage, WAV headers, manifest cross-check) while parameterizing
/// what is protocol-specific: the label count, the label vocabulary (none:
/// labels may be phrases or sentences), and the take window
/// (`window_secs` ± 0.5 s, the same tolerance shape as the strict 3.0 ± 0.5).
pub fn validate_session_general(
    dir: &Path,
    expected_labels: usize,
    window_secs: f64,
) -> Result<AcceptSummary, String> {
    // 1. session.meta exists + parses with all required keys.
    let meta_raw = std::fs::read_to_string(dir.join("session.meta")).map_err(|_| {
        format!(
            "REJECT: session.meta missing/unreadable in {}",
            dir.display()
        )
    })?;
    let mut meta = BTreeMap::new();
    for line in meta_raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| format!("REJECT: session.meta malformed line: {line}"))?;
        meta.insert(k.trim().to_string(), v.trim().to_string());
    }
    let need = |k: &str| {
        meta.get(k)
            .cloned()
            .ok_or_else(|| format!("REJECT: session.meta missing key: {k}"))
    };
    let _device = need("device")?;
    let meta_rate: u32 = need("rate_hz")?
        .parse()
        .map_err(|_| "REJECT: session.meta rate_hz not a number".to_string())?;
    let _date = need("date_utc")?;
    let meta_seed: u64 = need("seed")?
        .parse()
        .map_err(|_| "REJECT: session.meta seed not a number".to_string())?;
    let meta_order_hash = need("order_sha256")?;
    let meta_manifest_hash = need("manifest_sha256")?;
    let _commit = need("recorder_commit")?;
    let meta_kept: usize = need("kept")?
        .parse()
        .map_err(|_| "REJECT: session.meta kept not a number".to_string())?;
    // 2. Rate claim.
    if meta_rate != RATE_HZ {
        return Err(format!(
            "REJECT: session.meta rate_hz={meta_rate}, need {RATE_HZ}"
        ));
    }
    // 3. order.txt exists, hash matches, seed line matches, expected labels.
    let order_raw = std::fs::read(dir.join("order.txt"))
        .map_err(|_| format!("REJECT: order.txt missing in {}", dir.display()))?;
    let order_hash = sha256_hex(&order_raw);
    if order_hash != meta_order_hash {
        return Err(format!(
            "REJECT: order.txt hash {order_hash} != session.meta order_sha256 {meta_order_hash} (tampered order?)"
        ));
    }
    let order_text =
        String::from_utf8(order_raw).map_err(|_| "REJECT: order.txt not UTF-8".to_string())?;
    let mut olines = order_text.lines();
    let seed_line = olines.next().ok_or("REJECT: order.txt empty".to_string())?;
    let seed_here: u64 = seed_line
        .strip_prefix("seed ")
        .ok_or_else(|| format!("REJECT: order.txt line 1 must be 'seed <N>', got: {seed_line}"))?
        .parse()
        .map_err(|_| format!("REJECT: order.txt bad seed line: {seed_line}"))?;
    if seed_here != meta_seed {
        return Err(format!(
            "REJECT: order.txt seed {seed_here} != session.meta seed {meta_seed}"
        ));
    }
    let order: Vec<&str> = olines.map(str::trim).filter(|l| !l.is_empty()).collect();
    if order.len() != expected_labels {
        return Err(format!(
            "REJECT: order.txt has {} labels, need {}",
            order.len(),
            expected_labels
        ));
    }
    // 4. manifest.tsv exists, hash matches, rows parse (skip header).
    let man_raw = std::fs::read(dir.join("manifest.tsv"))
        .map_err(|_| format!("REJECT: manifest.tsv missing in {}", dir.display()))?;
    let man_hash = sha256_hex(&man_raw);
    if man_hash != meta_manifest_hash {
        return Err(format!(
            "REJECT: manifest.tsv hash {man_hash} != session.meta manifest_sha256 {meta_manifest_hash} (tampered manifest?)"
        ));
    }
    let man_text =
        String::from_utf8(man_raw).map_err(|_| "REJECT: manifest.tsv not UTF-8".to_string())?;
    let mut rows: Vec<(String, String, u32, u32)> = Vec::new();
    for (i, line) in man_text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let mut f = line.split('\t');
        let file = f.next();
        let word = f.next();
        let rate = f.next();
        let frames = f.next();
        let (file, word, rate, frames) = match (file, word, rate, frames) {
            (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
            _ => return Err(format!("REJECT: manifest line {} short: {line}", i + 1)),
        };
        rows.push((
            file.to_string(),
            word.to_string(),
            rate.parse()
                .map_err(|_| format!("REJECT: manifest line {} bad rate: {line}", i + 1))?,
            frames
                .parse()
                .map_err(|_| format!("REJECT: manifest line {} bad frames: {line}", i + 1))?,
        ));
    }
    if rows.len() != meta_kept {
        return Err(format!(
            "REJECT: session.meta kept={meta_kept} disagrees with manifest rows={}",
            rows.len()
        ));
    }
    // 5. No vocab gate on the general path: labels may be phrases or
    // sentences outside the 30-word set. Count only.
    if rows.len() != expected_labels {
        return Err(format!(
            "REJECT: manifest has {} rows, need {}",
            rows.len(),
            expected_labels
        ));
    }
    // 6. Positional: manifest row i word == order[i].
    for (i, (_, w, _, _)) in rows.iter().enumerate() {
        if w != order[i] {
            return Err(format!(
                "REJECT: manifest row {} word {w} != order.txt position {} word {}",
                i + 1,
                i + 1,
                order[i]
            ));
        }
    }
    // 7. Sidecar covers every expected file with a matching hash (extras
    // ignored: forward-compatible with recorder additions).
    let sums_raw = std::fs::read_to_string(dir.join("SHA256SUMS"))
        .map_err(|_| format!("REJECT: SHA256SUMS missing in {}", dir.display()))?;
    let mut sums = BTreeMap::new();
    for line in sums_raw.lines() {
        let mut p = line.split_whitespace();
        if let (Some(h), Some(n)) = (p.next(), p.next()) {
            sums.insert(n.to_string(), h.to_string());
        }
    }
    let mut expected: Vec<String> = rows.iter().map(|(f, _, _, _)| f.clone()).collect();
    expected.push("manifest.tsv".to_string());
    expected.push("order.txt".to_string());
    for n in &expected {
        let bytes =
            std::fs::read(dir.join(n)).map_err(|_| format!("REJECT: session file missing: {n}"))?;
        let recomputed = sha256_hex(&bytes);
        match sums.get(n) {
            Some(h) if *h == recomputed => {}
            Some(h) => {
                return Err(format!(
                    "REJECT: SHA256SUMS mismatch for {n} (sidecar {h} != recomputed {recomputed})"
                ));
            }
            None => return Err(format!("REJECT: SHA256SUMS has no entry for {n}")),
        }
    }
    // 8+9. Headers (PCM16 mono 16k, manifest cross-check) + window.
    for (file, _, mrate, mframes) in &rows {
        let (spec, n_probed) = corpus::audio::wav_probe(&dir.join(file))
            .map_err(|e| format!("REJECT: {file} unreadable WAV: {e}"))?;
        if spec.channels != 1
            || spec.sample_rate != RATE_HZ
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Err(format!(
                "REJECT: {file} must be PCM16 mono {RATE_HZ}Hz, got ch={} rate={} bits={} fmt={:?}",
                spec.channels, spec.sample_rate, spec.bits_per_sample, spec.sample_format
            ));
        }
        if *mrate != spec.sample_rate {
            return Err(format!(
                "REJECT: {file} manifest rate {mrate} != header {}",
                spec.sample_rate
            ));
        }
        let n = n_probed;
        if *mframes != n {
            return Err(format!(
                "REJECT: {file} manifest frames {mframes} != header {n}"
            ));
        }
        let secs = n as f64 / spec.sample_rate as f64;
        if !((window_secs - 0.5)..=(window_secs + 0.5)).contains(&secs) {
            return Err(format!(
                "REJECT: {file} duration {secs:.2}s outside {:.1}-{:.1}s",
                window_secs - 0.5,
                window_secs + 0.5
            ));
        }
    }
    Ok(AcceptSummary {
        labels: rows.len(),
        seed: meta_seed,
        order_sha256: order_hash,
    })
}
/// Score-once writer: refuses an existing output dir (no overwrite, no
/// re-score), else creates it and writes score.log.
pub fn write_score(out_dir: &Path, log_text: &str) -> Result<(), String> {
    if out_dir.exists() {
        return Err(format!(
            "REJECT: refusing to write into existing output dir {} (score-once: no overwrite, no re-score)",
            out_dir.display()
        ));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("REJECT: cannot create {}: {e}", out_dir.display()))?;
    std::fs::write(out_dir.join("score.log"), log_text)
        .map_err(|e| format!("REJECT: cannot write score.log: {e}"))?;
    Ok(())
}

#[derive(Debug)]
pub struct FrozenConfig {
    pub base_name: String,
    pub cmvn: bool,
    pub variant: DtwVariant,
    pub band: f64,
}

/// Parse a --frozen-config string (`<base>+tokens...`); fail closed on any
/// unknown token, wrong base, or out-of-range band.
pub fn parse_frozen_config(s: &str) -> Result<FrozenConfig, String> {
    let mut parts = s.split('+');
    let base = parts.next().unwrap_or("");
    if base != FROZEN_BASE {
        return Err(format!("REJECT: frozen base {base} != {FROZEN_BASE}"));
    }
    let mut cmvn = false;
    let mut variant = DtwVariant::Baseline;
    let mut band: Option<f64> = None;
    for tok in parts {
        if tok == "cmvn_replaces_cmn" {
            cmvn = true;
        } else if let Some(v) = tok.strip_prefix("dtw_") {
            variant = match v {
                "baseline" => DtwVariant::Baseline,
                "v1" => DtwVariant::SymWeighted,
                "v2" => DtwVariant::SlopeConstrained,
                "v4" => DtwVariant::SymWeightedSlope,
                _ => return Err(format!("REJECT: frozen dtw token unknown: {tok}")),
            };
        } else if let Some(b) = tok.strip_prefix("band") {
            let f: f64 = b
                .parse()
                .map_err(|_| format!("REJECT: frozen band bad: {tok}"))?;
            if !(f > 0.0 && f <= 1.0) {
                return Err(format!("REJECT: frozen band out of (0,1]: {tok}"));
            }
            band = Some(f);
        } else {
            return Err(format!("REJECT: frozen token unknown: {tok}"));
        }
    }
    let band = band.ok_or("REJECT: frozen string missing band".to_string())?;
    Ok(FrozenConfig {
        base_name: base.to_string(),
        cmvn,
        variant,
        band,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let p = Path::new("/tmp").join(format!("kyz_{name}"));
        if p.exists() {
            std::fs::remove_dir_all(&p).unwrap();
        }
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_sine_wav(path: &Path, freq: f64, secs: f64) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (secs * 16_000.0) as usize;
        for i in 0..n {
            let t = i as f64 / 16_000.0;
            let s = (8000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16;
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    fn write_rate_wav(path: &Path, rate: u32, secs: f64) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (secs * rate as f64) as usize;
        for _ in 0..n {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
    }

    /// Prompt order: fixed rotation by 7 (deterministic, non-identity, so
    /// positional correspondence is genuinely exercised).
    fn prompt_order() -> Vec<&'static str> {
        let vocab = vocab_words();
        (0..30).map(|i| vocab[(i + 7) % 30]).collect()
    }

    /// Build a session; `drop` omits one word's file+manifest row (order.txt
    /// keeps all 30). Hashes always computed over the files as written.
    fn build_session(dir: &Path, drop: Option<&str>) {
        let order = prompt_order();
        let seed = 7u64;
        let mut manifest = String::from("file\tword\trate_hz\tframes\n");
        for (i, w) in order.iter().enumerate() {
            if Some(*w) == drop {
                continue;
            }
            let fname = format!("utt{i:03}.wav");
            write_sine_wav(&dir.join(&fname), 300.0 + i as f64 * 20.0, 3.0);
            manifest.push_str(&format!("{fname}\t{w}\t16000\t48000\n"));
        }
        std::fs::write(dir.join("manifest.tsv"), &manifest).unwrap();
        let mut order_txt = format!("seed {seed}\n");
        for w in &order {
            order_txt.push_str(w);
            order_txt.push('\n');
        }
        std::fs::write(dir.join("order.txt"), &order_txt).unwrap();
        refresh_hashes(dir);
        let kept = if drop.is_some() { 29 } else { 30 };
        let meta = format!(
            "device=fixture\nrate_hz=16000\ndate_utc=2026-10-29T00:00:00Z\nseed={seed}\norder_sha256={}\nmanifest_sha256={}\nrecorder_commit=fixture\nkept={kept}\n",
            sha256_hex(&std::fs::read(dir.join("order.txt")).unwrap()),
            sha256_hex(&std::fs::read(dir.join("manifest.tsv")).unwrap()),
        );
        std::fs::write(dir.join("session.meta"), &meta).unwrap();
    }
    /// Build an N-label general session with sine takes of `secs` seconds.
    /// Labels may be phrases (non-vocab): the general path has no vocab gate.
    fn build_general_session(dir: &Path, labels: &[&str], secs: f64) {
        let seed = 11u64;
        let frames = (secs * RATE_HZ as f64).round() as u32;
        let mut manifest = String::from("file\tword\trate_hz\tframes\n");
        for (i, w) in labels.iter().enumerate() {
            let fname = format!("utt{i:03}.wav");
            write_sine_wav(&dir.join(&fname), 300.0 + i as f64 * 20.0, secs);
            manifest.push_str(&format!("{fname}\t{w}\t16000\t{frames}\n"));
        }
        std::fs::write(dir.join("manifest.tsv"), &manifest).unwrap();
        let mut order_txt = format!("seed {seed}\n");
        for w in labels {
            order_txt.push_str(w);
            order_txt.push('\n');
        }
        std::fs::write(dir.join("order.txt"), &order_txt).unwrap();
        let meta = format!(
            "device=fixture\nrate_hz=16000\ndate_utc=2026-10-29T00:00:00Z\nseed={seed}\norder_sha256={}\nmanifest_sha256={}\nrecorder_commit=fixture\nkept={}\n",
            sha256_hex(&std::fs::read(dir.join("order.txt")).unwrap()),
            sha256_hex(&std::fs::read(dir.join("manifest.tsv")).unwrap()),
            labels.len(),
        );
        std::fs::write(dir.join("session.meta"), &meta).unwrap();
        let mut names: Vec<String> = (0..labels.len())
            .map(|i| format!("utt{i:03}.wav"))
            .collect();
        names.push("manifest.tsv".to_string());
        names.push("order.txt".to_string());
        let mut sums = String::new();
        for n in &names {
            let b = std::fs::read(dir.join(n)).unwrap();
            sums.push_str(&format!("{}  {n}\n", sha256_hex(&b)));
        }
        std::fs::write(dir.join("SHA256SUMS"), &sums).unwrap();
    }
    #[test]
    fn general_accepts_phrase_labels() {
        let dir = test_dir("general7");
        let labels = [
            "the birch canoe",
            "glue the sheet",
            "dark blue background",
            "smooth planks",
            "bright sun",
            "quiet room",
            "second line",
        ];
        build_general_session(&dir, &labels, 3.0);
        let s = validate_session_general(&dir, 7, 3.0).expect("general session accepts");
        assert_eq!(s.labels, 7);
        assert_eq!(s.seed, 11);
        assert_eq!(s.order_sha256.len(), 64);
    }
    #[test]
    fn general_rejects_count_mismatch() {
        let dir = test_dir("general7_wrongcount");
        let labels = ["a", "b", "c", "d", "e", "f", "g"];
        build_general_session(&dir, &labels, 3.0);
        let e = validate_session_general(&dir, 6, 3.0).expect_err("count must reject");
        assert!(e.contains("need 6"), "violation: {e}");
    }
    #[test]
    fn general_rejects_window_violation() {
        let dir = test_dir("general7_short");
        let labels = ["a", "b", "c", "d", "e", "f", "g"];
        build_general_session(&dir, &labels, 1.0);
        let e = validate_session_general(&dir, 7, 3.0).expect_err("window must reject");
        assert!(e.contains("outside"), "violation: {e}");
    }
    #[test]
    fn general_window_follows_param() {
        let dir = test_dir("general7_1s");
        let labels = ["a", "b", "c", "d", "e", "f", "g"];
        build_general_session(&dir, &labels, 1.0);
        let s = validate_session_general(&dir, 7, 1.0).expect("1s window accepts");
        assert_eq!(s.labels, 7);
    }
    /// Recompute SHA256SUMS + the meta hashes over current dir state, and
    /// sync kept to the manifest row count (a consistent session, whatever
    /// its content violations).
    fn refresh_hashes(dir: &Path) {
        let manifest = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
        let mut names: Vec<String> = Vec::new();
        for (i, line) in manifest.lines().enumerate() {
            if i == 0 || line.trim().is_empty() {
                continue;
            }
            names.push(line.split('\t').next().unwrap().to_string());
        }
        names.push("manifest.tsv".to_string());
        names.push("order.txt".to_string());
        let mut sums = String::new();
        for n in &names {
            let b = std::fs::read(dir.join(n)).unwrap();
            sums.push_str(&format!("{}  {n}\n", sha256_hex(&b)));
        }
        std::fs::write(dir.join("SHA256SUMS"), &sums).unwrap();
        let meta_path = dir.join("session.meta");
        if meta_path.exists() {
            let meta = std::fs::read_to_string(&meta_path).unwrap();
            let mut out = String::new();
            for line in meta.lines() {
                if let Some((k, _)) = line.split_once('=') {
                    let v = match k {
                        "order_sha256" => {
                            sha256_hex(&std::fs::read(dir.join("order.txt")).unwrap())
                        }
                        "manifest_sha256" => {
                            sha256_hex(&std::fs::read(dir.join("manifest.tsv")).unwrap())
                        }
                        "kept" => (names.len() - 2).to_string(),
                        _ => line.split_once('=').unwrap().1.to_string(),
                    };
                    out.push_str(&format!("{k}={v}\n"));
                }
            }
            std::fs::write(&meta_path, out).unwrap();
        }
    }

    #[test]
    fn instrument_accepts_good_session() {
        let dir = test_dir("good");
        build_session(&dir, None);
        let s = validate_session(&dir).expect("good session accepts");
        assert_eq!(s.labels, 30);
        assert_eq!(s.seed, 7);
        assert_eq!(s.order_sha256.len(), 64);
    }

    #[test]
    fn instrument_rejects_partial_session() {
        let dir = test_dir("partial");
        let dropped = prompt_order()[5];
        build_session(&dir, Some(dropped));
        let e = validate_session(&dir).expect_err("partial session rejects");
        assert!(e.contains("missing label"), "violation: {e}");
        assert!(e.contains(dropped), "names {dropped}: {e}");
    }

    #[test]
    fn instrument_rejects_wrong_rate() {
        let dir = test_dir("rate");
        build_session(&dir, None);
        write_rate_wav(&dir.join("utt003.wav"), 48_000, 3.0);
        // Fix the manifest row to the new header so the RATE check (not the
        // frames cross-check) is what fires.
        let m = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
        let fixed = m.replace(
            &format!("utt003.wav\t{}\t16000\t48000", prompt_order()[3]),
            &format!("utt003.wav\t{}\t48000\t144000", prompt_order()[3]),
        );
        std::fs::write(dir.join("manifest.tsv"), fixed).unwrap();
        refresh_hashes(&dir);
        let e = validate_session(&dir).expect_err("wrong rate rejects");
        assert!(e.contains("utt003.wav"), "violation: {e}");
        assert!(e.contains("48000"), "violation: {e}");
    }

    #[test]
    fn instrument_rejects_unknown_label() {
        let dir = test_dir("unknown");
        build_session(&dir, None);
        let m = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
        let w = prompt_order()[3];
        let tampered = m.replace(&format!("\t{w}\t"), "\txyzzy\t");
        assert_ne!(m, tampered);
        std::fs::write(dir.join("manifest.tsv"), tampered).unwrap();
        refresh_hashes(&dir);
        let e = validate_session(&dir).expect_err("unknown label rejects");
        assert!(e.contains("xyzzy"), "violation: {e}");
        assert!(e.contains("unknown"), "violation: {e}");
    }

    #[test]
    fn instrument_rejects_duplicate_label() {
        let dir = test_dir("dup");
        build_session(&dir, None);
        let m = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
        let wa = prompt_order()[2];
        let wb = prompt_order()[4];
        let tampered = m.replace(&format!("\t{wb}\t"), &format!("\t{wa}\t"));
        assert_ne!(m, tampered);
        std::fs::write(dir.join("manifest.tsv"), tampered).unwrap();
        refresh_hashes(&dir);
        let e = validate_session(&dir).expect_err("duplicate label rejects");
        assert!(e.contains(wa), "violation: {e}");
        assert!(e.contains("duplicate"), "violation: {e}");
    }

    #[test]
    fn instrument_refuses_existing_output_dir() {
        let dir = test_dir("outdir");
        let existing = dir.join("taken");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("old.txt"), "x").unwrap();
        let e = write_score(&existing, "log").expect_err("existing dir refuses");
        assert!(e.contains("existing"), "violation: {e}");
        let fresh = dir.join("fresh");
        write_score(&fresh, "log-body").expect("fresh dir writes");
        assert_eq!(
            std::fs::read_to_string(fresh.join("score.log")).unwrap(),
            "log-body"
        );
    }

    #[test]
    fn instrument_rejects_tampered_order() {
        let dir = test_dir("tamper");
        build_session(&dir, None);
        let o = std::fs::read_to_string(dir.join("order.txt")).unwrap();
        let mut lines: Vec<&str> = o.lines().collect();
        lines.swap(1, 2);
        std::fs::write(dir.join("order.txt"), lines.join("\n") + "\n").unwrap();
        // No refresh: the stale meta hash must fire.
        let e = validate_session(&dir).expect_err("tampered order rejects");
        assert!(
            e.contains("order_sha256") || e.contains("order.txt"),
            "violation: {e}"
        );
    }

    #[test]
    fn instrument_rejects_tampered_manifest() {
        let dir = test_dir("tamperman");
        build_session(&dir, None);
        let m = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
        std::fs::write(dir.join("manifest.tsv"), m.replace("48000", "47999")).unwrap();
        let e = validate_session(&dir).expect_err("tampered manifest rejects");
        assert!(
            e.contains("manifest_sha256") || e.contains("manifest"),
            "violation: {e}"
        );
    }

    #[test]
    fn instrument_rejects_missing_metadata() {
        let dir = test_dir("nometa");
        build_session(&dir, None);
        std::fs::remove_file(dir.join("session.meta")).unwrap();
        let e = validate_session(&dir).expect_err("missing metadata rejects");
        assert!(e.contains("session.meta"), "violation: {e}");
    }

    #[test]
    fn instrument_rejects_bad_duration() {
        let dir = test_dir("dur");
        build_session(&dir, None);
        write_sine_wav(&dir.join("utt010.wav"), 440.0, 2.0);
        // Fix the manifest row to the new header so the DURATION check (not
        // the frames cross-check) is what fires.
        let m = std::fs::read_to_string(dir.join("manifest.tsv")).unwrap();
        let fixed = m.replace(
            &format!("utt010.wav\t{}\t16000\t48000", prompt_order()[10]),
            &format!("utt010.wav\t{}\t16000\t32000", prompt_order()[10]),
        );
        assert_ne!(m, fixed);
        std::fs::write(dir.join("manifest.tsv"), fixed).unwrap();
        refresh_hashes(&dir);
        let e = validate_session(&dir).expect_err("bad duration rejects");
        assert!(e.contains("utt010.wav"), "violation: {e}");
        assert!(e.contains("2.5"), "violation: {e}");
    }

    #[test]
    fn instrument_rejects_manifest_frames_mismatch() {
        let dir = test_dir("frames");
        build_session(&dir, None);
        write_sine_wav(&dir.join("utt011.wav"), 440.0, 2.0);
        // Manifest row left stale (48000) + refresh: the frames cross-check
        // fires before duration.
        refresh_hashes(&dir);
        let e = validate_session(&dir).expect_err("frames mismatch rejects");
        assert!(e.contains("utt011.wav"), "violation: {e}");
    }

    #[test]
    fn instrument_sha256_vectors() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn instrument_vocab_has_thirty_words() {
        let v = vocab_words();
        assert_eq!(v.len(), 30);
        assert!(v.contains(&"congealed"));
        assert!(v.contains(&"rhubarb"));
    }

    #[test]
    fn instrument_frozen_config_parses() {
        let f = parse_frozen_config(
            "base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn+dtw_baseline+band0.2",
        )
        .expect("frozen parses");
        assert!(f.cmvn);
        assert_eq!(f.band, 0.2);
        let g =
            parse_frozen_config("base_cmn_d_dd_lifter22_pre097+dtw_v4+band0.2").expect("v4 parses");
        assert!(!g.cmvn);
    }

    #[test]
    fn instrument_frozen_config_rejects_bad_token() {
        assert!(parse_frozen_config("").is_err());
        assert!(parse_frozen_config("base_cmn_d_dd_lifter22_pre097+dtw_bogus+band0.2").is_err());
        assert!(parse_frozen_config("wrong_base+dtw_baseline+band0.2").is_err());
        assert!(parse_frozen_config("base_cmn_d_dd_lifter22_pre097+dtw_baseline+band9").is_err());
    }
}
