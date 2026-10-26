# Buchberger Unconditional-Fraction Measurement — Report

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.9)
Date: 2026-08-15
Instrument: `src/groebner` (classify_pairs, unconditional_fraction) via
`measure` bin. Conversion inventory §2.1.

## Question

What fraction of Buchberger S-pair eliminations are **unconditional**
(product criterion, premise keyed solely on {f,g}) vs **defeasible** (chain
criterion, premise references a third basis element k)? This gates whether
the fabric's stability-classification claim is worth pursuing.

## Result

| Ideal | Pairs | Product (uncond) | Chain (defeasible) | Uncond fraction |
|---|---|---|---|---|
| disjoint | 3 | 3 | 0 | 1.00 |
| mixed | 6 | 4 | 2 | 0.67 |
| dense | 6 | 5 | 1 | 0.83 |
| **Overall** | 15 | **12** | **3** | **0.80** |

## Verdict

The overall unconditional fraction is **0.80** (12/15), and drops to 0.67
in the mixed ideal. Chain (defeasible) eliminations are common and real.

**Interpretation:** the fabric's stability classification — unconditional iff
every premise is keyed solely on the conclusion's own nodes — is a *general*
(premise-locality) definition, not merely convenient to this one algorithm.
Because real eliminations are frequently defeasible (referencing an external
k), the unconditional/defeasible distinction does real, load-bearing work.
This supports pursuing the stability-classification claim: the classification
is meaningful, not vacuous.

## Caveat (scope of the measurement)

This is a *representative synthetic* measurement over hand-built ideals, not
exhaustive over all polynomial ideals. The exact fraction depends on ideal
structure; the qualitative conclusion (defeasible eliminations are common and
the distinction matters) is robust to the exact value. A production sweep
over real benchmark ideals (e.g., from the literature) is possible follow-up
but not required for the gating decision.

## Invariants (for the PPT/C-Test system)

1. `INV: product_is_unconditional` — a product-criterion elimination is
   classified unconditional.
2. `INV: chain_is_defeasible` — a chain-criterion elimination is classified
   defeasible.
3. `INV: fraction_in_range` — the unconditional fraction ∈ [0,1].
