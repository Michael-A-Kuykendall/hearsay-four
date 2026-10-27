//! Core invariant + property/contract test system.
//!
//! Implements the layered test framework from `ppt_invariant_guide.md`:
//! - **E-Test**: exploration (temporary), `explore_test`
//! - **P-Test**: property test (generic input, stable behavior), `property_test`
//! - **C-Test**: contract test (permanent, must-pass), `contract_test`
//!
//! Runtime invariants are embedded directly in business logic via
//! [`assert_invariant`]. Each invocation logs the assertion, records its
//! presence (for later contract checks), and panics on violation. Contract
//! tests assert that a named set of invariants are still actively enforced.
//!
//! The E→P→C **test lifecycle** (see [`lifecycle`]) automates promotion of
//! tests from exploration to property to contract, preventing test bloat.

pub mod lifecycle;

#[cfg(test)]
mod fuzz;

pub mod test_lock;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{LazyLock, Mutex};
/// A single recorded invariant presence/count entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantRecord {
    /// The human-readable invariant message, e.g. "Payment must be positive".
    pub message: String,
    /// Free-form context string, e.g. "checkout flow".
    pub context: String,
    /// How many times this invariant was hit.
    pub hits: u64,
}

/// Process-wide invariant log (namespaced by source/crate name if provided).
static INVARIANT_LOG: LazyLock<Mutex<BTreeMap<String, InvariantRecord>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Normalize a message string into a stable key used for tracking.
fn key(message: &str) -> String {
    message.trim().to_string()
}

/// Assert a runtime invariant, logging its presence and crashing on violation.
///
/// Semantics (per `ppt_invariant_guide.md`):
/// - Logs the assertion (records presence for later contract checks).
/// - Crashes on violation.
/// - Records presence for later contract checks.
///
/// `context` is an optional free-form label (e.g. `"checkout flow"`).
pub fn assert_invariant(cond: bool, message: &str, context: Option<&str>) {
    // PERF-1 (bead hearsay-four-gmp): the process-global log mutex + map
    // insert ran on EVERY invariant hit (i.e. every fabric assert/retract).
    // Logging is presence-tracking for test-build contract checks only, so
    // it is compiled out of non-test builds: `cfg(test)` covers this
    // crate's own test harness and `debug_assertions` covers downstream
    // crates' test binaries (which link this crate as a plain rlib, where
    // `cfg(test)` is false). `cfg!` folds at compile time, so release
    // builds pay nothing — not even the context allocation. Violation
    // panics stay unconditional in every build.
    if cfg!(test) || cfg!(debug_assertions) {
        let ctx = context.unwrap_or("").to_string();
        {
            let mut log = INVARIANT_LOG.lock().unwrap();
            let entry = log.entry(key(message)).or_insert_with(|| InvariantRecord {
                message: message.to_string(),
                context: ctx.clone(),
                hits: 0,
            });
            entry.hits += 1;
            if entry.context.is_empty() && !ctx.is_empty() {
                entry.context = ctx.clone();
            }
        }
    }
    if !cond {
        let ctx = match context {
            Some(c) if !c.is_empty() => c,
            _ => "<none>",
        };
        panic!("INVARIANT VIOLATION [context={ctx}]: {message}");
    }
}

/// Reset the invariant log between runs (CI cleanup).
pub fn clear_invariant_log() {
    INVARIANT_LOG.lock().unwrap().clear();
}

/// Clear all registered contracts (test isolation; typically not needed in
/// production since contracts are registered once at startup).
pub fn clear_contracts() {
    CONTRACTS.lock().unwrap().clear();
}

/// Return the full set of invariants recorded since the last clear.
pub fn recorded_invariants() -> Vec<InvariantRecord> {
    INVARIANT_LOG.lock().unwrap().values().cloned().collect()
}

/// Return the set of invariant messages currently recorded.
pub fn recorded_messages() -> BTreeSet<String> {
    INVARIANT_LOG.lock().unwrap().keys().cloned().collect()
}

/// Check whether a specific invariant message has been recorded.
pub fn has_invariant(message: &str) -> bool {
    recorded_messages().contains(&key(message))
}

// ---------------------------------------------------------------------------
// E / P / C layered test framework
// ---------------------------------------------------------------------------

/// A contract that a named test must satisfy: a set of invariant messages
/// that must be actively enforced (present in the log).
#[derive(Debug, Clone)]
pub struct Contract {
    pub name: String,
    pub required_invariants: Vec<String>,
}

/// Registered contracts, keyed by name.
static CONTRACTS: LazyLock<Mutex<BTreeMap<String, Vec<String>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Register (or refresh) a contract test's required invariants.
///
/// Per the guide: `contract_test("payment processing", &["Payment must be
/// positive"])`. When run with contract verification enabled, all required
/// invariants must have been actively enforced at some point since the last
/// `clear_invariant_log()`.
pub fn register_contract(name: &str, required_invariants: &[&str]) {
    CONTRACTS.lock().unwrap().insert(
        name.to_string(),
        required_invariants.iter().map(|s| s.to_string()).collect(),
    );
}

/// Verify a named contract: panic if any required invariant is missing.
///
/// Call this at the end of a C-Test to enforce that the real-world
/// expectations are still actively checked. Returns `true` on success.
pub fn verify_contract(name: &str) -> bool {
    let required = CONTRACTS
        .lock()
        .unwrap()
        .get(name)
        .cloned()
        .unwrap_or_default();
    let recorded = recorded_messages();
    let mut missing: Vec<String> = required
        .into_iter()
        .filter(|m| !recorded.contains(m))
        .collect();
    missing.sort();
    if !missing.is_empty() {
        panic!(
            "CONTRACT VIOLATION [{name}]: missing required invariants: {:?}",
            missing
        );
    }
    true
}

/// Verify all registered contracts. Returns the number of contracts checked.
///
/// This is the CI contract-coverage gate: fail if any contract is missing a
/// required invariant. Called by the audit gate (vuo.4).
pub fn verify_all_contracts() -> usize {
    let names: Vec<String> = CONTRACTS.lock().unwrap().keys().cloned().collect();
    for name in &names {
        verify_contract(name);
    }
    names.len()
}

// Convenience wrappers mirroring the guide's layer names. These are thin
// aliases so call sites read as E-Test / P-Test / C-Test.

/// E-Test: exploration. Thin marker; bodies are free-form. Keep short-lived.
#[macro_export]
macro_rules! explore_test {
    ($name:literal, $body:block) => {
        #[test]
        fn $name() $body
    };
}

/// P-Test: property test. The body should call `assert_invariant` and check
/// stable properties over generic inputs.
#[macro_export]
macro_rules! property_test {
    ($name:literal, $body:block) => {
        #[test]
        fn $name() $body
    };
}

/// C-Test: contract test. The body must end by verifying required invariants.
#[macro_export]
macro_rules! contract_test {
    ($name:literal, $required:expr, $body:block) => {
        #[test]
        fn $name() {
            $crate::register_contract($name, $required);
            $body
            $crate::verify_contract($name);
        }
    };
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
