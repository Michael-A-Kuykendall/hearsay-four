//! A1 acoustic gate runner (bead hearsay-four-b7p).
//! Usage: a1_gate [--run1 sessions/run1] [--phase-test-dir DIR] [--band 0.2]
//! DEV = leave-one-take-out on run1 (take = per-word occurrence index).
//! PHASE-TEST runs only if a fresh 5th-take dir with manifest is given AND exists;
//! otherwise it reports NOT RUN. Never fabricates audio.

use fse::frontend::{FrontendConfig, extract_with_config};
use fse::gate::{Clip, is_degenerate, leave_one_take_out_with_dtw, phase_test_with_dtw};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn read_wav(path: &Path) -> Result<(Vec<i16>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = r.spec();
    let samples: Vec<i16> = r
        .samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{e}"))?;
    // downmix to mono by taking first channel interleaving average is unnecessary for owner mono; if stereo, average
    let mono = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|c| (c.iter().map(|&x| x as i32).sum::<i32>() / c.len() as i32) as i16)
            .collect()
    } else {
        samples
    };
    Ok((mono, spec.sample_rate))
}

fn load_session(dir: &Path, cfg: &FrontendConfig) -> Result<Vec<Clip>, String> {
    let manifest = dir.join("manifest.tsv");
    let text =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut clips = Vec::new();
    for line in text.lines().skip(1) {
        let mut it = line.split('\t');
        let file = it.next().unwrap_or("");
        let label = it.next().unwrap_or("");
        if file == "enroll.wav" || label.is_empty() {
            continue;
        }
        let take = {
            let e = seen.entry(label.to_string()).or_insert(0);
            let t = *e;
            *e += 1;
            t
        };
        let wav = dir.join(file);
        if !wav.exists() {
            return Err(format!("MISSING {}", wav.display()));
        }
        let (samples, sr) = read_wav(&wav)?;
        let frames = extract_with_config(&samples, sr, cfg).map_err(|e| format!("{e}"))?;
        clips.push(Clip {
            label: label.to_string(),
            take,
            frames,
        });
    }
    Ok(clips)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Standalone formal-session validation: scores nothing, exits immediately.
    if let Some(pos) = args.iter().position(|a| a == "--validate-only") {
        match args.get(pos + 1).map(PathBuf::from) {
            Some(d) => match fse::instrument::validate_session(&d) {
                Ok(s) => {
                    println!(
                        "A1 INSTRUMENT {}: ACCEPT labels={} seed={} order_sha256={}",
                        d.display(),
                        s.labels,
                        s.seed,
                        s.order_sha256
                    );
                }
                Err(v) => {
                    println!("A1 INSTRUMENT {}: {v}", d.display());
                    std::process::exit(1);
                }
            },
            None => {
                println!("A1 INSTRUMENT: usage --validate-only <session-dir>");
                std::process::exit(2);
            }
        }
        return;
    }
    let get = |k: &str| {
        args.windows(2)
            .find(|w| w[0] == k)
            .map(|w| PathBuf::from(&w[1]))
    };
    let run1 = get("--run1").unwrap_or_else(|| PathBuf::from("sessions/run1"));
    let band: f64 = args
        .windows(2)
        .find(|w| w[0] == "--band")
        .map(|w| w[1].parse().unwrap_or(0.2))
        .unwrap_or(0.2);
    let dtw_variant = args
        .windows(2)
        .find(|w| w[0] == "--dtw-variant")
        .map(|w| match w[1].as_str() {
            "v1" => fse::dtw::DtwVariant::SymWeighted,
            "v2" => fse::dtw::DtwVariant::SlopeConstrained,
            "v4" => fse::dtw::DtwVariant::SymWeightedSlope,
            _ => fse::dtw::DtwVariant::Baseline,
        })
        .unwrap_or(fse::dtw::DtwVariant::Baseline);
    let cmvn = args.iter().any(|a| a == "--cmvn");
    let base_only = args.iter().any(|a| a == "--base-only");
    // Data existence audit (always first)
    let manifest_entries = std::fs::read_to_string(run1.join("manifest.tsv"))
        .map(|t| t.lines().count().saturating_sub(1))
        .unwrap_or(0);
    let present = std::fs::read_dir(&run1)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    println!(
        "A1 DATA AUDIT run1={} manifest_entries={} wav_present={}",
        run1.display(),
        manifest_entries,
        present
    );
    if present == 0 {
        println!("A1 DEV: NOT RUN (0 WAV files under {})", run1.display());
        println!("A1 PHASE-TEST: NOT RUN (no fresh 5th-take session; owner audio absent)");
        println!("A1 GATE: NOT RUN");
        return;
    }
    // DEV tuning grid
    let mut best: Option<(String, f64, fse::gate::EvalResult, FrontendConfig)> = None;
    let grid = FrontendConfig::tuning_grid();
    let grid: Vec<(String, FrontendConfig)> = if base_only {
        grid.into_iter().take(1).collect()
    } else {
        grid
    };
    for (name, mut cfg) in grid {
        if cmvn {
            cfg.cmvn = true;
        }
        match load_session(&run1, &cfg) {
            Ok(clips) => {
                if clips.len() < 2 {
                    println!("A1 DEV {name}: insufficient clips");
                    continue;
                }
                let res = leave_one_take_out_with_dtw(&clips, band, dtw_variant);
                if is_degenerate(&res) {
                    println!("A1 GATE: NOT COMPARABLE ({name} has degenerate queries)");
                    std::process::exit(1);
                }
                println!(
                    "A1 DEV {name}: top1={:.4} top3={:.4} top5={:.4} top10={:.4} n={}",
                    res.accuracy(),
                    res.topk_inclusion(3),
                    res.topk_inclusion(5),
                    res.topk_inclusion(10),
                    res.predictions.len()
                );
                if best
                    .as_ref()
                    .map(|(_, a, _, _)| res.accuracy() > *a)
                    .unwrap_or(true)
                {
                    best = Some((name, res.accuracy(), res, cfg));
                }
            }
            Err(e) => println!("A1 DEV {name}: ERROR {e}"),
        }
    }
    if let Some((name, acc, res, cfg)) = &best {
        println!("A1 DEV BEST {name} top1={acc:.4}");
        print!("{}", res.confusion_csv());
        let _ = cfg;
    }
    // PHASE-TEST: sealed formal scoring (bead kyz). Requires --frozen-config
    // + --out-dir; validates the session, scores with the frozen config only,
    // writes one score.log to a fresh dir (score-once). DEV grid above is
    // unaffected.
    if let Some(dir) = get("--phase-test-dir") {
        let frozen_s = args
            .windows(2)
            .find(|w| w[0] == "--frozen-config")
            .map(|w| w[1].clone())
            .filter(|s| !s.is_empty());
        let frozen_s = match frozen_s {
            Some(s) => s,
            None => {
                println!(
                    "A1 PHASE-TEST: REFUSED (phase-test scoring requires --frozen-config <string>)"
                );
                std::process::exit(1);
            }
        };
        let out = match get("--out-dir") {
            Some(o) => o,
            None => {
                println!(
                    "A1 PHASE-TEST: REFUSED (phase-test scoring requires --out-dir <fresh-dir>)"
                );
                std::process::exit(1);
            }
        };
        if !dir.exists() {
            println!("A1 PHASE-TEST: NOT RUN (dir absent: {})", dir.display());
            return;
        }
        let summary = match fse::instrument::validate_session(&dir) {
            Ok(s) => s,
            Err(v) => {
                println!("A1 PHASE-TEST: {v} (session rejected, scored nothing)");
                std::process::exit(1);
            }
        };
        if out.exists() {
            println!(
                "A1 PHASE-TEST: REFUSED (output dir {} exists; score-once, no re-score)",
                out.display()
            );
            std::process::exit(1);
        }
        let frozen = match fse::instrument::parse_frozen_config(&frozen_s) {
            Ok(f) => f,
            Err(v) => {
                println!("A1 PHASE-TEST: {v}");
                std::process::exit(1);
            }
        };
        let grid0 = FrontendConfig::tuning_grid()
            .into_iter()
            .next()
            .expect("tuning grid nonempty");
        if grid0.0 != frozen.base_name || grid0.0 != fse::instrument::FROZEN_BASE {
            println!(
                "A1 PHASE-TEST: REFUSED (frozen base {} != grid row 0 {}; grid renamed?)",
                frozen.base_name, grid0.0
            );
            std::process::exit(1);
        }
        let mut fcfg = grid0.1;
        if frozen.cmvn {
            fcfg.cmvn = true;
        }
        let templates = load_session(&run1, &fcfg).expect("templates");
        match load_session(&dir, &fcfg) {
            Ok(q) => {
                let r = phase_test_with_dtw(&templates, &q, frozen.band, frozen.variant);
                if is_degenerate(&r) {
                    println!("A1 GATE: NOT COMPARABLE (phase-test has degenerate queries)");
                    std::process::exit(1);
                }
                let commit = std::process::Command::new("git")
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "unknown (git unavailable)".to_string());
                let log = format!(
                    "scorer_commit={commit}\nfrozen_config={frozen_s}\ntemplates={}\nphase_test_dir={}\norder_sha256={}\norder_seed={}\ntop1={:.4}\ntop3={:.4}\ntop5={:.4}\ntop10={:.4}\nn={}\ndegenerate={}\nconfusion:\n{}",
                    run1.display(),
                    dir.display(),
                    summary.order_sha256,
                    summary.seed,
                    r.accuracy(),
                    r.topk_inclusion(3),
                    r.topk_inclusion(5),
                    r.topk_inclusion(10),
                    r.predictions.len(),
                    r.degenerate,
                    r.confusion_csv()
                );
                match fse::instrument::write_score(&out, &log) {
                    Ok(()) => println!(
                        "A1 PHASE-TEST top1={:.4} n={} frozen={} -> {}/score.log",
                        r.accuracy(),
                        r.predictions.len(),
                        frozen_s,
                        out.display()
                    ),
                    Err(v) => {
                        println!("A1 PHASE-TEST: {v}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => println!("A1 PHASE-TEST: NOT RUN ({e})"),
        }
    } else {
        println!("A1 PHASE-TEST: NOT RUN (no --phase-test-dir fresh 5th-take session supplied)");
    }
}
