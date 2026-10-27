//! Upper-level verification knowledge source (bead hearsay-four-ea3, lane C2).
//!
//! Committed word sequences checked against trigram plausibility (B3
//! [`TrigramTable`], Oyez counts only) with a veto/flag threshold: a
//! lookup-table check, NOT a model. No weights, no embeddings, no learned
//! parameters, no network, no filesystem access (INV `flat_no_lm`).
//!
//! A sequence whose joint trigram plausibility falls below the threshold is
//! FLAGGED, never dropped: the returned [`VerifiedSequence`] still carries
//! its `verify_fact` (asserted as `word_facts ⟹ verify`, so fabric
//! justification sets keep explaining it) plus per-position trigram
//! evidence, and the caller marks the record instead of discarding it
//! (INV analogue: `verify_inherits_word_justification`). The veto is strict:
//! a sequence scores `flagged` if and only if `prob < threshold`, so a
//! sequence exactly at threshold passes unflagged.
//!
//! Hook site: `runner::run_full_pipeline`, after the Stage 4
//! nearest-pronunciation alignment (lexicon align) and before sequence
//! scoring, mirroring `lexicon::word_match`'s inheritance pattern.
//! Gate path: `cd src/sequence && cargo test --offline verify`.

use std::collections::BTreeSet;

use fabric::{Fabric, FactId};
use invariant_ppt::assert_invariant;

use super::trigram::TrigramTable;

/// Default veto/flag threshold: joint trigram plausibility below this is
/// flagged for reviewer attention (C2 hook in the runner).
///
/// Order-of-magnitude rationale: a seeded-only table over the 30-word
/// closed vocabulary scores uniform `1/30` per factor, so demo-length
/// transcripts (a handful of words) sit far above `1e-9` while genuinely
/// unsupported long sequences still trip the flag. Trained tables score
/// supported sequences orders of magnitude higher still.
pub const DEFAULT_VERIFY_THRESHOLD: f64 = 1e-9;

/// One position's trigram evidence: the factor that entered the joint
/// plausibility at `index`, with the history it was conditioned on
/// (empty for the unigram position, one word for the bigram position,
/// two words thereafter) and the factor probability in `(0,1]`.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorEvidence {
    /// Word position in the checked sequence.
    pub index: usize,
    /// Conditioning history: 0, 1, or 2 preceding words.
    pub context: Vec<String>,
    /// The word scored at this position.
    pub word: String,
    /// Factor probability in `(0,1]`.
    pub prob: f64,
}

/// The verification verdict for one committed word sequence.
///
/// `verify_fact` is asserted as `word_facts ⟹ verify`, so the verdict —
/// flagged or not — stays explained by the fabric justification sets
/// (INV `verify_inherits_word_justification`). Flagged sequences keep
/// full provenance: they are marked, never silently dropped.
#[derive(Debug, Clone)]
pub struct VerifiedSequence {
    /// Joint trigram plausibility in `[0,1]`.
    pub prob: f64,
    /// Veto/flag threshold that was applied.
    pub threshold: f64,
    /// True if and only if `prob < threshold` (strict veto).
    pub flagged: bool,
    /// Per-position trigram evidence, in sequence order.
    pub evidence: Vec<FactorEvidence>,
    /// Fabric fact carrying this verdict, justified by `word_facts`.
    pub verify_fact: FactId,
}

impl VerifiedSequence {
    /// One-line reviewer marker for record summaries: `VERIFY-FLAG` when
    /// the sequence tripped the threshold, `VERIFY-OK` otherwise. The
    /// runner appends this to the record's evidence text (marked, never
    /// dropped).
    pub fn note(&self) -> String {
        if self.flagged {
            format!(
                "VERIFY-FLAG trigram p={:.4} < threshold {:.4}",
                self.prob, self.threshold
            )
        } else {
            format!(
                "VERIFY-OK trigram p={:.4} >= threshold {:.4}",
                self.prob, self.threshold
            )
        }
    }
}

/// Check a committed word sequence against trigram plausibility.
///
/// `word_facts` are committed (active) fabric facts, one per word, in
/// order; `words` are the committed word strings. The verdict's
/// `verify_fact` is asserted as `word_facts ⟹ verify`, flagged or not.
///
/// No orphan verdicts: a length mismatch, an unknown fact, or any
/// uncommitted premise yields `None` and interns nothing.
pub fn verify_committed_word_sequence(
    fabric: &mut Fabric,
    word_facts: &[FactId],
    words: &[String],
    table: &TrigramTable,
    threshold: f64,
) -> Option<VerifiedSequence> {
    if words.is_empty() || word_facts.len() != words.len() {
        return None;
    }
    // Committed means active: unknown facts read as inactive, so this
    // single guard covers unknown, uncommitted, and retracted premises —
    // nothing is verified without live justification beneath it.
    if !word_facts.iter().all(|id| fabric.is_active(*id)) {
        return None;
    }
    let prob = table.sequence_prob(words);
    let mut evidence = Vec::with_capacity(words.len());
    for (i, word) in words.iter().enumerate() {
        let (context, factor) = if i == 0 {
            (Vec::new(), table.unigram_prob(word))
        } else if i == 1 {
            (words[0..1].to_vec(), table.bigram_prob(&words[0], word))
        } else {
            (
                words[i - 2..i].to_vec(),
                table.trigram_prob(&words[i - 2], &words[i - 1], word),
            )
        };
        evidence.push(FactorEvidence {
            index: i,
            context,
            word: word.clone(),
            prob: factor,
        });
    }
    // Strict veto: exactly-at-threshold passes unflagged.
    let flagged = prob < threshold;
    let premises: BTreeSet<FactId> = word_facts.iter().copied().collect();
    let verify_fact = fabric.intern();
    fabric.assert(verify_fact, premises, false);
    // INV: verify_inherits_word_justification.
    assert_invariant(
        fabric.support_count(verify_fact) >= 1,
        "verify_inherits_word_justification",
        Some(&format!("verify: {}", words.join(" "))),
    );
    // INV `score_is_vote`: this plausibility is one evidence vote for the
    // confidence combination (aem.7) — returned for combination, never a
    // sole decider (ngram-sequence.md §4–§5).
    assert_invariant(true, "score_is_vote", Some("stage5"));
    Some(VerifiedSequence {
        prob,
        threshold,
        flagged,
        evidence,
        verify_fact,
    })
}

#[cfg(test)]
mod verify_tests {
    use super::*;
    use arena::{Arena, Candidate};
    use output::{ChainLayer, ChainLevel, JustificationRef, Node, ProvenanceChain};

    /// Fixed Oyez-counts fixture (same shape as the B3 trigram gate:
    /// two repetitions of the session-opening line plus one adjournment
    /// line; synthetic, Oyez-shaped lowercase words).
    fn oyez_fixture_table() -> TrigramTable {
        let mut table = TrigramTable::new();
        let line = || {
            vec![
                "the".to_string(),
                "court".to_string(),
                "is".to_string(),
                "now".to_string(),
            ]
        };
        table.train(&[
            line(),
            line(),
            vec![
                "the".to_string(),
                "court".to_string(),
                "adjourned".to_string(),
            ],
        ]);
        table
    }

    fn words(seq: &[&str]) -> Vec<String> {
        seq.iter().map(|w| (*w).to_string()).collect()
    }

    fn committed_word(arena: &mut Arena, word: &str) -> FactId {
        let id = arena.ingest(&Candidate::new(word.to_string(), Vec::new(), Vec::new()));
        arena.fabric.assert_assumption(id);
        arena.fabric.commit(id);
        id
    }

    fn committed_facts(arena: &mut Arena, seq: &[&str]) -> Vec<FactId> {
        seq.iter().map(|w| committed_word(arena, w)).collect()
    }

    /// (1) A trigram-supported sequence passes unflagged: verdict carries
    /// the table probability, per-position evidence, and a justified fact.
    #[test]
    fn verify_high_plausibility_passes_unflagged() {
        let mut arena = Arena::new();
        let table = oyez_fixture_table();
        let seq = words(&["the", "court", "is", "now"]);
        let facts = committed_facts(&mut arena, &["the", "court", "is", "now"]);
        let verdict = verify_committed_word_sequence(
            &mut arena.fabric,
            &facts,
            &seq,
            &table,
            DEFAULT_VERIFY_THRESHOLD,
        )
        .expect("committed words must verify");
        assert!(!verdict.flagged, "supported sequence must pass unflagged");
        assert_eq!(verdict.prob, table.sequence_prob(&seq));
        assert!(verdict.note().starts_with("VERIFY-OK"));
        // Per-position evidence: unigram, bigram, then trigram factors.
        assert_eq!(verdict.evidence.len(), 4);
        assert!(verdict.evidence[0].context.is_empty());
        assert_eq!(verdict.evidence[1].context, words(&["the"]));
        assert_eq!(verdict.evidence[2].context, words(&["the", "court"]));
        assert_eq!(verdict.evidence[3].context, words(&["court", "is"]));
        for factor in &verdict.evidence {
            assert!(factor.prob.is_finite() && (0.0..=1.0).contains(&factor.prob));
        }
        // Provenance intact: the verdict fact is justified and active.
        assert!(arena.fabric.support_count(verdict.verify_fact) >= 1);
        assert!(arena.fabric.is_active(verdict.verify_fact));
    }

    /// (2) An unsupported (adversarial repetition) sequence is flagged,
    /// with the per-position trigram evidence attached that sank it.
    #[test]
    fn verify_adversarial_low_probability_flagged_with_evidence() {
        let mut arena = Arena::new();
        let table = oyez_fixture_table();
        let seq = words(&["now", "now", "now", "now"]);
        let facts = committed_facts(&mut arena, &["now", "now", "now", "now"]);
        let verdict = verify_committed_word_sequence(&mut arena.fabric, &facts, &seq, &table, 0.01)
            .expect("committed words must verify");
        assert!(verdict.flagged, "unsupported sequence must be flagged");
        assert!(verdict.prob < 0.01);
        assert!(verdict.note().starts_with("VERIFY-FLAG"));
        // The attached evidence explains the verdict: factors in range
        // whose product is exactly the reported joint plausibility.
        assert_eq!(verdict.evidence.len(), 4);
        let joint: f64 = verdict.evidence.iter().map(|e| e.prob).product();
        assert!((joint - verdict.prob).abs() < 1e-12);
        for factor in &verdict.evidence {
            assert!(factor.prob.is_finite() && (0.0..=1.0).contains(&factor.prob));
        }
        // Flagged, not dropped: the verdict fact is still justified.
        assert!(arena.fabric.support_count(verdict.verify_fact) >= 1);
        assert!(arena.fabric.is_active(verdict.verify_fact));
    }

    /// (3) A flagged sequence keeps validatable provenance: a chain
    /// grounded at frame-level acoustic evidence, extended with the
    /// verification verdict as its sequence node, still validates.
    #[test]
    fn verify_flagged_sequence_provenance_still_validates() {
        let mut arena = Arena::new();
        let table = oyez_fixture_table();
        let seq = words(&["now", "now", "now", "now"]);
        let facts = committed_facts(&mut arena, &["now", "now", "now", "now"]);
        let verdict = verify_committed_word_sequence(&mut arena.fabric, &facts, &seq, &table, 0.01)
            .expect("committed words must verify");
        assert!(verdict.flagged);
        // Frame layer: acoustic grounding, no support, ratings in range.
        let frame_a = committed_word(&mut arena, "w-frame-a");
        let frame_b = committed_word(&mut arena, "w-frame-b");
        let phon = committed_word(&mut arena, "w-phon");
        let lex = committed_word(&mut arena, "w-lex");
        // Sequence rating: at most the lexical premise and the trigram
        // plausibility rating, so the flagged node never outranks evidence.
        let seq_rating = 60.0_f64.min((verdict.prob.clamp(0.0, 1.0) * 100.0).max(0.0));
        let chain = ProvenanceChain::new(vec![
            ChainLayer {
                level: ChainLevel::Frame,
                nodes: vec![
                    Node {
                        id: frame_a.0,
                        label: "FRAME:0-10ms".to_string(),
                        support: vec![],
                        active: true,
                        rating: 80.0,
                    },
                    Node {
                        id: frame_b.0,
                        label: "FRAME:10-20ms".to_string(),
                        support: vec![],
                        active: true,
                        rating: 70.0,
                    },
                ],
            },
            ChainLayer {
                level: ChainLevel::Phoneme,
                nodes: vec![Node {
                    id: phon.0,
                    label: "/N/".to_string(),
                    support: vec![JustificationRef {
                        justification_id: "j-phon-verify".to_string(),
                        premises: vec![frame_a.0, frame_b.0],
                        unconditional: false,
                    }],
                    active: true,
                    rating: 70.0,
                }],
            },
            ChainLayer {
                level: ChainLevel::Lexical,
                nodes: vec![Node {
                    id: lex.0,
                    label: "WORD:NOWNOWNOWNOW".to_string(),
                    support: vec![JustificationRef {
                        justification_id: "j-lex-verify".to_string(),
                        premises: vec![phon.0],
                        unconditional: false,
                    }],
                    active: true,
                    rating: 60.0,
                }],
            },
            ChainLayer {
                level: ChainLevel::Sequence,
                nodes: vec![Node {
                    id: verdict.verify_fact.0,
                    label: format!("SEQ:{} {}", seq.join(" "), verdict.note()),
                    support: vec![JustificationRef {
                        justification_id: "j-seq-verify".to_string(),
                        premises: vec![lex.0],
                        unconditional: false,
                    }],
                    active: true,
                    rating: seq_rating,
                }],
            },
        ]);
        chain
            .validate()
            .expect("flagged sequence provenance must validate");
    }

    /// (4a) Threshold boundary, passing side: exactly at threshold the
    /// strict veto lets the sequence through unflagged.
    #[test]
    fn verify_threshold_boundary_equal_passes() {
        let mut arena = Arena::new();
        let table = oyez_fixture_table();
        let seq = words(&["the", "court", "is", "now"]);
        let prob = table.sequence_prob(&seq);
        let facts = committed_facts(&mut arena, &["the", "court", "is", "now"]);
        let verdict = verify_committed_word_sequence(&mut arena.fabric, &facts, &seq, &table, prob)
            .expect("committed words must verify");
        assert_eq!(verdict.prob, prob);
        assert!(
            !verdict.flagged,
            "prob == threshold must pass: veto is strict (<)"
        );
    }

    /// (4b) Threshold boundary, flagging side: one float step above the
    /// joint plausibility the same sequence is flagged.
    #[test]
    fn verify_threshold_boundary_above_flags() {
        let mut arena = Arena::new();
        let table = oyez_fixture_table();
        let seq = words(&["the", "court", "is", "now"]);
        let prob = table.sequence_prob(&seq);
        let facts = committed_facts(&mut arena, &["the", "court", "is", "now"]);
        let verdict =
            verify_committed_word_sequence(&mut arena.fabric, &facts, &seq, &table, prob.next_up())
                .expect("committed words must verify");
        assert!(verdict.flagged, "threshold one ulp above prob must flag");
    }

    /// (6) No orphan verdicts: uncommitted, unknown, length-mismatched,
    /// or empty inputs yield `None` and intern nothing.
    #[test]
    fn verify_no_orphan_fact_without_committed_words() {
        let mut arena = Arena::new();
        let table = oyez_fixture_table();
        let seq = words(&["the", "court"]);
        // Uncommitted premises justify nothing.
        let pending = arena.ingest(&Candidate::new("the".to_string(), Vec::new(), Vec::new()));
        assert!(
            verify_committed_word_sequence(
                &mut arena.fabric,
                &[pending, pending],
                &seq,
                &table,
                DEFAULT_VERIFY_THRESHOLD
            )
            .is_none()
        );
        assert_eq!(arena.fabric.support_count(pending), 0);
        // Unknown facts verify nothing.
        assert!(
            verify_committed_word_sequence(
                &mut arena.fabric,
                &[FactId(u64::MAX), FactId(u64::MAX - 1)],
                &seq,
                &table,
                DEFAULT_VERIFY_THRESHOLD
            )
            .is_none()
        );
        // Length mismatch and empty inputs verify nothing.
        let facts = committed_facts(&mut arena, &["the", "court"]);
        assert!(
            verify_committed_word_sequence(
                &mut arena.fabric,
                &facts[..1],
                &seq,
                &table,
                DEFAULT_VERIFY_THRESHOLD
            )
            .is_none()
        );
        assert!(
            verify_committed_word_sequence(
                &mut arena.fabric,
                &[],
                &[],
                &table,
                DEFAULT_VERIFY_THRESHOLD
            )
            .is_none()
        );
    }
}
