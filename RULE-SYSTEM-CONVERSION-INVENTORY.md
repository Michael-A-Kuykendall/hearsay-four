# Prior Rule-System Inventory
## Convertibility Assessment — Existing Research → Saturation Fabric Arena Facts

Author: Michael A. Kuykendall
Status: Working analysis — pre-implementation
Draft date: 2026-08-03

---

## 0. The actual question

Saturation Fabric's leverage is highest exactly where you said it: domains
where the hierarchical rule set is **already worked out** by someone else,
and the job is porting the rules into arena facts rather than discovering
the rules from scratch. That only pays off if the porting is real — if a
meaningful fraction of prior research turns out to be a straight relabeling
job, not a rewrite.

This document tests that directly, across four genuinely different bodies of
work: computer algebra (Buchberger), speech understanding (Hearsay),
formal language automata (YFilter/XPush/Aho-Corasick), and truth
maintenance (JTMS/ATMS) — the last being the mechanism the fabric's own
justification model descends from, included so the inventory is honest
about what's being reused wholesale versus adapted.

**If the classification holds up cleanly across all four, the method
generalizes and the remaining work is mostly labor. If it breaks on one of
them, that's the domain to be honest about before committing to it.**

---

## 1. The classification framework

Your original split — straight comparison vs. inversion — is correct as a
starting cut, but the actual porting work sorts into four buckets, not two.
The difference matters because each bucket has a different cost and a
different risk profile:

| Bucket | Definition | Cost | Risk |
|---|---|---|---|
| **A — Direct port** | Already fact-shaped: a condition, a state, a derived value read from and written to shared structure. Rehost with no logic change. | Low | Low — the hard thinking is already done by the original authors |
| **B — Port + augment** | The core logic is already rule-shaped, but lacks retraction, provenance, or justification tracking. Add the missing piece without touching the underlying rule logic. | Medium | Low-medium — augmentation is additive, unlikely to break the ported logic |
| **C — True inversion** | Currently expressed as imperative control flow — a decision embedded in a branch or loop, with a side effect that evaporates. Must be taken apart and re-expressed as an assertable fact plus a derivation rule. | Medium-high | **This is where the real engineering risk concentrates** |
| **D — Feeds facts, stays external** | The algorithm is legitimately ordinary computation — numerical iteration, signal processing — that should stay outside the fabric and simply hand its output in as a fact. Not every algorithm needs to *be* the fabric. | Low | Low, but easy to get wrong in the other direction — over-inverting internals that have no downstream reasoning consumer is pure overhead |

The fourth bucket matters as much as the third. The failure mode isn't only
"this is harder to invert than I thought" — it's also "I inverted something
that never needed inverting," which produces a slower system with no
architectural benefit.

---

## 2. Inventory

### 2.1 Buchberger's algorithm (Gröbner bases) — **Category C**

Current form, verbatim from `groebner.rs`:

```rust
if product_criterion(f, g) { continue; }
if chain_criterion(f, g, &basis) { continue; }
```

Both are boolean predicates over structural properties of polynomial pairs.
The predicates themselves are trivial to state as rules. What makes this
Category C is architectural, not logical: the `continue` is a decision that
**leaves nothing behind**. Nothing records that the pair was eliminated, or
why. The elimination has to be reified as its own fact — `CriterionElimination(i,
j, kind)` — asserted by a rule instead of executed as a control-flow branch.

**This case also sharpens the fabric's own stability definition, worth
stating precisely rather than assumed:**

- **Product criterion** (leading monomials of `f` and `g` are coprime) —
  its premise is keyed *solely* on `{f, g}` themselves. **Unconditional.**
  Intrinsic to the pair, and since the arena is append-only, that pair's
  leading monomials can never change underneath it.
- **Chain criterion** (a third basis element `k` makes the pair redundant)
  — its premise references `k`, a node outside `{i, j}`. Under the fabric's
  own definition — a justification is unconditional only if every premise
  is keyed solely on the conclusion's own nodes — this is **defeasible**,
  even though in this specific algorithm the basis only grows and the
  elimination will never actually be un-derived in practice. The
  classification is about premise locality, not about whether it happens to
  be stable in this one algorithm. Worth keeping that distinction explicit
  — it's the difference between a definition that's merely convenient here
  and one that's actually correct in general.

**Status:** design is fully worked out (see the fabric architecture doc,
§4 and §6). The measurement gating further investment — what fraction of
real eliminations are unconditional — is still unrun. This is the cheapest
item on the whole list to finish; it needs an afternoon, not a redesign.

### 2.2 SPE phonological rules (Chomsky & Halle, 1968) — **Category B**

Rules are already context-sensitive rewrites: `A → B / X_Y`. The rule
*form* needs no inversion — it's already "if this phoneme is in this
context, derive this output," which is a production rule in every sense
that matters. What's missing is that a conventional implementation performs
the rewrite as an **in-place string mutation**, destroying the prior form.
Porting means keeping the rule exactly as written and changing only the
output mechanism: assert the rewritten form as a new fact justified by the
original phoneme fact plus the context facts, rather than overwriting
anything.

### 2.3 Kaplan & Kay FST cascades (1994) — **Category B**

A finite-state transducer's transition function is already a rule:
`(state, symbol) → (state', output)`. Cheap to express as facts. What FST
cascades don't have, by design, is any notion of "why" — a cascade resolves
deterministically to an output with the intermediate transitions discarded.
Same shape as 2.2: keep the transition logic untouched, add provenance as a
wrapper rather than a rewrite.

### 2.4 Viterbi / HMM decoding (Rabiner, 1989) — **Category C, and the
hardest inversion in this inventory**

This one deserves more attention than the others, because it's the one
case where the *purpose* of the original algorithm is in direct tension
with what the fabric wants to do.

Viterbi's dynamic-programming table keeps, for each state at each time
step, only the single best path so far — and it **overwrites** every
suboptimal path that reaches the same state. That's not an incidental
implementation detail; it's the source of Viterbi's efficiency. Forgetting
dominated alternatives on purpose is the entire trick.

The fabric wants close to the opposite: keep competing alternatives around
as independent justification sets, so a hypothesis that currently loses can
still be inspected, and can still win later if new evidence favors it.
Porting this exactly as literature-Viterbi would mean re-introducing the
same discard-and-forget behavior the fabric exists to avoid. Porting it
naively the other way — keeping every competing path forever — reintroduces
the exact combinatorial blowup that sank Hearsay-II.

**The resolution already reached in the Hearsay-IV design (§4.4 of that
document) is the correct middle path**: bound competition to a local
window (roughly syllable-scale), keep N-best alternatives live as
justification sets *within* that window, and only commit — Tier 1 → Tier 2
promotion, Viterbi-style discard — once the window closes. That's not a
compromise reached by accident; it's the specific answer to this specific
tension, and it's worth flagging here as the reason that design decision
exists at all.

### 2.5 GMM speaker calibration (Reynolds & Rose, 1995) — **Category D**

Expectation-maximization parameter fitting is iterative numerical
computation with no downstream reasoning consumer for its intermediate
steps — nothing needs to ask "why did this EM iteration produce this
covariance update." The *output* — a per-speaker baseline — is naturally
fact-shaped and enters the arena as a single derived fact. The fitting
procedure itself should stay exactly what it is: ordinary code that feeds a
fact in, not a target for inversion. This is the clearest Category D case
in the inventory, and worth keeping as the reference example for "don't
invert this" going forward — the failure mode of over-applying this whole
methodology is turning something like an EM loop into fabric facts for no
reason, which would just be slower and gain nothing.

### 2.6 Hearsay-II / Hearsay-III blackboard (1974–1981) — **split: B, C, and
discarded, by component**

Already fully worked through in the Hearsay-IV synthesis; summarized here
for completeness:

- **Knowledge-source structure** (independent modules reading and posting
  to shared structure) — **B**. The blackboard *is* already arena-shaped in
  spirit; it just needs justification tracking added.
- **Hypothesis rejection** (a rejected unit is discarded and "poisons" its
  context, per Erman/London/Fickas 1981) — **C**. Must become retraction
  with survival, the one Hearsay-III explicitly didn't have.
- **The control blackboard / scheduler** — **not ported at all.**
  Structurally replaced by alpha-indexed dispatch, which solves the
  scheduling problem Hearsay-III needed a second blackboard for. This is
  the one component where the right move is deletion, not conversion.

### 2.7 YFilter / Green-Suciu DFA / XPush / Aho-Corasick — **Category A,
with a caveat**

An automaton transition — `(state, input) → state'` — is already the
cleanest possible fact-and-rule pair available in this entire inventory:
"if current-state fact and input-symbol fact, derive next-state fact." Both
YFilter's merged NFA and the XPush machine's compiled pushdown automaton
are, structurally, already exactly what a fabric rule looks like when
selectors are exact strings rather than continuous acoustic evidence.

**The caveat:** none of these systems have retraction as a concept at all —
they're forward-only stream processors, correctly so, since matching a
selector against a byte stream has no natural "undo" event (this is the
same FSE-vs-fabric boundary established for Pale Horse and the JSON
engines). So "conversion" here doesn't mean porting logic that's currently
wrong — it means recognizing this family is **already the FSE half** of the
architecture, not a fabric conversion candidate at all. Where it becomes
fabric-relevant is only at the layer above the automaton — e.g., a matched
selector deriving a Tier 2 semantic fact that *can* later be retracted, the
same FSE→fabric handoff already used everywhere else in the portfolio.

### 2.8 JTMS / ATMS (Doyle; de Kleer, 1986) — **not a conversion target —
this is the mechanism itself**

Included for completeness and honesty, not as a porting candidate. De
Kleer's environment structure — a set of assumptions, minimality via
subsumption — **is** the multi-justification-set design the fabric already
needs (and currently lacks, per the D-1 defect in both dzero and MikeDB).
Nothing here gets "converted." This is the source the fabric's own
justification model was drawn from; every other item in this inventory is
being measured against a target this family defines.

### 2.9 Soufflé provenance / `egg` proof extraction — **Category B**

Both produce proof structure — a provenance lattice, an equality-saturation
proof — but by **reconstruction at query time**, searching after the fact
for a minimal-height proof tree. The shape of what they produce is exactly
right; the mechanism producing it is the opposite of what the fabric does.
Porting means keeping their proof-tree *format* as the target output, while
sourcing it from justification sets that are live and always-on rather than
reconstructed on demand. This is the "proof-without-search" distinction
already established for dzero's Lean export — same pattern, worth naming
explicitly as a reusable target shape rather than rediscovering it per
domain.

---

## 3. Cross-cutting findings

Looking across all nine items, three patterns hold, and they're the actual
answer to "does this generalize":

1. **Category C concentrates in exactly one place: destructive overwrite.**
   Buchberger's `continue`, Hearsay's poison-and-discard, Viterbi's
   best-path overwrite — every true inversion in this inventory is a case
   where the original algorithm *throws something away on purpose*, usually
   for efficiency. That's a small, recognizable pattern, not an open-ended
   category of unknown difficulty. Auditing any new source for "does this
   discard alternatives to stay fast" is close to a mechanical test for
   which bucket it lands in.

2. **Category A is rarer than it feels like it should be, and it clusters
   around formal automata specifically** — the one family in this inventory
   that was already state-transition-shaped before anyone thought about
   fabric. That's a useful prior for scoping future candidates: state
   machines port near-free; everything with an optimization-driven memory
   discipline (DP tables, EM loops, greedy elimination) needs real work.

3. **Category D is a real category, not a hedge.** At least one item here
   (GMM fitting) is correctly excluded from conversion entirely. A
   methodology that classifies everything as convertible isn't rigorous,
   it's motivated reasoning. Having a legitimate "leave this alone" bucket
   is part of what makes the other three classifications trustworthy.

---

## 4. Verdict

The method holds up. Four unrelated fields — commutative algebra, speech
understanding, formal language theory, truth maintenance — all sort cleanly
into the same four buckets, and the one genuinely hard case (Viterbi)
already has a resolved design answer rather than an open problem. That's
real evidence the conversion approach generalizes, not just that it worked
once on a domain you'd already been thinking about for weeks.

The honest caveat: **classification is not implementation.** Buchberger's
Category C is small and scoped — an afternoon. Hearsay's Category C
(retraction-with-survival) has a design but no built code yet in dzero or
MikeDB, where it's needed regardless of this project. Viterbi's bounded
local-window resolution is a design decision, not a measured one — it still
needs the fail-fast experiment already scoped in the Hearsay-IV document to
confirm it actually works before more is built on top of it.

---

## 5. Suggested staging

In order of cost-to-finish, cheapest first:

1. **Buchberger elimination facts + the unconditional-fraction counter.**
   Smallest scope in this inventory, already designed, directly answers
   whether the fabric's stability-classification patent claim is worth
   pursuing. This should be first regardless of what else is prioritized.
2. **Automata-family (2.7) as the FSE ingestion layer** for any future
   fabric application with exact, discrete selectors — lowest-risk,
   highest-confidence port in the set, and reusable scaffolding for
   whatever domain comes after this one.
3. **SPE / FST rule ports (2.2, 2.3)** — Category B, additive, low risk,
   and the concrete next step for the phonology stages of Hearsay-IV.
4. **Viterbi bounded-window mechanism (2.4)** — gated on the fail-fast
   experiment already specified. Don't build the general mechanism before
   that experiment reports back.
5. **Hearsay hypothesis-rejection retraction (2.6)** — depends on the same
   multi-justification-set fix (D-1) already required in dzero and MikeDB.
   One correct implementation, now a third consumer.

Items 1 and 5 both depend on the same underlying fix — multi-support
justification sets with survival semantics — which is not new work created
by this document. It's the single piece of infrastructure this entire
inventory, across four unrelated fields, keeps arriving at as the load-bearing
requirement.
