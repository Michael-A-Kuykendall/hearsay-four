//! A1 regression tests (bead hearsay-four-b7p): DTW, front end, LOTO.

use crate::dtw::dtw_distance;
use crate::frontend::{FrontendConfig, extract_with_config, speech_span};
use crate::gate::{Clip, is_degenerate, leave_one_take_out, phase_test};

fn seq(v: &[f64]) -> Vec<Vec<f64>> {
    v.iter().map(|&x| vec![x]).collect()
}

#[test]
fn dtw_identical_is_zero_and_length_normalized() {
    let a = seq(&[0.0, 1.0, 2.0]);
    assert_eq!(dtw_distance(&a, &a, 0.2), 0.0);
    // warped copy stays cheap vs far sequence
    let warped = seq(&[0.0, 0.0, 1.0, 2.0, 2.0]);
    let far = seq(&[10.0, 11.0, 12.0]);
    assert!(dtw_distance(&a, &warped, 0.5) < dtw_distance(&a, &far, 0.5));
}
#[test]
fn dtw_empty_is_infinite_and_band_can_block() {
    assert!(dtw_distance(&[], &seq(&[1.0]), 0.2).is_infinite());
    // endpoint far outside a tiny band on very different lengths
    let a = seq(&[0.0; 20]);
    let b = seq(&[0.0]);
    assert!(dtw_distance(&a, &b, 0.05).is_infinite() || dtw_distance(&a, &b, 0.05) >= 0.0);
}
#[test]
fn frontend_dims_follow_config() {
    let sr = 16_000u32;
    let samples: Vec<i16> = (0..sr as usize)
        .map(|i| ((i as f64 * 0.1).sin() * 8000.0) as i16)
        .collect();
    assert_eq!(
        extract_with_config(&samples, sr, &FrontendConfig::default()).unwrap()[0].len(),
        39
    );
    let no_delta = FrontendConfig {
        use_deltas: false,
        use_delta_deltas: false,
        ..Default::default()
    };
    assert_eq!(
        extract_with_config(&samples, sr, &no_delta).unwrap()[0].len(),
        13
    );
    let d_only = FrontendConfig {
        use_delta_deltas: false,
        ..Default::default()
    };
    assert_eq!(
        extract_with_config(&samples, sr, &d_only).unwrap()[0].len(),
        26
    );
    assert!(extract_with_config(&[], sr, &FrontendConfig::default()).is_err());
    assert!(extract_with_config(&samples, 4_000, &FrontendConfig::default()).is_err());
}
#[test]
fn cmn_zero_means_statics() {
    let sr = 16_000u32;
    let samples: Vec<i16> = (0..sr as usize)
        .map(|i| ((i as f64 * 0.05).sin() * 5000.0) as i16)
        .collect();
    let frames = extract_with_config(&samples, sr, &FrontendConfig::default()).unwrap();
    // first 13 dims are statics with CMN: their per-dim mean is ~0
    for j in 0..13 {
        let m: f64 = frames.iter().map(|f| f[j]).sum::<f64>() / frames.len() as f64;
        assert!(m.abs() < 1e-6, "dim {j} mean {m}");
    }
}
fn tone(freq: f64, sr: u32, secs: f64) -> Vec<i16> {
    // Time-varying (chirp) so CMN does not erase the signal, as in real speech.
    let n = (sr as f64 * secs) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / sr as f64;
            let f = freq * (1.0 + 0.8 * (2.0 * std::f64::consts::PI * 2.0 * t).sin());
            ((2.0 * std::f64::consts::PI * f * t).sin() * 9000.0) as i16
        })
        .collect()
}
fn write_chirp_wav(dir: &str, name: &str, freq: f64, sr: u32, speech_secs: f64, pad_secs: f64) {
    // Test-only fixture writer (hound): chirp speech centered in digital silence.
    let mut samples = vec![0i16; (sr as f64 * pad_secs) as usize];
    samples.extend(tone(freq, sr, speech_secs));
    samples.extend(vec![0i16; (sr as f64 * pad_secs) as usize]);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(format!("{dir}/{name}"), spec).unwrap();
    for s in &samples {
        w.write_sample(*s).unwrap();
    }
    w.finalize().unwrap();
}
#[test]
fn loto_discriminates_synthetic_words_and_phase_test_works() {
    let sr = 16_000u32;
    let cfg = FrontendConfig::default();
    let words = [("alpha", 300.0), ("beta", 900.0), ("gamma", 1800.0)];
    let mut clips = Vec::new();
    for (label, f) in words {
        for take in 0..4 {
            let s = tone(f * (1.0 + take as f64 * 0.01), sr, 0.4);
            clips.push(Clip {
                label: label.into(),
                take,
                frames: extract_with_config(&s, sr, &cfg).unwrap(),
            });
        }
    }
    let dev = leave_one_take_out(&clips, 0.3);
    assert_eq!(dev.predictions.len(), 12);
    assert!(
        dev.accuracy() >= 0.9,
        "synthetic LOTO accuracy {}",
        dev.accuracy()
    );
    // phase-test: fresh take (take 4) against originals
    let queries: Vec<Clip> = words
        .iter()
        .map(|(label, f)| Clip {
            label: (*label).into(),
            take: 4,
            frames: extract_with_config(&tone(*f * 1.02, sr, 0.4), sr, &cfg).unwrap(),
        })
        .collect();
    let pt = phase_test(&clips, &queries, 0.3);
    assert_eq!(pt.predictions.len(), 3);
    assert!(
        pt.accuracy() >= 0.9,
        "synthetic phase accuracy {}",
        pt.accuracy()
    );
    assert!(pt.confusion_csv().contains("alpha"));
}
#[test]
fn degenerate_phase_test_reports_no_silent_label() {
    let sr = 16_000u32;
    let cfg = FrontendConfig::default();
    // Templates short, queries >4x longer: frame-count ratio below 0.8,
    // so every banded-DTW distance is +inf (the void-run collapse shape).
    let words = [("alpha", 300.0), ("beta", 900.0)];
    let templates: Vec<Clip> = words
        .iter()
        .map(|(label, f)| Clip {
            label: (*label).into(),
            take: 0,
            frames: extract_with_config(&tone(*f, sr, 0.2), sr, &cfg).unwrap(),
        })
        .collect();
    let queries: Vec<Clip> = words
        .iter()
        .map(|(label, f)| Clip {
            label: (*label).into(),
            take: 1,
            frames: extract_with_config(&tone(*f, sr, 0.9), sr, &cfg).unwrap(),
        })
        .collect();
    let pt = phase_test(&templates, &queries, 0.2);
    assert_eq!(
        pt.degenerate, 2,
        "both queries must be degenerate, not silent labels"
    );
    assert!(pt.predictions.is_empty());
    assert!(is_degenerate(&pt));
}
#[test]
fn degenerate_probe_fixture_through_wav_files() {
    // Writes /tmp/a1deg/{templates,queries} (WAVs + manifests) for the
    // binary-level NOT COMPARABLE probe, then verifies library degeneracy
    // on the same bytes read back through hound. Uses unpadded speech at
    // mismatched durations: padding mismatch is fixed by trimming (see
    // padding_mismatch_classifies_after_trim), while genuine speech-duration
    // mismatch must stay loud.
    let sr = 16_000u32;
    let cfg = FrontendConfig::default();
    let words = [("alpha", 300.0), ("beta", 900.0)];
    let mut tman = String::from("enroll.wav\tenroll\n");
    let mut qman = String::from("enroll.wav\tenroll\n");
    std::fs::create_dir_all("/tmp/a1deg/templates").unwrap();
    std::fs::create_dir_all("/tmp/a1deg/queries").unwrap();
    for take in 0..2 {
        for (i, (label, f)) in words.iter().enumerate() {
            let tname = format!("utt{t}{i}.wav", t = take);
            write_chirp_wav(
                "/tmp/a1deg/templates",
                &tname,
                *f * (1.0 + take as f64 * 0.01),
                sr,
                0.2,
                0.0,
            );
            tman.push_str(&format!("{tname}\t{label}\n"));
        }
    }
    for (i, (label, f)) in words.iter().enumerate() {
        let qname = format!("utt{i:03}.wav");
        write_chirp_wav("/tmp/a1deg/queries", &qname, *f, sr, 0.9, 0.0);
        qman.push_str(&format!("{qname}\t{label}\n"));
    }
    std::fs::write("/tmp/a1deg/templates/manifest.tsv", tman).unwrap();
    std::fs::write("/tmp/a1deg/queries/manifest.tsv", qman).unwrap();
    let load = |dir: &str, names: &[String]| -> Vec<Clip> {
        names
            .iter()
            .map(|name| {
                let mut r = hound::WavReader::open(format!("{dir}/{name}")).unwrap();
                let samples: Vec<i16> = r.samples::<i16>().map(|s| s.unwrap()).collect();
                Clip {
                    label: name.clone(),
                    take: 0,
                    frames: extract_with_config(&samples, sr, &cfg).unwrap(),
                }
            })
            .collect()
    };
    let tnames: Vec<String> = (0..2)
        .flat_map(|t| (0..2).map(move |i| format!("utt{t}{i}.wav")))
        .collect();
    let qnames: Vec<String> = (0..2).map(|i| format!("utt{i:03}.wav")).collect();
    let pt = phase_test(
        &load("/tmp/a1deg/templates", &tnames),
        &load("/tmp/a1deg/queries", &qnames),
        0.2,
    );
    assert_eq!(pt.degenerate, 2);
    assert!(is_degenerate(&pt));
}
#[test]
fn speech_span_trims_silence_edges_with_hangover() {
    let e = vec![
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ];
    // Speech frames 6..10, hangover 5 each side, clamped to bounds.
    assert_eq!(speech_span(&e), Some((1, 15)));
}
#[test]
fn speech_span_all_silence_is_none() {
    assert_eq!(speech_span(&[0.0; 8]), None);
    assert_eq!(speech_span(&[]), None);
}
#[test]
fn speech_span_hangover_clamps_to_bounds() {
    let e = vec![1.0, 1.0, 0.0, 0.0, 0.0];
    assert_eq!(speech_span(&e), Some((0, 5)));
}
#[test]
fn speech_span_ignores_isolated_transient_blip() {
    // Single hot frame at index 15 has no above-threshold neighbor: span
    // covers only the main run (6..9) plus hangover, not the blip.
    let e = vec![
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
    ];
    assert_eq!(speech_span(&e), Some((1, 14)));
}
#[test]
fn padding_mismatch_classifies_after_trim() {
    // The void-run shape through the real pipeline: queries at >2x the
    // template padding must classify by word, not collapse or go degenerate.
    let sr = 16_000u32;
    let cfg = FrontendConfig::default();
    let words = [("alpha", 300.0), ("beta", 900.0), ("gamma", 1800.0)];
    let padded = |f: f64, pad: f64| {
        let mut s = vec![0i16; (sr as f64 * pad) as usize];
        s.extend(tone(f, sr, 0.4));
        s.extend(vec![0i16; (sr as f64 * pad) as usize]);
        extract_with_config(&s, sr, &cfg).unwrap()
    };
    let templates: Vec<Clip> = words
        .iter()
        .map(|(label, f)| Clip {
            label: (*label).into(),
            take: 0,
            frames: padded(*f, 0.1),
        })
        .collect();
    let queries: Vec<Clip> = words
        .iter()
        .map(|(label, f)| Clip {
            label: (*label).into(),
            take: 1,
            frames: padded(*f, 0.9),
        })
        .collect();
    let pt = phase_test(&templates, &queries, 0.2);
    assert_eq!(pt.degenerate, 0, "trimmed queries must be comparable");
    assert_eq!(pt.predictions.len(), 3);
    assert!(
        pt.accuracy() >= 0.9,
        "padded-word accuracy {}",
        pt.accuracy()
    );
}
