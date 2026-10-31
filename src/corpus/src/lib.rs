//! Corpus & Audio Pipeline (epic ul2).
//!
//! Acquires, decodes, and validates speech audio + transcript corpora for
//! Hearsay-IV from two pools:
//!
//! 1. **Local better-voice** clips (ul2.3 [`manifest`]) — referenced, never
//!    moved (sibling checkout or `$BETTER_VOICE_ROOT`, see
//!    `docs/research/corpus/local-bettervoice-inventory.md`).
//! 2. **CourtListener** SCOTUS oral-argument audio + built-in STT transcripts
//!    (ul2.1 [`courtlistener`], see
//!    `docs/research/corpus/court-audio-transcript-sources.md`).
//!
//! Both pools normalize into canonical **PCM 16 kHz mono** (ul2.2 [`audio`]).
//! PPT/invariant coverage lives in [`tests`] (ul2.4).

pub mod audio;
pub mod canonical;
pub mod courtlistener;
pub mod dotenv;
pub mod ground_truth;
pub mod invariants;
pub mod manifest;

/// Reference better-voice root, relative to the workspace root
/// (see [`manifest::DEFAULT_BETTER_VOICE_ROOT`]).
pub const BETTER_VOICE_ROOT: &str = manifest::DEFAULT_BETTER_VOICE_ROOT;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
