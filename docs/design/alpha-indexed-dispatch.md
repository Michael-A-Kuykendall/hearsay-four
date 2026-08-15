# Alpha-Indexed Dispatch — Applicability to Speech

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.17)
Date: 2026-08-15
Relates to: Core Fabric (6za), Stage 2 arena (a6d), Stage 3 (d73), Stage 1 FSE

---

## 1. Purpose

Alpha-indexed dispatch is the portfolio mechanism that routes a fact only to
the rules that can fire on it, computed once at program construction — no
separate control blackboard (it replaces Hearsay-III's scheduler). This
document pins how that mechanism maps onto *speech* facts, which are keyed on
**continuous acoustic evidence** rather than exact strings. The conversion
inventory already flags that pure automata (YFilter, Aho-Corasick) are the
FSE half with exact selectors; speech adds continuous, inexact selectors.

---

## 2. What "indexed dispatch" means here

In the portfolio (MikeDB, dzero), a fact carries a key; the index maps each
key to the rules whose selector matches that key. Dispatch is O(1)-ish per
fact: look up the key, run only the matching rules.

For Hearsay-IV, the fact keys are:
- **Phoneme facts** (Stage 2 arena) — keyed by phoneme identity + feature
  vector.
- **Syllable facts** (Stage 3) — keyed by the hash-consed fragment NodeId.
- **Word facts** (Stage 4) — keyed by the dictionary pronunciation string.

The dispatch index maps these keys to the phonological rules, lexical rules,
and sequence rules that can fire on them.

---

## 3. The mapping onto continuous acoustic evidence

The tension: Stage 1 emits *continuous* acoustic evidence (formant/LPC
vectors), which is not a discrete key. Alpha-indexed dispatch needs discrete
keys. The resolution is the **two-layer split** already established in the
portfolio (FSE vs fabric):

1. **Stage 1 (FSE) is *not* dispatched** — it is single-pass, stateless,
   selector-first *scoring*. This is structurally identical to the FSE half
   elsewhere (cf. §2.7 of the conversion inventory, and synthesis §4.2
   "structurally identical to the selector-first dispatch used elsewhere").
   It emits N-best *discrete* phoneme candidates.

2. **The discrete phoneme candidates become the dispatch keys** (Stage 2
   arena). Once a candidate is quantized to a discrete phoneme
   (per the phoneme alphabet, aem.10), it is a hash-consed FactId — an exact,
   indexable key. Alpha-indexed dispatch then applies exactly as it does to
   any other fact store: the index maps each FactId to the phonological rules
   (SPE/FST) that can fire on it.

So: **continuous evidence is handled by FSE scoring (not dispatched); the
discrete results are dispatched by the fabric.** This is not a compromise —
it is the same FSE→fabric handoff used everywhere else in the portfolio
(synthesis §7: "the FSE / fabric split is the same two-layer shape as
everywhere else").

---

## 4. The dispatch index (implementation-ready)

```
DispatchIndex:
  phoneme_fact -> [PhonologicalRule]     // rules whose LHS matches this phoneme
  syllable_fact -> [LexicalRule]         // dictionary entries starting this syllable
  word_fact      -> [SequenceRule]       // n-gram rules containing this word
```

- Built **once at program construction** (alpha-indexed: precomputed), not at
  runtime.
- A fact is routed only to rules that can fire on it — a phoneme fact never
  invokes a lexical rule directly.
- Inexact/continuous matches (acoustic scores) are handled upstream by FSE
  and resolved into discrete keys before dispatch, per §3.

### 4.1 What is NOT dispatched

- Acoustic frame scoring (continuous) — FSE, single-pass.
- Confidence combination within the commit window — a fabric derivation, not
  a dispatch decision.
- The control/scheduling problem — alpha-indexed dispatch *eliminates* the
  need for a separate control blackboard (synthesis §2.1 table, §7).

---

## 5. Invariants (for the PPT/C-Test system)

1. `INV: dispatch_built_once` — the index is constructed at program setup,
   not rebuilt per fact.
2. `INV: routed_only_to_firable` — a fact is dispatched only to rules whose
   selector matches its key; no cross-layer mis-dispatch.
3. `INV: no_fabric_dispatch_of_continuous` — raw continuous acoustic evidence
   is never directly dispatched; it is FSE-scored and quantized first.
4. `INV: no_control_blackboard` — there is no separate scheduling layer; the
   index is the scheduler.

These become contract-test invariants for the fabric integration.

---

## 6. Relationship to other resolved decisions

- Depends on the **phoneme alphabet** (aem.10) to define discrete keys.
- Depends on **hash-consing NodeId equality** (aem.5) for Stage 2 keys.
- Feeds the fabric integration (6za.2), which it currently blocks.
