# Accent-Signature Feedback Loop — Semantics Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.6)
Date: 2026-08-15
Relates to: Stage 10 accent signature (91x.1, 91x.2, 91x.3)
Synthesis §4.7: accent signature persists/sharpens across the session, feeds
back into Stage 1 scoring, and handles mid-session drift as a live
retraction-and-recascade event.

---

## 1. Purpose

Pin the exact update rule, the feedback mechanism, and the mid-session drift
trigger for the accent-signature fact. These were the unspecified parts of
synthesis §4.7.

---

## 2. What the accent-signature fact is

A **Tier 2 derived fact** representing the systematic offset between this
speaker's realized phonemes and the canonical reference (the phoneme-alphabet
feature space, aem.10). Formally:

```
accent_signature = { Δ_i : i ∈ phoneme_alphabet }
```

where `Δ_i` is the measured mean displacement of the speaker's realization of
phoneme `i` from the canonical formant/feature position (a per-phoneme vector,
computed from committed phoneme facts).

---

## 3. The update rule (bounded exponential moving average)

The signature updates utterance-over-utterance via a **bounded EMA**, so it
sharpens without diverging:

```
Δ_i ← (1 − α) · Δ_i_prev + α · δ_i(new)
```

- `δ_i(new)` = the per-phoneme displacement measured from the just-committed
  utterance's phoneme facts.
- `α` = learning rate, pre-registered (e.g., 0.2) — a Stage-10 empirical
  dial, default 0.2.
- The EMA **bounds** the signature: without re-normalization, an outlier
  utterance moves `Δ_i` by at most `α · |δ|`, and repeated drift converges
  toward the moving mean. This satisfies the boundedness invariant
  (accent PPT test, 91x.4).

---

## 4. Feedback into Stage 1

The accent signature feeds back into Stage 1 scoring for subsequent
utterances as a **scoring bias**:

```
adjusted_score(phoneme_i) = raw_score(phoneme_i) + Δ_i · β
```

where `β` is a feedback gain (pre-registered, e.g., 1.0). The bias nudges
Stage 1 toward the speaker's realized pronunciations. This is the carry-forward
mechanism the fail-fast experiment (epic 2) measures.

---

## 5. Mid-session drift — live retraction-and-recascade

Drift (fatigue, mic shift, rising noise) is handled as a **live
retraction-and-recascade event**:

1. **Trigger detection** — drift is detected when the per-utterance
   displacement `δ_i` deviates from the current signature by more than a
   pre-registered threshold `τ` for multiple consecutive utterances (e.g., 3
   of 5 — a robustness guard against single-outlier false triggers).
2. **Retraction** — the old calibration/accent-signature fact is retracted
   via the fabric's survival semantics (aem.13): `retract(old_signature)`.
3. **Re-derivation** — a new accent-signature fact is derived from recent
   committed phoneme facts.
4. **Recascade** — the fabric automatically revisits everything that depended
   on the old baseline (the always-on provenance, 6za.2, walks the affected
   dependents; Stage 1 scoring for subsequent frames uses the new baseline).

The recascade is automatic because the fabric retraction cascades to
dependents (aem.13 §4) — this is the structural reason Hearsay-IV can
recalibrate mid-stream where Hearsay-II/III could not (synthesis §4.7).

---

## 6. Invariants (for the PPT/C-Test system)

1. `INV: signature_bounded` — the accent signature remains within a finite
   bound over the session (no divergence).
2. `INV: feedback_converges` — repeated consistent evidence moves the
   signature monotonically toward the speaker's realized mean (no
   oscillation/divergence).
3. `INV: drift_trigger_guarded` — drift retraction fires only after the
   multi-utterance threshold (τ) is met, not on a single outlier.
4. `INV: recascade_automatic` — retracting the old signature recascades to
   all dependents (no orphaned baseline-dependent facts).

These become contract-test invariants for Stage 10 (task 91x.4).

---

## 7. Deferred (not unresolved)

- Exact values of `α`, `β`, `τ` are Stage-10 empirical dials; the *semantics*
  (EMA update, feedback bias, guarded drift trigger, automatic recascade)
  are fixed here.
