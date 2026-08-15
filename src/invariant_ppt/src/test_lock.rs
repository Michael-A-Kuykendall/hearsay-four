//! Shared test serialization helper.
//!
//! The invariant machinery uses process-global static state (the invariant
//! log and contract registry). Rust runs tests in parallel by default, so
//! every test module that touches that shared state MUST acquire this single
//! lock to avoid racing. Fuzz (proptest) tests, contract tests, and property
//! tests all go through the same lock.

use std::sync::{LazyLock, Mutex, MutexGuard};

static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Acquire the shared test serialization lock, recovering from poisoning
/// (a `should_panic` test that panicked while holding it).
pub fn lock() -> MutexGuard<'static, ()> {
    match TEST_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}
