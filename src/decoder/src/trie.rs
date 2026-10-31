//! Pronunciation trie over phone strings.
//!
//! Each non-root node carries the phone on its incoming arc; a node may end
//! one or more words (homophones share the path, each recorded). The decoder
//! expands every trie node into its phone's HMM states at search time.

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

    /// Child of `node` reached by `phone`, if any.
    pub fn child(&self, node: usize, phone: &str) -> Option<usize> {
        self.find_child(node, phone)
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
}
