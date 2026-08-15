//! Stage 2 arena ingestion — hash-consing of phoneme-candidate facts (a6d.1).
//!
//! Implements the NodeId equality rule from
//! `docs/design/hash-consing-equality.md`: two facts collapse to the same
//! NodeId iff their **canonical content is identical** —
//!
//! ```text
//! content = (phoneme: ARPABET,
//!            context_window: [ARPABET],   // left/right phonological context
//!            feature_signature: [i32])    // quantized acoustic features
//! ```
//!
//! Equality is exact and normalized (element-wise, order-preserving) — the
//! arena is append-only and never sees continuous floats: Stage 1 quantizes
//! features first ([`quantize`]). The arena wraps the fabric's `FactId` as
//! the NodeId and dedupes identical content on ingest.

use std::collections::HashMap;

use fabric::FactId;
use invariant_ppt::assert_invariant;

/// Canonical, normalized fact content (hash-consing-equality.md §2).
///
/// Equality is derived: identical `phoneme` + `context_window` +
/// `feature_signature` (all element-wise, order-preserving).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Content {
    /// ARPABET phoneme (aem.10 alphabet).
    pub phoneme: String,
    /// Left/right phonological context, order-preserving.
    pub context_window: Vec<String>,
    /// Quantized acoustic feature signature (already discrete).
    pub feature_signature: Vec<i32>,
}

/// A phoneme-candidate fact entering the arena.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Candidate {
    pub content: Content,
}

impl Candidate {
    pub fn new(
        phoneme: impl Into<String>,
        context_window: Vec<String>,
        feature_signature: Vec<i32>,
    ) -> Self {
        Candidate {
            content: Content {
                phoneme: phoneme.into(),
                context_window,
                feature_signature,
            },
        }
    }
}

/// Quantize continuous features into discrete bins (done once, at the
/// Stage 1 boundary — hash-consing-equality.md §4). Two floats in the same
/// bin produce identical signatures.
pub fn quantize(signature: &[f32], epsilon: f32) -> Vec<i32> {
    assert_invariant(
        epsilon > 0.0,
        "quantize_epsilon_positive",
        Some("stage2 quantize"),
    );
    signature
        .iter()
        .map(|x| (x / epsilon).round() as i32)
        .collect()
}

/// Per-utterance ingest summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UtteranceStats {
    /// Facts interned for the first time anywhere (fabric grew).
    pub new_facts: usize,
    /// Facts that already existed (reused NodeIds, including within the
    /// same utterance).
    pub reused_facts: usize,
    /// NodeIds for every candidate, in order.
    pub ids: Vec<FactId>,
}

/// The Stage 2 arena: append-only, hash-consed fact store over the fabric.
#[derive(Default)]
pub struct Arena {
    /// Underlying fabric holding per-FactId state (justifications later).
    pub fabric: fabric::Fabric,
    /// Content -> NodeId (hash-consing map).
    content_to_id: HashMap<Content, FactId>,
    /// NodeId -> content (append-only; content never changes after intern).
    id_to_content: HashMap<FactId, Content>,
    /// Content -> first utterance index it was seen in (memoization proof).
    first_seen_utterance: HashMap<Content, usize>,
    /// Next utterance index (bumped by [`Arena::ingest_utterance`]).
    next_utterance: usize,
}

impl Arena {
    pub fn new() -> Self {
        Arena {
            fabric: fabric::Fabric::new(),
            content_to_id: HashMap::new(),
            id_to_content: HashMap::new(),
            first_seen_utterance: HashMap::new(),
            next_utterance: 0,
        }
    }

    /// Ingest a candidate, returning its interned NodeId.
    ///
    /// Identical content resolves to the existing NodeId (INV:
    /// `hash_consing_injective`); distinct content gets a fresh NodeId (INV:
    /// `hash_consing_distinct`). Content is never mutated after intern (INV:
    /// `arena_append_only`).
    pub fn ingest(&mut self, candidate: &Candidate) -> FactId {
        if let Some(&existing) = self.content_to_id.get(&candidate.content) {
            // INV: hash_consing_injective — identical content -> same NodeId.
            assert_invariant(
                self.id_to_content
                    .get(&existing)
                    .is_some_and(|c| c == &candidate.content),
                "hash_consing_injective",
                Some(&format!("stage2 ingest: {}", candidate.content.phoneme)),
            );
            return existing;
        }
        let id = self.fabric.intern();
        // INV: hash_consing_distinct — a fresh id never aliases prior content.
        assert_invariant(
            !self.id_to_content.contains_key(&id),
            "hash_consing_distinct",
            Some(&format!("stage2 ingest: {}", candidate.content.phoneme)),
        );
        let content = candidate.content.clone();
        self.content_to_id.insert(candidate.content.clone(), id);
        self.id_to_content.insert(id, content.clone());
        // INV: arena_append_only — stored content equals what was interned.
        assert_invariant(
            self.id_to_content.get(&id) == Some(&content),
            "arena_append_only",
            Some(&format!("stage2 ingest: {}", candidate.content.phoneme)),
        );
        id
    }

    /// Look up a NodeId's content (never mutated after intern).
    pub fn content(&self, id: FactId) -> Option<&Content> {
        self.id_to_content.get(&id)
    }

    /// Number of distinct facts interned.
    pub fn fact_count(&self) -> usize {
        self.id_to_content.len()
    }

    /// Resolve a NodeId by content (None if never ingested).
    pub fn node_id(&self, content: &Content) -> Option<FactId> {
        self.content_to_id.get(content).copied()
    }

    /// Ingest a whole utterance's candidates.
    ///
    /// A fragment that recurred in an *earlier* utterance resolves to the
    /// NodeId from its first occurrence and does not grow the fabric (INV:
    /// `cross_utterance_memoization`).
    pub fn ingest_utterance(&mut self, candidates: &[Candidate]) -> UtteranceStats {
        let utterance = self.next_utterance;
        self.next_utterance += 1;
        let mut stats = UtteranceStats {
            new_facts: 0,
            reused_facts: 0,
            ids: Vec::with_capacity(candidates.len()),
        };
        for candidate in candidates {
            let content = &candidate.content;
            let first_seen = self.first_seen_utterance.get(content).copied();
            let id = self.ingest(candidate);
            match first_seen {
                Some(first) if first < utterance => {
                    // INV: cross_utterance_memoization — a fragment first seen
                    // in an earlier utterance resolves to that same NodeId, so
                    // the fabric does not grow.
                    assert_invariant(
                        self.node_id(content) == Some(id),
                        "cross_utterance_memoization",
                        Some(&format!(
                            "stage2 utterance {utterance}: {}",
                            content.phoneme
                        )),
                    );
                    stats.reused_facts += 1;
                }
                Some(_) => {
                    // In-utterance repeat: same NodeId (hash-consing
                    // injective), not a cross-utterance recurrence.
                    stats.reused_facts += 1;
                }
                None => {
                    stats.new_facts += 1;
                    self.first_seen_utterance.insert(content.clone(), utterance);
                }
            }
            stats.ids.push(id);
        }
        stats
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
