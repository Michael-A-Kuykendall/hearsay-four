//! Lattice oracle WER + K sweep (A5 half, on A2 whole-word lattices).
//!
//! Oracle = minimum edit distance to the reference over all lattice paths
//! (exact DP over DAG x reference positions, not N-best rescoring).
//! Lattices are generated ONCE per acoustic config (K=20 max); [`sweep_k`]
//! filters the same lattice text per K in {3,5,10,20} (top-K by acoustic_llr
//! per end frame) and recomputes arcs. If the oracle gap (1-best WER minus
//! oracle WER) is under 5 points, the bottleneck is acoustic generation and
//! `hearsay-four-cjj--fabric-pruning-experiment` stays blocked.

use std::collections::{HashMap, HashSet};

/// Oracle result for one utterance at one K.
#[derive(Clone, Debug)]
pub struct OracleResult {
    pub errors: u32,
    pub ref_len: u32,
    pub wer: f64,
    /// Oracle word sequence (backtraced).
    pub path: Vec<String>,
    pub nodes: usize,
    pub arcs: usize,
}

/// Min-edit path through `lat` against `reference` (exact DP).
pub fn lattice_oracle_wer(
    lat: &lattice::Lattice,
    reference: &[String],
) -> Result<OracleResult, String> {
    let n = lat.tokens.len();
    if n == 0 {
        return Ok(OracleResult {
            errors: reference.len() as u32,
            ref_len: reference.len() as u32,
            wer: if reference.is_empty() { 0.0 } else { 1.0 },
            path: Vec::new(),
            nodes: 0,
            arcs: lat.arcs.len(),
        });
    }
    // Index by id; topo order = (end, start).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        (lat.tokens[a].end, lat.tokens[a].start).cmp(&(lat.tokens[b].end, lat.tokens[b].start))
    });
    let mut id_pos: HashMap<u64, usize> = HashMap::new();
    for &i in &order {
        id_pos.insert(lat.tokens[i].id, i);
    }
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for a in &lat.arcs {
        let (Some(&u), Some(&v)) = (id_pos.get(&a.from), id_pos.get(&a.to)) else {
            continue;
        };
        // Guard against non-forward arcs (cycle risk): keep DAG property.
        if lat.tokens[u].end > lat.tokens[v].start {
            continue;
        }
        preds[v].push(u);
    }
    let r = reference.len();
    const INF: u32 = u32::MAX / 4;
    // dp[i][j] over topo-ranked tokens; START handled as virtual column.
    let mut dp: Vec<Vec<u32>> = vec![vec![INF; r + 1]; n];
    // back[i][j] = (prev_token_or_START, prev_j, emitted: bool).
    let mut back: Vec<Vec<(Option<usize>, usize)>> = vec![vec![(None, 0); r + 1]; n];
    for &i in &order {
        let w = &lat.tokens[i].word;
        // Entry from START (any token may begin the utterance) or predecessors.
        let mut cand: Vec<(usize, u32, Option<usize>)> = Vec::new();
        for j in 0..=r {
            cand.push((j, j as u32, None));
        }
        for &u in &preds[i] {
            for (j, &d) in dp[u].iter().enumerate() {
                if d < INF {
                    cand.push((j, d, Some(u)));
                }
            }
        }
        // Emit w: sub/match from j-1, ins from j.
        for j in 0..=r {
            // Insertion: emit w without consuming ref.
            let mut best = INF;
            let mut bb: (Option<usize>, usize) = (None, j);
            for &(pj, base, pu) in &cand {
                if pj != j {
                    continue;
                }
                let c = base.saturating_add(1);
                if c < best {
                    best = c;
                    bb = (pu, pj);
                }
            }
            // Sub/match: consume ref[j-1].
            if j > 0 {
                for &(pj, base, pu) in &cand {
                    if pj != j - 1 {
                        continue;
                    }
                    let c = base.saturating_add(u32::from(w != &reference[j - 1]));
                    if c < best {
                        best = c;
                        bb = (pu, pj);
                    }
                }
            }
            dp[i][j] = best;
            back[i][j] = bb;
        }
        // Deletions at this node: consume ref without emitting.
        for j in 1..=r {
            if dp[i][j - 1].saturating_add(1) < dp[i][j] {
                dp[i][j] = dp[i][j - 1].saturating_add(1);
                back[i][j] = (Some(i), j - 1);
            }
        }
    }
    // END: min over ALL tokens (any token may end the utterance).
    let mut best_end = (reference.len() as u32, None);
    for &i in &order {
        if dp[i][r] < best_end.0 {
            best_end = (dp[i][r], Some(i));
        }
    }
    // Empty path (all deletions) always available.
    let errors = best_end.0.min(reference.len() as u32);
    let mut path_rev: Vec<String> = Vec::new();
    if let Some(mut i) = best_end.1
        && dp[i][r] == errors
    {
        let mut j = r;
        loop {
            let (pu, pj) = back[i][j];
            match pu {
                Some(u) if u == i => {
                    j = pj; // deletion, no emission
                }
                _ => {
                    // Emission happened iff j advanced or (j==pj and we
                    // chose insertion). Insertion keeps j==pj; match/sub
                    // has pj==j-1. Both emit.
                    path_rev.push(lat.tokens[i].word.clone());
                    match pu {
                        Some(u) => {
                            i = u;
                            j = pj;
                        }
                        None => break,
                    }
                }
            }
            if j == 0 && back[i][j].0.is_none() {
                // Check whether token i itself was emitted from START:
                // back[i][0] = (None,0) means insertion from START.
                if path_rev.last().map(|s| s.as_str()) != Some(lat.tokens[i].word.as_str()) {
                    path_rev.push(lat.tokens[i].word.clone());
                }
                break;
            }
            if j > r {
                break;
            }
        }
    }
    path_rev.reverse();
    Ok(OracleResult {
        errors,
        ref_len: r as u32,
        wer: if r == 0 {
            0.0
        } else {
            errors as f64 / r as f64
        },
        path: path_rev,
        nodes: n,
        arcs: lat.arcs.len(),
    })
}

/// Filter lattice text to top-`k` tokens per end frame + induced arcs.
/// Tokens with source `a2-1best` (the 1-best backbone) are always kept.
pub fn filter_k(lat_text: &str, k: usize) -> Result<lattice::Lattice, String> {
    let lat = lattice::read_lattice(lat_text)?;
    let mut by_end: HashMap<u32, Vec<&lattice::Token>> = HashMap::new();
    for t in &lat.tokens {
        by_end.entry(t.end).or_default().push(t);
    }
    let mut keep: HashSet<u64> = HashSet::new();
    for t in &lat.tokens {
        if t.source == "a2-1best" {
            keep.insert(t.id);
        }
    }
    for v in by_end.values_mut() {
        v.sort_by(|a, b| b.acoustic_llr.partial_cmp(&a.acoustic_llr).unwrap());
        v.truncate(k);
        for t in v.iter() {
            keep.insert(t.id);
        }
    }
    let tokens: Vec<lattice::Token> = lat
        .tokens
        .into_iter()
        .filter(|t| keep.contains(&t.id))
        .collect();
    let arcs: Vec<lattice::Arc> = lat
        .arcs
        .into_iter()
        .filter(|a| keep.contains(&a.from) && keep.contains(&a.to))
        .collect();
    Ok(lattice::Lattice {
        utterance: lat.utterance,
        frame_shift_ms: lat.frame_shift_ms,
        producer: lat.producer,
        tokens,
        arcs,
    })
}

/// K sweep over one lattice text: (K, oracle, kept tokens, kept arcs).
pub fn sweep_k(
    lat_text: &str,
    reference: &[String],
    ks: &[usize],
) -> Result<Vec<(usize, OracleResult, usize, usize)>, String> {
    let mut out = Vec::new();
    for &k in ks {
        let lat = filter_k(lat_text, k)?;
        let nt = lat.tokens.len();
        let na = lat.arcs.len();
        let o = lattice_oracle_wer(&lat, reference)?;
        out.push((k, o, nt, na));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lat_of(words: &[(&str, u32, u32, f64)]) -> lattice::Lattice {
        let tokens: Vec<lattice::Token> = words
            .iter()
            .enumerate()
            .map(|(i, &(w, s, e, llr))| lattice::Token {
                id: i as u64,
                word: w.to_string(),
                start: s,
                end: e,
                source: "t".to_string(),
                acoustic_llr: llr,
                lexical_llr: None,
                speaker_llr: None,
                verify_llr: None,
                user_llr: None,
            })
            .collect();
        let mut arcs = Vec::new();
        for i in 0..tokens.len() {
            for j in 0..tokens.len() {
                if i != j && tokens[i].end <= tokens[j].start {
                    arcs.push(lattice::Arc {
                        from: i as u64,
                        to: j as u64,
                    });
                }
            }
        }
        lattice::Lattice {
            utterance: "u".to_string(),
            frame_shift_ms: 10.0,
            producer: "t".to_string(),
            tokens,
            arcs,
        }
    }

    fn w(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn oracle_finds_exact_path() {
        let lat = lat_of(&[("a", 0, 1, -1.0), ("b", 2, 3, -1.0)]);
        let o = lattice_oracle_wer(&lat, &w(&["a", "b"])).expect("oracle");
        assert_eq!(o.errors, 0);
    }

    #[test]
    fn oracle_beats_single_wrong_token() {
        // Two alternatives for one region; oracle picks the right one.
        let lat = lat_of(&[("wrong", 0, 1, -0.5), ("right", 0, 1, -2.0)]);
        let o = lattice_oracle_wer(&lat, &w(&["right"])).expect("oracle");
        assert_eq!(o.errors, 0);
    }

    #[test]
    fn oracle_counts_deletion_when_empty() {
        let lat = lat_of(&[]);
        let o = lattice_oracle_wer(&lat, &w(&["a", "b"])).expect("oracle");
        assert_eq!(o.errors, 2);
    }

    #[test]
    fn k_filter_keeps_best_per_end() {
        let lat = lat_of(&[("a", 0, 1, -3.0), ("b", 0, 1, -1.0), ("c", 0, 1, -2.0)]);
        let text = lattice::write_lattice(&lat).expect("write");
        let f = filter_k(&text, 2).expect("filter");
        assert_eq!(f.tokens.len(), 2);
        assert!(f.tokens.iter().any(|t| t.word == "b"));
        assert!(f.tokens.iter().any(|t| t.word == "c"));
    }

    #[test]
    fn sweep_returns_all_ks() {
        let lat = lat_of(&[("a", 0, 1, -1.0)]);
        let text = lattice::write_lattice(&lat).expect("write");
        let s = sweep_k(&text, &w(&["a"]), &[3, 5, 10, 20]).expect("sweep");
        assert_eq!(s.len(), 4);
        assert!(s.iter().all(|(_, o, _, _)| o.errors == 0));
    }

    #[test]
    fn oracle_enters_mid_lattice_without_prefix_penalty() {
        // Regression (DEV-B 2026-10-31): the first correct word had incoming
        // arcs from garbage tokens; oracle must still start there for free.
        let lat = lat_of(&[("zzz", 0, 0, -0.1), ("a", 1, 2, -1.0), ("b", 3, 4, -1.0)]);
        // Drop the auto arc zzz->a? No: keep it — entry must be free anyway.
        assert!(lat.arcs.iter().any(|a| a.from == 0 && a.to == 1));
        let o = lattice_oracle_wer(&lat, &w(&["a", "b"])).expect("oracle");
        assert_eq!(o.errors, 0, "path={:?}", o.path);
    }

    #[test]
    fn oracle_exits_mid_lattice_without_suffix_penalty() {
        let lat = lat_of(&[("a", 0, 1, -1.0), ("b", 2, 3, -1.0), ("zzz", 4, 5, -0.1)]);
        let o = lattice_oracle_wer(&lat, &w(&["a", "b"])).expect("oracle");
        assert_eq!(o.errors, 0, "path={:?}", o.path);
    }
}
