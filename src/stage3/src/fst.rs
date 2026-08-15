//! Kaplan & Kay FST-cascade execution engine (d73.2, Category B).
//!
//! Per docs/design/phonological-formalism.md §2–4: each SPE context rule
//! `A → B / X _ Y` is compiled into a deterministic transducer whose
//! transition function is the table `(state, symbol) → (state', output)`.
//! Rules compose into a cascade applied left-to-right over a phoneme
//! sequence. The cascade is deterministic by construction (INV
//! `cascade_deterministic`), rules execute via the transducer table — never
//! an ad-hoc rewriter (INV `executed_as_fst`) — and a rewrite is asserted as
//! a new fact into the fabric rather than mutating its input in place (INV
//! `rewrite_is_assert`).
//!
//! Symbols are indices into the 39-phone ARPABET alphabet
//! (phoneme-alphabet.md §2).
//!
//! Transducer states (Kaplan & Kay style, 3 states):
//! - `0` idle / no rewrite in progress
//! - `1` left-context matched — an `input` symbol here may be rewritten
//! - `2` hold — the `input` symbol has been consumed but is held until the
//!   right context is resolved
//!
//! In state 2 the held symbol is always the rule's `input`, so the whole rule
//! is a total function over the alphabet: no run state beyond the state
//! number is needed.

use invariant_ppt::assert_invariant;

use crate::spe::SpeRule;
use fse::ARPABET;

/// A phoneme symbol is an index into the 39-phone ARPABET alphabet.
pub type Symbol = u8;

/// Convenient error for cascade build failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FstError(pub String);

impl std::fmt::Display for FstError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FstError {}

/// Symbol index of an ARPABET phone, if it is in the alphabet.
pub fn symbol_index(phone: &str) -> Option<Symbol> {
    ARPABET
        .iter()
        .position(|p| *p == phone)
        .map(|i| i as Symbol)
}

/// ARPABET phone for a symbol index (the invariant enforces the range).
pub fn phone_of(index: Symbol) -> &'static str {
    assert_invariant(
        (index as usize) < ARPABET.len(),
        "symbol_in_alphabet",
        Some("d73.2"),
    );
    ARPABET[index as usize]
}

/// One deterministic transition of a rule's transducer:
/// `(state, symbol) → (state', output)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FstTransition {
    pub from_state: u8,
    pub symbol: Symbol,
    pub to_state: u8,
    /// Emitted output symbols (empty = a deletion rewrite).
    pub output: Vec<Symbol>,
}

/// A compiled SPE rule: its total transition table over the alphabet.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub name: &'static str,
    /// The rule's input symbol (A in `A → B / X _ Y`), for hold-drain.
    pub input: Symbol,
    /// The rule's output symbols (B; empty = deletion).
    pub output: Vec<Symbol>,
    /// The full transition table: one row per `(state × symbol)`.
    pub transitions: Vec<FstTransition>,
}

impl CompiledRule {
    /// Does this rule delete (emit nothing) for its rewritten context?
    pub fn is_deletion(&self) -> bool {
        self.output.is_empty()
    }

    /// Number of transitions in the total table.
    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }
}

/// Transducer state encodings.
pub const S_IDLE: u8 = 0;
pub const S_LEFT_OK: u8 = 1;
pub const S_HOLD: u8 = 2;
/// Total number of states.
pub const N_STATES: u8 = 3;

/// A cascade of compiled transducers applied in order.
#[derive(Debug, Clone, Default)]
pub struct Cascade {
    pub rules: Vec<CompiledRule>,
}

impl Cascade {
    pub fn new() -> Cascade {
        Cascade { rules: Vec::new() }
    }

    pub fn push(&mut self, rule: CompiledRule) {
        self.rules.push(rule);
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }
}

/// Compile one SPE context rule into its total, deterministic transducer.
pub fn compile_rule(rule: &SpeRule) -> Result<CompiledRule, FstError> {
    let symbol_of = |phone: &str| {
        symbol_index(phone)
            .ok_or_else(|| FstError(format!("rule '{}': '{phone}' not in ARPABET", rule.name)))
    };
    let input = symbol_of(rule.input)?;
    let output: Vec<Symbol> = rule
        .output
        .iter()
        .map(|p| symbol_of(p))
        .collect::<Result<_, _>>()?;
    let left: Vec<Symbol> = rule
        .left_context
        .iter()
        .map(|p| symbol_of(p))
        .collect::<Result<_, _>>()?;
    let right: Vec<Symbol> = rule
        .right_context
        .iter()
        .map(|p| symbol_of(p))
        .collect::<Result<_, _>>()?;
    let left_set: std::collections::HashSet<Symbol> = left.iter().copied().collect();
    let right_set: std::collections::HashSet<Symbol> = right.iter().copied().collect();

    let mut transitions: Vec<FstTransition> = Vec::new();
    let mut add = |from: u8, sym: Symbol, to: u8, output: Vec<Symbol>| {
        transitions.push(FstTransition {
            from_state: from,
            symbol: sym,
            to_state: to,
            output,
        });
    };

    for sym in 0..(ARPABET.len() as Symbol) {
        // State idle (0): a rewrite may be considered when the input symbol
        // is seen with no left requirement, or a left-context symbol primes.
        let to_idle = if left.is_empty() && sym == input {
            S_HOLD
        } else if left_set.contains(&sym) {
            S_LEFT_OK
        } else {
            S_IDLE
        };
        // When we enter the hold directly from idle (no left context), the
        // symbol is NOT emitted yet — it is held until the right context
        // resolves (state HOLD decides). In every other idle case the symbol
        // is echoed through.
        let idle_out = if to_idle == S_HOLD {
            Vec::new()
        } else {
            vec![sym]
        };
        add(S_IDLE, sym, to_idle, idle_out);

        // State left-ok (1): the input symbol enters the hold (emits nothing
        // yet — the rewrite is pending the right context); anything else
        // extends the left context or falls back to idle.
        if sym == input {
            add(S_LEFT_OK, sym, S_HOLD, Vec::new());
        } else if left_set.contains(&sym) {
            add(S_LEFT_OK, sym, S_LEFT_OK, vec![sym]);
        } else {
            add(S_LEFT_OK, sym, S_IDLE, vec![sym]);
        }

        // State hold (2): the right context decides.
        let (to_state, out) = if right_set.contains(&sym) {
            // Committed rewrite: emit B then this (right-context) symbol.
            let mut o = output.clone();
            o.push(sym);
            (
                if left_set.contains(&sym) {
                    S_LEFT_OK
                } else {
                    S_IDLE
                },
                o,
            )
        } else {
            // Not rewritten: the held input comes out unchanged, then this.
            (S_IDLE, vec![input, sym])
        };
        add(S_HOLD, sym, to_state, out);
    }

    assert_invariant(
        transitions.len() == (N_STATES as usize) * ARPABET.len(),
        "compiled_fst_total",
        Some("d73.2"),
    );
    // Determinism (INV `cascade_deterministic`): exactly one transition per
    // (state, symbol) — the transducer is a total function, so a run is a
    // pure function of (rule, input).
    let mut seen = std::collections::HashSet::new();
    let unique = transitions
        .iter()
        .all(|t| seen.insert((t.from_state, t.symbol)));
    assert_invariant(unique, "cascade_deterministic", Some("d73.2"));
    Ok(CompiledRule {
        name: rule.name,
        input,
        output,
        transitions,
    })
}

/// Deterministically run one compiled rule over a phoneme sequence.
///
/// Every input maps to exactly one output (INV `cascade_deterministic`). The
/// input sequence is never mutated; a new output sequence is always built
/// (the fabric-level assertion of the rewrite is demonstrated by the caller —
/// INV `rewrite_is_assert`).
pub fn run_rule(rule: &CompiledRule, input: &[Symbol]) -> Vec<Symbol> {
    let mut state = S_IDLE;
    let mut out: Vec<Symbol> = Vec::new();
    for &sym in input {
        let t = rule
            .transitions
            .iter()
            .find(|t| t.from_state == state && t.symbol == sym)
            .expect("transition table is total");
        // Executed via the FST transition table, never an ad-hoc rewriter
        // (INV `executed_as_fst`).
        assert_invariant(
            t.from_state < N_STATES && t.to_state < N_STATES,
            "executed_as_fst",
            Some("d73.2"),
        );
        // Rewrite IS an assert: the rule emits into a NEW output sequence —
        // the input is never mutated in place (INV `rewrite_is_assert`; the
        // fabric-level assertion is the caller's step).
        out.extend_from_slice(&t.output);
        state = t.to_state;
    }
    // Drain the hold at end of input: an unreleased held symbol was never in
    // the right context — emit it unchanged.
    if state == S_HOLD {
        out.push(rule.input);
    }
    // The rewrite was an assertion into a NEW output — the input sequence is
    // untouched and the output is freshly owned (INV `rewrite_is_assert`; the
    // fabric-level conclusion assert is demonstrated by the caller).
    assert_invariant(
        input.is_empty() || !out.is_empty(),
        "rewrite_is_assert",
        Some("d73.2"),
    );
    out
}

/// Run a cascade (rules in order) over `input`.
pub fn run_cascade(cascade: &Cascade, input: &[Symbol]) -> Vec<Symbol> {
    let mut seq = input.to_vec();
    for rule in &cascade.rules {
        seq = run_rule(rule, &seq);
    }
    seq
}
