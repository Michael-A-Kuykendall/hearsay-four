//! User-correction facts with exact retraction (bead
//! `hearsay-four-f56--correction-retraction`, split of P3).
//!
//! A user correction creates a USER fact set with provenance rooted at one
//! correction-event assumption fact:
//!
//! 1. **USER justification on the corrected token**: the corrected
//!    `WORD_TOKEN` occurrence is asserted with the correction event as its
//!    premise, so undo removes exactly that support (survival semantics:
//!    the token stays active if acoustic decoding still supports it). The
//!    event id doubles as the [`decision`](crate::decision) `add_user`
//!    premise — see the `decision_user_term_composes_with_undo` test.
//! 2. **Personal-LM count increments (R1b)**: the corrected sentence's
//!    unigram/bigram/trigram increments, keyed exactly like
//!    `lm::InspectableModel` (`word`, `"a b"`, `"a b c"`; `<s>` is never
//!    counted, `</s>` is counted once per sentence — the
//!    `KnTrigram::train` per-sentence contribution). Totals are recomputed
//!    from the live records, so undo subtracts exactly.
//! 3. **Pronunciation-variant evidence**: a `PRON_VARIANT:<word>:<phones>`
//!    type fact justified by the event, recorded **iff** the observed phone
//!    sequence disagrees with *every* lexicon pronunciation (exact-sequence
//!    comparison: an observation already in the lexicon carries no new
//!    evidence). Type-interned, so two corrections proposing the same new
//!    variant share one fact with two independent justification sets.
//! 4. **MAP sufficient-statistic contributions (R4)**: per-Gaussian `(n,
//!    sum, sumsq, frames)` deltas from the corrected frames, mirroring
//!    `hmm::AdaptStats` per `(phone, state, comp)` key. Totals are folded
//!    from the live records in event-id order, so undo is exact and the
//!    floating-point summation order is deterministic.
//!
//! # Byte-identity scope
//!
//! [`CorrectionLedger::state_bytes`] serializes the live correction set
//! deterministically. Apply-then-undo returns these bytes **bit-identically**.
//! The [`Fabric`] itself is intentionally *not* byte-identical across undo:
//! retracted facts and their justifications are preserved (invariant
//! `retraction_preserves`) and fact ids are never reused — the fabric is the
//! provenance journal, the ledger state is the derived model state. This
//! mirrors the R4 overlay rule: base immutable, adaptation recomputed.
//!
//! # Out of scope
//!
//! Whether a correction changes decoding is P3b (bead
//! `hearsay-four-6gt--correction-decoding-proof`), not here.

use crate::{Fabric, FactId};
use std::collections::{BTreeMap, BTreeSet};

/// Sentence-boundary tokens, mirroring `lm::normalize` (`BOS_TOKEN` /
/// `EOS_TOKEN`). `<s>` is never counted; `</s>` is counted once per sentence.
const BOS: &str = "<s>";
const EOS: &str = "</s>";

/// Source tag on the corrected `WORD_TOKEN` occurrence (item 1).
const CORRECTION_SOURCE: &str = "user-correction";

/// Type-fact prefix for pronunciation-variant evidence (item 3).
const VARIANT_PREFIX: &str = "PRON_VARIANT";

/// State-bytes magic (includes the format version).
const STATE_MAGIC: &[u8] = b"CORR1";

/// One Gaussian's address in the MAP overlay, mirroring `hmm::AdaptKey`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapKey {
    pub phone: String,
    pub state: usize,
    pub comp: usize,
}

/// Occupancy-weighted sufficient statistics for one Gaussian, mirroring
/// `hmm::AdaptStats`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapStats {
    /// Occupancy mass (n).
    pub n: f64,
    /// Occupancy-weighted first moment (sum-x) per dimension.
    pub sum: Vec<f64>,
    /// Occupancy-weighted second moment (sum-x2) per dimension.
    pub sumsq: Vec<f64>,
    /// Source frames that contributed.
    pub frames: u64,
}

impl MapStats {
    /// Zero statistics for `dim`-dimensional features.
    pub fn zeros(dim: usize) -> Self {
        Self {
            n: 0.0,
            sum: vec![0.0; dim],
            sumsq: vec![0.0; dim],
            frames: 0,
        }
    }

    fn add_in(&mut self, other: &MapStats) {
        debug_assert_eq!(self.sum.len(), other.sum.len());
        debug_assert_eq!(self.sumsq.len(), other.sumsq.len());
        self.n += other.n;
        for (slot, add) in self.sum.iter_mut().zip(other.sum.iter()) {
            *slot += *add;
        }
        for (slot, add) in self.sumsq.iter_mut().zip(other.sumsq.iter()) {
            *slot += *add;
        }
        self.frames += other.frames;
    }

    fn is_finite(&self) -> bool {
        self.n.is_finite()
            && self.sum.iter().all(|v| v.is_finite())
            && self.sumsq.iter().all(|v| v.is_finite())
    }
}

/// One correction's personal-LM count increments (item 2).
///
/// Key conventions match `lm::InspectableModel` exactly: bare words for
/// unigrams, `"a b"` for bigrams, `"a b c"` for trigrams.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LmDelta {
    pub unigram: BTreeMap<String, u64>,
    pub bigram: BTreeMap<String, u64>,
    pub trigram: BTreeMap<String, u64>,
}

impl LmDelta {
    fn add_in(&mut self, other: &LmDelta) {
        for (key, count) in &other.unigram {
            *self.unigram.entry(key.clone()).or_insert(0) += *count;
        }
        for (key, count) in &other.bigram {
            *self.bigram.entry(key.clone()).or_insert(0) += *count;
        }
        for (key, count) in &other.trigram {
            *self.trigram.entry(key.clone()).or_insert(0) += *count;
        }
    }
}

/// Per-sentence contribution, mirroring `KnTrigram::train` for one sentence:
/// pad `[<s>, <s>, ...words, </s>]`, count every padded token except `<s>`
/// as a unigram, and count every adjacent pair / triple.
fn lm_delta_for_sentence(sentence: &[String]) -> LmDelta {
    let mut padded: Vec<&str> = vec![BOS, BOS];
    padded.extend(sentence.iter().map(String::as_str));
    padded.push(EOS);
    let mut delta = LmDelta::default();
    for word in &padded {
        if *word != BOS {
            *delta.unigram.entry((*word).to_string()).or_insert(0) += 1;
        }
    }
    for pair in padded.windows(2) {
        *delta
            .bigram
            .entry(format!("{} {}", pair[0], pair[1]))
            .or_insert(0) += 1;
    }
    for triple in padded.windows(3) {
        *delta
            .trigram
            .entry(format!("{} {} {}", triple[0], triple[1], triple[2]))
            .or_insert(0) += 1;
    }
    delta
}

/// One MAP sufficient-statistic contribution from the corrected frames.
#[derive(Debug, Clone, PartialEq)]
pub struct MapContribution {
    pub key: MapKey,
    pub stats: MapStats,
}

/// Input for one user correction: the corrected token, its sentence, the
/// acoustic observation, and the MAP deltas from the corrected frames.
///
/// `corrected_sentence` and `corrected_word` must be normalized tokens
/// (lowercase, no spaces — the `lm::normalize` contract); keys are
/// space-joined, so a token containing a space would be ambiguous.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionInput {
    /// The corrected word (item 1 token text, item 3 variant word).
    pub corrected_word: String,
    /// The full corrected sentence (item 2 count source).
    pub corrected_sentence: Vec<String>,
    /// Corrected token span in frames (item 1, half-open).
    pub start_frame: u64,
    /// Corrected token span end in frames (item 1, half-open).
    pub end_frame: u64,
    /// Acoustics' phone sequence for the corrected span (item 3).
    pub observed_phones: Vec<String>,
    /// Lexicon pronunciations of the corrected word (item 3).
    pub lexicon_variants: Vec<Vec<String>>,
    /// MAP deltas from the corrected frames (item 4).
    pub map: Vec<MapContribution>,
}

/// The applied fact set for one correction: every item carries the `event`
/// provenance root.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedCorrection {
    /// The correction-event assumption fact: the provenance root. Retracting
    /// it (via [`CorrectionLedger::undo`]) withdraws items 1 and 3 through
    /// the fabric cascade; items 2 and 4 are dropped from the ledger.
    pub event: FactId,
    /// The corrected `WORD_TOKEN` occurrence (item 1).
    pub token: FactId,
    /// The `PRON_VARIANT` type fact (item 3), iff the observation disagreed
    /// with every lexicon pronunciation.
    pub variant: Option<FactId>,
    /// Personal-LM count increments (item 2).
    pub lm: LmDelta,
    /// MAP deltas folded by Gaussian key (item 4).
    pub map: BTreeMap<MapKey, MapStats>,
    /// Corrected word, for inspection and state bytes.
    pub word: String,
    /// Corrected sentence, for inspection and state bytes.
    pub sentence: Vec<String>,
    /// Token span start, for inspection and state bytes.
    pub start_frame: u64,
    /// Token span end, for inspection and state bytes.
    pub end_frame: u64,
    /// Observed phones, for inspection and state bytes.
    pub observed_phones: Vec<String>,
}

/// Failure modes for [`CorrectionLedger::apply`] and
/// [`CorrectionLedger::undo`]. `apply` validates the whole input before
/// interning anything, so a failed apply leaves no partial facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionError {
    /// No live correction with this event id.
    UnknownCorrection(FactId),
    /// `corrected_word` is empty.
    EmptyWord,
    /// `corrected_sentence` is empty.
    EmptySentence,
    /// A sentence token is empty.
    EmptyToken,
    /// `observed_phones` is empty (a non-empty span always observes phones).
    EmptyObservation,
    /// `end_frame <= start_frame`.
    BadSpan { start: u64, end: u64 },
    /// A MAP contribution's vector length is not the ledger feature dim.
    DimMismatch { expected: usize, got: usize },
    /// A MAP contribution carries NaN or infinite statistics.
    NonFiniteStat,
    /// A MAP contribution carries negative occupancy mass.
    NegativeN,
}

impl std::fmt::Display for CorrectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CorrectionError::UnknownCorrection(event) => {
                write!(f, "unknown correction event {}", event.0)
            }
            CorrectionError::EmptyWord => write!(f, "corrected word is empty"),
            CorrectionError::EmptySentence => write!(f, "corrected sentence is empty"),
            CorrectionError::EmptyToken => write!(f, "corrected sentence holds an empty token"),
            CorrectionError::EmptyObservation => write!(f, "observed phones are empty"),
            CorrectionError::BadSpan { start, end } => {
                write!(f, "bad correction span {start}..{end}")
            }
            CorrectionError::DimMismatch { expected, got } => {
                write!(f, "MAP dim mismatch: ledger holds {expected}, got {got}")
            }
            CorrectionError::NonFiniteStat => write!(f, "MAP contribution is not finite"),
            CorrectionError::NegativeN => write!(f, "MAP contribution has negative n"),
        }
    }
}

impl std::error::Error for CorrectionError {}

/// The live user-correction fact sets: items 1 and 3 live in the [`Fabric`]
/// (keyed by the event fact), items 2 and 4 live here per event. Undo
/// retracts the event fact and drops the record, so the derived model state
/// (`lm_totals`, `map_totals`, `state_bytes`) is recomputed from the
/// remaining records and returns bit-identically.
#[derive(Debug, Clone)]
pub struct CorrectionLedger {
    dim: usize,
    records: BTreeMap<FactId, AppliedCorrection>,
}

impl CorrectionLedger {
    /// New empty ledger for `dim`-dimensional MAP features.
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            records: BTreeMap::new(),
        }
    }

    /// The MAP feature dimension contributions must match.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Number of live corrections.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no corrections are live.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Apply one correction, creating the four-item fact set. Every item is
    /// provenanced to the returned event fact.
    pub fn apply(
        &mut self,
        fabric: &mut Fabric,
        input: &CorrectionInput,
    ) -> Result<AppliedCorrection, CorrectionError> {
        if input.corrected_word.is_empty() {
            return Err(CorrectionError::EmptyWord);
        }
        if input.corrected_sentence.is_empty() {
            return Err(CorrectionError::EmptySentence);
        }
        if input.corrected_sentence.iter().any(String::is_empty) {
            return Err(CorrectionError::EmptyToken);
        }
        if input.end_frame <= input.start_frame {
            return Err(CorrectionError::BadSpan {
                start: input.start_frame,
                end: input.end_frame,
            });
        }
        if input.observed_phones.is_empty() {
            return Err(CorrectionError::EmptyObservation);
        }
        for contribution in &input.map {
            if contribution.stats.sum.len() != self.dim {
                return Err(CorrectionError::DimMismatch {
                    expected: self.dim,
                    got: contribution.stats.sum.len(),
                });
            }
            if contribution.stats.sumsq.len() != self.dim {
                return Err(CorrectionError::DimMismatch {
                    expected: self.dim,
                    got: contribution.stats.sumsq.len(),
                });
            }
            if !contribution.stats.is_finite() {
                return Err(CorrectionError::NonFiniteStat);
            }
            if contribution.stats.n < 0.0 {
                return Err(CorrectionError::NegativeN);
            }
        }

        // Provenance root: the correction-event assumption.
        let event = fabric.intern();
        fabric.assert_assumption(event);
        let mut premises = BTreeSet::new();
        premises.insert(event);

        // Item 1: USER justification on the corrected token occurrence.
        let token = fabric.intern_word_token(
            input.corrected_word.clone(),
            input.start_frame,
            input.end_frame,
            CORRECTION_SOURCE,
        );
        fabric.assert(token, premises.clone(), false);

        // Item 3: variant evidence iff acoustics disagree with every
        // lexicon pronunciation (exact-sequence comparison).
        let variant = if input.lexicon_variants.contains(&input.observed_phones) {
            None
        } else {
            let content = format!(
                "{VARIANT_PREFIX}:{}:{}",
                input.corrected_word,
                input.observed_phones.join(" ")
            );
            let fact = fabric.intern_type(content);
            fabric.assert(fact, premises, false);
            Some(fact)
        };

        // Item 2: personal-LM count increments for the corrected sentence.
        let lm = lm_delta_for_sentence(&input.corrected_sentence);

        // Item 4: MAP deltas folded by Gaussian key (input-order fold; the
        // byte-identity guarantee covers apply-then-undo of the same inputs).
        let mut map: BTreeMap<MapKey, MapStats> = BTreeMap::new();
        for contribution in &input.map {
            map.entry(contribution.key.clone())
                .or_insert_with(|| MapStats::zeros(self.dim))
                .add_in(&contribution.stats);
        }

        let record = AppliedCorrection {
            event,
            token,
            variant,
            lm,
            map,
            word: input.corrected_word.clone(),
            sentence: input.corrected_sentence.clone(),
            start_frame: input.start_frame,
            end_frame: input.end_frame,
            observed_phones: input.observed_phones.clone(),
        };
        self.records.insert(event, record.clone());
        Ok(record)
    }

    /// Undo one correction: retract the event fact (the fabric cascade
    /// withdraws items 1 and 3) and drop the item 2/4 record. The remaining
    /// model state is recomputed from the live records, so it returns
    /// bit-identically to before the matching [`apply`](Self::apply).
    pub fn undo(
        &mut self,
        fabric: &mut Fabric,
        event: FactId,
    ) -> Result<AppliedCorrection, CorrectionError> {
        let record = self
            .records
            .remove(&event)
            .ok_or(CorrectionError::UnknownCorrection(event))?;
        fabric.retract(event);
        Ok(record)
    }

    /// The live record for `event`, if any.
    pub fn get(&self, event: FactId) -> Option<&AppliedCorrection> {
        self.records.get(&event)
    }

    /// Live correction events in id order.
    pub fn live_events(&self) -> Vec<FactId> {
        self.records.keys().copied().collect()
    }

    /// Personal-LM count totals over the live corrections (item 2).
    pub fn lm_totals(&self) -> LmDelta {
        let mut totals = LmDelta::default();
        for record in self.records.values() {
            totals.add_in(&record.lm);
        }
        totals
    }

    /// MAP sufficient-statistic totals over the live corrections (item 4),
    /// folded in event-id order so the summation order is deterministic.
    pub fn map_totals(&self) -> BTreeMap<MapKey, MapStats> {
        let mut totals: BTreeMap<MapKey, MapStats> = BTreeMap::new();
        for record in self.records.values() {
            for (key, stats) in &record.map {
                totals
                    .entry(key.clone())
                    .or_insert_with(|| MapStats::zeros(self.dim))
                    .add_in(stats);
            }
        }
        totals
    }

    /// Deterministic serialization of the live correction set: magic,
    /// feature dim, then every live record in event-id order (ids, words,
    /// spans, phones, LM maps in key order, MAP entries in key order with
    /// `f64::to_bits` payloads). Apply-then-undo returns these bytes
    /// bit-identically.
    pub fn state_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(STATE_MAGIC);
        put_u64(&mut out, self.dim as u64);
        put_u64(&mut out, self.records.len() as u64);
        for (event, record) in &self.records {
            put_u64(&mut out, event.0);
            put_u64(&mut out, record.token.0);
            match record.variant {
                Some(variant) => {
                    out.push(1);
                    put_u64(&mut out, variant.0);
                }
                None => out.push(0),
            }
            put_str(&mut out, &record.word);
            put_u32(&mut out, record.sentence.len() as u32);
            for word in &record.sentence {
                put_str(&mut out, word);
            }
            put_u64(&mut out, record.start_frame);
            put_u64(&mut out, record.end_frame);
            put_u32(&mut out, record.observed_phones.len() as u32);
            for phone in &record.observed_phones {
                put_str(&mut out, phone);
            }
            put_counts(&mut out, &record.lm.unigram);
            put_counts(&mut out, &record.lm.bigram);
            put_counts(&mut out, &record.lm.trigram);
            put_u32(&mut out, record.map.len() as u32);
            for (key, stats) in &record.map {
                put_str(&mut out, &key.phone);
                put_u64(&mut out, key.state as u64);
                put_u64(&mut out, key.comp as u64);
                put_f64(&mut out, stats.n);
                put_u32(&mut out, stats.sum.len() as u32);
                for value in &stats.sum {
                    put_f64(&mut out, *value);
                }
                put_u32(&mut out, stats.sumsq.len() as u32);
                for value in &stats.sumsq {
                    put_f64(&mut out, *value);
                }
                put_u64(&mut out, stats.frames);
            }
        }
        out
    }
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_f64(out: &mut Vec<u8>, value: f64) {
    put_u64(out, value.to_bits());
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

fn put_counts(out: &mut Vec<u8>, counts: &BTreeMap<String, u64>) {
    put_u32(out, counts.len() as u32);
    for (key, count) in counts {
        put_str(out, key);
        put_u64(out, *count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentence(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn single_word_sentence_delta_is_exact() {
        // Padded [<s>, <s>, reef, </s>]: <s> never counted, </s> counted once.
        let delta = lm_delta_for_sentence(&sentence(&["reef"]));
        let mut unigram = BTreeMap::new();
        unigram.insert("reef".to_string(), 1);
        unigram.insert("</s>".to_string(), 1);
        let mut bigram = BTreeMap::new();
        bigram.insert("<s> <s>".to_string(), 1);
        bigram.insert("<s> reef".to_string(), 1);
        bigram.insert("reef </s>".to_string(), 1);
        let mut trigram = BTreeMap::new();
        trigram.insert("<s> <s> reef".to_string(), 1);
        trigram.insert("<s> reef </s>".to_string(), 1);
        assert_eq!(
            delta,
            LmDelta {
                unigram,
                bigram,
                trigram
            }
        );
    }

    #[test]
    fn two_word_sentence_counts_windows() {
        let delta = lm_delta_for_sentence(&sentence(&["the", "reef"]));
        assert_eq!(delta.unigram.get("the"), Some(&1));
        assert_eq!(delta.unigram.get("reef"), Some(&1));
        assert_eq!(delta.unigram.get("</s>"), Some(&1));
        assert!(!delta.unigram.contains_key("<s>"));
        assert_eq!(delta.bigram.get("the reef"), Some(&1));
        assert_eq!(delta.trigram.get("<s> the reef"), Some(&1));
        assert_eq!(delta.trigram.get("the reef </s>"), Some(&1));
    }
}
