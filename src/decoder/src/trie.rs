//! Pronunciation trie over phone strings.
//!
//! Each non-root node carries the phone on its incoming arc; a node may end
//! one or more words (homophones share the path, each recorded). The decoder
//! expands every trie node into its phone's HMM states at search time.

use std::collections::HashMap;

/// One lexicon entry: a word plus a single pronunciation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PronEntry {
    pub word: String,
    pub phones: Vec<String>,
}

/// One trie node. Node 0 is the root (`phone: None`).
#[derive(Debug, Clone, Default)]
pub struct TrieNode {
    pub phone: Option<String>,
    pub children: Vec<usize>,
    pub words: Vec<String>,
}

/// Prefix tree of pronunciations, indexed by node id.
#[derive(Debug, Clone, Default)]
pub struct PronTrie {
    pub nodes: Vec<TrieNode>,
}

impl PronTrie {
    /// Build a trie from pronunciation entries. Entries with an empty phone
    /// string are skipped (a word with no phones can never match).
    pub fn build(entries: &[PronEntry]) -> Self {
        let mut trie = Self {
            nodes: vec![TrieNode::default()],
        };
        for entry in entries {
            if entry.phones.is_empty() {
                continue;
            }
            let mut idx = 0;
            for phone in &entry.phones {
                idx = match trie.find_child(idx, phone) {
                    Some(next) => next,
                    None => {
                        let next = trie.nodes.len();
                        trie.nodes.push(TrieNode {
                            phone: Some(phone.clone()),
                            children: Vec::new(),
                            words: Vec::new(),
                        });
                        trie.nodes[idx].children.push(next);
                        next
                    }
                };
            }
            if !trie.nodes[idx].words.contains(&entry.word) {
                trie.nodes[idx].words.push(entry.word.clone());
            }
        }
        trie
    }

    /// Build a trie over word-internal triphone unit keys.
    ///
    /// Each entry's phone string is expanded position-wise with
    /// `hmm::triphone::unit_keys_for_word`: full `L-C+R` units
    /// word-internally, biphone units (`C+R` / `L-C`) at the word edges, and
    /// the bare center phone for single-phone words. Node labels are the
    /// unit keys (the decoder's acoustic lookup resolves them against the
    /// tied model, falling back to the center monophone for unseen units);
    /// word identity, homophone sharing, and LM lookahead still operate on
    /// words, unchanged from [`PronTrie::build`]. Cross-word contexts are
    /// excluded: every word starts with no left context and ends with no
    /// right context. Entries with an empty phone string are skipped.
    ///
    /// Note: words sharing a center-phone prefix but diverging later do NOT
    /// share prefix nodes (the edge units carry the differing right
    /// context), so a triphone trie is wider than its monophone twin.
    pub fn build_triphone(entries: &[PronEntry]) -> Self {
        let expanded: Vec<PronEntry> = entries
            .iter()
            .map(|entry| PronEntry {
                word: entry.word.clone(),
                phones: hmm::triphone::unit_keys_for_word(&entry.phones),
            })
            .collect();
        Self::build(&expanded)
    }

    /// Child of `node` reached by the exact arc label, if any.
    ///
    /// The label is a monophone on [`PronTrie::build`] tries and a unit key
    /// on [`PronTrie::build_triphone`] tries; matching is exact in both
    /// cases (no center backoff — use `hmm::triphone::center_of_unit` when
    /// a center view is needed).
    pub fn child(&self, node: usize, phone: &str) -> Option<usize> {
        self.find_child(node, phone)
    }
    /// Per-node LM lookahead: max static word score over the node's subtree.
    ///
    /// `word_score` maps UPPER-CASE trie words to static log-domain scores
    /// (the decoder passes unigram log-probs); words absent from the map
    /// score `default`. Each node's value is the max over its terminal words
    /// and all descendants, so values never increase along a root-to-leaf
    /// path. The decoder adds these to tokens for pruning rank only — stored
    /// path scores stay exact.
    pub fn build_lookahead(&self, word_score: &HashMap<String, f64>, default: f64) -> Vec<f64> {
        let mut out = vec![default; self.nodes.len()];
        // `build` appends children after parents, so reverse id order visits
        // every child before its parent (a valid post-order).
        for idx in (0..self.nodes.len()).rev() {
            let mut best = default;
            for word in &self.nodes[idx].words {
                best = best.max(word_score.get(word).copied().unwrap_or(default));
            }
            for &child in &self.nodes[idx].children {
                debug_assert!(child > idx, "trie child id must exceed parent id");
                best = best.max(out[child]);
            }
            out[idx] = best;
        }
        out
    }
    /// True when the trie holds no pronunciations (root only).
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Number of distinct words stored.
    pub fn word_count(&self) -> usize {
        let mut words: Vec<&str> = self
            .nodes
            .iter()
            .flat_map(|n| n.words.iter().map(String::as_str))
            .collect();
        words.sort_unstable();
        words.dedup();
        words.len()
    }

    fn find_child(&self, node: usize, phone: &str) -> Option<usize> {
        self.nodes[node]
            .children
            .iter()
            .copied()
            .find(|&c| self.nodes[c].phone.as_deref() == Some(phone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, phones: &[&str]) -> PronEntry {
        PronEntry {
            word: word.to_string(),
            phones: phones.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn shared_prefix_shares_nodes() {
        let trie = PronTrie::build(&[entry("AB", &["AH", "B"]), entry("AD", &["AH", "D"])]);
        // root + AH + B + D
        assert_eq!(trie.nodes.len(), 4);
        assert_eq!(trie.word_count(), 2);
        let ah = trie.child(0, "AH").expect("AH child");
        assert!(trie.child(ah, "B").is_some());
        assert!(trie.child(ah, "D").is_some());
    }

    #[test]
    fn homophones_share_terminal() {
        let trie = PronTrie::build(&[entry("W1", &["AH", "B"]), entry("W2", &["AH", "B"])]);
        assert_eq!(trie.nodes.len(), 3);
        assert_eq!(trie.word_count(), 2);
        let b = trie.child(trie.child(0, "AH").unwrap(), "B").unwrap();
        assert_eq!(trie.nodes[b].words.len(), 2);
    }

    #[test]
    fn empty_pronunciation_skipped() {
        let trie = PronTrie::build(&[entry("W", &[])]);
        assert!(trie.is_empty());
        assert_eq!(trie.word_count(), 0);
    }
    #[test]
    fn lookahead_is_subtree_max() {
        let trie = PronTrie::build(&[
            entry("AB", &["AH", "B"]),
            entry("AD", &["AH", "D"]),
            entry("X", &["Z"]),
        ]);
        let scores: HashMap<String, f64> = [
            ("AB".to_string(), -2.0),
            ("AD".to_string(), -5.0),
            ("X".to_string(), -1.0),
        ]
        .into_iter()
        .collect();
        let la = trie.build_lookahead(&scores, -99.0);
        assert_eq!(la.len(), trie.nodes.len());
        // Root sees the global max; shared prefix sees its subtree max only.
        assert_eq!(la[0], -1.0);
        let ah = trie.child(0, "AH").expect("AH child");
        assert_eq!(la[ah], -2.0);
        let b = trie.child(ah, "B").expect("B child");
        assert_eq!(la[b], -2.0);
        let d = trie.child(ah, "D").expect("D child");
        assert_eq!(la[d], -5.0);
        let z = trie.child(0, "Z").expect("Z child");
        assert_eq!(la[z], -1.0);
    }
    #[test]
    fn lookahead_missing_words_take_default() {
        let trie = PronTrie::build(&[entry("AB", &["AH", "B"])]);
        let scores: HashMap<String, f64> = [("ELSEWHERE".to_string(), -0.5)].into_iter().collect();
        let la = trie.build_lookahead(&scores, -99.0);
        assert!(la.iter().all(|&v| v == -99.0));
        let empty = PronTrie::build(&[entry("W", &[])]);
        assert_eq!(empty.build_lookahead(&scores, -99.0), vec![-99.0]);
    }

    #[test]
    fn triphone_build_expands_word_internal_units() {
        // 3-phone word: biphone edges + full triphone inside.
        let trie = PronTrie::build_triphone(&[entry("W", &["AH", "B", "D"])]);
        // root + AH+B + AH-B+D + B-D
        assert_eq!(trie.nodes.len(), 4);
        let n1 = trie.child(0, "AH+B").expect("first unit");
        let n2 = trie.child(n1, "AH-B+D").expect("middle unit");
        let n3 = trie.child(n2, "B-D").expect("last unit");
        assert_eq!(trie.nodes[n3].words, vec!["W".to_string()]);
        // Centers recover the monophone string.
        let centers: Vec<&str> = [n1, n2, n3]
            .iter()
            .map(|&n| hmm::triphone::center_of_unit(trie.nodes[n].phone.as_deref().unwrap()))
            .collect();
        assert_eq!(centers, vec!["AH", "B", "D"]);
    }

    #[test]
    fn triphone_build_splits_shared_prefix_by_right_context() {
        // Same center prefix AH, diverging tails: the edge units differ, so
        // the prefix is NOT shared (word-internal right context).
        let trie =
            PronTrie::build_triphone(&[entry("AB", &["AH", "B"]), entry("AD", &["AH", "D"])]);
        // root + AH+B + AH-B + AH+D + AH-D
        assert_eq!(trie.nodes.len(), 5);
        assert!(trie.child(0, "AH").is_none(), "no monophone arc remains");
        let ab1 = trie.child(0, "AH+B").expect("AB first unit");
        let ab2 = trie.child(ab1, "AH-B").expect("AB second unit");
        assert_eq!(trie.nodes[ab2].words, vec!["AB".to_string()]);
        let ad1 = trie.child(0, "AH+D").expect("AD first unit");
        let ad2 = trie.child(ad1, "AH-D").expect("AD second unit");
        assert_eq!(trie.nodes[ad2].words, vec!["AD".to_string()]);
        assert_eq!(trie.word_count(), 2);
    }

    #[test]
    fn triphone_build_single_phone_word_uses_monophone_key() {
        let trie = PronTrie::build_triphone(&[entry("S", &["AH"])]);
        assert_eq!(trie.nodes.len(), 2);
        let n = trie.child(0, "AH").expect("monophone unit");
        assert_eq!(trie.nodes[n].words, vec!["S".to_string()]);
    }

    #[test]
    fn triphone_build_skips_empty_pronunciation() {
        let trie = PronTrie::build_triphone(&[entry("W", &[])]);
        assert!(trie.is_empty());
    }
}
