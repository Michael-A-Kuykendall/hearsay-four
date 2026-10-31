# Fabric Decision Contract — Specification

Spec version: 1.2 (amends 1.1: section 10 unopposed margin; amends 1.0: section 8 tiling exclusion; see docs/design/a4-budget-semantics.md)

Author: Michael A. Kuykendall
Status: Frozen-candidate v1.0 (bead hearsay-four-p7u / A4a)
Date: 2026-10-27
Relates to: Master Plan v3 §6 A4 (parent bead hearsay-four-enu, closed by
split); implemented by hearsay-four-dg8 (A4b); consumed by A5
(hearsay-four-p3o), A6 (hearsay-four-cjj), P2 (hearsay-four-ofy).
Spec version: **1.0** — A4b must cite this version string in its code and
tests. Amendments happen on DEV only, bump the version, and re-freeze
(§14).

This document is the complete written contract for how evidence becomes a
commit in the saturation fabric. It is a spec only: A4a contains no code.
A4b implements exactly this document and nothing else; where this document
and the Master Plan differ, this document wins (differences are listed in
§15).

---

## 1. Scope and non-goals

In scope: the evidence model (source classes, declared nulls, `llr`
definitions), the two scores `S(T)` and `L(h)` and their separation, the
LM accounting rule, the conflict relation, dormancy/revival (including
where right-context evidence for a dormant hypothesis comes from, in
lattice and streaming mode), commit/defeat, and deterministic ties.

Out of scope: acoustic model training (R2–R4), LM training (R1), lattice
file format details (A5 owns the byte format; this contract owns what a
token publishes), the A/B/C experiment design (A6 owns it; §11 reproduces
the treatment table because the contract defines the arms), and the
correction-loop UI (P3 owns fact-set shape; this contract owns the USER
class semantics).

## 2. Notation and substrate

- A **token** `h` is a token fact per A3: `WORD_TOKEN{word, start_frame,
  end_frame, source}`, `SIL_TOKEN{…}`, or `PHONE_TOKEN{…}`. Only WORD and
  SIL tokens appear in tilings (§7). Token facts are never merged across
  time; the same word twice is two tokens.
- A token's **span** is the half-open frame interval
  `[start_frame, end_frame)`. Its length is `end_frame − start_frame`
  frames. Frame indices are whatever the emitting decoder (A2/R5) uses;
  this contract is agnostic to frame shift.
- A **justification** is the existing fabric object (`Justification` in
  `src/fabric/src/lib.rs`): a conclusion fact, a premise set, and an
  unconditional flag. A justification is **active** iff every premise is
  active. A fact is **active** iff ≥ 1 of its justifications is active
  (existing rule, unchanged).
- Every justification that carries decision weight additionally carries,
  as spec-level metadata A4b must represent: a **source class** `c`
  (§3), a numeric **`llr`** (§3), and the **declared null** it was computed
  against. A justification without class/`llr` metadata is provenance
  only and contributes 0 to both scores.
- **Weakening:** if a justification deactivates (a premise retracts), its
  `llr` stops counting. Scores are recomputed from currently active
  justifications only; there is no score caching across a retraction
  (§9.4, cascade).

## 3. Source classes, declared nulls, and `llr` definitions

There are exactly six source classes. Five are **unary** (attached to one
token); SEQUENCE is **pairwise** (attached to an ordered pair of adjacent
tokens, never to either token alone).

| Class | Arity | Declared null | `llr` definition |
|---|---|---|---|
| ACOUSTIC | unary | garbage/SIL model over the same frames | `log P(frames \| word model) − log P(frames \| garbage/SIL)` over exactly the token's frames, computed from frames only — no LM term (§6) |
| LEXICAL | unary | lexicon-free phone-unigram model over the same phone sequence | `log P(phones \| word's lexicon pronunciations) − log P(phones \| phone-unigram null)` |
| SEQUENCE | pairwise | unigram word model | `llr_SEQ(p → h) = log P(word(h) \| history ending at word(p)) − log P_unigram(word(h))`, from the R1 LM, once per adjacent pair (§4, §6) |
| SPEAKER | unary | speaker-independent (SI) model over the same frames | `log P(frames \| speaker-adapted model) − log P(frames \| SI null)` |
| VERIFY | unary | verifier run on a mismatched (shifted) span | `log P(verifier score \| genuine match) − log P(verifier score \| mismatched-span null)` from an independent verification pass |
| USER | unary | no correction present | fixed `+U0` while the owner's correction fact is active; contributes nothing (term absent) when no correction is present; fully retractable with the correction (P3) |

Rules that apply to every class:

1. **Declared null, always.** An `llr` with no named null is not
   admissible evidence. The null is part of the justification's metadata
   and is rendered by P2.
2. **Absent class = absent term.** If a token has no active justification
   in unary class `c`, class `c` contributes nothing to that token (the
   term is omitted, equivalently 0). Absence is never a negative `llr`.
3. **Max per unary class.** Within one unary class, only the strongest
   active justification counts (§4). Weak supports cannot stack.
4. `U0` (USER) is a single fixed constant, tuned on DEV, large enough
   that an active USER justification decides any conflict it participates
   in unless an opposing USER justification is also active. Its value is
   a frozen parameter (§12), not per-correction.
5. USER justifications are positive only. An owner correction asserts the
   corrected token; competing tokens are handled by conflict + defeat
   (§7, §10), not by negative USER `llr`s. Retracting the correction
   retracts the justification and the token reverts to its other
   evidence (weakening, §2).

## 4. Tiling score `S(T)` — what decides commits

For a candidate tiling `T = (t₁ … tₙ)` of a region (§7):

```
S(T) = Σ_i [ Σ_{c ∈ unary} w_c · max_{j ∈ active_c(tᵢ)} llr_j ]
     + Σ_i w_SEQ · llr_SEQ(tᵢ₋₁ → tᵢ)
```

- `unary = {ACOUSTIC, LEXICAL, SPEAKER, VERIFY, USER}`.
- The SEQUENCE sum runs over adjacent pairs in the tiling. The first
  token's pair term uses sentence-start history (`<s>` as `t₀`); every
  adjacent pair, including that boundary pair, is counted **exactly
  once**.
- `w_c` (including `w_SEQ`) are per-class weights tuned on DEV and frozen
  with this spec (§12, §14).

**No-double-counting rule (normative).** A given piece of evidence may
enter `S(T)` at most once, in exactly one of these ways: (a) as the max
`llr` of its unary class for its token, or (b) as the single SEQUENCE
term of its adjacent pair. In particular: the LM contributes to `S(T)`
only through (b) (§6); a justification counted in `L(h)` is not thereby
counted in `S(T)` (§5); and the decoder's internal `path_score` never
enters `S(T)` in any form (§6).

## 5. Liveness score `L(h)` — what decides live vs dormant only

```
L(h) = unary(h) + max_left  w_SEQ · llr_SEQ(p → h)
                + max_right w_SEQ · llr_SEQ(h → s)
```

- `unary(h)` is the same per-token unary sum used inside `S(T)`:
  `Σ_{c ∈ unary} w_c · max_{j ∈ active_c(h)} llr_j`.
- `max_left` ranges over currently available left neighbours `p`;
  `max_right` over currently available right neighbours `s` (§9 defines
  "available" per mode). A missing side contributes nothing.
- `L(h)` is a max-marginal approximation. Its only uses are (a) the
  live/dormant threshold (§8) and (b) hypothesis-budget accounting. It is
  **never** summed into `S(T)`, never compared between two tilings, and
  **never published** as evidence — P2 renders tiling-score components,
  never liveness values.

## 6. LM accounting rule (decoder ↔ fabric)

1. The decoder (A2/R5) may use the LM inside its search for pruning and
   lookahead. That internal score (`path_score`) is **never published**:
   it appears in no lattice field, no token metadata, and no provenance
   rendering.
2. Each published lattice token carries its **ACOUSTIC `llr` alone**,
   computed from frames only, with no LM term (§3).
3. LM evidence enters the fabric in exactly one place: the pairwise
   SEQUENCE term of `S(T)` (and the max-neighbour terms of `L(h)`),
   computed from the same LM, once per adjacent pair.
4. **Byte-identity requirement (A4b must test).** A lattice built with LM
   weight 0 and a lattice built with LM weight 1 must yield
   **byte-identical published token `llr`s**. A4b implements this as a
   property/unit test over its reference lattice construction; A5 re-runs
   the same check over the on-disk format. Any LM-weight-dependent byte
   in a published token is a contract violation, not a tuning issue.

## 7. Conflict and tiling validity

- Two tokens **conflict** iff their spans overlap by **> 50% of the
  shorter span**: `overlap_len / min(len_a, len_b) > 0.5`, spans half-open
  (§2). Overlap of exactly 50% is not a conflict. Zero-length spans are
  invalid tokens and are rejected at emission.
- A **tiling** of a region `[s, e)` is a sequence of WORD/SIL tokens that
  is non-overlapping (no two members conflict — in fact no two overlap at
  all) and covers `[s, e)` (SIL tokens fill non-word gaps). Only tilings
  are scored by `S(T)` and only a tiling can commit (§10).

## 8. Activation, dormancy, and budgets

- **Live:** `L(h) ≥ θ_live`. Live tokens may be extended (streaming),
  join tilings, and count against the **live budget**.
- **Dormant:** `L(h) < θ_live`. A dormant token is retained with all
  justifications but excluded from extension; it counts against the
  **dormant budget**. Dormancy is not retraction: no justification is
  removed, and §9 can raise `L(h)` again.

Amendment 1.1 (section 8): dormant tokens are excluded from tilings; only live tokens join tilings.

- θ_live and both budgets are frozen parameters (§12). Budget exhaustion
  eviction order (which dormant token is dropped first) is: lowest
  `L(h)`, ties by §10 tie order. Eviction is a deliberate retraction and
  is recorded in provenance; it is the only way a dormant token leaves
  before its region commits, other than defeat (§10).
- Arm difference (normative, §11): arm B discards below-threshold tokens
  forever (beam prune); arm C keeps them dormant and revivable, and C's
  budget counts live + dormant against the same total.

## 9. Right-context evidence, revival, and cascade

A dormant token is not extended, so its successors must be produced
independently of it. Where the right-context SEQUENCE justification on a
dormant `h` comes from depends on mode:

### 9.1 Lattice mode (A6)

The lattice already contains all K alternatives per region, generated
once by the decoder (A5). The successors of a dormant `h` are the lattice
tokens starting within ±J frames of `end(h)` (J pre-registered, default
**J = 3** frames). They exist whether or not `h` is live. The fabric
computes `llr_SEQ(h → s)` for each such `s` from the LM and attaches it
to `h` as a right-context justification.

### 9.2 Streaming mode (R5/P1)

The fabric keeps a **dormant-predecessor index** keyed by end frame.
Whenever the decoder creates a new token `s` (from any live path), the
fabric looks up dormant tokens with `end ∈ [start(s) − J, start(s) + J]`
(J = 3 frames default, §12). For each hit `h` it computes
`llr_SEQ(h → s)` from the LM and attaches it to `h` as a right-context
justification. The decoder never extends `h`; it only supplies
successors that live paths already produced. Left-context SEQUENCE
justifications (`p → h`) attach symmetrically when a new left neighbour
becomes available.

### 9.3 Non-SEQUENCE right context

VERIFY and USER evidence attach the same way, by span lookup: a
verification result or owner correction whose span conflicts with /
covers `h`'s span (§7) attaches to `h` regardless of `h`'s live/dormant
state. No mode distinction applies.

### 9.4 Revival and cascade

- **Revival:** if a newly attached justification raises `L(h)` above
  θ_live before `h`'s region commits, `h` rejoins the live set and
  becomes eligible for the tiling DP (and, in streaming, for extension
  from its end frame). The revival event is recorded in provenance:
  which successor token and which justification revived it.
- **Cascade:** if a premise token retracts (e.g. a left-context word
  changes), dependent SEQUENCE justifications deactivate and `L`/`S`
  values are recomputed through the reverse index (dependents of the
  retracted token). A revived token can go dormant again by the same
  recomputation; a token whose region has committed cannot (§10).

## 10. Commit, defeat, and deterministic ties

- A region `[s, e)` is **eligible** when the decoding frontier passes
  `e + H` frames. It **commits** when the best tiling beats the runner-up
  tiling by ≥ δ, or unconditionally at `e + H_max`. `H`, `δ`, `H_max` are
  frozen parameters (§12); `H_max ≥ H` always.
- The best tiling is found by DP over the region's tokens (identical DP
  in arms B and C, §11). Winning tokens are committed via the fabric's
  existing `commit()` (winning justification becomes unconditional; the
  `no_unsupported_promotion` invariant applies — never commit an inactive
  token).
- Non-winning tokens in a committed region are **defeated**: deliberately
  retracted (the fabric's deliberate-retraction marker, so a surviving
  premise cannot resurrect them), with provenance naming the winning
  tiling and the score margin (`S(winner) − S(runner-up incl. loser)`).
  Amendment 1.2 (section 10): with no live runner-up tiling the winner is
  unopposed and the margin is absent (`None`), never +inf; defeat and
  commit provenance carry the absence through, and renderers show no
  numeric margin.
- **Ties (fully deterministic):** higher `S(T)` → earlier start frame →
  lexicographic label → lower token id. The same order breaks ties in
  the DP, in runner-up selection, and in budget eviction (§8). No
  randomness, no iteration-order dependence.

## 11. A/B/C treatment difference (what A6 measures)

| | A acoustic 1-best | B conventional | C Hearsay-IV |
|---|---|---|---|
| Evidence | ACOUSTIC only | all classes | all classes |
| Score function | ACOUSTIC term | identical `Σ_c w_c·max llr` | identical |
| Commit DP | identical | identical | identical |
| Below threshold | discarded | discarded forever (beam prune) | dormant, revivable |
| Left-context change | n/a | not revisited | cascade re-score |
| Budget counted | live | live | live + dormant |

Holding evidence, score, and commit identical across B and C isolates
the hypothesis lifecycle. That lifecycle difference is the claim A6
tests.

## 12. Frozen parameters

All values below are tuned on DEV only, then frozen with this spec.
A4b ships the defaults; changing a value after freeze is a spec
amendment (§14).

| Parameter | Meaning | Default / constraint |
|---|---|---|
| `w_c`, `w_SEQ` | per-class tiling weights | DEV-tuned, frozen |
| `U0` | USER class fixed `llr` (§3) | DEV-tuned, fixed, large (§3 rule 4) |
| `θ_live` | live/dormant threshold on `L(h)` | DEV-tuned, frozen |
| `H`, `H_max` | commit frontier lag / unconditional lag (frames) | `H_max ≥ H` |
| `δ` | commit margin over runner-up | `δ ≥ 0` |
| `J` | dormant successor window (frames) | **3** (pre-registered default) |
| live / dormant budgets | hypothesis budgets (§8) | A6 sweeps totals {50, 100, 250, 500} |

## 13. Conformance — what A4b must implement and test

A4b (hearsay-four-dg8) cites spec version 1.0 and provides:

1. Justified `llr` weights per source class, with declared-null metadata
   (§3), represented on/around the existing `Justification` object.
2. The tiling DP computing `S(T)` exactly as §4, including the
   max-per-unary-class rule and once-per-pair SEQUENCE rule.
3. `L(h)` per §5, used only for thresholding and budgets.
4. The dormant set with budgets, the dormant-predecessor index (J = 3),
   revival with provenance, and cascade re-scoring via the reverse index
   (§8, §9).
5. Defeat with provenance (winning tiling + margin), via deliberate
   retraction (§10).
6. Property tests, all green, plus the existing fabric suite green:
   - **LM-accounting byte-identity:** LM weight 0 vs 1 → byte-identical
     published token `llr`s (§6.4).
   - **Dormancy/revival/defeat scenario:** acoustics slightly favour the
     wrong word, later context revives the dormant correct word, the
     record shows both supports, the revival, and the defeat.
   - **Conflict tiling:** > 50%-of-shorter overlap conflicts; exactly
     50% does not (§7).
   - **Deterministic tie order:** equal scores resolve by §10 order on
     repeated runs and shuffled insertion order.

## 14. Freezing and amendments

This spec freezes when its content hash (SHA-256 of this file) is
recorded in the notes of bead hearsay-four-p7u and A4b's text cites
"spec version 1.0". Amendments: edit on DEV, bump the version in the
header, re-record the hash. A6 (hearsay-four-cjj) may not start against
an unfrozen version. Frozen here means: A4b implements this version; it
does not mean parameter values can never change — it means they change
only by amendment, never silently in code.

## 15. Decisions made in this spec beyond the source text

The Master Plan §6 A4 text is the source. The following were underspecified
there and are decided here (flagged for review; changing any of them is a
§14 amendment):

1. **Declared nulls for LEXICAL, SPEAKER, VERIFY.** The source declares
   the ACOUSTIC null (garbage/SIL) and the SEQUENCE null (unigram)
   explicitly; the other three nulls in §3 are new decisions, chosen as
   the weakest model in the same family so each `llr` stays a
   like-for-like comparison over the same input.
2. **Absent class = absent term (§3 rule 2).** The source does not state
   what a class with no active justification contributes; "term omitted"
   is decided so missing evidence is neutral, never negative.
3. **Boundary pair in `S(T)` (§4).** The source counts SEQUENCE once per
   adjacent pair; the first token's pair against `<s>` is decided to
   count exactly once, so tilings of different lengths stay comparable.
4. **USER positive-only (§3 rule 5).** The source says "large fixed llr,
   retractable"; restricting USER to assertions of the corrected token
   (no negative USER `llr` on competitors) is decided here, with
   competition resolved by conflict + defeat.
5. **`L(h)` uses the same unary sum as `S(T)` (§5).** The source writes
   `unary(h)` without expanding it; identifying it with the §4 unary sum
   is decided so liveness and tiling can never disagree about a token's
   unary evidence.
6. **Eviction order under budget exhaustion (§8)** and the exactly-50%
   boundary (§7) are pinned here; the source gives the budget concept
   and the > 50% threshold but not these two edges.
7. **Version-freeze mechanics (§14).** The bead gate requires the hash
   recorded in bead notes and A4b citing the version; the version string
   itself ("1.0") is introduced here to give A4b a citable target.
