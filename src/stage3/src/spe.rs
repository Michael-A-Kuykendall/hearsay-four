//! SPE phonological rule inventory (d73.1, Category B).
//!
//! **Content, not execution.** Per docs/design/phonological-formalism.md §3:
//! SPE (Chomsky & Halle 1968) supplies the *rule inventory* — the
//! context-sensitive rewrite patterns `A → B / X _ Y`; the actual execution
//! engine is the Kaplan & Kay FST cascade (d73.2). Every phone referenced by
//! a rule is in the 39-phone ARPABET alphabet (aem.10) — INV:
//! `rule_content_is_spe`. Rewrites are described as new-fact assertions, never
//! in-place mutation (INV `rewrite_is_assert`, enforced at FST execution).

use invariant_ppt::assert_invariant;

use fse::ARPABET;

/// Rule families ported, traceable to SPE sources (phonological-formalism.md
/// §6: flapping, assimilation, elision, palatalization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeClass {
    /// Intervocalic flapping (SPE; Ohala-type): `/t/` → flap between vowels.
    Flapping,
    /// Nasal place assimilation (SPE Ch. 4.6): a coronal nasal takes the
    /// place of an adjacent following stop.
    Assimilation,
    /// Consonant elision / deletion in clusters (SPE Ch. 4.6).
    Elision,
    /// Palatalization before a palatal glide (SPE Ch. 4.9): `/t d s z/ + /j/`.
    Palatalization,
}

/// One SPE rule as a context-sensitive rewrite `A → B / X _ Y`.
///
/// `input` (`A`), `left_context` (`X`), and `right_context` (`Y`) are
/// ARPABET symbols. `output` (`B`) is the rewrite result, written as ARPABET
/// symbols; using the empty slice encodes deletion (an elision rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeRule {
    pub name: &'static str,
    pub class: SpeClass,
    /// The phoneme rewritten (A in `A → B / X _ Y`).
    pub input: &'static str,
    /// The rewrite output (B). Empty = the phone is deleted.
    pub output: &'static [&'static str],
    /// Left context symbols (X — must precede `input`).
    pub left_context: &'static [&'static str],
    /// Right context symbols (Y — must follow `input`).
    pub right_context: &'static [&'static str],
}

impl SpeRule {
    /// Fallback: name, rule renders as `A → B / X _ Y` (or `A → ε / X _ Y`
    /// for a deletion).
    pub fn rendered(&self) -> String {
        let b = if self.output.is_empty() {
            "ε".to_string()
        } else {
            self.output.join(" ")
        };
        if self.left_context.is_empty() && self.right_context.is_empty() {
            format!("{} → {} / _", self.input, b)
        } else {
            let l = if self.left_context.is_empty() {
                "".to_string()
            } else {
                self.left_context.join(" ")
            };
            let r = if self.right_context.is_empty() {
                "".to_string()
            } else {
                self.right_context.join(" ")
            };
            format!("{} → {} / {} _ {}", self.input, b, l, r)
        }
    }

    /// ARPABET references literal for INV checks + messages.
    pub fn referenced_symbols(&self) -> Vec<&'static str> {
        let mut v = vec![self.input];
        v.extend(self.output.iter().copied());
        v.extend(self.left_context.iter().copied());
        v.extend(self.right_context.iter().copied());
        v
    }
}

/// The ported SPE rule inventory, scoped to the 39-phone ARPABET alphabet.
///
/// From the synthesis (HEARSAY-IV §2.2) and phoneme-alphabet.md §2. `repeat`
/// styles use the ARPABET representatives closest to the SPE output.
#[rustfmt::skip]
pub const SPE_RULES: &[SpeRule] = &[
    // ---- Flapping ----
    SpeRule {
        name: "intervocalic-t-flap",
        class: SpeClass::Flapping,
        input: "T",
        output: &["D"],
        left_context: &["AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW"],
        right_context: &["AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW"],
    },
    // ---- Assimilation ----
    SpeRule {
        name: "coronal-nasal-to-labial",
        class: SpeClass::Assimilation,
        input: "N",
        output: &["M"],
        left_context: &[],
        right_context: &["P", "B"],
    },
    SpeRule {
        name: "coronal-nasal-to-velar",
        class: SpeClass::Assimilation,
        input: "N",
        output: &["NG"],
        left_context: &[],
        right_context: &["K", "G"],
    },
    // ---- Elision ----
    SpeRule {
        name: "alveolar-stop-cluster-elision",
        class: SpeClass::Elision,
        input: "T",
        output: &[],
        left_context: &["K"],
        right_context: &["S"],
    },
    SpeRule {
        name: "alveolar-stop-elision-before-s",
        class: SpeClass::Elision,
        input: "D",
        output: &[],
        left_context: &[],
        right_context: &["Z"],
    },
    // ---- Palatalization ----
    SpeRule {
        name: "t-palatalization-before-glide",
        class: SpeClass::Palatalization,
        input: "T",
        output: &["CH"],
        left_context: &[],
        right_context: &["Y"],
    },
    SpeRule {
        name: "d-palatalization-before-glide",
        class: SpeClass::Palatalization,
        input: "D",
        output: &["JH"],
        left_context: &[],
        right_context: &["Y"],
    },
    SpeRule {
        name: "s-palatalizer-before-glide",
        class: SpeClass::Palatalization,
        input: "S",
        output: &["SH"],
        left_context: &[],
        right_context: &["Y"],
    },
    SpeRule {
        name: "z-palatalization-before-glide",
        class: SpeClass::Palatalization,
        input: "Z",
        output: &["ZH"],
        left_context: &[],
        right_context: &["Y"],
    },
];

/// Enforce `rule_content_is_spe`: every symbol referenced by the inventory is
/// in the fixed 39-phone ARPABET alphabet. Returns the first offending rule
/// symbol if any symbol is out of alphabet.
pub fn validate_spe_inventory(rules: &[SpeRule]) -> Option<String> {
    let alphabet: std::collections::HashSet<&str> = ARPABET.iter().copied().collect();
    let mut offending: Vec<String> = Vec::new();
    for r in rules {
        for sym in r.referenced_symbols() {
            if !alphabet.contains(sym) {
                offending.push(format!(
                    "rule '{}' references out-of-alphabet symbol '{sym}'",
                    r.name
                ));
            }
        }
    }
    // Active enforcement of the INV on the happy path: until the last symbol
    // is verified in-alphabet, the invariant is not yet confirmed.
    let verified = offending.is_empty();
    if !verified {
        assert_invariant(false, "rule_content_is_spe", Some(&offending.join("; ")));
    } else {
        assert_invariant(true, "rule_content_is_spe", Some("d73.1"));
    }
    offending.first().cloned()
}
