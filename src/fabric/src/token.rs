//! Token facts (A3, bead hearsay-four-pri): time-anchored occurrences.
//!
//! Type facts (`WORD_TYPE`, `PRONUNCIATION`, n-gram entries, template ids,
//! model parameters) are hash-consed globally: one content, one `FactId`.
//! Token facts are occurrences and are never merged across time: the same
//! word at two spans is two facts with independent retraction and distinct
//! provenance chains.

/// `WORD_TOKEN{word, start_frame, end_frame, source}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordToken {
    pub word: String,
    pub start_frame: u64,
    pub end_frame: u64,
    pub source: String,
}

/// `PHONE_TOKEN{phone, start_frame, end_frame}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneToken {
    pub phone: String,
    pub start_frame: u64,
    pub end_frame: u64,
}

/// `SIL_TOKEN{start_frame, end_frame}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SilToken {
    pub start_frame: u64,
    pub end_frame: u64,
}

/// A token fact's payload, stored beside its `FactId` in the fabric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenFact {
    Word(WordToken),
    Phone(PhoneToken),
    Silence(SilToken),
}

impl TokenFact {
    pub fn start_frame(&self) -> u64 {
        match self {
            TokenFact::Word(t) => t.start_frame,
            TokenFact::Phone(t) => t.start_frame,
            TokenFact::Silence(t) => t.start_frame,
        }
    }
    pub fn end_frame(&self) -> u64 {
        match self {
            TokenFact::Word(t) => t.end_frame,
            TokenFact::Phone(t) => t.end_frame,
            TokenFact::Silence(t) => t.end_frame,
        }
    }
}
