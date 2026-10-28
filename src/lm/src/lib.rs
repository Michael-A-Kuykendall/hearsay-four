//! LM core (bead hearsay-four-baj, R1a): D0-use-verified text ingestion,
//! explicit normalization, and a trigram modified Kneser-Ney model with
//! inspectable counts. Interpolation, pruning, persistence formats, and
//! perplexity reporting are R1b (hearsay-four-2a0), not this crate's gate.

pub mod ingest;
pub mod kneser_ney;
pub mod normalize;

pub use ingest::{IngestedSource, extract_text_lines, load_lm_train_sources};
pub use kneser_ney::{Discounts, InspectableModel, KnTrigram};
pub use normalize::{
    BOS_TOKEN, EOS_TOKEN, NUM_TOKEN, UNK_TOKEN, normalize_sentence, normalize_text,
};

/// Normalize all ingested sources into token sentences.
pub fn sentences_from_sources(sources: &[IngestedSource]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for s in sources {
        for line in &s.lines {
            out.extend(normalize_text(line));
        }
    }
    out
}
