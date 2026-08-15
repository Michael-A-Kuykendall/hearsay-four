# N-Best Candidate Count — Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.4)
Date: 2026-08-15
Relates to: Stage 1 emission (d5n.2), Stage 3 competition (d73.4)
Synthesis §4.2: emit N-best candidates per frame, not argmax.

---

## 1. Chosen value: **N = 5, with a dynamic cap**

Stage 1 emits, per frame, the **top-5 phoneme candidates** by calibrated
score, with an additional constraint: a candidate is only emitted if its score
is within a threshold of the top candidate (see §3).

Rationale:
- **N=1** = argmax (rejected; discards the competition Stage 3 needs).
- **N=3** too small — may drop a plausible runner-up that a downstream
  phonological/lexical rule could rescue.
- **N=5** is large enough to preserve real competition, small enough to bound
  the arena fact volume (Stage 2) and the local commit window's combinatorial
  load (Stage 3) — the exact brittleness-vs-blowup tradeoff from synthesis
  §5.
- **N=10+** wastes work: below the top-5, candidates are dominated and
  rarely recover (the tail rarely carries independent signal for the ~30-word
  vocabulary).

---

## 2. The adaptive cap (dynamic N)

Rather than a fixed 5, use an **adaptive threshold** that naturally limits N:

```
emit candidates with score ≥ max_score − δ,   up to a hard cap of 5.
```

- `δ` = pre-registered score gap (e.g., 15% of max score, or a fixed
  log-odds gap — pinned at Stage 3 confidence math, aem.7).
- If only 1 candidate is within `δ`, only 1 is emitted (an unambiguous frame;
  this is fine — it means there is no real competition).
- If more than 5 are within `δ` (rare, high-entropy frame), cap at 5.

This gives **N = min(5, count within δ)**: adaptive, but never more than 5.

---

## 3. Interaction with Stage 3

The N-best candidates per frame become the arena facts (Stage 2) that
compete in Stage 3's local commit window. The adaptive cap ensures:

- Competition only exists where it is real (within δ).
- The commit window's combinatorial load is bounded by the cap (≤5 per
  frame), directly serving the synthesis §5 "not too wide" concern.
- The window-width sweep (aem.1) can treat N=5/δ as the fixed per-frame
  emission, isolating window width as the only swept dial.

---

## 4. Invariants (for the PPT/C-Test system)

1. `INV: n_best_bounded` — Stage 1 never emits more than 5 candidates/frame.
2. `INV: n_best_not_argmax` — when real competition exists (≥2 within δ),
   it emits >1 candidate; never collapses to argmax prematurely.
3. `INV: scores_relative` — emission uses relative (within-δ) scores, not a
   global hard threshold.
4. `INV: candidates_in_alphabet` — every emitted candidate is in the phoneme
   alphabet (aem.10).

These become contract-test invariants for Stage 1 (task d5n.3).

---

## 5. Deferred (not unresolved)

- The exact `δ` value is swept against the error curve in the fail-fast
  experiment (aem.1 / epic 2); the adaptive-cap *mechanism* is fixed here.
