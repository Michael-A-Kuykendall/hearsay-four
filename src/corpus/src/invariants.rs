//! Canonical invariant message names for the corpus pipeline.
//!
//! These are the runtime invariants enforced inside `corpus` business logic
//! (see `ppt_invariant_guide.md`). The contract test for the corpus pipeline
//! (ul2.4) requires every one of these to be actively enforced.

/// Auth credentials always come from env/.env, never a literal in source.
pub const TOKEN_NEVER_HARDCODED: &str = "token_never_hardcoded";
/// Every decoded clip is canonicalized to PCM 16 kHz mono.
pub const DECODE_PRODUCES_PCM_16K: &str = "decode_produces_pcm_16k";
/// Manifest rows parse cleanly (non-empty text, positive duration where present).
pub const MANIFEST_WELLFORMED: &str = "manifest_wellformed";
/// Every train row has both an audio file and a transcript.
pub const AUDIO_TRANSCRIPT_PAIRED: &str = "audio_transcript_paired";
