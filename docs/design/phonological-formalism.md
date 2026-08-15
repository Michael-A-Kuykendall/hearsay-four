# Phonological Rule Formalism — Selection & Port Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.8)
Date: 2026-08-15
Relates to: Stage 3 phonological rules (d73.1 SPE, d73.2 FST)
Source: Chomsky & Halle (1968) SPE; Kaplan & Kay (1994)

---

## 1. Purpose

Choose between the two candidate phonological rule formalisms for Stage 3's
phonological combination rules, and pin the port. The conversion inventory
(§2.2, §2.3) flags Kaplan & Kay's FST cascades as "closer fit for direct
implementation" than Chomsky & Halle's SPE rewrite rules.

---

## 2. The decision: **FST cascades (Kaplan & Kay) as the executable form, SPE as the source of rules**

**Chosen: compose FST cascades (Kaplan & Kay 1994) to implement Stage 3's
phonological rules, sourcing the actual rules from SPE (Chomsky & Halle
1968).**

Rationale:

1. **Executable directly.** Kaplan & Kay's FST cascade is already a
   transition function `(state, symbol) → (state', output)` — a fact-and-rule
   pair that maps cleanly onto the fabric (and matches the conversion
   inventory's Category B assessment). SPE rewrite rules `A → B / X_Y`
   require an interpreter; an FST is a compiled, runnable form.
2. **Provenance-friendly.** A cascade resolves deterministically, and per the
   conversion inventory §2.3, we keep the transition logic and add provenance
   as a wrapper — which is exactly the fabric's always-on provenance (6za.2).
3. **Rule content still comes from SPE.** The linguistic rules themselves
   (flapping, assimilation, elision, palatalization) are the fifty-years-settled
   SPE rules. FST is the *mechanism*; SPE is the *content*.
4. **The synthesis's own preference.** The synthesis (§2.2) already flags
   Kaplan & Kay as the practical path, and the conversion inventory concurs.

---

## 3. What this means for Stage 3 tasks

- **d73.1 (SPE rule ports)** is reframed: SPE rules are the *rule inventory*
  (the rewrite patterns), not the execution mechanism.
- **d73.2 (Kaplan & Kay FST cascade)** is the *execution engine*: each SPE
  rule (or rule group) is compiled into an FST; the cascade composes them.

Together: **SPE supplies the rules; FST cascades execute them; the fabric
supplies provenance and survival.**

---

## 4. Example (flapping) to pin the port

SPE rule (English intervocalic flapping, simplified):

```
t → [r] / V _ V     (a /t/ between vowels becomes a flap)
```

FST representation (one transition):

```
state_q : V t V  →  state_q : V [r] V
```

As a fabric justification (assert):

```
Justification { conclusion: flap_phone, premises: {V_left, t_phone, V_right} }
```

This is asserted by a rule triggered via alpha-indexed dispatch (aem.17) when
the phoneme facts `V_left, t_phone, V_right` are active in the arena.

---

## 5. Invariants (for the PPT/C-Test system)

1. `INV: rule_content_is_spe` — every phonological rule used is traceable to
   an SPE rule source.
2. `INV: executed_as_fst` — rules execute via the FST-cascade engine, not an
   ad-hoc rewriter.
3. `INV: rewrite_is_assert` — a rewrite produces a new asserted fact (via the
   fabric), never an in-place mutation (the Category B port requirement).
4. `INV: cascade_deterministic` — for a fixed input, the cascade yields a
   deterministic output.

These become contract-test invariants for Stage 3 rule execution (task d73.7).

---

## 6. Deferred (not unresolved)

- The exact SPE rule subset to port (flapping, assimilation, elision, etc.)
  is enumerated at Stage 3 implementation (d73.1), scoped to the phoneme
  alphabet (aem.10).
- FST library choice (hand-rolled vs. a crate) is an implementation detail.
