//! hearsay-bridge: decode one lattice file through the decision fabric.
//!
//! ```sh
//! hearsay-bridge --lattice <utt.lat.jsonl> --lm <lm.json> --out <dir>
//! ```
//!
//! Writes `<utterance>.transcript.txt` (space-joined winner words) and
//! `<utterance>.events.jsonl` (v1 event log, one [`DecisionEvent`] per
//! line) into `--out`. Exit 0 on success, 1 with a stderr message on any
//! failure. Utterance ids are sanitized to `[A-Za-z0-9._-]` for filenames.

use fabric::decision::DecisionParams;
use hearsay::{InterpolatedScorer, decode_lattice};
use lattice::read_lattice;
use lm::persist::load_interpolated;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> &'static str {
    "usage: hearsay-bridge --lattice <utt.lat.jsonl> --lm <lm-interpolated.json> --out <dir>"
}

fn take_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}\n{usage}", usage = usage()))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn run(args: &[String]) -> Result<String, String> {
    let mut lattice_path: Option<PathBuf> = None;
    let mut lm_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lattice" => lattice_path = Some(PathBuf::from(take_value(args, &mut i, "--lattice")?)),
            "--lm" => lm_path = Some(PathBuf::from(take_value(args, &mut i, "--lm")?)),
            "--out" => out_dir = Some(PathBuf::from(take_value(args, &mut i, "--out")?)),
            "--help" | "-h" => return Ok(usage().to_string()),
            other => return Err(format!("unknown argument {other:?}\n{}", usage())),
        }
        i += 1;
    }
    let lattice_path = lattice_path.ok_or_else(|| format!("missing --lattice\n{}", usage()))?;
    let lm_path = lm_path.ok_or_else(|| format!("missing --lm\n{}", usage()))?;
    let out_dir = out_dir.ok_or_else(|| format!("missing --out\n{}", usage()))?;

    let text = std::fs::read_to_string(&lattice_path)
        .map_err(|e| format!("read {}: {e}", lattice_path.display()))?;
    let lat = read_lattice(&text)?;
    let lm = load_interpolated(Path::new(&lm_path))?;
    let scorer = InterpolatedScorer::new(&lm);
    let outcome = decode_lattice(&lat, &scorer, DecisionParams::default())?;

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let stem = sanitize(&outcome.utterance);
    let transcript_path = out_dir.join(format!("{stem}.transcript.txt"));
    let events_path = out_dir.join(format!("{stem}.events.jsonl"));
    std::fs::write(&transcript_path, format!("{}\n", outcome.transcript.join(" ")))
        .map_err(|e| format!("write {}: {e}", transcript_path.display()))?;
    std::fs::write(&events_path, &outcome.events_jsonl)
        .map_err(|e| format!("write {}: {e}", events_path.display()))?;
    Ok(format!(
        "{}: {} words, {} commits, {} event lines -> {}",
        outcome.utterance,
        outcome.transcript.len(),
        outcome.commits.len(),
        outcome.events_jsonl.lines().count(),
        out_dir.display()
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hearsay-bridge: {e}");
            ExitCode::FAILURE
        }
    }
}
