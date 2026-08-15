# Fail-Fast Experiment — Quantitative Separation Criterion

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.12)
Date: 2026-08-15
Relates to: Fail-Fast Experiment epic (39c), measurement task (39c.5)

---

## 1. Purpose

The synthesis §6 success criterion — "B's error rate should separate
downward from A's as the session progresses" — is qualitative. This document
pins a concrete, pre-registered, quantitative decision rule so the
experiment's pass/fail verdict is unambiguous and not decided after seeing
the data.

---

## 2. Setup recap

- One speaker, closed vocabulary of **~30 words**, classical front end only.
- One ~20-minute natural session with real drift (fatigue, mic distance,
  ambient noise).
- Same audio scored under two conditions:
  - **A (control):** every utterance fresh, no cross-utterance memory.
  - **B (treatment):** accent-signature fact persists/updates per utterance.

---

## 3. Measure

Split the session into **chronological buckets**. Each bucket is a fixed
window of utterances (e.g., 40 utterances / bucket — pre-registered). Within
each bucket `k`, compute the word error rate for each condition:

```
WER_A(k) = errors_A(k) / total_words_A(k)
WER_B(k) = errors_B(k) / total_words_B(k)
```

The session yields an ordered sequence of per-bucket WER pairs
`(WER_A(k), WER_B(k))` for `k = 1..K`.

---

## 4. The pre-registered decision rule

The experiment **passes** (carry-forward does real work) if and only if
**all three** of the following hold:

1. **Directional separation in the tail.** In the *second half* of the
   session, B is strictly better than A on aggregate:
   ```
   mean(WER_B, k in 2nd half)  <  mean(WER_A, k in 2nd half)
   ```
   (strict `<`, not `<=`).

2. **A statistically meaningful margin.** The aggregate improvement exceeds
   a pre-registered floor:
   ```
   mean(WER_A, 2nd half) − mean(WER_B, 2nd half)  ≥  0.02  (2 percentage points absolute)
   ```
   This guards against declaring victory on a sub-measurement-noise delta.

3. **Monotone divergence, not a fluke.** B must *not* be worse than A in the
   final bucket, and B's WER must be non-increasing from the first half to
   the second half:
   ```
   WER_B(last bucket) ≤ WER_B(first bucket)
   ```
   and
   ```
   WER_B(final bucket) ≤ WER_A(final bucket)
   ```

If any of the three fails, the experiment **fails** (carry-forward is not
doing measurable work), which is an equally valid answer.

---

## 5. Why these thresholds

- **2 percentage points** is pre-registered as a floor above the expected
  measurement noise for a hand-instrumented ~30-word closed vocabulary. It
  is deliberately conservative: the claim being tested is not "B is slightly
  better" but "B meaningfully reduces error under drift."
- **Second-half comparison** targets where carry-forward should matter most
  (after drift has accumulated); the first half is a within-experiment
  sanity check that A and B start comparable.
- **Non-increasing B** rules out the confound where B wins only because it
  happened to stumble into a good late-session run.

---

## 6. Output of the measurement task (39c.5)

The measurement task reports, per bucket, both WERs; then applies this rule
and emits a single verdict: **PASS** or **FAIL**, with the three sub-checks
stated explicitly. The verdict is committed to the bead before any
pipeline stages (3–5) are built on the carry-forward assumption.

---

## 7. Invariants (for the PPT/C-Test system)

1. `INV: measurement_symmetry` — A and B are computed from the *same* audio
   and the *same* error-tally code; only the accent-signature state differs.
2. `INV: buckets_exhaustive` — buckets partition the session; every utterance
   lands in exactly one bucket.
3. `INV: verdict_pre_registered` — the rule in §4 is fixed before analysis
   and is not revised based on the outcome.

These become contract-test invariants for the fail-fast experiment
measurement (task 39c.6).
