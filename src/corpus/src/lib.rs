//! Corpus & Audio Pipeline (epic ul2).
//!
//! Acquires, decodes, and validates speech audio + transcript corpora for
//! Hearsay-IV from two pools:
//!
//! 1. **Local better-voice** clips (ul2.3 [`manifest`]) — referenced, never
//!    moved (`/home/michael/repos/better-voice`, see
//!    `docs/research/corpus/local-bettervoice-inventory.md`).
//! 2. **CourtListener** SCOTUS oral-argument audio + built-in STT transcripts
//!    (ul2.1 [`courtlistener`], see
//!    `docs/research/corpus/court-audio-transcript-sources.md`).
//!
//! Both pools normalize into canonical **PCM 16 kHz mono** (ul2.2 [`audio`]).
//! PPT/invariant coverage lives in [`tests`] (ul2.4).

pub mod audio;
pub mod courtlistener;
pub mod dotenv;
pub mod invariants;
pub mod manifest;

/// Reference better-voice root (also in [`manifest::DEFAULT_BETTER_VOICE_ROOT`]).
pub const BETTER_VOICE_ROOT: &str = manifest::DEFAULT_BETTER_VOICE_ROOT;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
