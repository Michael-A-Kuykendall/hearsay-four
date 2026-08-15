//! Proptest fuzzing integration for the invariant machinery.
//!
//! Property tests should generate generic/edge-case inputs rather than rely
//! on hand-written cases (per `ppt_invariant_guide.md` expansion idea:
//! "Property test fuzzing: integrate with proptest/quickcheck").
//!
//! These tests exercise `assert_invariant`'s tracking under fuzzed input
//! sequences and verify the invariant log never violates its own structural
//! properties regardless of the input stream.

use crate::{
    assert_invariant, clear_invariant_log, recorded_invariants, recorded_messages, test_lock,
};
use proptest::prelude::*;

fn setup(_g: &std::sync::MutexGuard<'static, ()>) {
    clear_invariant_log();
}

// A property the invariant log must always satisfy: recording N distinct
// messages yields exactly N tracked entries, no matter how the messages are
// generated or interleaved.
proptest! {
    #[test]
    fn log_tracks_distinct_messages_exactly(
        messages in prop::collection::vec("[a-z]{1,8}", 0..50),
    ) {
        let g = test_lock::lock();
        setup(&g);
        for m in &messages {
            assert_invariant(true, m, None);
        }
        // Distinct messages -> distinct keys.
        let distinct: std::collections::BTreeSet<_> = messages.iter().cloned().collect();
        prop_assert_eq!(recorded_messages().len(), distinct.len());
    }

    #[test]
    fn hits_accumulate_across_repetitions(
        messages in prop::collection::vec("[a-z]{1,6}", 0..30),
    ) {
        let g = test_lock::lock();
        setup(&g);
        // Count occurrences of each message.
        let mut expected: std::collections::BTreeMap<String, u64> = Default::default();
        for m in &messages {
            *expected.entry(m.clone()).or_insert(0) += 1;
            assert_invariant(true, m, None);
        }
        let records = recorded_invariants();
        for (msg, hits) in &expected {
            let actual = records
                .iter()
                .find(|r| &r.message == msg)
                .map(|r| r.hits)
                .unwrap_or(0);
            prop_assert_eq!(&actual, hits, "hit count mismatch for {}", msg);
        }
    }

    #[test]
    fn never_panics_on_true_conditions(
        _cond in prop::bool::ANY,
        _label in "[a-z]{0,16}",
    ) {
        let g = test_lock::lock();
        setup(&g);
        // Only ever assert true conditions here — must never panic.
        assert_invariant(true, "forced true", None);
    }
}
