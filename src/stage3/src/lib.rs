//! Stage 3 — Local Commit Window (d73).
//!
//! In-progress crate. `d73.1` delivers the SPE phonological rule inventory
//! (content only); the FST-cascade execution engine, the Viterbi bounded
//! window, and commit/retract semantics land in d73.2–d73.7.

pub mod competition;
pub mod fst;
pub mod spe;
pub mod sweep;
pub mod window;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
