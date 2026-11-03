//! A6 fabric experiment: arms A/B/C over word lattices.
//!
//! Bead `hearsay-four-ic1w--fabric-experiment-arms`.
//!
//! Lane status (2026-11-03): the decoder K-best bead, the lattice-fabric
//! bridge bead, and the fabric prune-arm bead have not landed, and the lane
//! worktree has no Librispeech audio, acoustic model, or language model
//! (all main-workspace externals). This crate therefore runs on synthetic
//! fixtures plus the fabric API directly:
//!
//! - [`sampling`]: the TEST-200 sampling spec (5 per speaker, seed
//!   20261103, exactly 200 rows) over [`corpus::manifest`]. The real
//!   manifest is sampled on main; the spec and verifier live here.
//! - [`oracle_sweep`]: oracle WER sweep over K, reusing the exact tested
//!   oracle in [`a2::oracle`] (no second oracle implementation).
//! - [`scoring`]: 1-best baseline path, WER aggregation via [`output::wer`],
//!   and paired-bootstrap delta CIs via [`eval::corpus_delta_ci`].
//! - [`arms`]: arm runners. Arm A is the 1-best baseline (no fabric).
//!   Arms B/C drive [`fabric::decision::DecisionFabric`] with the native
//!   [`fabric::decision::Arm`] flag: arm B beam-prunes below-threshold
//!   tokens at once (never revived); arm C keeps them dormant and revivable
//!   with live plus dormant sharing one combined total.
//! - [`fixtures`]: four synthetic utterances (revival, clean, bulk,
//!   absent-word) plus a fixture bigram used as the sequence-LLR source.
//! - [`table`]: the A/B/C by {50, 100, 250, 500} budget table.
//!
//! When the sibling beads land, the fixture lattice builders are replaced
//! by `lattice::read_lattice` over saved lattice files, the fixture bigram
//! by the interpolated language model, and the arm flag by the fabric
//! `Arm` enum — the table, scoring, and sampling code is unchanged.

pub mod arms;
pub mod fixtures;
pub mod oracle_sweep;
pub mod sampling;
pub mod scoring;
pub mod table;
