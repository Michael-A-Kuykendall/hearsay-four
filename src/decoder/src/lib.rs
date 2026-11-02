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
//! WAV → frontend → trie search → A5 lattice.
//!
//! R5a large-vocab path: [`vocab`] builds the trie word list (LM top-N by
//! frequency plus book words, tiered-lexicon pronunciations) and the search
//! applies trie-node LM lookahead in pruning (stored scores stay exact).
//! Follow-ups: N-best lattice with alternates, calibrated `acoustic_llr`,
//! incremental streaming, silence modeling.

use lattice::Lattice;

pub mod moore;
pub mod trie;
pub mod vocab;

pub use moore::{DecoderConfig, MooreDecoder};
pub use trie::{PronEntry, PronTrie};
pub use vocab::{
    BookSource, VocabReport, book_types_from_personal, book_types_from_text, build_entries,
    top_lm_words,
};

/// Frozen decoder trait (interfaces-v1 §I4).
///
/// `decode` takes canonical 16 kHz mono PCM and returns a word lattice.
/// `stream` drives live/VAD use: each call feeds one chunk and returns the
/// current partial hypotheses.
///
/// Chunking/latency contract (v0, see `MooreDecoder::stream`): a chunk is
/// any-length 16 kHz mono PCM appended to an internal buffer; each call
/// re-decodes the whole buffer (per-chunk latency ~one batch decode of
/// the audio so far, total work quadratic). Returns exactly one partial:
/// the current 1-best. Callers reset between utterances so no audio or
/// CMVN state leaks across them.
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
