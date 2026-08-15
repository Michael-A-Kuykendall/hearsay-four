//! Minimal `.env` loader (no external dependency).
//!
//! The CourtListener token and other secrets live in the gitignored repo-root
//! `.env` file. This loader is deliberately tiny: `KEY=VALUE` lines, `#`
//! comments, blank lines ignored, optional surrounding whitespace and quotes
//! stripped. Process environment takes precedence over the file.

use std::collections::HashMap;
use std::path::Path;

/// The repo root, derived from the crate's location at compile time
/// (`src/corpus` -> repo root). Used to locate the gitignored `.env`.
pub fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate must live at <repo>/src/<crate>")
}

/// Parse a `.env`-formatted file into key/value pairs.
pub fn parse_env(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim().to_string();
        let mut value = line[eq + 1..].trim().to_string();
        if value.len() >= 2 {
            let b = value.as_bytes();
            if (b[0] == b'"' && b[b.len() - 1] == b'"')
                || (b[0] == b'\'' && b[b.len() - 1] == b'\'')
            {
                value = value[1..value.len() - 1].to_string();
            }
        }
        map.insert(key, value);
    }
    map
}

/// Read the repo-root `.env` file (if present) into a map.
pub fn load_repo_env() -> HashMap<String, String> {
    let path = repo_root().join(".env");
    std::fs::read_to_string(&path)
        .map(|content| parse_env(&content))
        .unwrap_or_default()
}

/// Look up a variable: process environment first, then repo `.env` file.
pub fn get_var(name: &str) -> Option<String> {
    if let Some(v) = std::env::var(name).ok().filter(|v| !v.is_empty()) {
        return Some(v);
    }
    load_repo_env().get(name).cloned().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_handles_comments_quotes_whitespace() {
        let map = parse_env("# comment\nA=1\nB = \"two words\"\nC='x'\nD=\n  \nBAD LINE\n");
        assert_eq!(map.get("A").map(String::as_str), Some("1"));
        assert_eq!(map.get("B").map(String::as_str), Some("two words"));
        assert_eq!(map.get("C").map(String::as_str), Some("x"));
        assert!(!map.contains_key("D") || map.get("D").unwrap().is_empty());
        assert_eq!(map.len(), 4);
    }
}
