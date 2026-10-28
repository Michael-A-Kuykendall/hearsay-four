//! Explicit text normalization for LM training (bead hearsay-four-baj).
//!
//! Rules, applied in order:
//! 1. Casing: Unicode lowercase (`char::to_lowercase`).
//! 2. Sentence splitting: `.`, `!`, `?` end a sentence. Empty sentences drop.
//! 3. Hyphens/dashes (`-`, U+2010..U+2014) become spaces (word separator).
//! 4. Punctuation: every character that is not a letter, digit, or an
//!    apostrophe between two letters is a separator. Leading/trailing
//!    apostrophes are stripped, so `'tis` -> `tis`, `dogs'` -> `dogs`,
//!    while `don't` and `o'flaherty` keep their internal apostrophe.
//! 5. Numbers: any token containing a digit becomes `<num>`. This covers
//!    integers (`42`), decimals (`3.14` — the dot first splits it into
//!    digit tokens, each mapped to `<num>`), comma-grouped numbers
//!    (`1,000`), ordinals (`1st`), and mixed alphanumeric (`10-218`
//!    becomes `<num> <num>` after rule 3).
//! 6. Tokens that become empty are dropped.

/// Token substituted for any digit-containing token (rule 5).
pub const NUM_TOKEN: &str = "<num>";

/// Sentence-start padding token (never predicted).
pub const BOS_TOKEN: &str = "<s>";
/// Sentence-end token (predicted, in vocabulary).
pub const EOS_TOKEN: &str = "</s>";
/// Out-of-vocabulary token.
pub const UNK_TOKEN: &str = "<unk>";

fn is_dash(c: char) -> bool {
    c == '-' || ('\u{2010}'..='\u{2014}').contains(&c)
}

/// Normalize one already-lowercased raw token span into zero or one token.
fn push_token(out: &mut Vec<String>, raw: &str) {
    let t = raw.trim_matches('\'');
    if t.is_empty() {
        return;
    }
    if t.chars().any(|c| c.is_ascii_digit()) {
        out.push(NUM_TOKEN.to_string());
    } else {
        out.push(t.to_string());
    }
}

/// Normalize a single sentence (no sentence punctuation handling beyond
/// treating it as separator). Returns tokens in order.
pub fn normalize_sentence(text: &str) -> Vec<String> {
    let lower: String = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            cur.push(c);
        } else if c == '\'' {
            let prev_letter = i > 0 && chars[i - 1].is_alphabetic();
            let next_letter = chars.get(i + 1).is_some_and(|n| n.is_alphabetic());
            if prev_letter && next_letter {
                cur.push(c);
            } else {
                // Boundary apostrophe: separator; stripping happens in push.
                let taken = std::mem::take(&mut cur);
                push_token(&mut out, &taken);
            }
        } else if is_dash(c)
            || c.is_whitespace()
            || c.is_ascii_punctuation()
            || !c.is_alphanumeric()
        {
            let taken = std::mem::take(&mut cur);
            push_token(&mut out, &taken);
        }
    }
    let taken = std::mem::take(&mut cur);
    push_token(&mut out, &taken);
    out
}

/// Normalize raw text into sentences of tokens (rules 1-6).
/// Each returned sentence is non-empty.
pub fn normalize_text(text: &str) -> Vec<Vec<String>> {
    let lower: String = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut sentences = Vec::new();
    for part in lower.split(['.', '!', '?']) {
        let toks = normalize_sentence(part);
        if !toks.is_empty() {
            sentences.push(toks);
        }
    }
    sentences
}
