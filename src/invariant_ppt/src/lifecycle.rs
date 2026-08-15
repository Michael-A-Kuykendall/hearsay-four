//! Test lifecycle management — the E→P→C promotion mechanism.
//!
//! Per `ppt_invariant_guide.md`, one of the four core ideas is **Automating
//! the Test Lifecycle** to prevent test bloat. Tests begin life as
//! **E-Tests** (exploration, temporary), get promoted to **P-Tests**
//! (property, stable behavior), and finally to **C-Tests** (contract,
//! permanent must-pass).
//!
//! This module tracks every registered test's current layer and supports
//! promotion, so an agent or human can see the whole lifecycle in one place
//! and promote a test only when it is genuinely stable.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

/// The three lifecycle layers, in promotion order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Exploration — temporary, allowed to be throwaway.
    E,
    /// Property — stable behavior over generic inputs.
    P,
    /// Contract — permanent, must-pass, enforces tracked invariants.
    C,
}

impl Layer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Layer::E => "E",
            Layer::P => "P",
            Layer::C => "C",
        }
    }
}

/// Metadata for a tracked test's lifecycle position.
#[derive(Debug, Clone)]
pub struct LifecycleEntry {
    /// Fully-qualified test name (module::test).
    pub name: String,
    /// Current layer.
    pub layer: Layer,
    /// Optional short description of what the test guarantees.
    pub description: String,
}

/// Process-wide test lifecycle registry.
static LIFECYCLE: LazyLock<Mutex<BTreeMap<String, LifecycleEntry>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Register a test at a given layer. Idempotent — re-registering updates the
/// entry (used when a test is promoted).
pub fn register_test(name: &str, layer: Layer, description: &str) {
    LIFECYCLE.lock().unwrap().insert(
        name.to_string(),
        LifecycleEntry {
            name: name.to_string(),
            layer,
            description: description.to_string(),
        },
    );
}

/// Promote a test to a higher (or equal) layer. Returns `false` if the test
/// is not registered.
pub fn promote_test(name: &str, to: Layer) -> bool {
    let mut reg = LIFECYCLE.lock().unwrap();
    let Some(entry) = reg.get_mut(name) else {
        return false;
    };
    if to >= entry.layer {
        entry.layer = to;
    }
    true
}

/// Look up a test's current layer.
pub fn layer_of(name: &str) -> Option<Layer> {
    LIFECYCLE.lock().unwrap().get(name).map(|e| e.layer)
}

/// Count tests at each layer.
pub fn counts() -> (usize, usize, usize) {
    let reg = LIFECYCLE.lock().unwrap();
    let (mut e, mut p, mut c) = (0, 0, 0);
    for entry in reg.values() {
        match entry.layer {
            Layer::E => e += 1,
            Layer::P => p += 1,
            Layer::C => c += 1,
        }
    }
    (e, p, c)
}

/// All E-Tests (candidates for promotion or deletion).
pub fn explorations() -> Vec<String> {
    by_layer(Layer::E)
}

/// All P-Tests.
pub fn properties() -> Vec<String> {
    by_layer(Layer::P)
}

/// All C-Tests (the permanent must-pass set).
pub fn contracts() -> Vec<String> {
    by_layer(Layer::C)
}

fn by_layer(layer: Layer) -> Vec<String> {
    LIFECYCLE
        .lock()
        .unwrap()
        .values()
        .filter(|e| e.layer == layer)
        .map(|e| e.name.clone())
        .collect()
}

/// Clear the lifecycle registry (test isolation).
pub fn clear_lifecycle() {
    LIFECYCLE.lock().unwrap().clear();
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::test_lock;
    use std::sync::MutexGuard;

    fn setup(_g: &MutexGuard<'static, ()>) {
        clear_lifecycle();
    }

    #[test]
    fn registers_at_layer() {
        let g = test_lock::lock();
        setup(&g);
        register_test("t1", Layer::E, "exploring");
        assert_eq!(layer_of("t1"), Some(Layer::E));
    }

    #[test]
    fn promotes_e_to_c() {
        let g = test_lock::lock();
        setup(&g);
        register_test("t2", Layer::E, "will be permanent");
        assert!(promote_test("t2", Layer::P));
        assert!(promote_test("t2", Layer::C));
        assert_eq!(layer_of("t2"), Some(Layer::C));
        assert_eq!(counts(), (0, 0, 1));
    }

    #[test]
    fn does_not_demote() {
        let g = test_lock::lock();
        setup(&g);
        register_test("t3", Layer::C, "already permanent");
        // Trying to demote has no effect.
        assert!(promote_test("t3", Layer::E));
        assert_eq!(layer_of("t3"), Some(Layer::C));
    }

    #[test]
    fn unknown_test_not_promotable() {
        let g = test_lock::lock();
        setup(&g);
        assert!(!promote_test("nonexistent", Layer::P));
    }

    #[test]
    fn counts_layers() {
        let g = test_lock::lock();
        setup(&g);
        register_test("e1", Layer::E, "");
        register_test("p1", Layer::P, "");
        register_test("c1", Layer::C, "");
        assert_eq!(counts(), (1, 1, 1));
        assert_eq!(explorations(), vec!["e1".to_string()]);
        assert_eq!(properties(), vec!["p1".to_string()]);
        assert_eq!(contracts(), vec!["c1".to_string()]);
    }
}
