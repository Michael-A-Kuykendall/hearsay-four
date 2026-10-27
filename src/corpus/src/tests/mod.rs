//! Test suite for the corpus crate (gates ul2.1–ul2.4).
//!
//! - `manifest_tests` — ul2.3 gate (`cargo test manifest`)
//! - `decode_tests` — ul2.2 gate (`cargo test decode`)
//! - `ground_truth_tests` — A1 gate (`cargo test ground_truth`)
//! - `ingest_tests` — ul2.1 gate (`cargo test ingest`)
//! - `ppt_tests` — ul2.4 gate (`cargo test --all-features`), serialized via
//!   `invariant_ppt::test_lock`

mod decode_tests;
mod ground_truth_tests;
mod ingest_tests;
mod manifest_tests;
mod ppt_tests;
