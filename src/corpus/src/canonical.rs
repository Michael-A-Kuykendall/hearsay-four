//! RS-1 canonical session module (`docs/design/recording-standard-rs1.md` §4).
//!
//! The only path from raw owner sessions into experiments. [`canon_session`]
//! validates a raw session, resamples every take to 16 kHz mono with the ONE
//! pinned windowed-sinc polyphase resampler ([`resample_pinned`]), endpoints
//! with the RS-1 endpointer ([`endpoint_span`]), re-pads to exactly 250 ms
//! each side ([`repad_clip`]), and writes takes + `manifest.tsv` +
//! `provenance.tsv` + `qc.tsv` + `qc/` plots + `SHA256SUMS`.
//!
//! Determinism contract: the same raw input bytes produce byte-identical
//! output bytes (same file set, same contents). No timestamps, no hash-map
//! iteration, no randomness anywhere in this module.
//!
//! Two raw input flavors are accepted:
//! - `Rs1`: §3 layout (`takeNNN.wav`, `prompts.txt`, `session.meta`,
//!   `session.log`, `SHA256SUMS`) with full integrity validation.
//! - `LegacyRun1`: grandfathered run1 shape (`uttNNN.wav` + `enroll.wav`,
//!   headerless 4-column `manifest.tsv`, optional `session.tsv`). The enroll
//!   row is skipped, never canonicalized.

use std::path::{Path, PathBuf};

use crate::audio::{self, PcmAudio};

/// Canonicalizer version, recorded in every `provenance.tsv` row.
/// Bump ONLY with a bead that re-derives every canonical session.
pub const CANON_VERSION: &str = "rs1-canon-v1";

/// Exact re-pad on each side of the endpoint span (RS-1 §4 step 4).
pub const PAD_MS: u32 = 250;

/// Pinned resampler parameters. Frozen: never tuned, never configured.
/// Any change ships as a new [`RESAMPLER_ID`] + [`CANON_VERSION`].
pub const RESAMPLE_ZERO_CROSSINGS: u32 = 8;
pub const RESAMPLE_PHASES: u32 = 32;
pub const RESAMPLE_KAISER_BETA: f64 = 6.0;
pub const RESAMPLER_ID: &str = "kaiser-sinc-polyphase-zc8-ph32-beta6.0-v1";

/// RS-1 endpointer constants (RS-1 §4 step 3). Starting values, adjustable
/// ONLY from the tightness audit on DEV takes, never from accuracy.
pub const EP_WIN_MS: u32 = 25;
pub const EP_SHIFT_MS: u32 = 10;
pub const EP_GUARD_MS: u32 = 300;
pub const EP_NOISE_PERCENTILE: u32 = 10;
pub const EP_THRESH_ABOVE_NOISE_DB: f64 = 12.0;
pub const EP_THRESH_BELOW_PEAK_DB: f64 = 30.0;
pub const EP_MAX_GAP_MS: u32 = 200;
pub const EP_HANGOVER_MS: u32 = 50;
pub const EP_MIN_SPEECH_MS: u32 = 150;
pub const EP_MIN_PEAK_DBFS: f64 = -40.0;

/// QC plot cadence: every 10th take by 0-based index (0, 10, 20, ...).
pub const QC_PLOT_EVERY: usize = 10;

/// QC plot geometry (frozen so reruns are byte-identical).
pub const QC_PLOT_W: u32 = 800;
pub const QC_PLOT_H: u32 = 240;

// ---------------------------------------------------------------------------
// SHA-256 (std-only, pinned; corpus must not depend on fse, which will depend
// on corpus for the RS1-c loader migration — a shared dep would cycle).
// ---------------------------------------------------------------------------

const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, wv) in w.iter_mut().enumerate().take(16) {
        let o = i * 4;
        *wv = u32::from_be_bytes([block[o], block[o + 1], block[o + 2], block[o + 3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA_K[i])
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

/// Pinned SHA-256, hex digest. Pure std, deterministic.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut chunks = bytes.chunks_exact(64);
    for c in &mut chunks {
        let mut b = [0u8; 64];
        b.copy_from_slice(c);
        sha256_compress(&mut h, &b);
    }
    let rem = chunks.remainder();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut tail = Vec::with_capacity(128);
    tail.extend_from_slice(rem);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bit_len.to_be_bytes());
    for c in tail.chunks_exact(64) {
        let mut b = [0u8; 64];
        b.copy_from_slice(c);
        sha256_compress(&mut h, &b);
    }
    let mut s = String::with_capacity(64);
    for v in h {
        s.push_str(&format!("{v:08x}"));
    }
    s
}

// ---------------------------------------------------------------------------
// Pinned windowed-sinc polyphase resampler (mono i16 in/out).
// ---------------------------------------------------------------------------

/// Modified Bessel I0 by power series (25 terms; converges for β ≤ 10 and is
/// bit-deterministic for a fixed input). Kaiser window denominator/numerator.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let z = (x * x) / 4.0;
    for k in 1..=25 {
        term *= z / (k as f64 * k as f64);
        sum += term;
    }
    sum
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// One polyphase table row for fractional delay `frac` in [0, 1).
/// Support half-length `h` input samples; anti-alias scale `s` (≥ 1).
fn polyphase_row(frac: f64, h: usize, s: f64) -> Vec<f64> {
    let beta = RESAMPLE_KAISER_BETA;
    let denom = bessel_i0(beta);
    let taps = 2 * h + 1;
    let mut row = Vec::with_capacity(taps);
    for j in 0..taps {
        let x = (j as f64 - h as f64) - frac;
        let u = (x / h as f64).clamp(-1.0, 1.0);
        let w = bessel_i0(beta * (1.0 - u * u).sqrt()) / denom;
        row.push(sinc(x / s) / s * w);
    }
    // Unity DC gain per phase (deterministic; guards edge-heavy phases).
    let sum: f64 = row.iter().sum();
    if sum != 0.0 {
        for v in &mut row {
            *v /= sum;
        }
    }
    row
}

/// Resample mono `samples` from `src_rate` to 16 kHz with the ONE pinned
/// windowed-sinc polyphase resampler ([`RESAMPLER_ID`]).
///
/// Frozen design: Kaiser(β=6) windowed sinc, 8 zero-crossings each side
/// (scaled by the downsample factor for anti-aliasing), 32 quantized
/// fractional-delay phases, unity-DC rows, edge-sample clamping.
/// Output length is `(len * 16000 / src_rate)` by integer floor.
/// `src_rate == 16000` is identity; `src_rate == 0` yields empty output.
pub fn resample_pinned(samples: &[i16], src_rate: u32) -> Vec<i16> {
    if samples.is_empty() || src_rate == 0 {
        return Vec::new();
    }
    let dst_rate = audio::TARGET_SAMPLE_RATE;
    if src_rate == dst_rate {
        return samples.to_vec();
    }
    let out_len = (samples.len() * dst_rate as usize) / src_rate as usize;
    if out_len == 0 {
        return Vec::new();
    }
    // Anti-alias scale + support grow with the downsample factor only.
    let ratio = src_rate as f64 / dst_rate as f64;
    let s = ratio.max(1.0);
    let h = ((RESAMPLE_ZERO_CROSSINGS as f64 * s).ceil() as usize).max(1);
    let phases = RESAMPLE_PHASES as usize;
    // Deterministic table: one row per quantized fractional delay.
    let table: Vec<Vec<f64>> = (0..phases)
        .map(|p| polyphase_row(p as f64 / phases as f64, h, s))
        .collect();
    let n = samples.len();
    let mut out = Vec::with_capacity(out_len);
    for m in 0..out_len {
        let t = m as f64 * src_rate as f64 / dst_rate as f64;
        let i0 = t.floor() as isize;
        let frac = t - t.floor();
        let p = ((frac * phases as f64).floor() as usize).min(phases - 1);
        let row = &table[p];
        let mut acc = 0.0;
        for (j, c) in row.iter().enumerate() {
            let idx = (i0 - h as isize + j as isize).clamp(0, n as isize - 1) as usize;
            acc += samples[idx] as f64 * c;
        }
        out.push(acc.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
    }
    out
}

/// Collapse `audio` to mono and resample to 16 kHz with the pinned resampler.
pub fn resample_to_16k_mono(audio: &PcmAudio) -> Vec<i16> {
    let mono = audio::to_mono(audio);
    resample_pinned(&mono, audio.sample_rate)
}
// ---------------------------------------------------------------------------
// RS-1 endpointer (25 ms / 10 ms frame log-energy).
// ---------------------------------------------------------------------------

/// Endpoint span in 16 kHz sample offsets, `end_sample` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointSpan {
    pub start_sample: usize,
    pub end_sample: usize,
}

impl EndpointSpan {
    /// Post-hangover span duration in ms (16 samples per ms, exact).
    pub fn speech_ms(&self) -> u32 {
        (self.end_sample.saturating_sub(self.start_sample) / 16) as u32
    }
}

/// Full endpoint analysis: frame energies + threshold + span-or-rejection.
/// Rejections are `Err(String)` with reason `too quiet` / `too short`.
#[derive(Debug, Clone)]
pub struct EndpointAnalysis {
    pub db: Vec<f64>,
    pub threshold_db: f64,
    pub noise_db: f64,
    pub peak_frame_db: f64,
    pub span: Result<EndpointSpan, String>,
}

/// Frame log-energy in dB (25 ms RMS window, 10 ms shift) at 16 kHz.
/// Matches `scripts/probe_onsets.py` framing (`1e-6` energy floor).
pub fn frame_log_energy_db(pcm16k: &[i16]) -> Vec<f64> {
    let rate = audio::TARGET_SAMPLE_RATE as usize;
    let win = rate * EP_WIN_MS as usize / 1000;
    let shift = rate * EP_SHIFT_MS as usize / 1000;
    if pcm16k.len() < win {
        return Vec::new();
    }
    let n = (pcm16k.len() - win) / shift + 1;
    let mut db = Vec::with_capacity(n);
    for f in 0..n {
        let s0 = f * shift;
        let mut sum = 0.0;
        for &s in &pcm16k[s0..s0 + win] {
            let v = s as f64 / 32768.0;
            sum += v * v;
        }
        let rms = (sum / win as f64).sqrt().max(1e-6);
        db.push(20.0 * rms.log10());
    }
    db
}

/// Peak level in dBFS. Digital silence floors at -120 dB (never -inf, so QC
/// columns stay numeric).
pub fn peak_dbfs(samples: &[i16]) -> f64 {
    let peak = samples.iter().map(|&s| s.abs() as i32).max().unwrap_or(0);
    if peak == 0 {
        -120.0
    } else {
        20.0 * ((peak as f64 / 32768.0).log10())
    }
}

/// 10th-percentile frame energy by integer rank (deterministic; no float
/// interpolation). `frames` must be non-empty.
fn noise_floor_db(frames: &[f64]) -> f64 {
    let mut v = frames.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[(v.len() - 1) * EP_NOISE_PERCENTILE as usize / 100]
}

/// RS-1 endpoint analysis on 16 kHz mono PCM.
///
/// - Guard: frames starting before 300 ms are excluded from span search
///   (RS1-0 CLICK verdict: the Enter-key transient lives there).
/// - `T = max(N + 12 dB, peak − 30 dB)`; speech runs bridge gaps ≤ 200 ms;
///   the longest bridged run wins (ties → earliest); ±50 ms hangover.
/// - The 150 ms minimum applies to the PRE-hangover bridged region (hangover
///   padding is not speech); hangover extends the accepted span and may dip
///   into the guard zone.
pub fn analyze(pcm16k: &[i16]) -> EndpointAnalysis {
    let db = frame_log_energy_db(pcm16k);
    let peak = peak_dbfs(pcm16k);
    let blank = EndpointAnalysis {
        db: db.clone(),
        threshold_db: f64::NEG_INFINITY,
        noise_db: f64::NEG_INFINITY,
        peak_frame_db: f64::NEG_INFINITY,
        span: Err("rejected: too short (no frames)".to_string()),
    };
    if db.is_empty() {
        return blank;
    }
    if peak < EP_MIN_PEAK_DBFS {
        return EndpointAnalysis {
            threshold_db: f64::NEG_INFINITY,
            noise_db: noise_floor_db(&db),
            peak_frame_db: db.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            span: Err(format!(
                "rejected: too quiet (peak {peak:.1} dBFS < {})",
                EP_MIN_PEAK_DBFS
            )),
            ..blank
        };
    }
    let noise = noise_floor_db(&db);
    let peak_frame = db.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let thresh = (noise + EP_THRESH_ABOVE_NOISE_DB).max(peak_frame - EP_THRESH_BELOW_PEAK_DB);
    let first_eligible = EP_GUARD_MS as usize / EP_SHIFT_MS as usize;
    let bridge = EP_MAX_GAP_MS as usize / EP_SHIFT_MS as usize;
    // Above-threshold runs over eligible frames, bridging gaps ≤ 200 ms.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut cur: Option<(usize, usize)> = None;
    for (f, &e) in db.iter().enumerate() {
        if f < first_eligible || e <= thresh {
            continue;
        }
        match cur {
            Some((s, e0)) if f - e0 - 1 <= bridge => cur = Some((s, f)),
            _ => {
                if let Some(r) = cur {
                    runs.push(r);
                }
                cur = Some((f, f));
            }
        }
    }
    if let Some(r) = cur {
        runs.push(r);
    }
    if runs.is_empty() {
        return EndpointAnalysis {
            threshold_db: thresh,
            noise_db: noise,
            peak_frame_db: peak_frame,
            span: Err("rejected: too quiet (no frames above threshold)".to_string()),
            ..blank
        };
    }
    // Longest bridged run wins; ties → earliest (stable max).
    let mut best = runs[0];
    for &r in &runs[1..] {
        if r.1 - r.0 > best.1 - best.0 {
            best = r;
        }
    }
    let rate = audio::TARGET_SAMPLE_RATE as usize;
    let shift = rate * EP_SHIFT_MS as usize / 1000;
    let win = rate * EP_WIN_MS as usize / 1000;
    let hang = EP_HANGOVER_MS as usize / EP_SHIFT_MS as usize;
    let pre_start = best.0 * shift;
    let pre_end = (best.1 * shift + win).min(pcm16k.len());
    let pre_ms = (pre_end.saturating_sub(pre_start) * 1000 / rate) as u32;
    if pre_ms < EP_MIN_SPEECH_MS {
        return EndpointAnalysis {
            threshold_db: thresh,
            noise_db: noise,
            peak_frame_db: peak_frame,
            span: Err(format!(
                "rejected: too short (speech {pre_ms} ms < {})",
                EP_MIN_SPEECH_MS
            )),
            ..blank
        };
    }
    let f0 = best.0.saturating_sub(hang);
    let f1 = (best.1 + hang).min(db.len() - 1);
    let span = EndpointSpan {
        start_sample: f0 * shift,
        end_sample: (f1 * shift + win).min(pcm16k.len()),
    };
    EndpointAnalysis {
        threshold_db: thresh,
        noise_db: noise,
        peak_frame_db: peak_frame,
        span: Ok(span),
        ..blank
    }
}

/// RS-1 endpoint span, or a `too quiet` / `too short` rejection reason.
pub fn endpoint_span(pcm16k: &[i16]) -> Result<EndpointSpan, String> {
    analyze(pcm16k).span
}

// ---------------------------------------------------------------------------
// Re-pad to exactly 250 ms each side from the take's own audio.
// ---------------------------------------------------------------------------

/// Start offset of the take's lowest-energy `len`-sample window (first minimum
/// wins). Short takes (len < `len`) yield 0 (whole take is the filler pool).
pub fn lowest_energy_window(pcm: &[i16], len: usize) -> usize {
    if pcm.len() <= len {
        return 0;
    }
    // Exact integer prefix sums of squares (O(n); max window sum is
    // 4000 * 32768^2 << i64::MAX, so running differences are exact and the
    // first-minimum-wins order matches the naive scan).
    let mut prefix = vec![0i64; pcm.len() + 1];
    for (i, &s) in pcm.iter().enumerate() {
        prefix[i + 1] = prefix[i] + (s as i64) * (s as i64);
    }
    let mut best = 0;
    let mut best_e = prefix[len] - prefix[0];
    for s0 in 1..=pcm.len() - len {
        let e = prefix[s0 + len] - prefix[s0];
        if e < best_e {
            best_e = e;
            best = s0;
        }
    }
    best
}

/// Re-pad `span` to exactly 250 ms each side from the take's own audio.
/// When a clip edge is closer than 250 ms, the deficit is filled from the
/// start of the take's lowest-energy 250 ms window (tiled if the take itself
/// is shorter) — never zeros, which break CMN. Output length is always
/// `span + 8000` samples.
pub fn repad_clip(pcm: &[i16], span: &EndpointSpan) -> Vec<i16> {
    let pad = PAD_MS as usize * audio::TARGET_SAMPLE_RATE as usize / 1000;
    debug_assert_eq!(pad, 4000);
    let mut out = Vec::with_capacity(span.end_sample - span.start_sample + 2 * pad);
    // Filler pool: lowest-energy pad-length window (whole take if shorter).
    let pool_start = lowest_energy_window(pcm, pad);
    let pool: &[i16] = if pcm.len() <= pad {
        pcm
    } else {
        &pcm[pool_start..pool_start + pad]
    };
    // Filler block (tiled from the pool; empty only when the take is empty,
    // which endpoint rejects before this runs).
    let fill = |need: usize| -> Vec<i16> {
        if pool.is_empty() {
            return Vec::new();
        }
        (0..need).map(|i| pool[i % pool.len()]).collect()
    };
    // Pre-pad: take audio immediately before the span, else filler first.
    let pre_avail = span.start_sample.min(pcm.len());
    if pre_avail >= pad {
        out.extend_from_slice(&pcm[span.start_sample - pad..span.start_sample]);
    } else {
        out.extend_from_slice(&fill(pad - pre_avail));
        out.extend_from_slice(&pcm[0..pre_avail]);
    }
    // Span.
    out.extend_from_slice(&pcm[span.start_sample..span.end_sample.min(pcm.len())]);
    // Post-pad: take audio immediately after the span, then filler.
    let post_avail = pcm.len().saturating_sub(span.end_sample);
    if post_avail >= pad {
        out.extend_from_slice(&pcm[span.end_sample..span.end_sample + pad]);
    } else {
        out.extend_from_slice(&pcm[span.end_sample.min(pcm.len())..]);
        out.extend_from_slice(&fill(pad - post_avail));
    }
    out
}
// ---------------------------------------------------------------------------
// Raw session validation (RS-1 §4 step 1).
// ---------------------------------------------------------------------------

/// Accepted raw input flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFlavor {
    /// RS-1 §3 sealed layout with full integrity validation.
    Rs1,
    /// Grandfathered run1 shape (headerless manifest, enroll row, no sums).
    LegacyRun1,
}

/// One raw take: file name + prompt text.
#[derive(Debug, Clone)]
pub struct RawTake {
    pub file_name: String,
    pub prompt: String,
}

/// Validated raw session: ordered takes ready to canonicalize.
#[derive(Debug, Clone)]
pub struct RawSession {
    pub dir: PathBuf,
    pub flavor: RawFlavor,
    pub takes: Vec<RawTake>,
}

/// Canonicalization report.
#[derive(Debug, Clone)]
pub struct CanonReport {
    pub flavor: RawFlavor,
    pub takes_total: usize,
    pub takes_accepted: usize,
    pub rejected: Vec<(String, String)>,
    pub out_dir: PathBuf,
}

/// Frozen order-commitment scheme (RS-1 §7): the recorder prints
/// `PRECOMMIT order_sha256=<hex>` before the first take and stores the same
/// value in `session.meta`; the canonicalizer recomputes it from the seed +
/// `prompts.txt` bytes. RS1-a recorders implement this exact scheme:
/// `sha256("rs1-order-v1\n{seed}\n" + prompts_bytes)`.
pub fn order_sha256(seed: u64, prompts_bytes: &[u8]) -> String {
    let mut v = format!("rs1-order-v1\n{seed}\n").into_bytes();
    v.extend_from_slice(prompts_bytes);
    sha256_hex(&v)
}

fn read_text(dir: &Path, name: &str) -> Result<String, String> {
    std::fs::read_to_string(dir.join(name))
        .map_err(|e| format!("{}: cannot read {name}: {e}", dir.display()))
}

fn parse_meta(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

fn meta_get(meta: &[(String, String)], key: &str) -> Result<String, String> {
    meta.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| format!("session.meta: missing key {key}"))
}

/// Parse one `SHA256SUMS` line (`<hex64>  <name>` or `<hex64> *<name>`).
fn parse_sums_line(line: &str) -> Option<(String, String)> {
    let line = line.trim_end();
    if line.len() < 66 {
        return None;
    }
    let (hex, rest) = line.split_at(64);
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let name = rest
        .strip_prefix(" *")
        .or_else(|| rest.strip_prefix("  "))?;
    // Subpaths (e.g. canonical `qc/take000.png`) are allowed; traversal is not.
    if name.is_empty()
        || name.contains('\\')
        || name.starts_with('/')
        || name
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return None;
    }
    Some((name.to_string(), hex.to_lowercase()))
}

fn wav_spec(dir: &Path, name: &str) -> Result<(hound::WavSpec, u32), String> {
    let path = dir.join(name);
    let r = hound::WavReader::open(&path)
        .map_err(|e| format!("{}: cannot open wav: {e}", path.display()))?;
    let spec = r.spec();
    let len = r.len();
    if spec.sample_format != hound::SampleFormat::Int {
        return Err(format!(
            "{name}: not integer PCM ({:?})",
            spec.sample_format
        ));
    }
    Ok((spec, len))
}

fn validate_rs1(dir: &Path) -> Result<RawSession, String> {
    let meta = parse_meta(&read_text(dir, "session.meta")?);
    let prompt_bytes = std::fs::read(dir.join("prompts.txt"))
        .map_err(|e| format!("{}: cannot read prompts.txt: {e}", dir.display()))?;
    // Required keys.
    for key in [
        "host",
        "rate_hz",
        "prompt_file_sha256",
        "order_seed",
        "order_sha256",
        "partition",
        "list",
        "date_utc",
    ] {
        meta_get(&meta, key)?;
    }
    // Prompt file hash + order commitment.
    let want_prompts = meta_get(&meta, "prompt_file_sha256")?;
    let got_prompts = sha256_hex(&prompt_bytes);
    if got_prompts != want_prompts.to_lowercase() {
        return Err(format!(
            "{}: prompt_file_sha256 mismatch (meta {want_prompts}, actual {got_prompts})",
            dir.display()
        ));
    }
    let seed: u64 = meta_get(&meta, "order_seed")?
        .parse()
        .map_err(|_| "session.meta: order_seed is not a u64".to_string())?;
    let want_order = meta_get(&meta, "order_sha256")?;
    let got_order = order_sha256(seed, &prompt_bytes);
    if got_order != want_order.to_lowercase() {
        return Err(format!(
            "{}: order_sha256 mismatch (meta {want_order}, actual {got_order})",
            dir.display()
        ));
    }
    // Prompt count = take count, exact take-name set.
    let prompt_text =
        String::from_utf8(prompt_bytes).map_err(|_| "prompts.txt: not valid UTF-8".to_string())?;
    let prompts: Vec<&str> = prompt_text
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !l.is_empty())
        .collect();
    if prompts.is_empty() {
        return Err(format!("{}: prompts.txt is empty", dir.display()));
    }
    let mut take_files: Vec<String> = Vec::new();
    let mut entries: Vec<String> = Vec::new();
    let rd =
        std::fs::read_dir(dir).map_err(|e| format!("{}: cannot list dir: {e}", dir.display()))?;
    for e in rd {
        let e = e.map_err(|e| format!("{}: dir entry: {e}", dir.display()))?;
        if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        entries.push(name.clone());
        if name.starts_with("take") && name.ends_with(".wav") {
            take_files.push(name);
        }
    }
    take_files.sort();
    let want: Vec<String> = (0..prompts.len())
        .map(|i| format!("take{i:03}.wav"))
        .collect();
    if take_files != want {
        return Err(format!(
            "{}: take count mismatch (prompts {}, takes {:?})",
            dir.display(),
            prompts.len(),
            take_files
        ));
    }
    // SHA256SUMS verifies + covers every file in the dir.
    let mut listed = verify_sha256sums(dir)?;
    listed.sort();
    let mut covered: Vec<String> = entries
        .iter()
        .filter(|n| n.as_str() != "SHA256SUMS")
        .cloned()
        .collect();
    covered.sort();
    if listed != covered {
        return Err(format!(
            "SHA256SUMS coverage mismatch (listed {listed:?}, files {covered:?})"
        ));
    }
    // Format: integer PCM, rate matches session.meta.
    let meta_rate: u32 = meta_get(&meta, "rate_hz")?
        .parse()
        .map_err(|_| "session.meta: rate_hz is not a u32".to_string())?;
    let mut takes = Vec::with_capacity(prompts.len());
    for (i, prompt) in prompts.iter().enumerate() {
        let name = format!("take{i:03}.wav");
        let (spec, _) = wav_spec(dir, &name)?;
        if spec.sample_rate != meta_rate {
            return Err(format!(
                "{name}: rate mismatch (wav {}, meta {meta_rate})",
                spec.sample_rate
            ));
        }
        takes.push(RawTake {
            file_name: name,
            prompt: prompt.to_string(),
        });
    }
    Ok(RawSession {
        dir: dir.to_path_buf(),
        flavor: RawFlavor::Rs1,
        takes,
    })
}

fn validate_legacy(dir: &Path) -> Result<RawSession, String> {
    let manifest = read_text(dir, "manifest.tsv")?;
    let mut takes = Vec::new();
    for (i, line) in manifest.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 4 {
            return Err(format!("manifest.tsv line {}: want 4 columns", i + 1));
        }
        let (file, prompt, rate_s, frames_s) = (cols[0], cols[1], cols[2], cols[3]);
        if file == "enroll.wav" {
            continue;
        }
        if i == 0 && line == "file\tword\trate_hz\tframes" {
            continue; // header row (new-recorder sealed manifests, e.g. 100240)
        }
        if prompt.is_empty() {
            return Err(format!("manifest.tsv line {}: empty prompt", i + 1));
        }
        let want_rate: u32 = rate_s
            .parse()
            .map_err(|_| format!("manifest.tsv line {}: bad rate", i + 1))?;
        let want_frames: u32 = frames_s
            .parse()
            .map_err(|_| format!("manifest.tsv line {}: bad frames", i + 1))?;
        let path = dir.join(file);
        if !path.is_file() {
            return Err(format!("manifest.tsv line {}: missing {file}", i + 1));
        }
        let (spec, len) = wav_spec(dir, file)?;
        if spec.sample_rate != want_rate {
            return Err(format!(
                "{file}: rate mismatch (wav {}, manifest {want_rate})",
                spec.sample_rate
            ));
        }
        if len != want_frames {
            return Err(format!(
                "{file}: frame count mismatch (wav {len}, manifest {want_frames})"
            ));
        }
        takes.push(RawTake {
            file_name: file.to_string(),
            prompt: prompt.to_string(),
        });
    }
    if takes.is_empty() {
        return Err(format!("{}: no takes (only enroll?)", dir.display()));
    }
    Ok(RawSession {
        dir: dir.to_path_buf(),
        flavor: RawFlavor::LegacyRun1,
        takes,
    })
}

/// Validate a raw session dir (RS-1 flavor when `prompts.txt` + `session.meta`
/// exist, else grandfathered run1 flavor when `manifest.tsv` exists).
pub fn validate_raw(dir: &Path) -> Result<RawSession, String> {
    if dir.join("prompts.txt").is_file() && dir.join("session.meta").is_file() {
        validate_rs1(dir)
    } else if dir.join("manifest.tsv").is_file() {
        validate_legacy(dir)
    } else {
        Err(format!(
            "{}: no RS-1 (prompts.txt + session.meta) or legacy (manifest.tsv) session files",
            dir.display()
        ))
    }
}

// ---------------------------------------------------------------------------
// Minimal std-only PNG writer (8-bit grayscale, stored deflate; deterministic).
// ---------------------------------------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &x in bytes {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn png_chunk(kind: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut c = Vec::with_capacity(4 + data.len());
    c.extend_from_slice(kind);
    c.extend_from_slice(data);
    out.extend_from_slice(&crc32(&c).to_be_bytes());
}

/// Write an 8-bit grayscale PNG. `pixels` is row-major, `w * h` bytes.
pub fn write_png_grayscale(path: &Path, w: u32, h: u32, pixels: &[u8]) -> Result<(), String> {
    if pixels.len() != (w * h) as usize {
        return Err("png: pixel buffer size mismatch".to_string());
    }
    let mut raw = Vec::with_capacity((w as usize + 1) * h as usize);
    for row in pixels.chunks_exact(w as usize) {
        raw.push(0); // filter type 0 (None)
        raw.extend_from_slice(row);
    }
    // zlib wrapper (0x78 0x01) + stored (uncompressed) deflate blocks.
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = i == raw.len().div_ceil(65535) - 1;
        z.push(if last { 0x01 } else { 0x00 });
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8-bit gray, no compression/filter/interlace
    png_chunk(b"IHDR", &ihdr, &mut png);
    png_chunk(b"IDAT", &z, &mut png);
    png_chunk(b"IEND", &[], &mut png);
    std::fs::write(path, &png).map_err(|e| format!("{}: {e}", path.display()))
}

/// QC energy plot: frame dB curve (black), threshold (mid gray), 300 ms guard
/// line (dark gray), endpoint span band (light gray). No text (no font in
/// std); the file name carries the take identity.
pub fn write_qc_plot(
    path: &Path,
    db: &[f64],
    threshold_db: f64,
    span_frames: Option<(usize, usize)>,
) -> Result<(), String> {
    let (w, h) = (QC_PLOT_W as usize, QC_PLOT_H as usize);
    let mut px = vec![255u8; w * h];
    let set = |px: &mut [u8], x: usize, y: usize, v: u8| {
        if x < w && y < h {
            px[y * w + x] = v;
        }
    };
    // Border.
    for x in 0..w {
        set(&mut px, x, 0, 0);
        set(&mut px, x, h - 1, 0);
    }
    for y in 0..h {
        set(&mut px, 0, y, 0);
        set(&mut px, w - 1, y, 0);
    }
    if db.is_empty() {
        return write_png_grayscale(path, w as u32, h as u32, &px);
    }
    let n = db.len();
    let x_of = |i: usize| -> usize { 8 + i * (w - 16) / n.max(1) };
    let mut lo = db.iter().cloned().fold(f64::INFINITY, f64::min) - 3.0;
    let mut hi = db.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 3.0;
    if threshold_db.is_finite() {
        lo = lo.min(threshold_db - 3.0);
        hi = hi.max(threshold_db + 3.0);
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    let y_of = |v: f64| -> usize {
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        (h - 8) - (t * (h - 16) as f64) as usize
    };
    // Span band.
    if let Some((f0, f1)) = span_frames {
        let (xa, xb) = (x_of(f0.min(n)), x_of((f1 + 1).min(n)));
        for x in xa..xb.max(xa + 1).min(w - 1) {
            for y in 8..h - 8 {
                set(&mut px, x, y, 225);
            }
        }
    }
    // Threshold line.
    if threshold_db.is_finite() {
        let y = y_of(threshold_db).clamp(8, h - 9);
        for x in 8..w - 8 {
            set(&mut px, x, y, 140);
        }
    }
    // Guard line at 300 ms.
    let gf = EP_GUARD_MS as usize / EP_SHIFT_MS as usize;
    if gf < n {
        let x = x_of(gf);
        for y in 8..h - 8 {
            set(&mut px, x, y, 100);
        }
    }
    // Energy polyline.
    let mut prev: Option<(usize, usize)> = None;
    for (i, &v) in db.iter().enumerate() {
        let (x, y) = (x_of(i), y_of(v).clamp(8, h - 9));
        if let Some((px0, py0)) = prev {
            let steps = (x.abs_diff(px0) + y.abs_diff(py0)).max(1);
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let xi = (px0 as f64 + (x as f64 - px0 as f64) * t) as usize;
                let yi = (py0 as f64 + (y as f64 - py0 as f64) * t) as usize;
                set(&mut px, xi, yi, 0);
            }
        }
        prev = Some((x, y));
    }
    write_png_grayscale(path, w as u32, h as u32, &px)
}

// ---------------------------------------------------------------------------
// Canonicalizer driver (RS-1 §4 steps 1-6).
// ---------------------------------------------------------------------------

fn write_wav_16k_mono(path: &Path, samples: &[i16]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio::TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w =
        hound::WavWriter::create(path, spec).map_err(|e| format!("{}: {e}", path.display()))?;
    for &s in samples {
        w.write_sample(s)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    w.finalize().map_err(|e| format!("{}: {e}", path.display()))
}

fn fmt_db(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.2}")
    } else {
        "NA".to_string()
    }
}

/// Canonicalize `raw_dir` into `out_dir` (must not exist).
///
/// Writes per-take `takeNNN.wav` (16 kHz mono i16, accepted takes only),
/// `manifest.tsv` (take, prompt, frames, clip-coordinate span ms, flags),
/// `provenance.tsv` (raw sha → canon sha chain), `qc.tsv` (per-take QC with
/// take-coordinate spans), `qc/takeNNN.png` (every 10th take), `SHA256SUMS`.
pub fn canon_session(raw_dir: &Path, out_dir: &Path) -> Result<CanonReport, String> {
    let session = validate_raw(raw_dir)?;
    if out_dir.exists() {
        return Err(format!(
            "{}: output dir exists (refusing to overwrite)",
            out_dir.display()
        ));
    }
    std::fs::create_dir_all(out_dir.join("qc"))
        .map_err(|e| format!("{}: cannot create out dir: {e}", out_dir.display()))?;
    let rate = audio::TARGET_SAMPLE_RATE as usize;
    let mut manifest = String::from("take\tprompt\tframes\tspan_ms\tqc_flags\n");
    let mut provenance =
        String::from("raw_file\traw_sha256\tcanon_file\tcanon_sha256\tcanon_version\tresampler\n");
    let mut qc = String::from(
        "take\tpeak_dbfs\tclip_count\tsnr_db\tspan_start_ms\tspan_end_ms\tspeech_ms\tmuted\tguard_db\tstatus\n",
    );
    let mut accepted = 0usize;
    let mut rejected: Vec<(String, String)> = Vec::new();
    for (idx, take) in session.takes.iter().enumerate() {
        let raw_path = session.dir.join(&take.file_name);
        let raw_sha = sha256_hex(
            &std::fs::read(&raw_path).map_err(|e| format!("{}: {e}", raw_path.display()))?,
        );
        let audio = audio::decode_wav(&raw_path)?;
        let pcm16k = resample_to_16k_mono(&audio);
        let analysis = analyze(&pcm16k);
        let peak = peak_dbfs(&pcm16k);
        let clips = pcm16k
            .iter()
            .filter(|&&s| s == i16::MAX || s == i16::MIN)
            .count();
        let snr = analysis.peak_frame_db - analysis.noise_db;
        let gf = EP_GUARD_MS as usize / EP_SHIFT_MS as usize;
        let guard_db = if analysis.db.is_empty() {
            f64::NEG_INFINITY
        } else {
            analysis.db[..analysis.db.len().min(gf)]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let out_name = format!("take{idx:03}.wav");
        let muted = peak < EP_MIN_PEAK_DBFS;
        let span_frames = analysis.span.as_ref().ok().map(|s| {
            let shift = rate * EP_SHIFT_MS as usize / 1000;
            let win = rate * EP_WIN_MS as usize / 1000;
            (
                s.start_sample / shift,
                s.end_sample.saturating_sub(win) / shift,
            )
        });
        match &analysis.span {
            Ok(span) => {
                let clip = repad_clip(&pcm16k, span);
                write_wav_16k_mono(&out_dir.join(&out_name), &clip)?;
                let canon_sha = sha256_hex(
                    &std::fs::read(out_dir.join(&out_name))
                        .map_err(|e| format!("{out_name}: reread: {e}"))?,
                );
                // Manifest span: canonical-clip ms (pre-pad is exactly 250).
                let end_ms = PAD_MS + (span.end_sample - span.start_sample) as u32 / 16;
                manifest.push_str(&format!(
                    "{out_name}\t{}\t{}\t{}-{end_ms}\tOK\n",
                    take.prompt,
                    clip.len(),
                    PAD_MS
                ));
                provenance.push_str(&format!(
                    "{}\t{raw_sha}\t{out_name}\t{canon_sha}\t{CANON_VERSION}\t{RESAMPLER_ID}\n",
                    take.file_name
                ));
                let (ss, se) = (
                    span.start_sample * 1000 / rate,
                    span.end_sample * 1000 / rate,
                );
                qc.push_str(&format!(
                    "{out_name}\t{}\t{clips}\t{}\t{ss}\t{se}\t{}\t{muted}\t{}\tOK\n",
                    fmt_db(peak),
                    fmt_db(snr),
                    span.speech_ms(),
                    fmt_db(guard_db)
                ));
                accepted += 1;
            }
            Err(reason) => {
                manifest.push_str(&format!(
                    "{out_name}\t{}\t0\tNA\tREJECTED: {reason}\n",
                    take.prompt
                ));
                provenance.push_str(&format!(
                    "{}\t{raw_sha}\tNA\tNA\t{CANON_VERSION}\t{RESAMPLER_ID}\n",
                    take.file_name
                ));
                qc.push_str(&format!(
                    "{out_name}\t{}\t{clips}\t{}\t-1\t-1\t0\t{muted}\t{}\tREJECTED: {reason}\n",
                    fmt_db(peak),
                    fmt_db(snr),
                    fmt_db(guard_db)
                ));
                rejected.push((out_name.clone(), reason.clone()));
            }
        }
        if idx % QC_PLOT_EVERY == 0 {
            let stem = out_name.strip_suffix(".wav").unwrap_or(&out_name);
            write_qc_plot(
                &out_dir.join("qc").join(format!("{stem}.png")),
                &analysis.db,
                analysis.threshold_db,
                span_frames,
            )?;
        }
    }
    std::fs::write(out_dir.join("manifest.tsv"), &manifest)
        .map_err(|e| format!("manifest.tsv: {e}"))?;
    std::fs::write(out_dir.join("provenance.tsv"), &provenance)
        .map_err(|e| format!("provenance.tsv: {e}"))?;
    std::fs::write(out_dir.join("qc.tsv"), &qc).map_err(|e| format!("qc.tsv: {e}"))?;
    // SHA256SUMS over every output file except itself, sorted rel paths.
    let mut files: Vec<String> = Vec::new();
    let mut stack = vec![out_dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut ents: Vec<PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| format!("{}: {e}", d.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        ents.sort();
        for p in ents {
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().map(|n| n != "SHA256SUMS").unwrap_or(true) {
                files.push(
                    p.strip_prefix(out_dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    files.sort();
    let mut sums = String::new();
    for rel in &files {
        let sha = sha256_hex(&std::fs::read(out_dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?);
        sums.push_str(&format!("{sha}  {rel}\n"));
    }
    std::fs::write(out_dir.join("SHA256SUMS"), &sums).map_err(|e| format!("SHA256SUMS: {e}"))?;
    Ok(CanonReport {
        flavor: session.flavor,
        takes_total: session.takes.len(),
        takes_accepted: accepted,
        rejected,
        out_dir: out_dir.to_path_buf(),
    })
}

// ---------------------------------------------------------------------------
// One loader (RS-1 §5, interfaces-v1 I1): the ONLY function experiments may
// use to read owner audio. Canonical sessions live at
// `sessions/canon/<session-id>` (populated by RS1-d runs of canon-session).
// ---------------------------------------------------------------------------

/// Canonical loader error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonError {
    pub message: String,
}

impl std::fmt::Display for CanonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CanonError {}

fn canon_err(message: String) -> CanonError {
    CanonError { message }
}

/// One canonical take: 16 kHz mono i16 with its endpoint span in clip ms.
#[derive(Debug, Clone)]
pub struct CanonTake {
    pub id: String,
    pub prompt: String,
    pub pcm: Vec<i16>,
    pub span_ms: (u32, u32),
}

/// One canonical session: ordered accepted takes (`REJECTED` manifest rows
/// carry no audio and are skipped).
#[derive(Debug, Clone)]
pub struct CanonSession {
    pub dir: PathBuf,
    pub takes: Vec<CanonTake>,
}

/// Verify `dir/SHA256SUMS` entry-by-entry; returns the listed names.
fn verify_sha256sums(dir: &Path) -> Result<Vec<String>, String> {
    let sums_text = read_text(dir, "SHA256SUMS")?;
    let mut listed = Vec::new();
    for (i, line) in sums_text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (name, want_sha) =
            parse_sums_line(line).ok_or_else(|| format!("SHA256SUMS line {}: malformed", i + 1))?;
        let bytes = std::fs::read(dir.join(&name))
            .map_err(|e| format!("SHA256SUMS: cannot read {name}: {e}"))?;
        let got = sha256_hex(&bytes);
        if got != want_sha {
            return Err(format!(
                "SHA256SUMS mismatch for {name} (want {want_sha}, got {got})"
            ));
        }
        listed.push(name);
    }
    Ok(listed)
}

/// Load a canonical session dir (RS-1 §5): strict header, SHA256SUMS
/// integrity, 16 kHz mono i16 takes, frame/span cross-checks.
pub fn load_session(dir: &Path) -> Result<CanonSession, CanonError> {
    let fail = |m: String| canon_err(format!("{}: {m}", dir.display()));
    let manifest = std::fs::read_to_string(dir.join("manifest.tsv"))
        .map_err(|e| fail(format!("cannot read manifest.tsv: {e}")))?;
    let mut lines = manifest.lines();
    let header = lines.next().unwrap_or("");
    if header != "take\tprompt\tframes\tspan_ms\tqc_flags" {
        return Err(fail(format!(
            "not a canonical manifest (bad header: {header:?}); raw sessions must go through canon-session first"
        )));
    }
    let listed = verify_sha256sums(dir).map_err(fail)?;
    let mut takes = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 {
            return Err(fail(format!("manifest.tsv row {}: want 5 columns", i + 1)));
        }
        let (file, prompt, frames_s, span_s, flags) = (cols[0], cols[1], cols[2], cols[3], cols[4]);
        if flags.starts_with("REJECTED") {
            continue;
        }
        if !listed.iter().any(|n| n == file) {
            return Err(fail(format!("{file}: not covered by SHA256SUMS")));
        }
        if !listed.iter().any(|n| n == "manifest.tsv") {
            return Err(fail("manifest.tsv: not covered by SHA256SUMS".to_string()));
        }
        let want_frames: usize = frames_s
            .parse()
            .map_err(|_| fail(format!("{file}: bad frames {frames_s:?}")))?;
        let (s0, s1) = span_s
            .split_once('-')
            .and_then(|(a, b)| a.parse::<u32>().ok().zip(b.parse::<u32>().ok()))
            .ok_or_else(|| fail(format!("{file}: bad span_ms {span_s:?}")))?;
        let audio = audio::decode_wav(&dir.join(file)).map_err(fail)?;
        if audio.sample_rate != audio::TARGET_SAMPLE_RATE {
            return Err(fail(format!(
                "{file}: rate {} != 16000 (not canonical audio)",
                audio.sample_rate
            )));
        }
        if audio.channels != 1 {
            return Err(fail(format!("{file}: channels {} != 1", audio.channels)));
        }
        if audio.samples.len() != want_frames {
            return Err(fail(format!(
                "{file}: frames {} != manifest {want_frames}",
                audio.samples.len()
            )));
        }
        if s1 as usize * 16 > audio.samples.len() || s0 >= s1 {
            return Err(fail(format!("{file}: span_ms {span_s:?} outside frames")));
        }
        takes.push(CanonTake {
            id: file.strip_suffix(".wav").unwrap_or(file).to_string(),
            prompt: prompt.to_string(),
            pcm: audio.samples,
            span_ms: (s0, s1),
        });
    }
    Ok(CanonSession {
        dir: dir.to_path_buf(),
        takes,
    })
}
