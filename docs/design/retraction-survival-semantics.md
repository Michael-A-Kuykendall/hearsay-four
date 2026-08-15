# Retraction Survival Semantics — Design Specification

Author: Michael A. Kuykendall
Status: Resolved design (bead hearsay-four-aem.13)
Date: 2026-08-15
Relates to: D-1 fix (6za.1), Stage 3 retraction (d73.6), Core Fabric (6za)

---

## 1. Purpose

This document pins the precise rule for what happens when a hypothesis loses
local competition in Stage 3's commit window. The synthesis and conversion
inventory say "a rejected hypothesis is **retracted with provenance**, and
survives if it retains independent support from another justification set,"
but leave the exact rule unspecified. This is the load-bearing decision: the
D-1 multi-support justification fix in the fabric (also needed by dzero and
MikeDB) is built from this definition.

---

## 2. Definitions

**Justification set** — a set of premise facts (assumptions or derived facts)
that jointly entail a conclusion fact. Written `J = {p1, p2, ..., pn} ⟹ c`.

**Support** — a fact `c` is *supported* if it has at least one justification
set whose premises are all currently *active* (not retracted).

**Active** — a fact is active iff it has not been retracted.

**Retraction** — an event that removes a fact from active status while
*preserving its justification sets and provenance* (it is not deleted; it is
marked inactive). A retracted fact may become active again.

**Losing candidate** — in Stage 3, the candidate phoneme fact that did not
win the local N-best competition.

---

## 3. The precise survival rule

> **A fact `c` is retracted if and only if it has no active justification
> set. It survives (stays active) if and only if it has at least one active
> justification set.**

Equivalently, in the de Kleer / ATMS framing: `c` is in the current
environment iff at least one of its justification sets is entirely active.

This is a **local, per-fact, monotone-descending rule**:

- Retraction of a *premise* `p` cascades: every conclusion that loses its
  last active justification set becomes inactive too (recursive, bounded by
  the local commit window).
- A fact is only ever retracted when its *last* active justification set
  becomes inactive. If it has another independent justification set that is
  still active, it **survives** — this is the survival semantics.

### 3.1 What "independent support" means

Two justification sets `J1 ⟹ c` and `J2 ⟹ c` are *independent* if they share
no common premise that, were it retracted, would break both. The minimal
independence test: `J1 ∩ J2 = ∅`. Where sets overlap, they are not fully
independent; survival requires at least one set whose premises are all
active *and* which does not share a failing premise with every other set.

### 3.2 Unconditional vs defeasible justifications

From the conversion inventory (Buchberger analysis, §2.1):

- **Unconditional** — every premise is keyed solely on the conclusion's own
  nodes (cannot change once asserted; arena is append-only). Once active,
  never retracted by premise failure.
- **Defeasible** — at least one premise references a node outside the
  conclusion's own nodes. Can be retracted if that external premise is
  retracted.

The survival rule treats both uniformly: a fact is active iff ≥1 active
justification set, regardless of unconditionality. Unconditionality only
means the premise cannot fail; it does not change the survival test.

---

## 4. Algorithm (implementation-ready)

Maintain, per fact `c`:

```
support_count(c) = number of justification sets of c
                  whose premises are all active
```

**Retract premise `p`** (e.g., a losing candidate or a drifted calibration):
1. Mark `p` inactive (preserve its justifications + provenance).
2. For each conclusion `c` that has `p` in one of its justification sets:
   - Recompute `support_count(c)`.
   - If `support_count(c) == 0` and `c` is currently active → retract `c`
     (recursively apply this procedure to `c`'s dependents).
   - Else → `c` **survives** (it has another active justification set).

**Assert justification set** `J ⟹ c`:
1. If all premises of `J` are active and `c` is inactive → reactivate `c`
   (survival across re-assertion).
2. Increment `support_count(c)`.

**Promotion Tier 1 → Tier 2 (commit)**: a candidate that wins local
competition is *committed*: its winning justification set is marked
unconditional (premises are all in-window, append-only), so it is stable
thereafter. Losing candidates are retracted per the rule above — but they
survive if they still hold an independent active justification set.

---

## 5. Invariants (for the PPT/C-Test system)

These are the runtime invariants that must be actively enforced, per the
invariant_ppt framework (epic vuo), and will be the basis of contract tests:

1. `INV: fact_active_iff_supported` — a fact is active iff
   `support_count(c) > 0`.
2. `INV: retraction_preserves_provenance` — retraction never deletes a
   justification set; it only changes active status.
3. `INV: survival_requires_independent_support` — a fact is never retracted
   while it has an active justification set.
4. `INV: no_unsupported_promotion` — a fact is never promoted (committed)
   unless it has ≥1 active justification set.
5. `INV: cascade_is_bounded` — a single premise retraction propagates only
   within the local commit window; no cross-window cascade.

---

## 6. Relationship to existing defects

The **D-1 defect** in dzero and MikeDB fact stores is: facts are
*overwritten on second derivation*, losing prior justification sets. This
design fixes D-1 by storing a *set* of justification sets per fact and
tracking `support_count`, rather than overwriting. One correct
implementation of this design serves four consumers (dzero, MikeDB,
Hearsay-IV Stage 3, and the fire-control system).

---

## 7. Open parameters (deferred, not unresolved)

- The exact **window width** governing `INV: cascade_is_bounded` is a Stage 3
  empirical dial (bead aem.1), not fixed here.
- Whether re-activation of a retracted fact should itself be an *event* for
  provenance display is a Stage 11 output-schema decision (bead aem.16).

These are explicitly deferred; the survival rule itself (Sections 3–5) is
fully determined by this document and implementable without them.
