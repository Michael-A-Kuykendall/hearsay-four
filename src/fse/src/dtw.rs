//! DTW with Sakoe-Chiba band and length-normalized cost (bead hearsay-four-b7p).

/// Euclidean frame distance.
pub fn frame_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// DTW step-pattern/slope variants (bead t5i bounded baseline). Baseline is
/// unit steps with no slope limit; V1 doubles diagonal weight; V2 caps
/// consecutive horizontal/vertical runs at 2; V4 is V1+V2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtwVariant {
    Baseline,
    SymWeighted,
    SlopeConstrained,
    SymWeightedSlope,
}

/// Length-normalized DTW cost under an explicit step-pattern variant.
/// Baseline is bit-identical to the original recurrence: same predecessor
/// set, same band, same (n+m) normalizer.
pub fn dtw_distance_with(
    a: &[Vec<f64>],
    b: &[Vec<f64>],
    band_frac: f64,
    variant: DtwVariant,
) -> f64 {
    let sym = matches!(
        variant,
        DtwVariant::SymWeighted | DtwVariant::SymWeightedSlope
    );
    let slope = matches!(
        variant,
        DtwVariant::SlopeConstrained | DtwVariant::SymWeightedSlope
    );
    dtw_inner(a, b, band_frac, sym, slope)
}

fn dtw_inner(a: &[Vec<f64>], b: &[Vec<f64>], band_frac: f64, sym: bool, slope: bool) -> f64 {
    if a.is_empty() || b.is_empty() {
        return f64::INFINITY;
    }
    let (n, m) = (a.len(), b.len());
    let band = ((band_frac.clamp(0.01, 1.0)) * (n.max(m) as f64)).ceil() as isize;
    let mut prev = vec![f64::INFINITY; m];
    let mut cur = vec![f64::INFINITY; m];
    let mut prev_hr = vec![0u8; m];
    let mut prev_vr = vec![0u8; m];
    let mut cur_hr = vec![0u8; m];
    let mut cur_vr = vec![0u8; m];
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            if (i as isize - j as isize).abs() > band {
                cur[j] = f64::INFINITY;
                cur_hr[j] = 0;
                cur_vr[j] = 0;
                continue;
            }
            let cost = frame_distance(ai, bj);
            if i == 0 && j == 0 {
                cur[j] = cost;
                cur_hr[j] = 0;
                cur_vr[j] = 0;
                continue;
            }
            // Predecessor order (vertical, horizontal, diagonal) with strict
            // improvement: ties keep the earliest, matching the baseline.
            let mut best = f64::INFINITY;
            let mut nhr = 0u8;
            let mut nvr = 0u8;
            if i > 0 && (!slope || prev_vr[j] < 2) {
                let c = prev[j] + cost;
                if c < best {
                    best = c;
                    nvr = prev_vr[j].saturating_add(1);
                }
            }
            if j > 0 && (!slope || cur_hr[j - 1] < 2) {
                let c = cur[j - 1] + cost;
                if c < best {
                    best = c;
                    nhr = cur_hr[j - 1].saturating_add(1);
                    nvr = 0;
                }
            }
            if i > 0 && j > 0 {
                let w = if sym { 2.0 } else { 1.0 };
                let c = prev[j - 1] + w * cost;
                if c < best {
                    best = c;
                    nhr = 0;
                    nvr = 0;
                }
            }
            if best.is_infinite() {
                cur[j] = f64::INFINITY;
                cur_hr[j] = 0;
                cur_vr[j] = 0;
            } else {
                cur[j] = best;
                cur_hr[j] = nhr;
                cur_vr[j] = nvr;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
        std::mem::swap(&mut prev_hr, &mut cur_hr);
        std::mem::swap(&mut prev_vr, &mut cur_vr);
    }
    let total = prev[m - 1];
    if total.is_infinite() {
        return f64::INFINITY;
    }
    total / (n + m) as f64
}

/// Length-normalized DTW cost. `band_frac` in (0,1]: half-width =
/// ceil(band_frac * max(n,m)); 1.0 = unconstrained. Returns inf if
/// the endpoint is outside the band or either sequence is empty.
pub fn dtw_distance(a: &[Vec<f64>], b: &[Vec<f64>], band_frac: f64) -> f64 {
    dtw_distance_with(a, b, band_frac, DtwVariant::Baseline)
}
