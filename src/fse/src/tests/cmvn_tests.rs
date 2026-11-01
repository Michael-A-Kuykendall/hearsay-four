//! CMVN tests (hmm-fe2): batch normalization, A1-family floor, raw-path
//! guard, and live running estimator.

use crate::{
    apply_cmvn, cmvn_stats, extract_features, extract_features_cmvn, window_audio, CmvnLive,
    CMVN_STD_FLOOR,
};

/// Concatenated tone bursts with varying pitch/level (varying spectra, no model).
fn speechish() -> Vec<i16> {
    let rate: f64 = 16_000.0;
    let mut out = Vec::new();
    for (seg, &(freq, amp)) in [(440.0, 8_000.0), (220.0, 4_000.0), (880.0, 12_000.0)]
        .iter()
        .enumerate()
    {
        let n = (rate * 0.5).round() as usize;
        for i in 0..n {
            let t = (seg * n + i) as f64 / rate;
            out.push(
                (amp * (2.0 * std::f64::consts::PI * freq * t).sin()
                    + 500.0 * (2.0 * std::f64::consts::PI * 60.0 * t).sin()) as i16,
            );
        }
    }
    out
}

fn pop_mean_std(xs: &[f64]) -> (f64, f64) {
    let t = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / t;
    let v = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / t;
    (m, v.sqrt())
}

/// Batch CMVN: static dims come out ~zero mean, unit variance.
#[test]
fn cmvn_batch_zero_mean_unit_variance() {
    let windows = window_audio(&speechish(), 16_000);
    assert!(!windows.is_empty());
    let frames = extract_features_cmvn(&windows, 16_000).expect("cmvn extracts");
    assert_eq!(frames.len(), windows.len());
    for d in 0..13 {
        let col: Vec<f64> = frames.iter().map(|f| f[d]).collect();
        let (m, s) = pop_mean_std(&col);
        assert!(m.abs() < 1e-9, "dim {d} mean {m}");
        assert!((s - 1.0).abs() < 1e-6, "dim {d} std {s}");
    }
    assert!(frames.iter().all(|f| f.iter().all(|x| x.is_finite())));
}

/// A1 family: std floor is exactly 1e-6; constant signal normalizes to zeros.
#[test]
fn cmvn_floor_matches_a1_family() {
    assert_eq!(CMVN_STD_FLOOR, 1e-6);
    let windows = window_audio(&vec![4000i16; 16_000], 16_000);
    let frames = extract_features_cmvn(&windows, 16_000).expect("cmvn extracts");
    assert!(frames.iter().all(|f| f.iter().all(|x| x.is_finite())));
    for f in &frames {
        for v in f.iter().take(13) {
            assert!(
                v.abs() < 1e-6,
                "constant signal must center to fp noise, got {v}"
            );
        }
    }
}

/// Guard the split: the RAW extractor stays unnormalized (A1-adjacent
/// callers depend on it); only the cmvn entry normalizes.
#[test]
fn raw_extractor_has_no_cmvn() {
    let windows = window_audio(&speechish(), 16_000);
    let raw = extract_features(&windows, 16_000).expect("raw extracts");
    let energy: Vec<f64> = raw.iter().map(|f| f[12]).collect();
    let (m, _) = pop_mean_std(&energy);
    assert!(
        m > 1.0,
        "raw log-energy mean must stay far from zero, got {m}"
    );
    let norm = extract_features_cmvn(&windows, 16_000).expect("cmvn extracts");
    let energy_n: Vec<f64> = norm.iter().map(|f| f[12]).collect();
    let (mn, _) = pop_mean_std(&energy_n);
    assert!(
        mn.abs() < 1e-9,
        "cmvn log-energy mean must center, got {mn}"
    );
}

/// Live estimator converges to the batch stats on the same frames.
#[test]
fn live_matches_batch() {
    let windows = window_audio(&speechish(), 16_000);
    let raw = extract_features(&windows, 16_000).expect("raw extracts");
    let statics: Vec<Vec<f64>> = raw.iter().map(|f| f[..13].to_vec()).collect();
    let (bm, bs) = cmvn_stats(&statics);
    let mut live = CmvnLive::new();
    live.update(&statics);
    let (lm, ls) = live.stats().expect("stats after update");
    for d in 0..13 {
        assert!((bm[d] - lm[d]).abs() < 1e-9, "dim {d} mean drift");
        assert!((bs[d] - ls[d]).abs() < 1e-9, "dim {d} std drift");
    }
    let mut via_live = statics.clone();
    apply_cmvn(&mut via_live, &lm, &ls);
    let mut via_batch = statics.clone();
    apply_cmvn(&mut via_batch, &bm, &bs);
    for (a, b) in via_live.iter().zip(&via_batch) {
        for d in 0..13 {
            assert!((a[d] - b[d]).abs() < 1e-9, "apply path must match");
        }
    }
}

/// Live estimator is order-chunk invariant (two halves equal one whole).
#[test]
fn live_incremental_updates() {
    let windows = window_audio(&speechish(), 16_000);
    let raw = extract_features(&windows, 16_000).expect("raw extracts");
    let statics: Vec<Vec<f64>> = raw.iter().map(|f| f[..13].to_vec()).collect();
    let mut whole = CmvnLive::new();
    whole.update(&statics);
    let mut halves = CmvnLive::new();
    let mid = statics.len() / 2;
    halves.update(&statics[..mid]);
    halves.update(&statics[mid..]);
    let (wm, ws) = whole.stats().expect("whole");
    let (hm, hs) = halves.stats().expect("halves");
    for d in 0..13 {
        assert!((wm[d] - hm[d]).abs() < 1e-9, "dim {d} mean chunk drift");
        assert!((ws[d] - hs[d]).abs() < 1e-9, "dim {d} std chunk drift");
    }
}
