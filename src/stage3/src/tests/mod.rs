//! d73.1 gate: `cd src/stage3 && cargo test spe_rules`
//! d73.2 gate: `cd src/stage3 && cargo test fst`
//! d73.3 gate: `cd src/stage3 && cargo test viterbi`
//! d73.4 gate: `cd src/stage3 && cargo test nbest_competition`
//! aem.1 gate: `cd src/stage3 && cargo test sweep`
//! d73.5 gate: `cd src/stage3 && cargo test promotion`
//! d73.6 gate: `cd src/stage3 && cargo test retraction`
//! d73.7 gate: `cd src/stage3 && cargo test --all-features`

mod fst_tests;
mod nbest_tests;
mod ppt_tests;
mod promotion_tests;
mod retraction_tests;
mod spe_rule_tests;
mod sweep_tests;
mod viterbi_tests;
