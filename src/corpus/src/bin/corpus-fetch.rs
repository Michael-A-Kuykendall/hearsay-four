//! corpus-fetch: download CourtListener oral-argument audio + transcripts.
//!
//! Bead hearsay-four-79z. Walks cursor pagination for a court filter
//! (`docket__court=scotus`), downloads `local_path_mp3` (fallback
//! `download_url`) into `data/corpus/court/<court>/<id>/`, and writes
//! `transcript.txt` + `transcript.json` (STT transcript, duration, docket
//! metadata) plus `manifest.json` (via `save_object_manifest`) per object.
//! Re-running skips ids whose three artifacts already exist (idempotent).
//! Audio payloads stay out of git (`data/corpus/raw/` gitignored). The API
//! token is read from the environment (never committed, never printed).
//!
//! Reference: docs/research/corpus/court-audio-transcript-sources.md.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use corpus::courtlistener::{AudioObject, CourtListenerClient, save_object_manifest};

fn usage() -> &'static str {
    "usage: corpus-fetch [--court scotus] [--max 5] [--out <dir>]"
}

/// Candidate audio payload URLs, best first: the enhanced CourtListener MP3
/// copy (`local_path_mp3`, served from the storage host), then the source
/// `download_url` (usually an absolute court-site URL).
fn audio_urls(obj: &AudioObject) -> Vec<String> {
    let enhanced = obj
        .local_path_mp3
        .as_deref()
        .map(str::trim)
        .filter(|local| !local.is_empty())
        .map(|local| {
            if local.starts_with("http://") || local.starts_with("https://") {
                local.to_string()
            } else {
                let path = local.strip_prefix('/').unwrap_or(local);
                format!("https://storage.courtlistener.com/{path}")
            }
        });
    let direct = obj
        .download_url
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && (d.starts_with("http://") || d.starts_with("https://")))
        .map(str::to_string);
    enhanced.into_iter().chain(direct).collect()
}

fn write_text(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Fetch one audio object into `court_dir/<id>/`. Returns true when the
/// object was downloaded, false when it was already present (skipped).
fn fetch_object(
    client: &CourtListenerClient,
    obj: &AudioObject,
    court_dir: &Path,
) -> Result<bool, String> {
    let obj_dir = court_dir.join(obj.id.to_string());
    let mp3_path = obj_dir.join("audio.mp3");
    let txt_path = obj_dir.join("transcript.txt");
    let manifest_path = obj_dir.join("manifest.json");
    if mp3_path.exists() && txt_path.exists() && manifest_path.exists() {
        return Ok(false);
    }
    let urls = audio_urls(obj);
    if urls.is_empty() {
        return Err(format!(
            "object {}: no audio URL (no local_path_mp3/download_url)",
            obj.id
        ));
    }
    // Try candidates in order (enhanced copy first, source URL fallback).
    let mut bytes: Option<Vec<u8>> = None;
    let mut last_err = String::new();
    for url in &urls {
        match client.download_bytes(url) {
            Ok(b) if !b.is_empty() => {
                bytes = Some(b);
                break;
            }
            Ok(_) => last_err = format!("empty payload from {url}"),
            Err(e) => last_err = e,
        }
    }
    let bytes = bytes.ok_or_else(|| format!("object {}: {last_err}", obj.id))?;
    fs::create_dir_all(&obj_dir).map_err(|e| format!("mkdir {}: {e}", obj_dir.display()))?;
    fs::write(&mp3_path, &bytes).map_err(|e| format!("write {}: {e}", mp3_path.display()))?;
    write_text(&txt_path, obj.stt_transcript.as_deref().unwrap_or_default())?;
    let transcript_meta = serde_json::json!({
        "id": obj.id,
        "stt_transcript": obj.stt_transcript,
        "duration": obj.duration,
        "docket": obj.docket,
    });
    write_text(
        &obj_dir.join("transcript.json"),
        &serde_json::to_string_pretty(&transcript_meta).unwrap_or_default(),
    )?;
    // Manifest via the existing ingestor helper, renamed to manifest.json.
    save_object_manifest(obj, &obj_dir)?;
    let staged = obj_dir.join(format!("audio_{}.json", obj.id));
    fs::rename(&staged, &manifest_path).map_err(|e| format!("rename {}: {e}", staged.display()))?;
    Ok(true)
}

fn parse_args() -> Result<(String, usize, PathBuf), String> {
    let mut court = String::from("scotus");
    let mut max = 5usize;
    let mut out: Option<String> = None;
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--court" => {
                court = it.next().ok_or("--court needs a value")?;
            }
            "--max" => {
                let v = it.next().ok_or("--max needs a value")?;
                max = v
                    .parse::<usize>()
                    .map_err(|_| format!("--max needs a number, got {v}"))?;
            }
            "--out" => {
                out = Some(it.next().ok_or("--out needs a value")?);
            }
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            _ => return Err(format!("unknown arg: {arg}\n{}", usage())),
        }
    }
    if max == 0 {
        return Err("--max must be >= 1".to_string());
    }
    // Default out dir is repo-root-relative; the gate runs from src/corpus.
    let base = out
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../data/corpus"));
    Ok((court, max, base))
}

fn run() -> Result<(), String> {
    let (court, max, base) = parse_args()?;
    let client = CourtListenerClient::from_env()?;
    // Over-fetch pages slightly, then prefer transcript-bearing objects.
    let pages = max.div_ceil(5).max(2);
    let mut objects = client.list_audio(&court, pages)?;
    if objects.is_empty() {
        return Err(format!("no audio objects for court={court}"));
    }
    objects.sort_by_key(|o| !o.has_transcript());
    objects.truncate(max);
    let court_dir = base.join("court").join(&court);
    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;
    for obj in &objects {
        match fetch_object(&client, obj, &court_dir) {
            Ok(true) => downloaded += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                eprintln!("corpus-fetch: object {}: {e}", obj.id);
                errors += 1;
            }
        }
    }
    if downloaded == 0 && skipped == 0 {
        return Err(format!(
            "nothing fetched for court={court} ({errors} errors)"
        ));
    }
    println!(
        "fetch_summary: court={court} downloaded={downloaded} skipped={skipped} errors={errors}"
    );
    println!("fetch_dir: {}", court_dir.display());
    println!("fetch_done: exit ok");
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("corpus-fetch: error: {e}");
        std::process::exit(1);
    }
}
