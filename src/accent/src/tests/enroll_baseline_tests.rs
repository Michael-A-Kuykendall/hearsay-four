//! Enrollment-baseline wiring tests (bead fo7): the `--enroll` path builds
//! a per-speaker baseline through the real front end, distinct from the
//! generic reference — and empty enrollment fails loudly, never silently.

use crate::baseline_from_frames;
use calibration::{Baseline, FEATURE_DIM};

fn tone(len: usize) -> Vec<i16> {
    (0..len).map(|i| ((i % 97) as i16) * 173).collect()
}

#[test]
fn enroll_baseline_differs_from_generic() {
    let fitted = baseline_from_frames(&tone(48_000), 48_000).expect("enrollment fit");
    assert!(fitted.frame_count > 0);
    assert!(fitted.is_valid());
    let generic = Baseline {
        mean: vec![0.0; FEATURE_DIM],
        variance: vec![1.0; FEATURE_DIM],
        frame_count: 0,
    };
    assert_ne!(
        fitted.mean, generic.mean,
        "enrollment fit must not equal the generic reference"
    );
}

#[test]
fn empty_enrollment_fails_loudly() {
    assert!(baseline_from_frames(&[], 48_000).is_err());
}
