//! A1 acoustic gate runner (bead hearsay-four-b7p).
//! Usage: a1_gate [--run1 sessions/run1] [--phase-test-dir DIR] [--band 0.2]
//! DEV = leave-one-take-out on run1 (take = per-word occurrence index).
//! PHASE-TEST runs only if a fresh 5th-take dir with manifest is given AND exists;
//! otherwise it reports NOT RUN. Never fabricates audio.

use fse::frontend::{FrontendConfig, extract_with_config};
use fse::gate::{Clip, leave_one_take_out, phase_test};
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
    for (name, cfg) in FrontendConfig::tuning_grid() {
        match load_session(&run1, &cfg) {
            Ok(clips) => {
                if clips.len() < 2 {
                    println!("A1 DEV {name}: insufficient clips");
                    continue;
                }
                let res = leave_one_take_out(&clips, band);
                println!(
                    "A1 DEV {name}: top1={:.4} n={}",
                    res.accuracy(),
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
    // PHASE-TEST: scored once, only with an explicit fresh session
    if let (Some(dir), Some((_, _, _, cfg))) = (get("--phase-test-dir"), &best) {
        if !dir.exists() {
            println!("A1 PHASE-TEST: NOT RUN (dir absent: {})", dir.display());
            return;
        }
        let templates = load_session(&run1, cfg).expect("templates");
        match load_session(&dir, cfg) {
            Ok(q) => {
                let r = phase_test(&templates, &q, band);
                println!(
                    "A1 PHASE-TEST top1={:.4} n={} gate90={}",
                    r.accuracy(),
                    r.predictions.len(),
                    r.accuracy() >= 0.90
                );
                print!("{}", r.confusion_csv());
            }
            Err(e) => println!("A1 PHASE-TEST: NOT RUN ({e})"),
        }
    } else {
        println!("A1 PHASE-TEST: NOT RUN (no --phase-test-dir fresh 5th-take session supplied)");
    }
}
