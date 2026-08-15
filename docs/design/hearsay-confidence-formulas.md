# Hearsay-II Confidence / Evidence-Combination Formulas — Port Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.7)
Date: 2026-08-15
Relates to: Stage 3 N-best competition (d73.4), confidence math
Source: Erman, Hayes-Roth, Lesser, Reddy (1980), ACM Computing Surveys 12(2)
Verified against: primary PDF (mas.cs.umass.edu/Documents/Erman_Hearsay80.pdf)

---

## 1. Purpose

The synthesis (§2.1) claims "confidence-combination and evidence-rating
formulas" from Erman et al. 1980 will be ported directly into Stage 3's
N-best competition. This document extracts what the paper *actually* specifies
and pins the concrete formulas for Hearsay-IV. It includes one important
correction surfaced by reading the primary source.

---

## 2. What the primary source actually says

### 2.1 The rating representation

Hearsay-II attaches a **credibility rating** to each hypothesis, on an
arbitrary 0–100 scale. The paper's trace shows the concrete form:

```
REDDY(85, 26: 52)     // rating 85, created at step 26, evidence span 52
[+ARE+REDDY (91, 0: 52)  // combined phrase rating 91
```

- The number **before the colon** is the credibility rating (0–100).
- The number **after the colon** is evidence location/time.
- Each hypothesis carries one scalar rating.

### 2.2 The critical correction: Hearsay-II used ad hoc combination

Reading the primary source (pp. 2276–2278), the paper states:

> "Hearsay-II uses **ad hoc schemes** for such rating combinations
> [HAYE77d]. **HWIM takes a formal approach, using an application of
> Bayes' theorem.** To implement this, each KS's ratings are calibrated by
> using performance statistics gathered on test data."

This is an important finding: **Hearsay-II's actual combination was ad hoc,
not a clean published formula.** The formal, portable combination — the
calibrated Bayes' theorem approach — is from **HWIM** (the bolt-on
successor), not the Hearsay-II paper the synthesis cited as the port source.

The synthesis's premise ("port Hearsay-II's formulas directly") is therefore
partially inaccurate. Hearsay-IV should port the **HWIM calibrated-Bayes
mechanism** (the formal, reusable form) rather than Hearsay-II's ad hoc
schemes — which were the very thing the synthesis (rightly) wanted to avoid
reimplementing.

---

## 3. The ported formula (HWIM calibrated-Bayes, adapted to Hearsay-IV)

### 3.1 Calibration

Each knowledge source (KS) — here: acoustic scorer (Stage 1), phonological
rules (Stage 3), lexical match (Stage 4), sequence (Stage 5) — outputs an
evidence strength that is **calibrated** against performance statistics into a
log-likelihood weight:

```
w_i = ln( P(h | e_i) / (1 − P(h | e_i)) )
```

where `P(h | e_i)` is the measured posterior of hypothesis `h` given evidence
`e_i` (from a calibration pass). This maps arbitrary per-KS ratings onto a
common scale.

### 3.2 Combination (Bayes independence)

For independent evidence sources `e_1 .. e_k` supporting hypothesis `h`, the
combined log-odds:

```
L(h | e_1..e_k) = L0(h) + Σ_i w_i
```

with prior log-odds `L0(h) = ln(P0(h)/(1−P0(h)))`.

### 3.3 Back to a 0–100 credibility rating

For the Hearsay-IV output schema (aem.16) and the commit decision, convert
log-odds to a credibility rating on the Hearsay-II 0–100 scale:

```
rating(h) = 100 · σ( L(h | e_1..e_k) )   = 100 / (1 + e^(−L(h)))
```

(σ = logistic sigmoid). This preserves the Hearsay-II "arbitrary 0–100"
surface while using the principled, calibrated combination underneath.

---

## 4. How this feeds Stage 3

In Stage 3's N-best competition (d73.4), each candidate phoneme/word fact
accumulates a log-odds score as evidence arrives:

- Start from prior `L0(h)` (e.g., lexical frequency for a word, or uniform
  for a phoneme).
- Add calibrated weight `w_i` for each supporting justification/evidence.
- The **winning** candidate (highest rating) is committed (Tier 1→2).
- The **losing** candidates retain their evidence as justification sets (per
  the survival semantics, aem.13) — so the "why this won" question is
  answerable via the accumulated `w_i` per candidate.

This composes with:
- **Survival semantics** (aem.13): a losing candidate survives if it retains
  an independent active justification.
- **Local commit window** (aem.1): competition is bounded to the window.

---

## 5. Invariants (for the PPT/C-Test system)

1. `INV: rating_in_range` — every credibility rating ∈ [0,100], finite.
2. `INV: logodds_finite` — intermediate log-odds are finite (no NaN/inf),
   which the sigmoid mapping preserves for any finite input.
3. `INV: evidence_additive` — combining more supporting evidence never
   decreases a candidate's rating (monotone in supporting evidence).
4. `INV: calibration_consistent` — every KS's evidence is mapped through its
   calibration weight before combination; no raw un-calibrated rating enters
   the combination.

These become contract-test invariants for Stage 3 confidence math (task
d73.7).

---

## 6. Impact on the synthesis (documented correction)

The synthesis §2.1 said the "confidence-combination and evidence-rating
formulas in the 1980 Computing Surveys paper" would be ported directly.
Reading the primary source shows:

- Hearsay-II's combination was **ad hoc** ([HAYE77d]), not a clean formula.
- The **formal, portable** approach is **HWIM's calibrated Bayes' theorem**,
  mentioned in the same paper (pp. 2277–2278).

**Recommendation:** update the synthesis's §2.1/§4.4 framing to cite HWIM's
calibrated-Bayes mechanism as the actual port source, with Hearsay-II's
0–100 credibility-rating surface preserved as the output scale. This is a
refinement, not a redesign — the architecture is unchanged; only the citation
of the formula's origin is corrected.
