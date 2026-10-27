//! 39c.3 gate: `cd src/ && cargo test --manifest-path accent/Cargo.toml condition_a`
//! 39c.4 gate: `cd src/ && cargo test --manifest-path accent/Cargo.toml condition_b`
//! 91x.1 gate: `cd src/accent && cargo test derive`

mod condition_a_tests;
mod condition_b_tests;
mod derive_tests;
mod drift_dial_tests;
mod drift_tests;
mod enroll_baseline_tests;
mod feedback_tests;
mod measure_tests;
mod ppt_accent_tests;
mod ppt_tests;
mod window_feedback_tests;
