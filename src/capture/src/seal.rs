//! Sealed-session writers for the phase-test instrument.
//!
//! Layout contract (mirrors `fse::instrument::validate_session`):
//! - `order.txt`: `seed <N>` on line 1, then one prompt per line.
//! - `manifest.tsv`: header `file\tword\trate_hz\tframes` on line 1 (skipped
//!   by the validator), then one `file\tword\trate_hz\tframes` row per take.
//! - `session.meta`: `key=value` lines; required keys are `device`, `rate_hz`,
//!   `date_utc`, `seed`, `order_sha256`, `manifest_sha256`,
//!   `recorder_commit`, `kept`.
//! - `SHA256SUMS`: `<hex>  <filename>` per wav plus manifest + order
//!   (order-free; the validator checks set membership).
//!
//! All builders are pure (arguments in, text out) so they unit-test without
//! a microphone. Hashing reuses `fse::instrument::sha256_hex` (std-only).

use std::path::Path;

/// Header line the validator skips (line 1 of every manifest).
pub const MANIFEST_HEADER: &str = "file\tword\trate_hz\tframes";

/// Replace bytes that would break TSV columns or line framing.
fn sanitize(cell: &str) -> String {
    cell.replace(['\t', '\n', '\r'], " ")
}

/// `seed <N>` line plus one prompt per line (trailing newline included).
pub fn build_order_txt(seed: u64, prompts: &[String]) -> String {
    let mut out = format!("seed {seed}\n");
    for p in prompts {
        out.push_str(&sanitize(p));
        out.push('\n');
    }
    out
}

/// Header plus one `file\tword\trate_hz\tframes` row per take.
pub fn build_manifest_tsv(rows: &[(String, String, u32, u32)]) -> String {
    let mut out = String::from(MANIFEST_HEADER);
    out.push('\n');
    for (file, word, rate, frames) in rows {
        out.push_str(&sanitize(file));
        out.push('\t');
        out.push_str(&sanitize(word));
        out.push('\t');
        out.push_str(&rate.to_string());
        out.push('\t');
        out.push_str(&frames.to_string());
        out.push('\n');
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn build_session_meta(
    device: &str,
    rate_hz: u32,
    date_utc: &str,
    seed: u64,
    order_sha256: &str,
    manifest_sha256: &str,
    recorder_commit: &str,
    kept: usize,
) -> String {
    format!(
        "device={device}\nrate_hz={rate_hz}\ndate_utc={date_utc}\nseed={seed}\norder_sha256={order_sha256}\nmanifest_sha256={manifest_sha256}\nrecorder_commit={recorder_commit}\nkept={kept}\n"
    )
}

/// Seconds since the Unix epoch (the validator only requires presence).
pub fn utc_now_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Current HEAD, or `unknown` outside a git checkout.
pub fn recorder_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Write `SHA256SUMS` (`<hex>  <name>` per file) into `dir`.
pub fn write_sums_file(dir: &Path, names: &[String]) -> Result<(), String> {
    let mut sums = String::new();
    for n in names {
        let bytes = std::fs::read(dir.join(n)).map_err(|e| format!("cannot hash {n}: {e}"))?;
        sums.push_str(&format!("{}  {n}\n", fse::instrument::sha256_hex(&bytes)));
    }
    std::fs::write(dir.join("SHA256SUMS"), sums)
        .map_err(|e| format!("cannot write SHA256SUMS: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_txt_has_seed_line_first() {
        let txt = build_order_txt(42, &["b".to_string(), "a".to_string()]);
        assert_eq!(txt, "seed 42\nb\na\n");
    }

    #[test]
    fn manifest_has_header_plus_rows() {
        let rows = vec![(
            "utt000.wav".to_string(),
            "hello world".to_string(),
            16000,
            48000,
        )];
        let tsv = build_manifest_tsv(&rows);
        assert_eq!(
            tsv,
            "file\tword\trate_hz\tframes\nutt000.wav\thello world\t16000\t48000\n"
        );
    }

    #[test]
    fn manifest_sanitizes_tabs_and_newlines() {
        let rows = vec![("a.wav".to_string(), "x\ty\n".to_string(), 16000, 1)];
        assert_eq!(build_manifest_tsv(&rows).lines().count(), 2);
    }

    #[test]
    fn session_meta_carries_all_eight_keys() {
        let meta = build_session_meta("mic", 16000, "0", 1, "o", "m", "c", 30);
        for key in [
            "device=",
            "rate_hz=",
            "date_utc=",
            "seed=",
            "order_sha256=",
            "manifest_sha256=",
            "recorder_commit=",
            "kept=",
        ] {
            assert!(meta.contains(key), "missing {key}");
        }
    }

    #[test]
    fn sums_file_format_matches_sha256sum() {
        let dir = std::env::temp_dir().join("seal_sums_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.wav"), b"abc").unwrap();
        write_sums_file(&dir, &["a.wav".to_string()]).unwrap();
        let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).unwrap();
        assert_eq!(
            sums,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.wav\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    fn write_tone_wav(path: &Path, freq: f64, secs: f64) {
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

    fn write_muted_wav(path: &Path, secs: f64) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (secs * 16_000.0) as usize;
        for i in 0..n {
            let s = if i % 6 == 0 {
                1i16
            } else if i % 11 == 0 {
                -1i16
            } else {
                0i16
            };
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    /// Build a synthetic 2-label general session; `silent_second` mutes take 2.
    /// Same layout `record-session --validate --labels 2 --dur 3.0` checks.
    fn build_two_label_session(dir: &Path, silent_second: bool) {
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(dir).unwrap();
        let prompts = vec!["alpha phrase".to_string(), "beta phrase".to_string()];
        write_tone_wav(&dir.join("utt000.wav"), 440.0, 3.0);
        if silent_second {
            write_muted_wav(&dir.join("utt001.wav"), 3.0);
        } else {
            write_tone_wav(&dir.join("utt001.wav"), 550.0, 3.0);
        }
        let order_txt = build_order_txt(9, &prompts);
        std::fs::write(dir.join("order.txt"), &order_txt).unwrap();
        let rows = vec![
            ("utt000.wav".to_string(), prompts[0].clone(), 16000, 48000),
            ("utt001.wav".to_string(), prompts[1].clone(), 16000, 48000),
        ];
        let manifest = build_manifest_tsv(&rows);
        std::fs::write(dir.join("manifest.tsv"), &manifest).unwrap();
        let meta = build_session_meta(
            "fixture",
            16000,
            "2026-10-29T00:00:00Z",
            9,
            &fse::instrument::sha256_hex(order_txt.as_bytes()),
            &fse::instrument::sha256_hex(manifest.as_bytes()),
            "fixture",
            2,
        );
        std::fs::write(dir.join("session.meta"), &meta).unwrap();
        write_sums_file(
            dir,
            &[
                "utt000.wav".to_string(),
                "utt001.wav".to_string(),
                "manifest.tsv".to_string(),
                "order.txt".to_string(),
            ],
        )
        .unwrap();
    }

    #[test]
    fn validate_rejects_session_with_silent_take() {
        let dir = std::env::temp_dir().join("seal_validate_silent_vyz");
        build_two_label_session(&dir, true);
        let e = fse::instrument::validate_session_general(&dir, 2, 3.0)
            .expect_err("silent take must REJECT");
        assert!(e.starts_with("REJECT:"), "violation: {e}");
        assert!(e.contains("utt001.wav"), "violation: {e}");
        assert!(e.contains("speech floor"), "violation: {e}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validate_accepts_session_without_silent_take() {
        let dir = std::env::temp_dir().join("seal_validate_audible_vyz");
        build_two_label_session(&dir, false);
        let s = fse::instrument::validate_session_general(&dir, 2, 3.0)
            .expect("audible session accepts");
        assert_eq!(s.labels, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_gate_shares_validator_floor() {
        assert_eq!(fse::instrument::SILENT_TAKE_PEAK_FLOOR, 0.01);
        assert!(fse::instrument::take_is_silent(&vec![0i16; 100]));
        assert!(!fse::instrument::take_is_silent(&vec![8000i16; 100]));
    }
}
