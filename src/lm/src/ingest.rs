//! Partition-guarded ingestion (bead hearsay-four-baj).
//!
//! Only files listed in `data/partitions/lm-train-text.json` (D1,
//! bead hearsay-four-7ms) may be ingested. Every listed path is checked
//! against the DEV / PHASE-TEST / LOCKBOX manifests: any overlap is an
//! error, so LM text can never contain DEV/PHASE-TEST/LOCKBOX
//! transcripts. D0 use-verification for these sets is recorded in
//! `docs/results/D0-DATA-INVENTORY.md` (free, local, no account/key).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One ingested source: partition-relative path plus extracted raw text lines.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestedSource {
    pub path: String,
    pub dataset: String,
    pub lines: Vec<String>,
}

fn manifest_paths(manifest: &Path) -> Result<BTreeSet<String>, String> {
    let raw = std::fs::read_to_string(manifest)
        .map_err(|e| format!("read {}: {e}", manifest.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", manifest.display()))?;
    let mut out = BTreeSet::new();
    if let Some(entries) = v.get("entries").and_then(|e| e.as_array()) {
        for e in entries {
            if let Some(p) = e.get("path").and_then(|p| p.as_str()) {
                out.insert(p.to_string());
            }
        }
    }
    Ok(out)
}

/// Extract raw text lines from one LM-train file by extension.
/// TSV: the `text` column when a header names it, else the last column.
/// JSON: Oyez `transcript.sections[].turns[].text_blocks[].text`.
pub fn extract_text_lines(path: &Path) -> Result<Vec<String>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if path.extension().is_some_and(|e| e == "json") {
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let mut lines = Vec::new();
        if let Some(sections) = v.pointer("/transcript/sections").and_then(|s| s.as_array()) {
            for s in sections {
                if let Some(turns) = s.get("turns").and_then(|t| t.as_array()) {
                    for t in turns {
                        if let Some(blocks) = t.get("text_blocks").and_then(|b| b.as_array()) {
                            for b in blocks {
                                if let Some(txt) = b.get("text").and_then(|x| x.as_str()) {
                                    lines.push(txt.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        return Ok(lines);
    }
    // TSV (tab-separated; also tolerates single-column plain text).
    let mut lines = Vec::new();
    let mut text_col: Option<usize> = None;
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if i == 0 {
            if let Some(pos) = cols.iter().position(|c| *c == "text") {
                text_col = Some(pos);
                continue; // header row
            }
            if cols.contains(&"audio_path") {
                continue; // header without a text column position found above
            }
        }
        let idx = text_col.unwrap_or(cols.len() - 1);
        if let Some(t) = cols.get(idx)
            && !t.trim().is_empty()
        {
            lines.push((*t).to_string());
        }
    }
    Ok(lines)
}

/// Load every LM-train text source allowed by D1.
/// `repo_root` holds `data/partitions/` and `sessions/`; `better_voice_root`
/// holds `data/...` for `root == "better-voice"` entries.
pub fn load_lm_train_sources(
    repo_root: &Path,
    better_voice_root: &Path,
) -> Result<Vec<IngestedSource>, String> {
    let part_dir = repo_root.join("data/partitions");
    let lm_raw = std::fs::read_to_string(part_dir.join("lm-train-text.json"))
        .map_err(|e| format!("read lm-train-text.json: {e}"))?;
    let lm: serde_json::Value =
        serde_json::from_str(&lm_raw).map_err(|e| format!("parse lm-train-text.json: {e}"))?;
    let mut forbidden = BTreeSet::new();
    for name in ["dev.json", "phase-test.json", "lockbox.json"] {
        for p in manifest_paths(&part_dir.join(name))? {
            forbidden.insert(p);
        }
    }
    let entries = lm
        .get("entries")
        .and_then(|e| e.as_array())
        .ok_or("lm-train-text.json: missing entries")?;
    let mut out = Vec::new();
    for e in entries {
        let path = e
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or("lm-train entry missing path")?;
        if forbidden.contains(path) {
            return Err(format!(
                "LM-train entry {path} also appears in DEV/PHASE-TEST/LOCKBOX partition"
            ));
        }
        let dataset = e.get("dataset").and_then(|d| d.as_str()).unwrap_or("");
        let root = e.get("root").and_then(|r| r.as_str()).unwrap_or("repo");
        let full: PathBuf = match root {
            "better-voice" => better_voice_root.join(path),
            _ => repo_root.join(path),
        };
        let lines = extract_text_lines(&full)?;
        out.push(IngestedSource {
            path: path.to_string(),
            dataset: dataset.to_string(),
            lines,
        });
    }
    Ok(out)
}
