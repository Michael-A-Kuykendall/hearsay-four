//! A1 acoustic gate runner (bead hearsay-four-b7p).
//! Usage: a1_gate [--run1 sessions/canon/run1] [--phase-test-dir DIR] [--band 0.2]
//!   [--phase-test-sealed-dir SEALED] (RS-1: validate seals on the raw session,
//!   score queries from the canonical session; defaults to --phase-test-dir)
//! DEV = leave-one-take-out on canonical run1 (take = per-word occurrence
//! index). RS-1 §5: owner audio ONLY via corpus::canonical::load_session.
//! PHASE-TEST runs only if a fresh 5th-take dir with manifest is given AND exists;
//! otherwise it reports NOT RUN. Never fabricates audio.

use fse::frontend::{FrontendConfig, extract_with_config};
use fse::gate::{Clip, is_degenerate, leave_one_take_out_with_dtw, phase_test_with_dtw};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn load_session(dir: &Path, cfg: &FrontendConfig) -> Result<Vec<Clip>, String> {
    // RS-1 §5: owner audio ONLY through the canonical loader. `dir` is a
    // canonical session (canon-session output), never a raw session.
    let session = corpus::canonical::load_session(dir).map_err(|e| e.to_string())?;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut clips = Vec::new();
    for take in &session.takes {
        let label = take.prompt.clone();
        let n = {
            let e = seen.entry(label.clone()).or_insert(0);
            let t = *e;
            *e += 1;
            t
        };
        let frames = extract_with_config(&take.pcm, corpus::audio::TARGET_SAMPLE_RATE, cfg)
            .map_err(|e| format!("{e}"))?;
        clips.push(Clip {
            label,
            take: n,
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
    let run1 = get("--run1").unwrap_or_else(|| PathBuf::from("sessions/canon/run1"));
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
    // unaffected. RS-1: seals are validated on --phase-test-sealed-dir (the
    // raw sealed session; defaults to --phase-test-dir) while queries are
    // loaded from --phase-test-dir (the canonical session). Both dirs are
    // recorded in score.log; scoring math is untouched.
    if let Some(dir) = get("--phase-test-dir") {
        let sealed = get("--phase-test-sealed-dir").unwrap_or_else(|| dir.clone());
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
        let summary = match fse::instrument::validate_session(&sealed) {
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
                    "scorer_commit={commit}\nfrozen_config={frozen_s}\ntemplates={}\nphase_test_dir={}\nphase_test_sealed_dir={}\norder_sha256={}\norder_seed={}\ntop1={:.4}\ntop3={:.4}\ntop5={:.4}\ntop10={:.4}\nn={}\ndegenerate={}\nconfusion:\n{}",
                    run1.display(),
                    dir.display(),
                    sealed.display(),
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
