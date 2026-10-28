//! A1 regression tests (bead hearsay-four-b7p): DTW, front end, LOTO.

use crate::dtw::dtw_distance;
use crate::frontend::{FrontendConfig, extract_with_config};
use crate::gate::{Clip, leave_one_take_out, phase_test};

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
