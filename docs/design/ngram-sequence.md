# N-Gram Table Order + Corpus — Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.15)
Date: 2026-08-15
Relates to: Stage 5 sequence plausibility (idw.1)
Synthesis §4.6: a classical n-gram table over word sequences, deliberately
flat — no neural LM / long-range semantic disambiguation.

---

## 1. Chosen configuration: **bigram (2-gram) table, closed-corpus trained**

- **Order: bigram (N=2).** A unigram is too weak to add sequence context; a
  trigram adds marginal benefit for the small closed vocabulary while
  increasing table sparsity. Bigram is the right flat-order compromise for
  the ~30-word fail-fast vocabulary and the deliberate flatness (§3).
- **Corpus: the experiment's own training/enrollment material + a small
  hand-annotated set of likely transcriptions.** Because the target use case
  is forensic/legal transcript review with a *known or enrolled speaker*
  (synthesis §3), the sequence model can be trained on the speaker's own
  material — which is exactly the kind of closed, domain-specific corpus a
  general neural LM would not have. This is a legitimate advantage, not a
  shortcut.

---

## 2. Table shape

```
bigram_table: HashMap<(word_a, word_b), count>
P(word_b | word_a) = count(a,b) / count(a,·)
```

With Laplace (add-1) smoothing to avoid zero probabilities for unseen
bigrams:

```
P_smoothed(w_b | w_a) = (count(a,b) + 1) / (count(a,·) + V)
```

where `V` = vocabulary size. This keeps every score a valid, bounded
probability-like value (invariant `bounded_scores`).

---

## 3. Why flat-by-design

Per synthesis §3/§4.6, Stage 5 is *deliberately flat* — it is where a neural
language model would add long-range semantic disambiguation ("the bank" near
"river" vs. near "loan officer") that this architecture explicitly does not
attempt to replicate. The n-gram table provides **local** sequence
plausibility only. No Stage-5 design should add an LM; that is out of scope by
construction.

---

## 4. Interaction with Stage 3/4

The n-gram table scores **committed word sequences** (from Stage 4). Its
probability becomes one evidence source in the calibrated confidence
combination (aem.7) — a sequence-plausibility weight `w_seq` added to a
candidate word's log-odds. It never overrides acoustic/lexical evidence; it
is one vote among several (per the multi-source evidence combination).

---

## 5. Invariants (for the PPT/C-Test system)

1. `INV: bounded_scores` — every n-gram score is a finite probability in
   [0,1] (no NaN/inf; Laplace smoothing guarantees this).
2. `INV: flat_no_lm` — Stage 5 contains only the n-gram table; no neural LM
   / long-range model.
3. `INV: bigram_order` — the table is exactly bigram order (N=2).
4. `INV: score_is_vote` — the n-gram score enters the confidence combination
   as one evidence weight, never as a sole decider.

These become contract-test invariants for Stage 5 (task idw.2).

---

## 6. Deferred (not unresolved)

- Corpus size/curation specifics for the real system; the fail-fast uses the
  speaker's own enrollment material. The *order* (bigram) and *mechanism*
  (Laplace-smoothed lookup) are fixed here.
