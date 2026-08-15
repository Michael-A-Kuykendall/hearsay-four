//! a6d.2 gate: `cd src/arena && cargo test memo`

use crate::{Arena, Candidate};

fn candidate(phoneme: &str, sig: &[i32]) -> Candidate {
    Candidate::new(phoneme, vec![], sig.to_vec())
}

#[test]
fn memo_shared_fragment_reuses_first_occurrence_nodeid() {
    let mut arena = Arena::new();

    // Utterance 1: [IH, K, AE]
    let u1 = [
        candidate("IH", &[1, 1, 1]),
        candidate("K", &[2, 2, 2]),
        candidate("AE", &[3, 3, 3]),
    ];
    let s1 = arena.ingest_utterance(&u1);
    assert_eq!(s1.new_facts, 3);
    assert_eq!(s1.reused_facts, 0);
    assert_eq!(arena.fact_count(), 3, "fabric grew by 3");

    // Utterance 2: [K, N] — K is a recurring fragment.
    let u2 = [candidate("K", &[2, 2, 2]), candidate("N", &[4, 4, 4])];
    let s2 = arena.ingest_utterance(&u2);
    assert_eq!(
        s2.ids[0], s1.ids[1],
        "shared fragment K resolves to utterance 1's NodeId"
    );
    assert_eq!(s2.new_facts, 1, "only the new fragment N grows the fabric");
    assert_eq!(s2.reused_facts, 1, "K was memoized");
    assert_eq!(
        arena.fact_count(),
        4,
        "INV: cross_utterance_memoization — no new fact for K"
    );
}

#[test]
fn memo_repeated_across_many_utterances() {
    let mut arena = Arena::new();
    let shared = candidate("OW", &[7, 7, 7]);
    let mut first_id = None;
    for utterance in 0..5 {
        let extra = candidate(&format!("U{utterance}"), &[utterance, 0, 0]);
        let stats = arena.ingest_utterance(&[shared.clone(), extra]);
        match first_id {
            None => {
                first_id = Some(stats.ids[0]);
                assert_eq!(stats.new_facts, 2);
            }
            Some(expected) => {
                assert_eq!(
                    stats.ids[0], expected,
                    "every recurrence reuses the original NodeId"
                );
                assert_eq!(stats.new_facts, 1, "only the unique extra fragment is new");
                assert_eq!(stats.reused_facts, 1);
            }
        }
    }
    assert_eq!(arena.fact_count(), 6, "5 extras + 1 shared");
}

#[test]
fn memo_in_utterance_duplicates_do_not_grow_fabric() {
    let mut arena = Arena::new();
    let frag = candidate("SH", &[0, 0, 0]);
    let stats = arena.ingest_utterance(&[frag.clone(), frag.clone()]);
    assert_eq!(stats.ids[0], stats.ids[1], "same NodeId within utterance");
    assert_eq!(arena.fact_count(), 1);
    assert_eq!(stats.new_facts, 1);
    assert_eq!(stats.reused_facts, 1);
}

#[test]
fn memo_fragment_appearing_earlier_resolves_across_sessions() {
    let mut arena = Arena::new();
    let a = candidate("AA", &[5, 5, 5]);
    arena.ingest_utterance(std::slice::from_ref(&a));
    let b = candidate("AA", &[5, 5, 5]); // identical content, later session
    let before = arena.fact_count();
    let stats = arena.ingest_utterance(&[b]);
    assert_eq!(before, 1);
    assert_eq!(stats.new_facts, 0, "fully memoized utterance");
    assert_eq!(stats.reused_facts, 1);
    assert_eq!(arena.fact_count(), 1);
}
