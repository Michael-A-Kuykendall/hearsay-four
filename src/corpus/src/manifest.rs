//! Better-voice local pool manifest reader (ul2.3).
//!
//! Reads the pre-existing TSV manifests under the better-voice checkout
//! (sibling `../better-voice`, or `$BETTER_VOICE_ROOT`; see
//! `docs/research/corpus/local-bettervoice-inventory.md` §1–3) into a
//! unified typed manifest. **Audio is referenced, never copied** — rows carry
//! resolved absolute paths into the better-voice tree.
//!
//! Sources (all paths relative to the better-voice root):
//! - `data/phrase_training/train_phrases.tsv` — 3500 rows, `audio_path<TAB>text`
//!   (no header, no duration column)
//! - `data/bulk_tts_1k/train_1k.tsv` + `val_1k.tsv` — header
//!   `audio_path<TAB>text<TAB>duration`
//! - `data/quality_checked_1k/train.tsv` (900) + `val.tsv` (100) — columns
//!   `audio_path<TAB>duration<TAB>text` (no header, duration is middle column)
//!
//! The three sources differ in column order and header presence, so each is
//! parsed by its own routine.

use std::path::{Path, PathBuf};

use invariant_ppt::assert_invariant;

use crate::dotenv;
use crate::invariants::{AUDIO_TRANSCRIPT_PAIRED, MANIFEST_WELLFORMED};

/// Env var overriding the better-voice root.
pub const BETTER_VOICE_ROOT_ENV: &str = "BETTER_VOICE_ROOT";
/// Alternate env var overriding the better-voice root (D1 scripts use this name).
pub const HEARSAY_BETTER_VOICE_ROOT_ENV: &str = "HEARSAY_BETTER_VOICE_ROOT";
/// Default better-voice checkout, resolved against the workspace root: the
/// `better-voice` sibling of this repo (audited 2026-08-15). Never a
/// host-absolute path, so fresh clones resolve without edits.
pub const DEFAULT_BETTER_VOICE_ROOT: &str = "../better-voice";

/// Which TSV source a row came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorpusSource {
    /// `phrase_training/train_phrases.tsv` — 16 kHz PCM WAV phrases.
    PhraseTraining,
    /// `bulk_tts_1k/train_1k.tsv` + `val_1k.tsv` — 16 kHz PCM word clips.
    BulkTts,
    /// `quality_checked_1k/train.tsv` + `val.tsv` — MP3-formatted legal words.
    QualityChecked,
}

impl CorpusSource {
    pub fn label(self) -> &'static str {
        match self {
            CorpusSource::PhraseTraining => "phrase_training",
            CorpusSource::BulkTts => "bulk_tts",
            CorpusSource::QualityChecked => "quality_checked",
        }
    }
}

/// Train/validation split, where the source has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Split {
    Train,
    Val,
}

impl Split {
    pub fn label(self) -> &'static str {
        match self {
            Split::Train => "train",
            Split::Val => "val",
        }
    }
}

/// One manifest row: a referenced audio clip plus its transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct CorpusRow {
    pub source: CorpusSource,
    /// `Some` for sources with a train/val split.
    pub split: Option<Split>,
    /// Resolved absolute path to the audio file (never copied into this repo).
    pub audio_path: PathBuf,
    /// The transcript text for the clip.
    pub text: String,
    /// Duration in seconds; `None` when the source TSV lacks the column.
    pub duration: Option<f32>,
}

/// The unified corpus manifest (ul2.3 output).
#[derive(Debug, Clone, Default)]
pub struct CorpusManifest {
    /// Better-voice root all relative paths resolved against.
    pub root: PathBuf,
    pub rows: Vec<CorpusRow>,
}

impl CorpusManifest {
    /// Workspace root (`<repo>`), derived from this crate's manifest dir.
    pub fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Resolve the better-voice root: `$BETTER_VOICE_ROOT` (or
    /// `$HEARSAY_BETTER_VOICE_ROOT`), else the `better-voice` sibling of the
    /// workspace root. Never a host-absolute path.
    pub fn root() -> PathBuf {
        dotenv::get_var(BETTER_VOICE_ROOT_ENV)
            .or_else(|| dotenv::get_var(HEARSAY_BETTER_VOICE_ROOT_ENV))
            .map(PathBuf::from)
            .unwrap_or_else(|| Self::workspace_root().join(DEFAULT_BETTER_VOICE_ROOT))
    }

    /// True when the resolved root holds the TSV manifests `load()` needs.
    /// Tests skip (never fail) when this is false: a fresh clone has no
    /// better-voice sibling and no env override.
    pub fn is_available() -> bool {
        let root = Self::root();
        root.join("data/phrase_training/train_phrases.tsv")
            .is_file()
            && root.join("data/bulk_tts_1k/train_1k.tsv").is_file()
            && root.join("data/quality_checked_1k/train.tsv").is_file()
    }

    /// Load every TSV source into one manifest.
    pub fn load() -> Result<CorpusManifest, String> {
        Self::load_from(&Self::root())
    }

    /// Load from an explicit root (testable against any checkout).
    pub fn load_from(root: &Path) -> Result<CorpusManifest, String> {
        let mut manifest = CorpusManifest {
            root: root.to_path_buf(),
            rows: Vec::new(),
        };
        manifest.load_phrase_training()?;
        manifest.load_bulk_tts()?;
        manifest.load_quality_checked()?;
        Ok(manifest)
    }

    /// Rows from one source (and optional split).
    pub fn rows_from(&self, source: CorpusSource, split: Option<Split>) -> Vec<&CorpusRow> {
        self.rows
            .iter()
            .filter(|r| r.source == source && (split.is_none() || r.split == split))
            .collect()
    }

    fn load_phrase_training(&mut self) -> Result<(), String> {
        let rel = "data/phrase_training/train_phrases.tsv";
        let content = read_tsv(&self.root, rel)?;
        for line in content.lines().filter(|l| !l.trim().is_empty()) {
            let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
            if cols.len() < 2 {
                return Err(format!("{rel}: malformed row: {line:?}"));
            }
            let path = resolve_audio_path(&self.root, cols[0]);
            let row = CorpusRow {
                source: CorpusSource::PhraseTraining,
                split: None,
                audio_path: path,
                text: cols[1].to_string(),
                duration: None,
            };
            self.push_checked(row);
        }
        Ok(())
    }

    fn load_bulk_tts(&mut self) -> Result<(), String> {
        for (file, split) in [
            ("data/bulk_tts_1k/train_1k.tsv", Split::Train),
            ("data/bulk_tts_1k/val_1k.tsv", Split::Val),
        ] {
            let content = read_tsv(&self.root, file)?;
            let mut lines = content.lines().filter(|l| !l.trim().is_empty()).peekable();
            let first = lines.peek().map(|l| l.split('\t').collect::<Vec<_>>());
            let skip_header = first
                .map(|cols| cols[0].trim().eq_ignore_ascii_case("audio_path"))
                .unwrap_or(false);
            for line in lines.skip(if skip_header { 1 } else { 0 }) {
                let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
                if cols.len() < 2 {
                    return Err(format!("{file}: malformed row: {line:?}"));
                }
                let row = CorpusRow {
                    source: CorpusSource::BulkTts,
                    split: Some(split),
                    audio_path: resolve_audio_path(&self.root, cols[0]),
                    text: cols[1].to_string(),
                    duration: cols.get(2).and_then(|d| d.parse::<f32>().ok()),
                };
                self.push_checked(row);
            }
        }
        Ok(())
    }

    fn load_quality_checked(&mut self) -> Result<(), String> {
        for (file, split) in [
            ("data/quality_checked_1k/train.tsv", Split::Train),
            ("data/quality_checked_1k/val.tsv", Split::Val),
        ] {
            let content = read_tsv(&self.root, file)?;
            for line in content.lines().filter(|l| !l.trim().is_empty()) {
                let cols: Vec<&str> = line.split('\t').map(str::trim).collect();
                if cols.len() < 3 {
                    return Err(format!("{file}: malformed row: {line:?}"));
                }
                let row = CorpusRow {
                    source: CorpusSource::QualityChecked,
                    split: Some(split),
                    audio_path: resolve_audio_path(&self.root, cols[0]),
                    text: cols[2].to_string(),
                    duration: cols[1].parse::<f32>().ok(),
                };
                self.push_checked(row);
            }
        }
        Ok(())
    }

    /// Push a row after enforcing the manifest invariants.
    fn push_checked(&mut self, row: CorpusRow) {
        let wellformed = !row.text.trim().is_empty()
            && !row.audio_path.as_os_str().to_string_lossy().is_empty()
            && row.duration.is_none_or(|d| d > 0.0)
            && row.audio_path.extension().is_some_and(|e| !e.is_empty());
        assert_invariant(
            wellformed,
            MANIFEST_WELLFORMED,
            Some(&format!("{}:{}", row.source.label(), row.text)),
        );

        // Train rows must pair an existing audio file with a transcript.
        let paired = !row.text.trim().is_empty()
            && row.audio_path.is_file()
            && row.audio_path.file_name().is_some();
        assert_invariant(
            paired,
            AUDIO_TRANSCRIPT_PAIRED,
            Some(&format!("{}:{}", row.source.label(), row.text)),
        );

        self.rows.push(row);
    }
}

fn read_tsv(root: &Path, rel: &str) -> Result<String, String> {
    let path = root.join(rel);
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Resolve a TSV audio path (backslash-separated, possibly absolute) to an
/// absolute path under the root.
fn resolve_audio_path(root: &Path, raw: &str) -> PathBuf {
    let normalized = raw.replace('\\', "/");
    let p = PathBuf::from(&normalized);
    if p.is_absolute() { p } else { root.join(p) }
}

// ---- LibriSpeech TSV sampling (stratified tuning subsets) ----

/// One row of a LibriSpeech-style 4-column TSV (id, relpath, seconds, transcript).
#[derive(Debug, Clone, PartialEq)]
pub struct LibriRow {
    pub id: String,
    pub rel: String,
    pub sec: f64,
    pub text: String,
}

/// Parse 4-col TSV; skips blank lines and rows with empty id/rel or
/// non-positive seconds (same guards as the training/eval consumers).
pub fn parse_librispeech_tsv(text: &str) -> Vec<LibriRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(4, '\t');
        let (id, rel, sec, txt) = (
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
            cols.next().unwrap_or("").to_string(),
        );
        if id.is_empty() || rel.is_empty() || sec <= 0.0 {
            continue;
        }
        rows.push(LibriRow {
            id,
            rel,
            sec,
            text: txt,
        });
    }
    rows
}

fn xorshift64star(state: &mut u64) -> u64 {
    // xorshift64* (Vigna): deterministic cross-platform RNG for seeded sampling.
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}

/// Stratified sample: `per_speaker` rows per speaker (id prefix before the
/// first '-'), Fisher-Yates with the given seed. Speakers iterate in sorted
/// order; short speakers contribute all their rows.
pub fn sample_stratified(rows: &[LibriRow], per_speaker: usize, seed: u64) -> Vec<LibriRow> {
    use std::collections::BTreeMap;
    let mut by_spk: BTreeMap<&str, Vec<&LibriRow>> = BTreeMap::new();
    for r in rows {
        let spk = r.id.split('-').next().unwrap_or("");
        by_spk.entry(spk).or_default().push(r);
    }
    let mut rng = seed | 1; // xorshift degenerates on 0; force nonzero
    let mut out = Vec::new();
    for (_, mut v) in by_spk {
        for i in (1..v.len()).rev() {
            let j = (xorshift64star(&mut rng) % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
        out.extend(v.into_iter().take(per_speaker).cloned());
    }
    out
}
