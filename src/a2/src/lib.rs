//! A2 connected-word DTW decoder (token-passing, Ney family) over the A1 front end.
//!
//! One-stage search: DTW cells are NOT fabric facts. Token state per bead
//! `hearsay-four-5n2--connected-word-decoder`: unit in {word, SIL, SP},
//! template position, start frame, path cost, backpointer, LM history
//! (empty until R1; GARBAGE deferred).
//!
//! DEV-only. Templates + phrases load from any `--template-dir` with a
//! `manifest.tsv` (run1 shape); L1 canonical output drops in with zero code
//! change. See `docs/results/A2-CONNECTED-DTW.md`.

pub mod concat;
pub mod decoder;
pub mod oracle;
pub mod templates;
pub mod wer;

pub use decoder::{DecodeResult, DecoderConfig, WordHyp, decode, emit_lattice};
pub use oracle::{OracleResult, lattice_oracle_wer, sweep_k};
pub use templates::{
    TemplateSet, WordTemplate, frozen_frontend_config, load_bulk_templates, load_templates,
};
pub use wer::{WerSummary, edit_distance, wer_corpus};
