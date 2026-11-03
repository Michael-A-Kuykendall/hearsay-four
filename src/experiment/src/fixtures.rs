//! Synthetic fixtures: four utterances plus a fixture bigram.
//!
//! All words are generic vocabulary invented for this crate; no corpus,
//! book, or owner text appears here. Token `source` mimics decoder output:
//! the 1-best backbone carries `a2-1best` (which the K filter always keeps),
//! alternatives carry `fixture-alt`.
//!
//! - `u1-revival`: acoustics favour the wrong word `bat` over the correct
//!   `cat` (ranked 5th of 6 by `acoustic_llr` at its end frame, so the K=3
//!   filter drops it but K>=5 keeps it). Correct at birth-dormant.
//! - `u2-clean`: single correct path; every arm and the oracle score 0.
//! - `u3-bulk`: one correct token plus 100 same-span distractors, so budget
//!   caps bind and peak live/dormant track the budget.
//! - `u4-absent`: the correct word `blue` is absent from the lattice; every
//!   arm and the oracle err (attribution case: generation miss, not search).

use lattice::{Arc, Lattice, Token};

/// One fixture utterance: lattice plus reference transcript.
pub struct Fixture {
    pub id: &'static str,
    pub lattice: Lattice,
    pub reference: Vec<String>,
}

/// 1-best backbone marker (kept by the K filter at every K).
pub const BACKBONE: &str = "a2-1best";
/// Lattice-alternative marker.
pub const ALT: &str = "fixture-alt";

fn tok(id: u64, word: &str, start: u32, end: u32, source: &str, acoustic_llr: f64) -> Token {
    Token {
        id,
        word: word.to_string(),
        start,
        end,
        source: source.to_string(),
        acoustic_llr,
        lexical_llr: None,
        speaker_llr: None,
        verify_llr: None,
        user_llr: None,
    }
}

fn lat(utterance: &str, tokens: Vec<Token>, arcs: Vec<(u64, u64)>) -> Lattice {
    Lattice {
        utterance: utterance.to_string(),
        frame_shift_ms: 10.0,
        producer: "experiment-fixture".to_string(),
        tokens,
        arcs: arcs
            .into_iter()
            .map(|(from, to)| Arc { from, to })
            .collect(),
    }
}

fn words(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

pub fn u1_revival() -> Fixture {
    let tokens = vec![
        tok(1, "the", 0, 10, BACKBONE, 5.0),
        tok(2, "bat", 10, 20, BACKBONE, 4.0),
        tok(3, "hat", 10, 20, ALT, 3.5),
        tok(4, "mat", 10, 20, ALT, 3.0),
        tok(5, "rat", 10, 20, ALT, 2.5),
        tok(6, "cat", 10, 20, ALT, 1.0),
        tok(7, "cab", 10, 20, ALT, 0.5),
        tok(8, "sat", 20, 30, BACKBONE, 5.0),
        tok(9, "down", 30, 40, BACKBONE, 5.0),
    ];
    let mut arcs = Vec::new();
    for id in 2..=7 {
        arcs.push((1, id));
        arcs.push((id, 8));
    }
    arcs.push((8, 9));
    Fixture {
        id: "u1-revival",
        lattice: lat("u1-revival", tokens, arcs),
        reference: words(&["the", "cat", "sat", "down"]),
    }
}

pub fn u2_clean() -> Fixture {
    let tokens = vec![
        tok(1, "up", 0, 10, BACKBONE, 5.0),
        tok(2, "and", 10, 20, BACKBONE, 5.0),
        tok(3, "away", 20, 30, BACKBONE, 5.0),
    ];
    Fixture {
        id: "u2-clean",
        lattice: lat("u2-clean", tokens, vec![(1, 2), (2, 3)]),
        reference: words(&["up", "and", "away"]),
    }
}

pub fn u3_bulk() -> Fixture {
    let mut tokens = vec![tok(1, "alpha", 0, 10, BACKBONE, 5.0)];
    for i in 0..100 {
        tokens.push(tok(
            2 + i as u64,
            &format!("w{:03}", i + 1),
            0,
            10,
            ALT,
            2.0,
        ));
    }
    Fixture {
        id: "u3-bulk",
        lattice: lat("u3-bulk", tokens, Vec::new()),
        reference: words(&["alpha"]),
    }
}

pub fn u4_absent() -> Fixture {
    let tokens = vec![
        tok(1, "red", 0, 10, BACKBONE, 5.0),
        tok(2, "green", 10, 20, BACKBONE, 5.0),
        tok(3, "yellow", 20, 30, BACKBONE, 4.0),
    ];
    Fixture {
        id: "u4-absent",
        lattice: lat("u4-absent", tokens, vec![(1, 2), (2, 3)]),
        reference: words(&["red", "green", "blue"]),
    }
}

pub fn all_fixtures() -> Vec<Fixture> {
    vec![u1_revival(), u2_clean(), u3_bulk(), u4_absent()]
}

/// Fixture sequence-LLR source: `llr_SEQ(a -> b)` for word labels, with
/// `("<s>", w)` for utterance-start boundary pairs.
///
/// Positive pairs reward the correct continuations (`cat -> sat`,
/// `up -> and`, ...); everything else defaults to -1.0. On real data this
/// is replaced by the interpolated language model; the runner signature is
/// unchanged.
pub fn fixture_bigram(a: &str, b: &str) -> f64 {
    match (a, b) {
        ("<s>", "the") => 1.0,
        ("the", "cat") => -1.5,
        // Adversarial left context: the distractors go dormant, so the
        // commit DP (top-2 partials per frame) shortlists {bat, cat} and
        // the revived `cat` survives to meet its right context.
        ("the", "hat") => -5.0,
        ("the", "mat") => -5.0,
        ("the", "rat") => -5.0,
        ("cat", "sat") => 4.0,
        ("sat", "down") => 2.0,
        ("<s>", "up") => 1.0,
        ("up", "and") => 2.0,
        ("and", "away") => 2.0,
        ("<s>", "alpha") => 1.0,
        ("<s>", "red") => 1.0,
        ("red", "green") => 2.0,
        ("green", "yellow") => 1.0,
        _ => -1.0,
    }
}

/// Serialize a fixture lattice to versioned JSONL (round-trips through
/// [`lattice::read_lattice`], the same reader the oracle sweep uses).
pub fn lattice_text(fixture: &Fixture) -> Result<String, String> {
    lattice::write_lattice(&fixture.lattice)
}
