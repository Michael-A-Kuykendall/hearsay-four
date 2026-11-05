//! Generate the tier-3 rule-engine pronunciation file for book words missing
//! from both lexicon tiers (bead
//! `hearsay-four-k8d--book-missing-pronunciation-test-set`).
//!
//! usage: `book_g2p <book.txt> <out.tier3.txt>`
//!
//! Recipe (this file is the whole recipe): load tiers 1+2 from their default
//! paths, scan `<book.txt>` with the exact `wordlist-scan` lookup chain,
//! derive one pronunciation per missing type from [`lexicon::rule_g2p`],
//! and write CMUdict-shaped `WORD  PHONES...` lines (sorted, with a `;;;`
//! header) to `<out.tier3.txt>` — which must live OUTSIDE the repo
//! (personal data, `owner-book*` is gitignored).
//!
//! Prints COUNTS ONLY — never the words — so stdout is safe to paste into
//! beads or notes.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use lexicon::{RULE_ENGINE_ID, load_tiered, rule_g2p};

type Lexicon = HashMap<String, Vec<Vec<String>>>;

fn lexicon_table_path() -> PathBuf {
    if let Ok(root) = std::env::var("BETTER_VOICE_ROOT") {
        let cand =
            PathBuf::from(root).join("data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt");
        if cand.is_file() {
            return cand;
        }
    }
    let lane = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    lane.join("../better-voice/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt")
}

fn librispeech_lexicon_path() -> PathBuf {
    if let Ok(path) = std::env::var("LIBRISPEECH_LEXICON") {
        let cand = PathBuf::from(path);
        if cand.is_file() {
            return cand;
        }
    }
    let lane = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    lane.join("data/lexicon/librispeech-lexicon.txt")
}

fn norm_token(tok: &str) -> String {
    let mut w = tok.trim().to_uppercase();
    if w.ends_with(')')
        && let Some(open) = w.rfind('(')
        && w[open + 1..w.len() - 1].chars().all(|c| c.is_ascii_digit())
    {
        w.truncate(open);
    }
    while w.ends_with('\'') {
        w.pop();
    }
    w
}

fn strip_edge_punct(w: &str) -> String {
    w.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '(' | ')' | '[' | ']' | '-' | '—' | '–'
        )
    })
    .to_string()
}

fn lex_lookup(lex: &Lexicon, tok: &str) -> Option<(String, Vec<String>)> {
    let w = norm_token(tok);
    if w.is_empty() {
        return None;
    }
    let mut cands = vec![w.clone()];
    let stripped = strip_edge_punct(&w);
    if stripped != w {
        cands.push(stripped);
    }
    let mut extra = Vec::new();
    for c in &cands {
        if let Some(base) = c.strip_suffix("'S")
            && base.len() >= 2
        {
            extra.push(base.to_string());
        }
    }
    cands.extend(extra);
    for c in cands {
        if c.is_empty() {
            continue;
        }
        if let Some(prons) = lex.get(&c)
            && let Some(first) = prons.first()
        {
            return Some((c, first.clone()));
        }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 3,
        "usage: book_g2p <book.txt> <out.tier3.txt>"
    );
    let tiered =
        load_tiered(lexicon_table_path(), librispeech_lexicon_path()).expect("read tiered lexicon");
    let text = std::fs::read_to_string(&args[1]).expect("read text");
    let sents = lm::normalize_text(&text);
    // Same chain as r2_perf wordlist-scan: a type is missing only when none
    // of its lookup candidates hits either tier.
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    for sent in &sents {
        for tok in sent {
            if tok == lm::NUM_TOKEN {
                continue;
            }
            let up = tok.to_uppercase();
            if lex_lookup(&tiered.table, &up).is_none() {
                *missing.entry(up).or_insert(0) += 1;
            }
        }
    }
    let mut out = String::new();
    out.push_str(";;; tier-3 owner-book rule-engine pronunciations (personal data: never track)\n");
    out.push_str(&format!(";;; engine {RULE_ENGINE_ID}\n"));
    out.push_str(&format!(";;; types {}\n", missing.len()));
    for word in missing.keys() {
        out.push_str(word);
        out.push_str("  ");
        out.push_str(&rule_g2p(word).join(" "));
        out.push('\n');
    }
    std::fs::write(&args[2], &out).expect("write tier3");
    let miss_tok: usize = missing.values().sum();
    println!("gen_missing_types={}", missing.len());
    println!("gen_missing_tokens={miss_tok}");
    println!("gen_out_bytes={}", out.len());
}
