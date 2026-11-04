//! Scale calibration for the fabric experiment
//! (bead `hearsay-four-v7fa--recalibrate-retune-resweep`).
//!
//! Static lattice statistics that set the tuning grid. The per-token
//! liveness mirror follows `fabric::decision::refresh_liveness` exactly:
//! unary max per source class times its weight, plus the best SEQUENCE
//! pair term on each side times `w_sequence`. Two subtleties, both read
//! off the fabric source:
//!
//! - Boundary SEQUENCE terms (`from: None`, attached by the bridge for
//!   arc-less tokens) are filtered out of `fold_seq_side`
//!   (`e.from.is_some()`), so they score commits `S(T)` but never enter
//!   `L(h)`. Arc-less tokens get no to-side sequence term here either.
//! - The static value is the steady-state (post-intake) liveness: during
//!   streaming intake the from-side terms attach only as successors mint.
//!   Pair premises are assumed active (no pruning yet).

use lattice::Lattice;
use std::collections::{BTreeMap, BTreeSet};

/// Nearest-rank percentile over a sample. `p` is in `[0, 100]`.
/// Returns `NaN` on an empty sample.
pub fn percentile(sample: &[f64], p: f64) -> f64 {
    if sample.is_empty() {
        return f64::NAN;
    }
    let mut sorted: Vec<f64> = sample.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    percentile_sorted(&sorted, p)
}

/// Nearest-rank percentile over an ascending-sorted sample.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    assert!(
        !sorted.is_empty(),
        "percentile_sorted needs a non-empty sample"
    );
    let n = sorted.len();
    let rank = (p / 100.0 * n as f64).ceil() as usize;
    sorted[rank.clamp(1, n) - 1]
}

/// Best SEQUENCE pair `llr` per side for one token, mirroring
/// `fold_seq_side`: to-side ranges over incoming arcs only (arc-less
/// tokens get `None` — the boundary term never enters `L(h)`), from-side
/// over outgoing arcs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeqSides {
    pub in_max: Option<f64>,
    pub out_max: Option<f64>,
}

/// Per-token [`SeqSides`] keyed by lattice token id, scored with the same
/// `sequence_llr(prev, word)` the bridge feeds the fabric.
pub fn sequence_sides(
    lat: &Lattice,
    seq: &dyn Fn(Option<&str>, &str) -> f64,
) -> BTreeMap<u64, SeqSides> {
    let words: BTreeMap<u64, &str> = lat.tokens.iter().map(|t| (t.id, t.word.as_str())).collect();
    let mut incoming: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut outgoing: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for a in &lat.arcs {
        incoming.entry(a.to).or_default().push(a.from);
        outgoing.entry(a.from).or_default().push(a.to);
    }
    let mut out = BTreeMap::new();
    for t in &lat.tokens {
        let word = words[&t.id];
        let in_max = match incoming.get(&t.id) {
            None => None,
            Some(froms) => froms
                .iter()
                .filter_map(|f| words.get(f))
                .map(|fw| seq(Some(fw), word))
                .fold(None, |acc: Option<f64>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                }),
        };
        let out_max = match outgoing.get(&t.id) {
            None => None,
            Some(tos) => tos
                .iter()
                .filter_map(|to| words.get(to))
                .map(|tw| seq(Some(word), tw))
                .fold(None, |acc: Option<f64>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                }),
        };
        out.insert(t.id, SeqSides { in_max, out_max });
    }
    out
}

/// Static per-token liveness `(token_id, liveness)` in lattice order:
/// `w_acoustic * acoustic` + optional unaries at weight 1.0 (the bridge
/// only ever overrides the acoustic and sequence weights) + `w_sequence`
/// times each present sequence side.
pub fn static_liveness(
    lat: &Lattice,
    sides: &BTreeMap<u64, SeqSides>,
    w_acoustic: f64,
    w_sequence: f64,
) -> Vec<(u64, f64)> {
    lat.tokens
        .iter()
        .map(|t| {
            let mut live = w_acoustic * t.acoustic_llr;
            for llr in [t.lexical_llr, t.speaker_llr, t.verify_llr]
                .into_iter()
                .flatten()
            {
                live += llr;
            }
            if let Some(s) = sides.get(&t.id) {
                if let Some(v) = s.in_max {
                    live += w_sequence * v;
                }
                if let Some(v) = s.out_max {
                    live += w_sequence * v;
                }
            }
            (t.id, live)
        })
        .collect()
}

/// Every SEQUENCE `llr` the bridge attaches, in the order it attaches
/// them: one boundary term per arc-less token, one pair term per arc
/// (frame order of the `to` token). This is the commit-score population,
/// not the liveness population (boundary terms excluded there).
pub fn sequence_terms(lat: &Lattice, seq: &dyn Fn(Option<&str>, &str) -> f64) -> Vec<f64> {
    let words: BTreeMap<u64, &str> = lat.tokens.iter().map(|t| (t.id, t.word.as_str())).collect();
    let mut incoming: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for a in &lat.arcs {
        incoming.entry(a.to).or_default().push(a.from);
    }
    let mut order: Vec<&lattice::Token> = lat.tokens.iter().collect();
    order.sort_by_key(|a| (a.start, a.end, a.id));
    let mut terms = Vec::new();
    for t in order {
        match incoming.get(&t.id) {
            None => terms.push(seq(None, &t.word)),
            Some(froms) => {
                for f in froms {
                    if let Some(fw) = words.get(f) {
                        terms.push(seq(Some(fw), &t.word));
                    }
                }
            }
        }
    }
    terms
}

/// Unsealed-window size after each intake step for horizon `j`, in the
/// bridge's frame order `(start, end, id)`: the frontier is the max `end`
/// minted so far and a minted token is unsealed while
/// `end + j >= frontier` (`is_sealed` iff `frontier > end + j`,
/// saturating).
pub fn unsealed_windows(lat: &Lattice, j: u32) -> Vec<usize> {
    let mut order: Vec<(u32, u32, u64)> =
        lat.tokens.iter().map(|t| (t.start, t.end, t.id)).collect();
    order.sort();
    let mut ends: Vec<u32> = Vec::with_capacity(order.len());
    let mut frontier: u32 = 0;
    let mut windows = Vec::with_capacity(order.len());
    for (_, end, _) in order {
        frontier = frontier.max(end);
        ends.push(end);
        windows.push(
            ends.iter()
                .filter(|e| e.saturating_add(j) >= frontier)
                .count(),
        );
    }
    windows
}

/// Non-null `margin` values from `committed` / `defeated` bridge events.
/// Malformed lines are skipped; the bridge guarantees one JSON object
/// per line.
pub fn harvest_margins(events_jsonl: &str) -> Vec<f64> {
    let mut margins = Vec::new();
    for line in events_jsonl.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = v.get("event").and_then(|k| k.as_str()).unwrap_or("");
        if (kind == "committed" || kind == "defeated")
            && let Some(m) = v.get("margin").and_then(|m| m.as_f64())
        {
            margins.push(m);
        }
    }
    margins
}

/// Event-kind counts plus the eviction budget breakdown from one
/// `events_jsonl` blob.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EventCounts {
    pub pruned: usize,
    pub revived: usize,
    pub defeated: usize,
    pub evicted: usize,
    pub committed: usize,
    pub evicted_by_budget: BTreeMap<String, usize>,
}

/// Tokens named by `pruned` / `evicted` events (with duplicates — one
/// entry per event, matching [`EventCounts`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EventTokens {
    pub pruned: Vec<String>,
    pub evicted: Vec<String>,
}

fn event_word(v: &serde_json::Value) -> String {
    v.get("token")
        .and_then(|t| t.get("word"))
        .and_then(|w| w.as_str())
        .unwrap_or("")
        .to_string()
}

/// Count events and collect pruned/evicted token words.
pub fn scan_events(events_jsonl: &str) -> (EventCounts, EventTokens) {
    let mut counts = EventCounts::default();
    let mut tokens = EventTokens::default();
    for line in events_jsonl.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v.get("event").and_then(|k| k.as_str()).unwrap_or("") {
            "pruned" => {
                counts.pruned += 1;
                tokens.pruned.push(event_word(&v));
            }
            "revived" => counts.revived += 1,
            "defeated" => counts.defeated += 1,
            "committed" => counts.committed += 1,
            "evicted" => {
                counts.evicted += 1;
                tokens.evicted.push(event_word(&v));
                let budget = v
                    .get("budget")
                    .and_then(|b| b.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                *counts.evicted_by_budget.entry(budget).or_default() += 1;
            }
            _ => {}
        }
    }
    (counts, tokens)
}

/// Deduplicated sorted word list (display form; counts live in [`EventCounts`]).
pub fn dedup_words(words: &[String]) -> Vec<String> {
    let set: BTreeSet<&str> = words.iter().map(String::as_str).collect();
    set.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice::{Arc, Lattice, Token};

    fn toy() -> Lattice {
        // Three tokens, one arc 1 -> 2; token 3 is arc-less.
        Lattice {
            utterance: "toy".to_string(),
            frame_shift_ms: 10.0,
            producer: "test".to_string(),
            tokens: vec![
                Token {
                    id: 1,
                    word: "A".to_string(),
                    start: 0,
                    end: 2,
                    source: "t".to_string(),
                    acoustic_llr: 10.0,
                    lexical_llr: None,
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
                Token {
                    id: 2,
                    word: "B".to_string(),
                    start: 2,
                    end: 4,
                    source: "t".to_string(),
                    acoustic_llr: 20.0,
                    lexical_llr: Some(1.5),
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
                Token {
                    id: 3,
                    word: "C".to_string(),
                    start: 5,
                    end: 6,
                    source: "t".to_string(),
                    acoustic_llr: 30.0,
                    lexical_llr: None,
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
            ],
            arcs: vec![Arc { from: 1, to: 2 }],
        }
    }

    fn stub_seq(prev: Option<&str>, word: &str) -> f64 {
        match (prev, word) {
            (None, _) => -1.0,
            (Some("A"), "B") => 2.0,
            (Some("A"), "C") => 3.0,
            _ => -5.0,
        }
    }

    #[test]
    fn percentile_nearest_rank() {
        let s: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(percentile(&s, 5.0), 5.0);
        assert_eq!(percentile(&s, 25.0), 25.0);
        assert_eq!(percentile(&s, 50.0), 50.0);
        assert_eq!(percentile(&s, 95.0), 95.0);
        assert!(percentile(&[], 50.0).is_nan());
    }

    #[test]
    fn sides_exclude_boundary_from_liveness() {
        let lat = toy();
        let sides = sequence_sides(&lat, &stub_seq);
        // Token 2 has an incoming arc: to-side is the pair term.
        assert_eq!(
            sides[&2],
            SeqSides {
                in_max: Some(2.0),
                out_max: None
            }
        );
        // Token 3 is arc-less: NO to-side term (boundary excluded).
        assert_eq!(
            sides[&3],
            SeqSides {
                in_max: None,
                out_max: None
            }
        );
        // Token 1: from-side only.
        assert_eq!(
            sides[&1],
            SeqSides {
                in_max: None,
                out_max: Some(2.0)
            }
        );
    }

    #[test]
    fn static_liveness_weights() {
        let lat = toy();
        let sides = sequence_sides(&lat, &stub_seq);
        let live = static_liveness(&lat, &sides, 1.0, 6.0);
        // id 1: 10 + 6*2 = 22; id 2: 20 + 1.5 + 6*2 = 33.5; id 3: 30.
        assert_eq!(live, vec![(1, 22.0), (2, 33.5), (3, 30.0)]);
    }

    #[test]
    fn terms_include_boundary_and_pairs() {
        let lat = toy();
        let mut terms = sequence_terms(&lat, &stub_seq);
        terms.sort_by(|a, b| a.total_cmp(b));
        // Boundary for tokens 1 and 3 (-1 each) + pair 1->2 (+2).
        assert_eq!(terms, vec![-1.0, -1.0, 2.0]);
    }

    #[test]
    fn windows_grow_with_j() {
        let lat = toy();
        // Frame order 1,2,3; ends 2,4,6.
        assert_eq!(unsealed_windows(&lat, 0), vec![1, 1, 1]);
        assert_eq!(unsealed_windows(&lat, 3), vec![1, 2, 2]);
        assert_eq!(unsealed_windows(&lat, 100), vec![1, 2, 3]);
    }

    #[test]
    fn margins_skip_null_and_noise() {
        let jsonl = concat!(
            "{\"event\":\"committed\",\"margin\":2.5}\n",
            "{\"event\":\"committed\",\"margin\":null}\n",
            "{\"event\":\"defeated\",\"margin\":1.5}\n",
            "{\"event\":\"evicted\",\"liveness\":3.0,\"budget\":\"live\"}\n",
            "not json\n",
        );
        assert_eq!(harvest_margins(jsonl), vec![2.5, 1.5]);
        let (counts, _) = scan_events(jsonl);
        assert_eq!(counts.committed, 2);
        assert_eq!(counts.defeated, 1);
        assert_eq!(counts.evicted, 1);
        assert_eq!(counts.evicted_by_budget.get("live"), Some(&1));
    }
}
