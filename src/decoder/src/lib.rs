//! R5a decoder skeleton: the frozen `Recognizer` trait plus a minimal
//! pronunciation-trie + token-passing implementor (`MooreDecoder`).
//!
//! Frozen contract: `docs/design/interfaces-v1.md` §I4.
//!
//! ```text
//! pub trait Recognizer {
//!     fn decode(&self, pcm16k: &[i16]) -> Lattice;
//!     fn stream(&mut self, chunk: &[i16]) -> Vec<Partial>;
//! }
//! ```
//!
//! Model inputs arrive by FILE PATH ONLY (acoustic `model.json`, optional LM
//! file, CMUdict text). This crate depends on the `hmm`/`lm` model-file
//! readers and scorers only — no training code — so models swap later by
//! file with no decoder change.
//!
//! v0 scope (Oct 31 plumbing checkpoint): single-best token passing over a
//! phone trie, linear (1-best) lattice emission. Word output today is
//! unchecked and may be garbage; what is proven is the end-to-end path
//! WAV → frontend → trie search → A5 lattice. Follow-ups: N-best lattice
//! with alternates, calibrated `acoustic_llr`, incremental streaming,
//! silence modeling, LM lookahead inside search.

use lattice::Lattice;

pub mod moore;
pub mod trie;

pub use moore::{DecoderConfig, MooreDecoder};
pub use trie::{PronEntry, PronTrie};

/// Frozen decoder trait (interfaces-v1 §I4).
///
/// `decode` takes canonical 16 kHz mono PCM and returns a word lattice.
/// `stream` drives live/VAD use: each call feeds one chunk and returns the
/// current partial hypotheses.
pub trait Recognizer {
    fn decode(&self, pcm16k: &[i16]) -> Lattice;
    fn stream(&mut self, chunk: &[i16]) -> Vec<Partial>;
}

/// One streaming hypothesis (v0 shape; L6 may extend with additive fields).
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    /// Current best word sequence, space-joined, lexicon case (UPPER).
    pub text: String,
    /// Last frame index covered by this partial (10 ms frames).
    pub frame_end: u32,
    /// Total path log-score (acoustic + LM + penalties).
    pub score: f64,
}
