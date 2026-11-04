//! RS1-b canonicalizer tests (failing-first for `corpus::canonical`).
//!
//! Every test builds synthetic sessions under a unique temp dir (no `tempfile`
//! crate in this workspace) and drives the public `canonical` API only.

use crate::canonical;
use std::path::{Path, PathBuf};

fn test_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("canon-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");
    root
}

fn write_wav_i16(path: &Path, samples: &[i16], rate: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("wav create");
    for &s in samples {
        w.write_sample(s).expect("wav sample");
    }
    w.finalize().expect("wav finalize");
}

/// 3 s take at `rate`: silence, then a `tone_ms` tone burst starting at
/// `tone_start_ms` (peak `peak`), then silence. Deterministic.
fn tone_take(rate: u32, tone_start_ms: u32, tone_ms: u32, peak: i16) -> Vec<i16> {
    let n = (rate * 3) as usize;
    let mut x = vec![0i16; n];
    let s0 = (tone_start_ms * rate / 1000) as usize;
    let s1 = ((tone_start_ms + tone_ms) * rate / 1000) as usize;
    for (i, s) in x[s0..s1.min(n)].iter_mut().enumerate() {
        let t = i as f64 / rate as f64;
        *s = (peak as f64 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as i16;
    }
    x
}

fn write_legacy_session(dir: &Path, takes: &[(&str, Vec<i16>)], rate: u32) {
    std::fs::create_dir_all(dir).expect("raw dir");
    // enroll.wav row first (run1 legacy shape), then one row per take.
    let enroll = vec![0i16; (rate * 3) as usize];
    write_wav_i16(&dir.join("enroll.wav"), &enroll, rate);
    let mut manifest = format!(
        "enroll.wav\tThe quick brown fox\t{rate}\t{}\n",
        enroll.len()
    );
    for (i, (prompt, samples)) in takes.iter().enumerate() {
        let name = format!("utt{i:03}.wav");
        write_wav_i16(&dir.join(&name), samples, rate);
        manifest.push_str(&format!("{name}\t{prompt}\t{rate}\t{}\n", samples.len()));
    }
    std::fs::write(dir.join("manifest.tsv"), &manifest).expect("manifest");
}

fn file_sha(path: &Path) -> String {
    canonical::sha256_hex(&std::fs::read(path).expect("read for sha"))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read_dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn sha256_known_vector() {
    assert_eq!(
        canonical::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        canonical::sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn canon_version_and_resampler_id_frozen() {
    assert_eq!(canonical::CANON_VERSION, "rs1-canon-v1");
    assert_eq!(
        canonical::RESAMPLER_ID,
        "kaiser-sinc-polyphase-zc8-ph32-beta6.0-v1"
    );
}

#[test]
fn pinned_resampler_identity_at_16k() {
    let src: Vec<i16> = (0..1600).map(|i| (i % 251) as i16 - 125).collect();
    assert_eq!(canonical::resample_pinned(&src, 16_000), src);
}

#[test]
fn pinned_resampler_downsample_length_and_determinism() {
    // run1 geometry: 3.0 s @ 48 kHz -> exactly 48000 @ 16 kHz.
    let src = tone_take(48_000, 1000, 500, 20_000);
    let a = canonical::resample_pinned(&src, 48_000);
    let b = canonical::resample_pinned(&src, 48_000);
    assert_eq!(a.len(), 48_000);
    assert_eq!(a, b);
    // Odd ratio is deterministic too.
    let odd = tone_take(44_100, 1000, 500, 20_000);
    let c = canonical::resample_pinned(&odd, 44_100);
    let d = canonical::resample_pinned(&odd, 44_100);
    assert_eq!(c.len(), (odd.len() * 16_000 / 44_100));
    assert_eq!(c, d);
    // Tone survives resampling with most of its energy.
    let peak = c.iter().map(|&s| s.abs() as i32).max().unwrap_or(0);
    assert!(peak > 10_000, "resampled peak too small: {peak}");
}

#[test]
fn endpointer_finds_centered_burst() {
    let x = tone_take(16_000, 1000, 500, 20_000);
    let span = canonical::endpoint_span(&x).expect("accepted");
    let start_ms = span.start_sample as u32 * 1000 / 16_000;
    let end_ms = span.end_sample as u32 * 1000 / 16_000;
    // 500 ms burst at 1000-1500 ms, +/-50 ms hangover: expect ~950-1550.
    assert!(
        (900..=1000).contains(&start_ms),
        "start {start_ms} ms out of range"
    );
    assert!(
        (1500..=1600).contains(&end_ms),
        "end {end_ms} ms out of range"
    );
    assert!(span.speech_ms() >= 500 && span.speech_ms() <= 700);
}

#[test]
fn endpointer_ignores_guard_click() {
    // RS1-0 CLICK regression: loud 50 ms click at 50 ms must not anchor the
    // span; the real burst at 1000 ms must.
    let mut x = tone_take(16_000, 1000, 500, 12_000);
    for (i, s) in x[800..1600].iter_mut().enumerate() {
        let t = i as f64 / 16_000.0;
        *s = (30_000f64 * (2.0 * std::f64::consts::PI * 2000.0 * t).sin()) as i16;
    }
    let span = canonical::endpoint_span(&x).expect("accepted");
    let start_ms = span.start_sample as u32 * 1000 / 16_000;
    assert!(
        start_ms >= 900,
        "span anchored in guard zone at {start_ms} ms"
    );
}

#[test]
fn muted_take_rejected() {
    let root = test_root("muted");
    let raw = root.join("raw");
    // Peak -50 dBFS: 32768 * 10^(-50/20) ~= 104.
    let quiet = vec![50i16; 48_000 * 3];
    write_legacy_session(&raw, &[("whisper", quiet)], 48_000);
    let report = canonical::canon_session(&raw, &root.join("canon")).expect("canon runs");
    assert_eq!(report.takes_accepted, 0);
    assert_eq!(report.rejected.len(), 1);
    assert!(report.rejected[0].1.contains("quiet"));
    assert!(!root.join("canon").join("take000.wav").exists());
    let manifest =
        std::fs::read_to_string(root.join("canon").join("manifest.tsv")).expect("manifest");
    assert!(manifest.contains("REJECTED"));
}
#[test]
fn legacy_manifest_header_row_skipped() {
    let root = test_root("header");
    let raw = root.join("raw");
    let tone = tone_take(16_000, 400, 800, 8000);
    write_legacy_session(&raw, &[("hello", tone)], 16_000);
    // New-recorder sealed manifests carry a header row (100240).
    let path = raw.join("manifest.tsv");
    let body = std::fs::read_to_string(&path).expect("manifest");
    std::fs::write(&path, format!("file\tword\trate_hz\tframes\n{body}")).expect("header");
    let report = canonical::canon_session(&raw, &root.join("canon")).expect("canon runs");
    assert_eq!(report.takes_accepted, 1);
    assert!(report.rejected.is_empty());
}
#[test]
fn short_take_rejected() {
    let root = test_root("short");
    let raw = root.join("raw");
    // 100 ms burst < 150 ms minimum speech.
    let x = tone_take(48_000, 1000, 100, 20_000);
    write_legacy_session(&raw, &[("blip", x)], 48_000);
    let report = canonical::canon_session(&raw, &root.join("canon")).expect("canon runs");
    assert_eq!(report.takes_accepted, 0);
    assert_eq!(report.rejected.len(), 1);
    assert!(report.rejected[0].1.contains("short"));
}

#[test]
fn padding_geometry_exact() {
    let root = test_root("pad");
    let raw = root.join("raw");
    let x = tone_take(48_000, 1000, 500, 20_000);
    write_legacy_session(&raw, &[("congealed", x)], 48_000);
    let out = root.join("canon");
    let report = canonical::canon_session(&raw, &out).expect("canon runs");
    assert_eq!(report.takes_accepted, 1);
    let wav = std::fs::read(out.join("take000.wav")).expect("take wav");
    // 44-byte RIFF header + i16 payload.
    let payload = &wav[44..];
    let got: Vec<i16> = payload
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let manifest = std::fs::read_to_string(out.join("manifest.tsv")).expect("manifest");
    let row = manifest.lines().nth(1).expect("one row");
    let cols: Vec<&str> = row.split('\t').collect();
    assert_eq!(cols[0], "take000.wav");
    let frames: usize = cols[2].parse().expect("frames");
    assert_eq!(got.len(), frames);
    // Exactly 250 ms pads: frames == speech + 8000 (span_ms in clip ms).
    let span: Vec<&str> = cols[3].split('-').collect();
    let (s0, s1): (usize, usize) = (span[0].parse().unwrap(), span[1].parse().unwrap());
    assert_eq!(s0, 250, "span starts exactly after 250 ms pre-pad");
    assert_eq!(got.len(), (s1 - s0) * 16 + 8000);
}

#[test]
fn padding_edge_uses_take_audio_never_zero_pad() {
    // 600 ms take, burst at 300-450 ms: post-span audio (150 ms) is short of
    // the 250 ms pad, so the filler must come from the take itself and the
    // output must still be exactly speech + 8000.
    let root = test_root("edge");
    let raw = root.join("raw");
    let rate = 48_000u32;
    let mut x = vec![0i16; (rate * 6 / 10) as usize];
    // Nonzero noise floor everywhere so any zero-pad is detectable.
    for (i, s) in x.iter_mut().enumerate() {
        *s = ((i % 7) as i16 - 3) * 40;
    }
    let s0 = (300 * rate / 1000) as usize;
    let s1 = (450 * rate / 1000) as usize;
    for (i, s) in x[s0..s1].iter_mut().enumerate() {
        let t = i as f64 / rate as f64;
        *s = (20_000f64 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as i16;
    }
    write_legacy_session(&raw, &[("edge", x.clone())], rate);
    let out = root.join("canon");
    let report = canonical::canon_session(&raw, &out).expect("canon runs");
    assert_eq!(report.takes_accepted, 1);
    let wav = std::fs::read(out.join("take000.wav")).expect("take wav");
    let got: Vec<i16> = wav[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let manifest = std::fs::read_to_string(out.join("manifest.tsv")).expect("manifest");
    let cols: Vec<&str> = manifest.lines().nth(1).unwrap().split('\t').collect();
    let frames: usize = cols[2].parse().unwrap();
    assert_eq!(got.len(), frames);
    let span: Vec<&str> = cols[3].split('-').collect();
    let (s0, s1): (usize, usize) = (span[0].parse().unwrap(), span[1].parse().unwrap());
    assert_eq!(s0, 250, "span starts exactly after 250 ms pre-pad");
    assert_eq!(got.len(), (s1 - s0) * 16 + 8000);
    // Post-pad tail must be take audio (nonzero floor), never zero-fill.
    let tail = &got[got.len() - 1000..];
    assert!(
        tail.iter().any(|&s| s != 0),
        "post-pad tail is all zeros: zero-fill detected"
    );
}

#[test]
fn rerun_is_byte_identical() {
    let root = test_root("rerun");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[
            ("congealed", tone_take(48_000, 900, 600, 20_000)),
            ("survival", tone_take(48_000, 1100, 400, 18_000)),
            ("whisper", vec![50i16; 48_000 * 3]),
        ],
        48_000,
    );
    canonical::canon_session(&raw, &root.join("a")).expect("first run");
    canonical::canon_session(&raw, &root.join("b")).expect("second run");
    let mut fa = Vec::new();
    let mut fb = Vec::new();
    collect_files(&root.join("a"), &mut fa);
    collect_files(&root.join("b"), &mut fb);
    let rel = |base: &Path, files: &[PathBuf]| -> Vec<String> {
        files
            .iter()
            .map(|p| p.strip_prefix(base).unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(rel(&root.join("a"), &fa), rel(&root.join("b"), &fb));
    for (pa, pb) in fa.iter().zip(fb.iter()) {
        let (ba, bb) = (std::fs::read(pa).unwrap(), std::fs::read(pb).unwrap());
        assert_eq!(ba, bb, "byte mismatch in {}", pa.display());
    }
}

#[test]
fn provenance_chain_records_raw_to_canon() {
    let root = test_root("prov");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[("congealed", tone_take(48_000, 1000, 500, 20_000))],
        48_000,
    );
    let out = root.join("canon");
    canonical::canon_session(&raw, &out).expect("canon runs");
    let prov = std::fs::read_to_string(out.join("provenance.tsv")).expect("provenance");
    let mut lines = prov.lines();
    assert_eq!(
        lines.next().unwrap(),
        "raw_file\traw_sha256\tcanon_file\tcanon_sha256\tcanon_version\tresampler"
    );
    let row = lines.next().expect("one row");
    let cols: Vec<&str> = row.split('\t').collect();
    assert_eq!(cols[0], "utt000.wav");
    assert_eq!(cols[2], "take000.wav");
    assert_eq!(cols[4], canonical::CANON_VERSION);
    assert_eq!(cols[5], canonical::RESAMPLER_ID);
    assert_eq!(cols[1], file_sha(&raw.join("utt000.wav")));
    assert_eq!(cols[3], file_sha(&out.join("take000.wav")));
}

#[test]
fn legacy_enroll_skipped() {
    let root = test_root("enroll");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[("congealed", tone_take(48_000, 1000, 500, 20_000))],
        48_000,
    );
    let out = root.join("canon");
    let report = canonical::canon_session(&raw, &out).expect("canon runs");
    assert_eq!(report.takes_total, 1);
    assert_eq!(report.takes_accepted, 1);
    let manifest = std::fs::read_to_string(out.join("manifest.tsv")).expect("manifest");
    assert!(!manifest.contains("enroll"));
    assert_eq!(manifest.lines().count(), 2); // header + 1 row
}

#[test]
fn qc_plot_written_every_10th_take() {
    let root = test_root("plots");
    let raw = root.join("raw");
    let takes: Vec<(String, Vec<i16>)> = (0..11)
        .map(|i| (format!("w{i:02}"), tone_take(48_000, 1000, 500, 20_000)))
        .collect();
    let refs: Vec<(&str, Vec<i16>)> = takes.iter().map(|(p, x)| (p.as_str(), x.clone())).collect();
    write_legacy_session(&raw, &refs, 48_000);
    let out = root.join("canon");
    canonical::canon_session(&raw, &out).expect("canon runs");
    assert!(out.join("qc").join("take000.png").exists());
    assert!(out.join("qc").join("take010.png").exists());
    assert!(!out.join("qc").join("take001.png").exists());
    let png = std::fs::read(out.join("qc").join("take000.png")).expect("plot");
    assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
    let qc = std::fs::read_to_string(out.join("qc.tsv")).expect("qc.tsv");
    assert_eq!(qc.lines().count(), 12); // header + 11 rows
}

fn write_rs1_session(dir: &Path, prompts: &[&str], rates_ok: bool) {
    std::fs::create_dir_all(dir).expect("raw dir");
    let mut prompt_txt = String::new();
    for p in prompts {
        prompt_txt.push_str(p);
        prompt_txt.push('\n');
    }
    std::fs::write(dir.join("prompts.txt"), &prompt_txt).expect("prompts");
    std::fs::write(dir.join("session.log"), "capture ok\n").expect("log");
    let n = if rates_ok {
        prompts.len()
    } else {
        prompts.len() - 1
    };
    for i in 0..n {
        write_wav_i16(
            &dir.join(format!("take{i:03}.wav")),
            &tone_take(48_000, 1000, 500, 20_000),
            48_000,
        );
    }
    let prompt_sha = canonical::sha256_hex(prompt_txt.as_bytes());
    let order_sha = canonical::order_sha256(42, prompt_txt.as_bytes());
    let meta = format!(
        "host=windows-wasapi\ndevice=fifine\nmic_distance_cm=30\nrate_hz=48000\nchannels_in=1\nlist=words30\nprompt_file_sha256={prompt_sha}\norder_seed=42\norder_sha256={order_sha}\npartition=DEV\nrecorder_version=abc123\nclick_guard_ms=300\ndate_utc=2026-10-31\n"
    );
    std::fs::write(dir.join("session.meta"), &meta).expect("meta");
    let mut names: Vec<String> = (0..n).map(|i| format!("take{i:03}.wav")).collect();
    names.extend(
        ["prompts.txt", "session.meta", "session.log"]
            .iter()
            .map(|s| s.to_string()),
    );
    names.sort();
    let mut sums = String::new();
    for name in &names {
        sums.push_str(&format!("{}  {name}\n", file_sha(&dir.join(name))));
    }
    std::fs::write(dir.join("SHA256SUMS"), &sums).expect("sums");
}

#[test]
fn rs1_validation_accepts_sealed_session() {
    let root = test_root("rs1ok");
    let raw = root.join("raw");
    write_rs1_session(&raw, &["alpha", "beta"], true);
    let session = canonical::validate_raw(&raw).expect("valid rs1");
    assert_eq!(session.takes.len(), 2);
    let out = root.join("canon");
    let report = canonical::canon_session(&raw, &out).expect("canon runs");
    assert_eq!(report.takes_accepted, 2);
}

#[test]
fn rs1_validation_rejects_tampered_sums() {
    let root = test_root("rs1tamper");
    let raw = root.join("raw");
    write_rs1_session(&raw, &["alpha", "beta"], true);
    // Flip one byte in a take after sealing.
    let wav = raw.join("take000.wav");
    let mut bytes = std::fs::read(&wav).expect("read take");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&wav, &bytes).expect("rewrite take");
    let err = canonical::validate_raw(&raw).expect_err("must reject");
    assert!(err.contains("SHA256SUMS"), "unexpected error: {err}");
}

#[test]
fn rs1_validation_rejects_count_mismatch() {
    let root = test_root("rs1count");
    let raw = root.join("raw");
    write_rs1_session(&raw, &["alpha", "beta", "gamma"], false);
    let err = canonical::validate_raw(&raw).expect_err("must reject");
    assert!(err.contains("count"), "unexpected error: {err}");
}

// --- RS1-c: canonical loader (interfaces-v1 I1) ---

#[test]
fn loader_roundtrip_on_synthetic_canon() {
    let root = test_root("load");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[
            ("congealed", tone_take(48_000, 1000, 500, 20_000)),
            ("survival", tone_take(48_000, 900, 600, 18_000)),
        ],
        48_000,
    );
    let out = root.join("canon");
    canonical::canon_session(&raw, &out).expect("canon runs");
    let session = canonical::load_session(&out).expect("load canon");
    assert_eq!(session.dir, out);
    assert_eq!(session.takes.len(), 2);
    assert_eq!(session.takes[0].id, "take000");
    assert_eq!(session.takes[0].prompt, "congealed");
    assert_eq!(session.takes[1].prompt, "survival");
    assert!(!session.takes[0].pcm.is_empty());
    // Span is canonical-clip ms starting exactly after the 250 ms pre-pad.
    assert_eq!(session.takes[0].span_ms.0, 250);
    assert_eq!(
        session.takes[0].pcm.len(),
        (session.takes[0].span_ms.1 - 250) as usize * 16 + 8000
    );
}

#[test]
fn loader_skips_rejected_takes() {
    let root = test_root("loadrej");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[
            ("congealed", tone_take(48_000, 1000, 500, 20_000)),
            ("whisper", vec![50i16; 48_000 * 3]),
        ],
        48_000,
    );
    let out = root.join("canon");
    canonical::canon_session(&raw, &out).expect("canon runs");
    let session = canonical::load_session(&out).expect("load canon");
    assert_eq!(session.takes.len(), 1);
    assert_eq!(session.takes[0].prompt, "congealed");
}

#[test]
fn loader_rejects_raw_legacy_manifest() {
    let root = test_root("loadraw");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[("congealed", tone_take(48_000, 1000, 500, 20_000))],
        48_000,
    );
    let err = canonical::load_session(&raw).expect_err("raw dir is not canonical");
    assert!(
        err.to_string().contains("header"),
        "unexpected error: {err}"
    );
}

#[test]
fn loader_rejects_tampered_audio() {
    let root = test_root("loadtamper");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[("congealed", tone_take(48_000, 1000, 500, 20_000))],
        48_000,
    );
    let out = root.join("canon");
    canonical::canon_session(&raw, &out).expect("canon runs");
    let wav = out.join("take000.wav");
    let mut bytes = std::fs::read(&wav).expect("read take");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&wav, &bytes).expect("rewrite take");
    let err = canonical::load_session(&out).expect_err("must reject");
    assert!(
        err.to_string().contains("SHA256SUMS"),
        "unexpected error: {err}"
    );
}

#[test]
fn loader_rejects_rate_mismatch() {
    let root = test_root("loadrate");
    let raw = root.join("raw");
    write_legacy_session(
        &raw,
        &[("congealed", tone_take(48_000, 1000, 500, 20_000))],
        48_000,
    );
    let out = root.join("canon");
    canonical::canon_session(&raw, &out).expect("canon runs");
    // Swap the take for a 48 kHz impostor and re-seal SHA256SUMS so the only
    // failure is the rate check.
    write_wav_i16(
        &out.join("take000.wav"),
        &tone_take(48_000, 1000, 500, 20_000),
        48_000,
    );
    let mut sums: Vec<String> = std::fs::read_to_string(out.join("SHA256SUMS"))
        .expect("sums")
        .lines()
        .map(|l| {
            if l.ends_with("take000.wav") {
                format!("{}  take000.wav", file_sha(&out.join("take000.wav")))
            } else {
                l.to_string()
            }
        })
        .collect();
    sums.sort();
    std::fs::write(out.join("SHA256SUMS"), sums.join("\n") + "\n").expect("rewrite sums");
    let err = canonical::load_session(&out).expect_err("must reject");
    assert!(err.to_string().contains("16000"), "unexpected error: {err}");
}

#[test]
fn wav_probe_reports_spec_and_frames() {
    let root = test_root("probe");
    let wav = root.join("p.wav");
    write_wav_i16(&wav, &vec![0i16; 1234], 48_000);
    let (spec, frames) = crate::audio::wav_probe(&wav).expect("probe");
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.sample_rate, 48_000);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(frames, 1234);
}

#[test]
fn canon_loader_grep_no_raw_wav_readers() {
    // RS-1 §5 CI gate: `hound::WavReader` may appear only under src/corpus
    // (the loader + canonicalizer own all WAV reads).
    let needle = "hound::WavReader";
    // Canonicalize: the raw join keeps a `corpus` path component inside the
    // `..` prefix, which would wrongly exempt every file below.
    let src_root =
        std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src"))
            .expect("workspace src root");
    let mut offenders = Vec::new();
    let mut stack = vec![src_root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("read src dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                if path.file_name().map(|n| n == "target").unwrap_or(false) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().map(|x| x != "rs").unwrap_or(true) {
                continue;
            }
            // Allowed: everything under src/corpus (loader, canonicalizer,
            // their bins and tests).
            if path.components().any(|c| c.as_os_str() == "corpus") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read rs");
            if text.contains(needle) {
                offenders.push(
                    path.strip_prefix(&src_root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "raw WAV readers outside src/corpus: {offenders:?}"
    );
}

// --- RS1-d: GO prompt cuts (machine-exact generator + forever verifier) ---

fn sessions_dir() -> Option<PathBuf> {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sessions"))
        .ok()
}

fn prompts_dir() -> PathBuf {
    std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prompts"))
        .expect("prompts dir")
}

/// Content lines: non-blank, not `#` comments (recorder `script_lines`
/// semantics: the recorder never sees headers).
fn content_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect()
}

fn cut_bytes(
    cut_id: &str,
    rule: &str,
    source_name: &str,
    source_text: &str,
    lines: &[&str],
) -> Vec<u8> {
    let sha = canonical::sha256_hex(source_text.as_bytes());
    let mut out = format!(
        "# RS1-d GO cut {cut_id}: {rule} from prompts/{source_name}\n# source_sha256={sha} content_lines={}\n",
        lines.len()
    );
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
    out.into_bytes()
}

fn check_or_write_cut(path: &Path, expected: &[u8]) {
    if std::env::var("RS1D_WRITE_CUTS").as_deref() == Ok("1") {
        std::fs::write(path, expected).expect("write cut file");
        return;
    }
    let actual = std::fs::read(path).unwrap_or_else(|_| {
        panic!(
            "missing cut file {} (run with RS1D_WRITE_CUTS=1 to generate)",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "cut file {} disagrees with its source (re-run with RS1D_WRITE_CUTS=1 to regenerate)",
        path.display()
    );
}

#[test]
fn rs1d_go_prompt_cuts() {
    let prompts = prompts_dir();
    // Fly cuts: first 10 fly + first 5 family content lines, source order.
    for (source, cut, cut_id) in [
        (
            "script-p4-fly-dev.txt",
            "prompts-fly-dev-01.txt",
            "fly-DEV-01",
        ),
        (
            "script-p4-fly-ptest.txt",
            "prompts-fly-phase-test-01.txt",
            "fly-PHASE-TEST-01",
        ),
    ] {
        let text = std::fs::read_to_string(prompts.join(source)).expect("read fly source");
        let content = content_lines(&text);
        assert_eq!(content.len(), 30, "{source}: want 30 content lines");
        let mut kept: Vec<&str> = Vec::new();
        let (mut fly, mut fam) = (0, 0);
        for l in &content {
            let is_fam = l.contains("family");
            let is_fly = l.contains("fly");
            assert!(
                is_fam != is_fly,
                "{source}: line matches exactly one class: {l:?}"
            );
            if is_fam && fam < 5 {
                fam += 1;
                kept.push(l);
            } else if is_fly && fly < 10 {
                fly += 1;
                kept.push(l);
            }
        }
        assert_eq!((fly, fam), (10, 5), "{source}: class counts");
        assert_eq!(kept.len(), 15);
        let expected = cut_bytes(
            cut_id,
            "first 10 fly + first 5 family content lines in source order",
            source,
            &text,
            &kept,
        );
        check_or_write_cut(&prompts.join(cut), &expected);
    }
    // Read cut: first 50 content lines of script-c0r-read.txt.
    let source = "script-c0r-read.txt";
    let text = std::fs::read_to_string(prompts.join(source)).expect("read c0r source");
    let content = content_lines(&text);
    assert_eq!(content.len(), 104, "{source}: want 104 content lines");
    let kept = &content[..50];
    assert_eq!(kept.len(), 50);
    let expected = cut_bytes(
        "read-PHASE-TEST-01",
        "first 50 content lines",
        source,
        &text,
        kept,
    );
    check_or_write_cut(&prompts.join("prompts-read-phase-test-01.txt"), &expected);
}

#[test]
#[ignore = "main-only: needs every session dir present (lanes lack gitignored audio dirs)"]
fn rs1d_catalog_coverage() {
    // RS1-d gate, both directions: every sessions/<dir> has exactly one
    // CATALOG.tsv row, and every row names an existing dir. Run on main:
    // cargo test -p corpus --lib rs1d_catalog_coverage -- --ignored
    let Some(sessions) = sessions_dir() else {
        eprintln!(
            "SKIP rs1d_catalog_coverage: sessions/ absent (owner recordings, untracked in public clones)"
        );
        return;
    };
    let catalog = sessions.join("CATALOG.tsv");
    if !catalog.is_file() {
        eprintln!(
            "SKIP rs1d_catalog_coverage: {} absent (owner session catalog, untracked in public clones)",
            catalog.display()
        );
        return;
    }
    let text = std::fs::read_to_string(&catalog).expect("read CATALOG.tsv");
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    assert_eq!(
        header,
        "session_id|list|partition|chain|raw_sha|canon_version|canon_sha|status|consumed_by|notes"
    );
    let mut rows: Vec<Vec<&str>> = Vec::new();
    for line in lines {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('|').collect();
        assert_eq!(cols.len(), 10, "CATALOG row has 10 columns: {line:?}");
        assert!(
            ["VALID", "INVALID", "VOID", "RETIRED", "QUARANTINED"].contains(&cols[7]),
            "bad status: {line:?}"
        );
        rows.push(cols);
    }
    let mut dirs: Vec<String> = std::fs::read_dir(&sessions)
        .expect("list sessions")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        // sessions/canon/ is derived canonical output (gitignored), not a raw session.
        .filter(|name| name != "canon")
        .collect();
    dirs.sort();
    let mut ids: Vec<String> = rows.iter().map(|c| c[0].to_string()).collect();
    ids.sort();
    assert_eq!(ids, dirs, "catalog rows must match session dirs exactly");
}
