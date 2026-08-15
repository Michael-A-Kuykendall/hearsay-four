# Cross-Cutting Findings

Looking across all nine items, three patterns hold, and they're the actual answer to "does this generalize":

1. **Category C concentrates in exactly one place: destructive overwrite.** 
   Buchberger's `continue`, Hearsay's poison-and-discard, Viterbi's best-path overwrite — every true inversion in this inventory is a case where the original algorithm *throws something away on purpose*, usually for efficiency. That's a small, recognizable pattern, not an open-ended category of unknown difficulty. Auditing any new source for "does this discard alternatives to stay fast" is close to a mechanical test for which bucket it lands in.

2. **Category A is rarer than it feels like it should be, and it clusters around formal automata specifically.** 
   The one family in this inventory that was already state-transition-shaped before anyone thought about fabric. That's a useful prior for scoping future candidates: state machines port near-free; everything with an optimization-driven memory discipline (DP tables, EM loops, greedy elimination) needs real work.

3. **Category D is a real category, not a hedge.** 
   At least one item here (GMM fitting) is correctly excluded from conversion entirely. A methodology that classifies everything as convertible isn't rigorous, it's motivated reasoning. Having a legitimate "leave this alone" bucket is part of what makes the other three classifications trustworthy.

---

## Verdict

The method holds up. Four unrelated fields — commutative algebra, speech understanding, formal language theory, truth maintenance — all sort cleanly into the same four buckets, and the one genuinely hard case (Viterbi) already has a resolved design answer rather than an open problem. That's real evidence the conversion approach generalizes, not just that it worked once on a domain you'd already been thinking about for weeks.

The honest caveat: **classification is not implementation.** Buchberger's Category C is small and scoped — an afternoon. Hearsay's Category C (retraction-with-survival) has a design but no built code yet in dzero or MikeDB, where it's needed regardless of this project. Viterbi's bounded local-window resolution is a design decision, not a measured one — it still needs the fail-fast experiment already scoped in the Hearsay-IV document to confirm it actually works before more is built on top of it.

---

## Suggested Staging (cheapest first)

In order of cost-to-finish, cheapest first:

1. **Buchberger elimination facts + the unconditional-fraction counter.** 
   Smallest scope in this inventory, already designed, directly answers whether the fabric's stability-classification patent claim is worth pursuing. This should be first regardless of what else is prioritized.

2. **Automata-family (2.7) as the FSE ingestion layer** for any future fabric application with exact, discrete selectors — lowest-risk, highest-confidence port in the set, and reusable scaffolding for whatever domain comes after this one.

3. **SPE / FST rule ports (2.2, 2.3)** — Category B, additive, low risk, and the concrete next step for the phonology stages of Hearsay-IV.

4. **Viterbi bounded-window mechanism (2.4)** — gated on the fail-fast experiment already specified. Don't build the general mechanism before that experiment reports back.

5. **Hearsay hypothesis-rejection retraction (2.6)** — depends on the same multi-justification-set fix (D-1) already required in dzero and MikeDB. One correct implementation, now a third consumer.

Items 1 and 5 both depend on the same underlying fix — multi-support justification sets with survival semantics — which is not new work created by this document. It's the single piece of infrastructure this entire inventory, across four unrelated fields, keeps arriving at as the load-bearing requirement.