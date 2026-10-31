//! A1 scorer forensics (item 3): per-config predictions, misses, margins, controls.
//! Usage: cargo run --release -p fse --example a1_probe -- [canon-dir] [band]
use std::collections::HashMap;
use std::path::PathBuf;

fn fnv_bytes(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn fnv_frames(frames: &[Vec<f64>]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= frames.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for row in frames {
        for &x in row {
            h ^= x.to_bits();
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn load_clips(dir: &str, cfg: &fse::frontend::FrontendConfig) -> Vec<fse::gate::Clip> {
    let session =
        corpus::canonical::load_session(&PathBuf::from(dir)).expect("load canon session");
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut clips = Vec::new();
    for take in &session.takes {
        let n = {
            let e = seen.entry(take.prompt.clone()).or_insert(0);
            let t = *e;
            *e += 1;
            t
        };
        let frames = fse::frontend::extract_with_config(
            &take.pcm,
            corpus::audio::TARGET_SAMPLE_RATE,
            cfg,
        )
        .expect("extract");
        clips.push(fse::gate::Clip {
            label: take.prompt.clone(),
            take: n,
            frames,
        });
    }
    clips
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sessions/canon/run1".to_string());
    let band: f64 = std::env::args()
        .nth(2)
        .map(|s| s.parse().expect("band"))
        .unwrap_or(0.2);
    let manifest_entries = std::fs::read_to_string(format!("{dir}/manifest.tsv"))
        .map(|t| t.lines().count().saturating_sub(1))
        .unwrap_or(0);
    let present = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    println!("PROBE dir={dir} manifest_entries={manifest_entries} wav_present={present} band={band}");

    let grid = fse::frontend::FrontendConfig::tuning_grid();
    for (name, cfg) in &grid {
        let clips = load_clips(&dir, cfg);
        // Hash of the exact matrices handed to the scorer.
        let mut fh: u64 = 0xcbf29ce484222325;
        for c in &clips {
            fh ^= fnv_frames(&c.frames);
            fh = fh.wrapping_mul(0x100000001b3);
        }
        let res = fse::gate::leave_one_take_out_with_dtw(
            &clips,
            band,
            fse::dtw::DtwVariant::Baseline,
        );
        let misses: Vec<String> = res
            .predictions
            .iter()
            .filter(|p| !p.correct)
            .map(|p| format!("t{}:{}->{}", p.take, p.truth, p.predicted))
            .collect();
        let mut ph: u64 = 0xcbf29ce484222325;
        for p in &res.predictions {
            ph = fnv_bytes(p.predicted.as_bytes(), ph);
        }
        println!(
            "{name} acc={:.4} n={} frames_hash={fh:016x} pred_hash={ph:016x}",
            res.accuracy(),
            res.predictions.len()
        );
        println!("  misses({}): {}", misses.len(), misses.join(" "));
    }

    // Margins for base config: best vs 2nd-best distance per query.
    let (_, base_cfg) = &grid[0];
    let clips = load_clips(&dir, base_cfg);
    let takes: std::collections::BTreeSet<usize> = clips.iter().map(|c| c.take).collect();
    let mut ratios: Vec<f64> = Vec::new();
    let mut finite_counts: Vec<usize> = Vec::new();
    let mut miss_margins: Vec<String> = Vec::new();
    for take in takes {
        let templates: Vec<&fse::gate::Clip> =
            clips.iter().filter(|c| c.take != take).collect();
        for q in clips.iter().filter(|c| c.take == take) {
            let mut ds: Vec<(f64, &str)> = templates
                .iter()
                .map(|t| {
                    (
                        fse::dtw::dtw_distance_with(
                            &q.frames,
                            &t.frames,
                            band,
                            fse::dtw::DtwVariant::Baseline,
                        ),
                        t.label.as_str(),
                    )
                })
                .filter(|(d, _)| d.is_finite())
                .collect();
            finite_counts.push(ds.len());
            let same_finite = ds.iter().filter(|(_, l)| *l == q.label).count();
            ds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let ratio = ds[1].0 / ds[0].0;
            ratios.push(ratio);
            if ds[0].1 != q.label || same_finite == 0 {
                miss_margins.push(format!(
                    "t{}:{}->{} qframes={} d1={:.3} d2={:.3} sameword_finite={same_finite}",
                    take, q.label, ds[0].1, q.frames.len(), ds[0].0, ds[1].0
                ));
            }
        }
    }
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    finite_counts.sort();
    println!(
        "FINITE per-query templates-of-90: min={} p10={} median={} max={}",
        finite_counts[0],
        finite_counts[finite_counts.len() / 10],
        finite_counts[finite_counts.len() / 2],
        finite_counts[finite_counts.len() - 1]
    );
    println!(
        "MARGINS base n={} min={:.4} p10={:.4} median={:.4} max={:.2}",
        ratios.len(),
        ratios[0],
        ratios[ratios.len() / 10],
        ratios[ratios.len() / 2],
        ratios[ratios.len() - 1]
    );
    for m in &miss_margins {
        println!("  miss-margin {m}");
    }

    // Control A: zeroed features. Scorer consuming features must collapse.
    let mut zeroed = load_clips(&dir, base_cfg);
    for c in &mut zeroed {
        for row in &mut c.frames {
            for x in row.iter_mut() {
                *x = 0.0;
            }
        }
    }
    let res_a = fse::gate::leave_one_take_out_with_dtw(
        &zeroed,
        band,
        fse::dtw::DtwVariant::Baseline,
    );
    println!(
        "CONTROL-A zeroed-features acc={:.4} n={}",
        res_a.accuracy(),
        res_a.predictions.len()
    );
    // Debug: fold structure + per-fold prediction constancy under zeroed feats.
    let mut take_hist: HashMap<usize, usize> = HashMap::new();
    for c in &zeroed {
        *take_hist.entry(c.take).or_insert(0) += 1;
    }
    let mut folds: Vec<usize> = take_hist.keys().cloned().collect();
    folds.sort();
    println!("DEBUG folds={folds:?} take_hist={take_hist:?}");
    let order: Vec<String> = zeroed
        .iter()
        .take(12)
        .map(|c| format!("{}(t{})", c.label, c.take))
        .collect();
    println!("DEBUG order12={}", order.join(" "));
    for f in &folds {
        let mut preds: HashMap<String, usize> = HashMap::new();
        for p in res_a.predictions.iter().filter(|p| p.take == *f) {
            *preds.entry(p.predicted.clone()).or_insert(0) += 1;
        }
        println!("DEBUG fold{f} n={} distinct_preds={preds:?}", preds.values().sum::<usize>());
    }

    // Control B: rotated labels (content intact, identities wrong).
    let mut shuffled = clips;
    let n = shuffled.len();
    let labels: Vec<String> = shuffled.iter().map(|c| c.label.clone()).collect();
    for (i, c) in shuffled.iter_mut().enumerate() {
        c.label = labels[(i + 1) % n].clone();
    }
    let res_b = fse::gate::leave_one_take_out_with_dtw(
        &shuffled,
        band,
        fse::dtw::DtwVariant::Baseline,
    );
    println!(
        "CONTROL-B shuffled-labels acc={:.4} n={}",
        res_b.accuracy(),
        res_b.predictions.len()
    );
}
