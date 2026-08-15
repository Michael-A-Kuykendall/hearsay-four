# Long-Session Memory & Provenance Growth — Measurement Report

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.2)
Date: 2026-08-15
Instrument: `provenance_stats()` (fabric crate) via `long_session_measure` bin
Relates to: synthesis §5 open question, core fabric (6za), fail-fast (39c)

---

## 1. Question

Synthesis §5: *long-session memory and provenance growth is unmeasured.* The
fire-control precedent (state smaller than a photograph) does not transfer —
that system's state is bounded and known at compile time, whereas speech is
an open, continuous stream. Does memory/provenance stay bounded over a long
session?

## 2. Method

Simulated a 2000-utterance session on the D-1 fabric (src/fabric):

- Fixed closed vocabulary of 15 syllable fragments (~30-word scale),
  interned once. Recurring fragments hash-cons to the same FactId (the
  cross-utterance memoization mechanism, Stage 2).
- Each utterance asserts one meta-fact justified by 3–6 chosen word facts
  (modeling Stage 5 sequence-plausibility links).
- `provenance_stats()` sampled at intervals; see
  `src/fabric/src/bin/long_session_measure.rs`.

## 3. Growth curve

| Utterances | Facts | est. bytes |
|---|---|---|
| 0 | 45 | 6,480 |
| 1 | 46 | 6,624 |
| 250 | 295 | 42,480 |
| 500 | 545 | 78,480 |
| 750 | 795 | 114,480 |
| 1000 | 1045 | 150,480 |
| 1250 | 1295 | 186,480 |
| 1500 | 1545 | 222,480 |
| 1750 | 1795 | 258,480 |
| 2000 | 2045 | 294,480 (~288 KiB) |

## 4. Findings

1. **The vocabulary is bounded.** Recurring phoneme/syllable fragments
   hash-cons to existing FactIds — the 45-fact base vocabulary does not grow
   with repetition. Cross-utterance memoization works as designed.

2. **Per-utterance facts grow linearly.** Each utterance adds one meta-fact:
   `facts = 45 + utterances`. At 2000 utterances this is 2045 facts, ~288 KiB.
   Growth is **O(utterances)**, not O(1) — over a genuinely long session
   (hours/days) this accumulates without bound.

3. **No retraction in this model.** Because the harness never retracts
   completed utterances, all facts remain active and `inactive_facts = 0`.
   In the real system, the local commit window (Stage 3) closes and
   retracts/commits per window, which would bound the *active* set even if
   the *provenance-retained* (inactive) set grows.

## 5. Capacity implications (the actionable conclusion)

- **Active-state memory is bounded by the commit window** if Stage 3
  actually closes windows and retracts (per the survival design, aem.13).
  This is the structural fix that keeps the live working set small.
- **Provenance-retained history grows O(utterances)** and is the real
  long-session cost. This is *by design* (provenance must persist for the
  forensic/legal use case), but it means:
  - Long-session capacity is dominated by provenance retention, not live
    state — matching the synthesis's MikeDB-fact-volume analogy, not the
    fire-control precedent.
  - A **retention policy** is required (what to prune, when) — flagged as
    future work, not resolved here.

## 6. Invariants (for the PPT/C-Test system)

1. `INV: vocabulary_bounded` — interning a recurring fragment reuses its
   FactId; the base vocabulary size stays constant across utterances.
2. `INV: per_utterance_linear` — each utterance contributes exactly one
   meta-fact (growth is linear in utterances, not superlinear).
3. `INV: active_bounded_by_window` — the number of active facts is bounded by
   the commit window when retraction is enabled (verified once Stage 3
   closes windows).

These become contract-test invariants for long-session behavior.

## 7. Deferred (not unresolved)

- The actual **retention/pruning policy** is a product decision (forensic
  completeness vs. storage), out of scope for this measurement bead.
- A **wall-clock long session** with real drift is the fail-fast experiment's
  job (epic 39c), not this synthetic model.
