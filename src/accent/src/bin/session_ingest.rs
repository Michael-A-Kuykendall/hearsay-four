//! session-ingest: recorded WAVs + manifest.tsv → scorer session layout.
//!
//! Bead `o20`. Reads a `record-session` manifest (file, expected, rate,
//! samples), validates it, concatenates the utterance WAVs in order, and
//! emits the `session.tsv` + single-audio layout that `failfast_run`
//! (39c.5) scores. The enrollment row (expected text equals
//! `calibration::ENROLLMENT_PROMPT`) is excluded from scoring —
//! enrollment-as-baseline wiring is follow-up work, not this bead.

use std::path::{Path, PathBuf};

/// Pre-registered session geometry (bead `o20` notes): K buckets of
/// `per_bucket` utterances over the scored rows in manifest order.
pub struct IngestConfig {
    pub buckets: usize,
    pub per_bucket: usize,
    pub speaker: String,
    pub drift_tags: String,
}

/// Validate + convert. Returns the number of scored utterances.
/// Errors name the offending row; nothing is written on error.
///
/// Rows whose expected text equals `calibration::ENROLLMENT_PROMPT` are
/// excluded from scoring (enrollment-as-baseline is follow-up work).
/// Timestamps come from actual WAV sample counts, never from assumed
/// uniform lengths.
pub fn ingest(
    manifest: &Path,
    wav_dir: &Path,
    out_tsv: &Path,
    out_wav: &Path,
    config: &IngestConfig,
) -> Result<usize, String> {
    if config.buckets == 0 || config.per_bucket == 0 {
        return Err("buckets and per-bucket must both be positive".to_string());
    }

    let content =
        std::fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    struct Row {
        file: String,
        expected: String,
        rate: u32,
        samples: usize,
    }
    let mut rows: Vec<Row> = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 4 {
            return Err(format!(
                "row {}: expected 4 tab columns, got {}",
                lineno + 1,
                cols.len()
            ));
        }
        if cols[1].trim() == calibration::ENROLLMENT_PROMPT {
            continue; // enrollment: not scored here.
        }
        let rate: u32 = cols[2]
            .trim()
            .parse()
            .map_err(|_| format!("row {}: bad rate in {line:?}", lineno + 1))?;
        let samples: usize = cols[3]
            .trim()
            .parse()
            .map_err(|_| format!("row {}: bad samples in {line:?}", lineno + 1))?;
        if samples == 0 {
            return Err(format!(
                "row {}: zero samples for {}",
                lineno + 1,
                cols[0].trim()
            ));
        }
        rows.push(Row {
            file: cols[0].trim().to_string(),
            expected: cols[1].trim().to_string(),
            rate,
            samples,
        });
    }

    let want = config.buckets * config.per_bucket;
    if rows.len() != want {
        return Err(format!(
            "scored rows {} != buckets {} × per-bucket {} ({want}); buckets would not be exhaustive",
            rows.len(),
            config.buckets,
            config.per_bucket
        ));
    }
    let rate = rows[0].rate;
    if rows.iter().any(|r| r.rate != rate) {
        return Err("mixed sample rates across rows; refusing to concatenate".to_string());
    }

    // Read + concatenate in manifest order.
    let mut pcm: Vec<i16> = Vec::new();
    let mut spans: Vec<(f64, f64)> = Vec::new();
    let mut elapsed: usize = 0;
    for row in &rows {
        let path = wav_dir.join(&row.file);
        let mut reader =
            hound::WavReader::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let spec = reader.spec();
        if spec.channels != 1 || spec.sample_rate != rate || spec.bits_per_sample != 16 {
            return Err(format!(
                "{}: expected mono/16-bit/{}Hz, got {:?}",
                path.display(),
                rate,
                spec
            ));
        }
        let samples: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
        let samples = samples.map_err(|e| format!("{}: {e}", path.display()))?;
        if samples.len() != row.samples {
            return Err(format!(
                "{}: manifest says {} samples, WAV holds {}",
                path.display(),
                row.samples,
                samples.len()
            ));
        }
        let start = elapsed as f64 / rate as f64;
        elapsed += samples.len();
        let stop = elapsed as f64 / rate as f64;
        spans.push((start, stop));
        pcm.extend_from_slice(&samples);
    }

    // Write concatenated audio first (keeps “nothing on error” for the TSV:
    // a failed WAV write aborts before the manifest is emitted).
    if let Some(parent) = out_wav.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(out_wav, spec)
        .map_err(|e| format!("{}: {e}", out_wav.display()))?;
    for &s in &pcm {
        writer
            .write_sample(s)
            .map_err(|e| format!("{}: {e}", out_wav.display()))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("{}: {e}", out_wav.display()))?;

    if let Some(parent) = out_tsv.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut tsv = String::from("bucket\tidx\tstart_sec\tstop_sec\tspeaker\tdrift_tags\ttext\n");
    for (i, (row, (start, stop))) in rows.iter().zip(spans.iter()).enumerate() {
        tsv.push_str(&format!(
            "{}\t{i}\t{start:.3}\t{stop:.3}\t{}\t{}\t{}\n",
            i / config.per_bucket,
            config.speaker,
            config.drift_tags,
            row.expected
        ));
    }
    std::fs::write(out_tsv, tsv).map_err(|e| format!("{}: {e}", out_tsv.display()))?;

    Ok(rows.len())
}

fn usage() -> ! {
    eprintln!(
        "usage: session-ingest --manifest <manifest.tsv> --wav-dir <dir> --out-tsv <session.tsv> --out-wav <audio.wav> --buckets K --per-bucket N [--speaker S] [--drift-tags T]"
    );
    std::process::exit(2);
}

fn main() {
    let mut manifest: Option<PathBuf> = None;
    let mut wav_dir: Option<PathBuf> = None;
    let mut out_tsv: Option<PathBuf> = None;
    let mut out_wav: Option<PathBuf> = None;
    let mut buckets: Option<usize> = None;
    let mut per_bucket: Option<usize> = None;
    let mut speaker = "owner".to_string();
    let mut drift_tags = "fatigue:live,mic:usb-fifine,noise:room".to_string();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--manifest" => manifest = args.next().map(PathBuf::from),
            "--wav-dir" => wav_dir = args.next().map(PathBuf::from),
            "--out-tsv" => out_tsv = args.next().map(PathBuf::from),
            "--out-wav" => out_wav = args.next().map(PathBuf::from),
            "--buckets" => buckets = args.next().and_then(|v| v.parse().ok()),
            "--per-bucket" => per_bucket = args.next().and_then(|v| v.parse().ok()),
            "--speaker" => {
                if let Some(v) = args.next() {
                    speaker = v;
                }
            }
            "--drift-tags" => {
                if let Some(v) = args.next() {
                    drift_tags = v;
                }
            }
            _ => usage(),
        }
    }
    let (
        Some(manifest),
        Some(wav_dir),
        Some(out_tsv),
        Some(out_wav),
        Some(buckets),
        Some(per_bucket),
    ) = (manifest, wav_dir, out_tsv, out_wav, buckets, per_bucket)
    else {
        usage()
    };

    let config = IngestConfig {
        buckets,
        per_bucket,
        speaker,
        drift_tags,
    };
    match ingest(&manifest, &wav_dir, &out_tsv, &out_wav, &config) {
        Ok(n) => println!("session-ingest: OK ({n} utterances)"),
        Err(e) => {
            eprintln!("session-ingest: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, samples: &[i16], rate: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    fn fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "session-ingest-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 2 buckets × 2 utterances of synthetic speech-rate PCM: ingest must
    /// accept it and `load_session` must accept the emitted manifest.
    #[test]
    fn ingest_two_buckets_roundtrip() {
        let dir = fixture_dir("roundtrip");
        let tone: Vec<i16> = (0..48_000).map(|i| ((i % 100) as i16) * 200).collect();
        for name in ["a.wav", "b.wav", "c.wav", "d.wav"] {
            write_wav(&dir.join(name), &tone, 48_000);
        }
        std::fs::write(
            dir.join("manifest.tsv"),
            "a.wav\tcongealed\t48000\t48000\n\
             b.wav\tsurvival\t48000\t48000\n\
             c.wav\tripple\t48000\t48000\n\
             d.wav\tprocess\t48000\t48000\n",
        )
        .unwrap();

        let config = IngestConfig {
            buckets: 2,
            per_bucket: 2,
            speaker: "test".to_string(),
            drift_tags: "fatigue:test".to_string(),
        };
        let n = ingest(
            &dir.join("manifest.tsv"),
            &dir,
            &dir.join("session.tsv"),
            &dir.join("audio.wav"),
            &config,
        )
        .expect("ingest accepts the synthetic fixture");
        assert_eq!(n, 4);

        let rows =
            accent::measure::load_session(&dir).expect("load_session accepts the emitted manifest");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].bucket, 0);
        assert_eq!(rows[3].bucket, 1);
        assert!((rows[1].start_sec - 1.0).abs() < 1e-9);
        assert_eq!(rows[3].text, "process");
    }

    /// A short row must fail loudly, never silently shift buckets.
    #[test]
    fn ingest_rejects_short_manifest() {
        let dir = fixture_dir("short");
        let tone: Vec<i16> = vec![0; 1000];
        write_wav(&dir.join("a.wav"), &tone, 48_000);
        std::fs::write(dir.join("manifest.tsv"), "a.wav\tcongealed\t48000\t1000\n").unwrap();
        let config = IngestConfig {
            buckets: 1,
            per_bucket: 2,
            speaker: "test".to_string(),
            drift_tags: "fatigue:test".to_string(),
        };
        assert!(
            ingest(
                &dir.join("manifest.tsv"),
                &dir,
                &dir.join("session.tsv"),
                &dir.join("audio.wav"),
                &config
            )
            .is_err()
        );
    }

    /// The enrollment row is excluded from scoring.
    #[test]
    fn ingest_skips_enrollment() {
        let dir = fixture_dir("enroll");
        let tone: Vec<i16> = vec![0; 48_000];
        write_wav(&dir.join("enroll.wav"), &tone, 48_000);
        write_wav(&dir.join("a.wav"), &tone, 48_000);
        std::fs::write(
            dir.join("manifest.tsv"),
            format!(
                "enroll.wav\t{}\t48000\t48000\na.wav\tcongealed\t48000\t48000\n",
                calibration::ENROLLMENT_PROMPT
            ),
        )
        .unwrap();
        let config = IngestConfig {
            buckets: 1,
            per_bucket: 1,
            speaker: "test".to_string(),
            drift_tags: "fatigue:test".to_string(),
        };
        let n = ingest(
            &dir.join("manifest.tsv"),
            &dir,
            &dir.join("session.tsv"),
            &dir.join("audio.wav"),
            &config,
        )
        .expect("ingest skips enrollment");
        assert_eq!(n, 1);
    }
}
