# Lexicon Dictionary Scope + Source — Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.14)
Date: 2026-08-15
Relates to: Stage 4 lexicon (mwa.1, mwa.2)
Synthesis §4.5: "CMUdict-shaped — a lookup table, not a model."

---

## 1. Chosen source: **CMUdict, subset to the experiment vocabulary**

- **Full source:** CMU Pronouncing Dictionary (CMUdict), the standard
  ARPABET-keyed pronunciation dictionary. It is freely available, public
  domain, and keyed on the exact phoneme alphabet chosen (aem.10).
- **Scope for the prototype/fail-fast:** a **closed subset** covering the
  ~30-word experiment vocabulary. For the full system, the dictionary can be
  loaded in whole; the fail-fast experiment (epic 2) only needs its ~30
  words.

---

## 2. Data shape

CMUdict entries map an orthographic word to one or more ARPABET
pronunciations:

```
QUICK  K W IH1 K
BROWN  B R AW1 N
FOX    F AA1 K S
```

The `_N` stress marks are stripped for Stage 4 matching (Stage 1–3 work on
phones, not stress), OR retained if Stage 3 uses stress — a Stage-3 detail.
For the fail-fast, strip stress.

---

## 3. Ingestion format (Stage 4, task mwa.1)

```
PronunciationEntry {
  word: String,               // e.g. "QUICK"
  phones: [String],           // ARPABET, e.g. ["K","W","IH","K"]
  phoneme_ids: [FactId],      // resolved to arena NodeIds (aem.5)
}
```

The dictionary is loaded as a lookup table (a `HashMap<word, phones>`),
**not** a model. No probabilities are baked in here — lexical frequency, if
needed, comes from the n-gram table (aem.15) at Stage 5.

---

## 4. Matching (Stage 4, task mwa.2)

Committed syllable sequences (from Stage 3) are matched against the
dictionary's phone sequences. A word candidate is created when a committed
sequence aligns to a dictionary pronunciation. The word fact **inherits
justification** from the syllable facts beneath it (the fabric assertion),
so its provenance chain extends down to acoustic evidence (aem.16).

---

## 5. Invariants (for the PPT/C-Test system)

1. `INV: lookup_not_model` — the dictionary is a lookup table; no learned
   weights.
2. `INV: words_in_alphabet` — every dictionary phone is in the ARPABET
   alphabet (aem.10); no out-of-alphabet phones.
3. `INV: word_inherits_syllable_justification` — every word fact is justified
   by the syllable facts beneath it (no orphan word; feeds the provenance
   invariant).
4. `INV: pronunciation_complete` — every word in the closed subset has ≥1
   pronunciation entry.

These become contract-test invariants for Stage 4 (task mwa.3).

---

## 6. Deferred (not unresolved)

- Whether to include the full CMUdict or only the experiment subset in the
  final binary is a packaging decision; the subset is sufficient for the
  fail-fast.
