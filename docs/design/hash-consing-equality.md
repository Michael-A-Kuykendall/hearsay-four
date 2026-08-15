# Hash-Consing NodeId Equality — Design Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.5)
Date: 2026-08-15
Relates to: Stage 2 arena (a6d.1, a6d.2), alpha-indexed dispatch (aem.17)
Synthesis §4.3: recurring fragments collapse to the same NodeId, giving
cross-utterance memoization.

---

## 1. Purpose

Define the equality criterion that determines when two phoneme/fragment
facts are "the same" — i.e. when they collapse to the same hash-consed NodeId.
This is the cross-utterance memoization mechanism.

---

## 2. The NodeId equality rule

Two arena facts have the **same NodeId iff they have identical content under
an exact, normalized representation.**

For a phoneme-candidate fact, the canonical content is:

```
content = (phoneme: ARPABET,            // from aem.10 alphabet
           context_window: [ARPABET],   // left/right phonological context (if any)
           feature_signature: [f32])     // optional acoustic features (compact)
```

Two facts are equal (collapse to the same NodeId) iff:
- same `phoneme`, **and**
- same `context_window` (element-wise, order-preserving), **and**
- same `feature_signature` (element-wise, within a fixed quantization
  epsilon — see §4).

---

## 3. Why exact-and-normalized (not fuzzy)

- The arena is **append-only** (per the fabric design, aem.13 §3.2). A fact,
  once interned, is never mutated. So equality must be a pure function of the
  content at intern time.
- Cross-utterance memoization is only correct if identical content reliably
  maps to one NodeId. Fuzzy equality would risk two near-identical fragments
  splitting into different NodeIds, defeating memoization, or worse, merging
  genuinely different fragments.
- The **fuzzy matching lives upstream in Stage 1** (FSE scoring), which
  quantizes continuous acoustic evidence into discrete phoneme candidates.
  By the time a candidate reaches Stage 2, it is already discrete (aem.17
  §3: FSE quantizes, fabric dispatches on discrete keys). So the arena only
  ever sees discrete content — exact equality is both sufficient and correct.

---

## 4. The quantization epsilon (the only "fuzzy" parameter)

To make continuous feature signatures comparable, Stage 1's feature output is
quantized into a fixed set of discrete bins before entering the arena. The
`feature_signature` in a NodeId is therefore already a discrete vector.

Two signatures are equal iff they are **bit-identical** after quantization.
No runtime epsilon comparison is needed in the arena — quantization is done
once in Stage 1.

---

## 5. Implementation (matches the fabric's `intern`)

The fabric already interns facts to a stable `FactId` (6za.1). The NodeId is
this FactId. The equality criterion above is exactly what `intern`'s
deduplication uses: intern a candidate only if no fact with identical content
exists; else return the existing FactId.

---

## 6. Invariants (for the PPT/C-Test system)

1. `INV: hash_consing_injective` — identical content → same NodeId (never
   split).
2. `INV: hash_consing_distinct` — distinct content → distinct NodeId (never
   merged).
3. `INV: arena_append_only` — a NodeId's content never changes after intern.
4. `INV: cross_utterance_memoization` — a recurring fragment in a later
   utterance resolves to the NodeId from the first occurrence.

These become contract-test invariants for Stage 2 (task a6d.3).

---

## 7. Deferred (not unresolved)

- The exact quantization bins for `feature_signature` are a Stage 1 detail
  (aem.4/aem.10); the equality rule here works on the already-quantized
  vector.
