//! Padding-fingerprint proof protocol (bead zco).
//!
//! Committed fast tests: re-padding-condition LOTO protocol on synthetic
//! tones (all four conditions scored + deterministic) and the timing-only
//! classifier protocol over real run1-16k audio (all 120 clips evaluated,
//! deterministic). The expensive release-only LOTO measurement over the
//! re-padded real clips lives in the scratch probe
//! `examples/fingerprint_probe.rs`, run once per the bead text and deleted
//! after its numbers are recorded.

use std::collections::BTreeMap;
use std::path::Path;

use fse::frontend::{
    FrontendConfig, extract_untrimmed_with_config, extract_with_config, speech_span,
};
use fse::gate::{Clip, leave_one_take_out};
use fse::{shift_len, window_len};

const SR: u32 = 16_000;
const BAND: f64 = 0.2;

/// Deterministic PRNG (xorshift64*): std-only, seed-pinned, no RNG crate.
struct Rng(u64);
impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x % bound as u64) as usize
    }
}

/// Frame-RMS energy series on raw samples, mirroring the extractor.
fn energies_of(samples: &[i16], sr: u32) -> (Vec<f64>, usize, usize) {
    let wl = window_len(sr);
    let sh = shift_len(sr);
    let n = samples.len().saturating_sub(wl) / sh + 1;
    let raw: Vec<f64> = samples
        .iter()
        .map(|&s| s as f64 / i16::MAX as f64)
        .collect();
    let e: Vec<f64> = (0..n)
        .map(|i| {
            let st = i * sh;
            let sum: f64 = raw[st..st + wl].iter().map(|x| x * x).sum::<f64>();
            (sum / wl as f64).sqrt()
        })
        .collect();
    (e, wl, sh)
}

/// Speech-sample range [start, end) for a trimmed span, from frame indices.
fn speech_range(span: (usize, usize), wl: usize, sh: usize) -> (usize, usize) {
    let (ts, te) = span;
    (ts * sh, (te - 1) * sh + wl)
}

fn repad(speech: &[i16], total: usize, lead: usize) -> Vec<i16> {
    assert!(lead + speech.len() <= total, "repad overflow");
    let mut v = vec![0i16; total];
    v[lead..lead + speech.len()].copy_from_slice(speech);
    v
}

/// The four pinned re-padding conditions. Returns (name, total_len, lead).
/// `speech_len` = trimmed speech samples, `orig` = original window length.
fn conditions(speech_len: usize, orig: usize, rng: &mut Rng) -> Vec<(String, usize, usize)> {
    let ms = |m: usize| m * SR as usize / 1000;
    vec![
        ("centered".to_string(), orig, (orig - speech_len) / 2),
        ("random".to_string(), orig, rng.below(orig - speech_len + 1)),
        ("short".to_string(), speech_len + 2 * ms(50), ms(50)),
        (
            "long".to_string(),
            speech_len + 2 * ms(1000).min((orig - speech_len) / 2),
            ms(1000).min((orig - speech_len) / 2),
        ),
    ]
}

fn synth_tone(freq: f64, secs: f64) -> Vec<i16> {
    let n = (secs * SR as f64) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / SR as f64;
            (8000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16
        })
        .collect()
}

/// Minimal PCM16-mono WAV reader (integration tests cannot use the fse
/// binary's hound dependency; this keeps the test std-only).
fn read_wav_mono16(path: &Path) -> (u32, Vec<i16>) {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[0..4], b"RIFF", "not a WAV: {:?}", path);
    assert_eq!(&b[8..12], b"WAVE", "not a WAV: {:?}", path);
    let mut pos = 12;
    let mut sr = 0u32;
    let mut samples = Vec::new();
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let sz = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let data = pos + 8;
        if id == b"fmt " {
            let fmt = u16::from_le_bytes([b[data], b[data + 1]]);
            let ch = u16::from_le_bytes([b[data + 2], b[data + 3]]);
            assert_eq!(fmt, 1, "WAV must be PCM: {:?}", path);
            assert_eq!(ch, 1, "WAV must be mono: {:?}", path);
            sr = u32::from_le_bytes(b[data + 4..data + 8].try_into().unwrap());
            let bits = u16::from_le_bytes([b[data + 14], b[data + 15]]);
            assert_eq!(bits, 16, "WAV must be 16-bit: {:?}", path);
        } else if id == b"data" {
            samples = b[data..data + sz]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        pos = data + sz + (sz % 2);
    }
    assert!(sr > 0 && !samples.is_empty(), "no audio: {:?}", path);
    (sr, samples)
}

#[test]
fn repadding_conditions_all_scored_deterministically() {
    let cfg = FrontendConfig::default();
    // Two grossly distinct synthetic words, two takes each.
    let words = [("tone300", 300.0), ("tone900", 900.0)];
    let pad = vec![0i16; (0.2 * SR as f64) as usize];
    let mut takes: Vec<(String, usize, Vec<i16>)> = Vec::new();
    for (w, f) in words {
        for take in 0..2 {
            let mut clip = pad.clone();
            clip.extend_from_slice(&synth_tone(f, 0.4));
            clip.extend_from_slice(&pad);
            takes.push((w.to_string(), take, clip));
        }
    }
    // Trimmed spans -> speech samples per take.
    let mut speeches: Vec<(String, usize, Vec<i16>, usize)> = Vec::new();
    for (w, take, clip) in &takes {
        let (e, wl, sh) = energies_of(clip, SR);
        let span = speech_span(&e).expect("synth speech detected");
        let (s0, s1) = speech_range(span, wl, sh);
        speeches.push((w.clone(), *take, clip[s0..s1].to_vec(), clip.len()));
    }
    // One LOTO run per condition through the UNTRIMMED pipeline.
    for ci in 0..4 {
        let mut rng = Rng(7);
        // Advance the RNG identically regardless of condition order so the
        // random offsets are a pure function of (seed, clip order).
        let mut cond_of =
            |speech_len: usize, orig: usize| conditions(speech_len, orig, &mut rng)[ci].clone();
        let mut clips: Vec<Clip> = Vec::new();
        for (w, take, speech, orig) in &speeches {
            let (_, total, lead) = cond_of(speech.len(), *orig);
            let padded = repad(speech, total, lead);
            let frames = extract_untrimmed_with_config(&padded, SR, &cfg).unwrap();
            clips.push(Clip {
                label: w.clone(),
                take: *take,
                frames,
            });
        }
        let res = leave_one_take_out(&clips, BAND);
        assert_eq!(res.predictions.len(), 4, "condition {ci}: all takes scored");
        assert_eq!(res.degenerate, 0, "condition {ci}: no degeneracy");
        assert!(
            (res.accuracy() - 1.0).abs() < 1e-9,
            "condition {ci}: easy synths classify perfectly, got {:.4}",
            res.accuracy()
        );
    }
    // Determinism: same re-padded clip extracted twice is bit-identical.
    let (w0, t0, speech0, orig0) = &speeches[0];
    let _ = (w0, t0);
    let mut rng = Rng(7);
    let (_, total, lead) = conditions(speech0.len(), *orig0, &mut rng)[1].clone();
    let padded = repad(speech0, total, lead);
    let f1 = extract_untrimmed_with_config(&padded, SR, &cfg).unwrap();
    let f2 = extract_untrimmed_with_config(&padded, SR, &cfg).unwrap();
    assert_eq!(f1, f2, "extraction is deterministic");
    // And the RNG stream itself is seed-pinned.
    let mut r1 = Rng(7);
    let mut r2 = Rng(7);
    for _ in 0..16 {
        assert_eq!(r1.below(1000), r2.below(1000));
    }
}

#[test]
fn timing_classifier_protocol_complete() {
    // Real run1-16k audio through the timing-only pipeline: 120 clips
    // evaluated, deterministic. Manifest columns: file, word, rate, samples
    // (no take column; take = per-word occurrence order, as in a1_gate).
    let root = Path::new("../../sessions/run1-16k");
    let manifest = std::fs::read_to_string(root.join("manifest.tsv")).unwrap();
    let mut rows: Vec<(String, String)> = Vec::new();
    for (i, line) in manifest.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let mut f = line.split('\t');
        let file = f.next().unwrap().to_string();
        let word = f.next().unwrap().to_string();
        rows.push((file, word));
    }
    assert_eq!(rows.len(), 120, "DEV session has 120 clips");
    let mut take_of: BTreeMap<String, usize> = BTreeMap::new();
    // Per clip: (word, take, [lead_ms, trail_ms, dur_ms, centroid_ms]).
    let mut feats: Vec<(String, usize, [f64; 4])> = Vec::new();
    for (file, word) in &rows {
        let take = *take_of.entry(word.clone()).or_insert(0);
        take_of.insert(word.clone(), take + 1);
        let (sr, samples) = read_wav_mono16(&root.join(file));
        assert_eq!(sr, SR, "canonical 16k: {}", file);
        let (e, wl, sh) = energies_of(&samples, sr);
        let (ts, te) = speech_span(&e).expect("speech span");
        let (s0, s1) = speech_range((ts, te), wl, sh);
        let n = samples.len() as f64;
        let lead = 1000.0 * s0 as f64 / sr as f64;
        let trail = 1000.0 * (samples.len() - s1) as f64 / sr as f64;
        let dur = 1000.0 * (s1 - s0) as f64 / sr as f64;
        // Energy centroid over the speech span, in ms from clip start.
        let mut num = 0.0;
        let mut den = 0.0;
        for (k, fi) in (ts..te).enumerate() {
            let t_ms = 1000.0 * (fi * sh + wl / 2) as f64 / sr as f64;
            num += t_ms * e[fi];
            den += e[fi];
            let _ = k;
        }
        let centroid = if den > 0.0 { num / den } else { 0.0 };
        let _ = n;
        feats.push((word.clone(), take, [lead, trail, dur, centroid]));
    }
    // Leave-one-take-out nearest-mean, run twice for determinism.
    let run = |feats: &[(String, usize, [f64; 4])]| -> (usize, usize) {
        let mut correct = 0usize;
        let mut total = 0usize;
        for held in 0..4 {
            let mut sums: BTreeMap<&str, ([f64; 4], usize)> = BTreeMap::new();
            for (w, t, f) in feats {
                if *t == held {
                    continue;
                }
                let e = sums.entry(w.as_str()).or_insert(([0.0; 4], 0));
                for (acc, val) in e.0.iter_mut().zip(f.iter()) {
                    *acc += *val;
                }
                e.1 += 1;
            }
            let means: BTreeMap<&str, [f64; 4]> = sums
                .iter()
                .map(|(w, (s, c))| {
                    (
                        *w,
                        [
                            s[0] / *c as f64,
                            s[1] / *c as f64,
                            s[2] / *c as f64,
                            s[3] / *c as f64,
                        ],
                    )
                })
                .collect();
            for (w, t, f) in feats {
                if *t != held {
                    continue;
                }
                total += 1;
                let mut best = ("", f64::INFINITY);
                for (mw, m) in &means {
                    let d: f64 = (0..4).map(|d| (f[d] - m[d]).powi(2)).sum::<f64>().sqrt();
                    if d < best.1 {
                        best = (mw, d);
                    }
                }
                if best.0 == w.as_str() {
                    correct += 1;
                }
            }
        }
        (correct, total)
    };
    let (c1, t1) = run(&feats);
    let (c2, t2) = run(&feats);
    assert_eq!(t1, 120, "all 120 clips evaluated");
    assert_eq!((c1, t1), (c2, t2), "timing classifier is deterministic");
    println!("timing-only LOTO: {c1}/{t1} = {:.4}", c1 as f64 / t1 as f64);
    // Trimmed-extractor sanity on one real clip: production path unaffected.
    let (sr, samples) = read_wav_mono16(&root.join(&rows[0].0));
    let cfg = FrontendConfig::default();
    let a = extract_with_config(&samples, sr, &cfg).unwrap();
    assert!(!a.is_empty() && !a[0].is_empty());
}
