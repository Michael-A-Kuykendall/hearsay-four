//! DTW with Sakoe-Chiba band and length-normalized cost (bead hearsay-four-b7p).

/// Euclidean frame distance.
pub fn frame_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// Length-normalized DTW cost. `band_frac` in (0,1]: half-width =
/// ceil(band_frac * max(n,m)); 1.0 = unconstrained. Returns inf if
/// the endpoint is outside the band or either sequence is empty.
pub fn dtw_distance(a: &[Vec<f64>], b: &[Vec<f64>], band_frac: f64) -> f64 {
    if a.is_empty() || b.is_empty() {
        return f64::INFINITY;
    }
    let (n, m) = (a.len(), b.len());
    let band = ((band_frac.clamp(0.01, 1.0)) * (n.max(m) as f64)).ceil() as isize;
    let mut prev = vec![f64::INFINITY; m];
    let mut cur = vec![f64::INFINITY; m];
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            if (i as isize - j as isize).abs() > band {
                cur[j] = f64::INFINITY;
                continue;
            }
            let cost = frame_distance(ai, bj);
            let best = if i == 0 && j == 0 {
                0.0
            } else {
                let mut bbest = f64::INFINITY;
                if i > 0 {
                    bbest = bbest.min(prev[j]);
                }
                if j > 0 {
                    bbest = bbest.min(cur[j - 1]);
                }
                if i > 0 && j > 0 {
                    bbest = bbest.min(prev[j - 1]);
                }
                if i == 0 && j > 0 {
                    bbest = cur[j - 1];
                }
                if j == 0 && i > 0 {
                    bbest = prev[j];
                }
                bbest
            };
            cur[j] = if best.is_infinite() && !(i == 0 && j == 0) {
                f64::INFINITY
            } else {
                cost + best
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let total = prev[m - 1];
    if total.is_infinite() {
        return f64::INFINITY;
    }
    total / (n + m) as f64
}
