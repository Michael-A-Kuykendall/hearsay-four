# Hearsay-IV
## Offline, Model-Free Speech Transcription on Saturation Fabric
### A Research Synthesis

Author: Michael A. Kuykendall
Status: Working thesis — pre-prototype
Draft date: 2026-08-03

---

## 0. Thesis

Every offline-capable speech transcription system in wide use today is a
trained model — a compressed statistical approximation of a training corpus,
opaque by construction, and stateless across utterances unless someone bolts
speaker-adaptation on top of it as an afterthought.

The bet here is that a **reactive, derivation-based fact engine** — Saturation
Fabric, the same architecture underneath dzero-cas, MikeDB, and the fire-control
system — can do the same job through a different mechanism entirely: no
training corpus, no weight matrix, no black box. Phonetic and phonological
knowledge encoded directly as rules, probability handled as bounded local
competition between justification sets rather than as opaque numbers baked
into a model, and the entire derivation chain inspectable after the fact.

This is not proposed as a Whisper competitor on open-vocabulary accuracy. It
is proposed as something Whisper structurally cannot be: **small, fully
offline, stateful across a session by construction, and able to show its
work** — which of two candidate words won, and exactly why, all the way down
to the acoustic evidence.

That last property is the actual target market: forensic and legal
transcript review, where a human is already re-checking every line and
currently has no way to ask the machine why it wrote what it wrote.

---

## 1. Motivation

Two lineages of speech recognition exist, and each gave up something the
other has:

**Rule-based / knowledge-based systems** (1970s–80s) encoded real linguistic
and acoustic knowledge directly, and could in principle explain a decision.
They lost to statistical methods because they had no coherent way to manage
*uncertainty at scale*. Adding a knowledge source made the system slower and
harder to schedule, not more capable — the opposite of the property this
architecture is built to guarantee elsewhere in the portfolio.

**Statistical / neural systems** (1980s HMMs onward, culminating in
transformer-based models like Whisper) solved uncertainty by learning it from
enormous corpora. They win decisively on raw accuracy and on long-range
semantic disambiguation. They lose on everything else that matters for this
use case: they require training data most people can't collect, they run
opaque, they carry no notion of *this specific session* beyond whatever
context window is in flight, and there is no way to ask one *why* it chose
one transcription over a very close second.

**The gap:** nothing pairs classical, inspectable phonetic knowledge with a
mechanism that (a) handles real acoustic uncertainty without combinatorial
blowup, and (b) treats "why" as a first-class, always-available output rather
than a research problem in interpretability.

Saturation Fabric was not built for this domain. But the two failure modes
above are, respectively, exactly what alpha-indexed dispatch and
justification-set retraction were built to solve in every other domain this
engine has touched.

---

## 2. Prior art and lineage

### 2.1 The Hearsay lineage — the direct ancestor

- **Lesser, Fennell, Erman, Reddy (1974).** *Organization of the Hearsay-II
  speech understanding system.* IEEE Symposium on Speech Recognition. The
  original multi-level, multi-knowledge-source organization.
- **Erman & Lesser (1975).** *A multi-level organization for problem solving
  using many, diverse, cooperating sources of knowledge.* IJCAI-75. The
  cooperating-knowledge-source formulation that became the blackboard model.
- **Erman, Hayes-Roth, Lesser, Reddy (1980).** *The Hearsay-II
  speech-understanding system: Integrating knowledge to resolve uncertainty.*
  ACM Computing Surveys, 12(2), 213–253. **The primary source for the
  confidence-combination and evidence-rating formulas this project intends to
  port directly.**
- **Balzer, Erman, London, Williams (1980).** *Hearsay-III: A domain
  independent framework for expert systems.* AAAI-80, 108–110. The extraction
  of Hearsay-II's blackboard architecture into a domain-independent engine —
  representation primitives plus an interpreter for large-grained,
  flexibly-schedulable production rules ("knowledge sources"), intended for
  use well outside speech.
- **Erman, London, Fickas (1981).** *The design and an example use of
  Hearsay-III.* IJCAI-81, 409–415. The implementation detail: acceptance
  routines run per unit, rejected units "poison" their context, and a
  *separate, user-defined control blackboard* is required just to decide
  which knowledge source runs next.

**What transfers directly:** the knowledge-source decomposition itself
(segmentation → phonetic classification → syllable → lexical → syntactic),
already validated by the field as the right layering; the confidence-rating
and evidence-combination formulas in the 1980 Computing Surveys paper.

**What does not transfer, and why this is a real fourth generation rather
than a re-implementation:**

| Hearsay-II | Domain-specific. Hardwired to speech only. |
|---|---|
| Hearsay-III | Domain-independent, but a rejected hypothesis is **discarded and poisons its context** — no survival semantics — and the engine needs a *second, separate control blackboard* just to schedule which knowledge source fires next, with (by their own account) "no theory of effective control problem-solving." |
| **Hearsay-IV (this)** | Domain-independent; a rejected hypothesis is **retracted with provenance**, and survives if it retains independent support from another justification set; scheduling is **alpha-indexed dispatch** — a fact is routed only to rules that can fire on it, computed once at program construction, no separate control layer required. |

### 2.2 Classical phonetics and phonology

- **Davis, Biddulph, Balashek (1952).** *Automatic Recognition of Spoken
  Digits.* Bell Labs' "Audrey" — the earliest working recognizer, hardwired
  formant filters against fixed reference patterns, zero training corpus.
  The historical precedent that a training-free acoustic front end is not a
  new idea, only an old one that lost a fair fight to statistical methods at
  larger vocabulary sizes.
- **Chomsky & Halle (1968).** *The Sound Pattern of English.* The generative
  phonological rule framework — assimilation, elision, flapping — that Stage
  3 below is built from. Fifty years settled, symbolic by construction.
- **Kaplan & Kay (1994).** *Regular models of phonological rule systems.*
  Computational Linguistics. Finite-state transducer cascades for
  phonological rule application (see also the Pynini toolkit) — mature,
  fast, but opaque: a cascade resolves to an answer with no inspectable
  derivation. Saturation Fabric's job here is to keep FST-equivalent
  correctness while keeping the derivation legible.

### 2.3 Handling uncertainty without a trained model

- **Rabiner (1989).** *A tutorial on hidden Markov models and selected
  applications in speech recognition.* Proceedings of the IEEE, 77(2). The
  canonical reference for the Viterbi algorithm and why HMMs displaced
  rule-based systems — principled, tractable combination of probabilistic
  evidence at scale. **Viterbi / N-best lattice pruning is adopted directly**
  as the bounded local-commit mechanism (§4.3).
- **Reynolds & Rose (1995).** *Robust text-independent speaker
  identification using Gaussian mixture speaker models.* IEEE Trans. Speech
  and Audio Processing. The classical (non-deep-learning) precedent for
  per-speaker acoustic calibration — a lightweight statistical fit from a
  short enrollment sample, not a trained model in the sense this project
  means to avoid. Relevant to Stage 0 calibration; **not** adopted for any
  authentication purpose (§5.3).
- **de Kleer (1986).** *An Assumption-Based Truth Maintenance System.*
  Artificial Intelligence, 28(2). The disjunction-across / conjunction-within
  justification structure — multiple independent support sets per fact, and
  label minimality via subsumption — that underlies weighted hypothesis
  survival in this design and, not coincidentally, the multi-justification
  fix already required in dzero and MikeDB.

### 2.4 Considered and explicitly rejected

- **Richardson & Domingos (2006).** *Markov Logic Networks.* Machine
  Learning, 62. **Rejected.** Treating probability as a first-class object
  that rule layers reason over directly is well-trodden — MLNs, and
  De Raedt, Kimmig & Toivonen's **ProbLog** (2007, IJCAI) — and inference in
  both is provably intractable in the general case (#P-hard), typically
  requiring approximate MCMC or variational inference with no clean,
  terminating fixpoint. This would trade Saturation Fabric's core
  guarantee — a deterministic, terminating, provenance-tracked saturation
  loop — for exactly the property that makes it useful. Rejected on that
  basis, not on novelty grounds.
- **Global hard-threshold discretization.** Committing a phone the instant
  its confidence crosses a fixed density point, with no local competition
  window. Equivalent to Viterbi with no beam: a single bad frame propagates
  its error forward with nothing downstream able to recover it. Rejected in
  favor of the bounded local N-best window in §4.3.
- **Voice as an authentication credential.** The Stage 0 calibration
  utterance was briefly considered as a security/verification feature ("my
  voice is my password"). Explicitly rejected as a product framing: voice
  biometric authentication is now considered broken industry-wide, following
  demonstrated cheap AI voice-clone bypasses of production systems (e.g. a
  2023 clone of a reporter's own voice, made with roughly five minutes of
  audio via a consumer tool, walked through a major bank's voice-ID system).
  The calibration *mechanism* is retained — it is unrelated to
  authentication and does real work in §5.3 — but nothing here should be
  pitched as a security product.
- **Porting Hearsay-III's scheduler wholesale.** Its best-first,
  focus-of-attention scheduling — and the separate control blackboard needed
  to run it — exists to solve a problem alpha-indexed dispatch already
  solves structurally. Porting it would re-import the exact bottleneck this
  architecture eliminates.

---

## 3. Honest scope

This system is **not** being built to match Whisper-class open-vocabulary
accuracy, and no version of this document should claim otherwise. The reason
is structural, not a matter of engineering effort: long-range semantic
disambiguation ("the bank" near "river" vs. near "loan officer") is
compressed into a trained model's weights from a training corpus this
project does not have and is not trying to build. Stage 5 (§4.5) is
deliberately flat there, by design, not by oversight.

What it targets instead:

- **Accent and drift robustness for a known or enrolled speaker**, via
  persistent, session-scoped calibration facts that a stateless model does
  not carry by default.
- **A fully offline, no-training-corpus, small-footprint deployment.**
- **A legible provenance chain** — for every word, the competing candidate
  that lost and the specific evidence that decided it — for a downstream
  human reviewer who already has the semantic and case context a general
  listener lacks.

That combination points at one buyer first: forensic and legal transcript
review, not consumer dictation. That buyer is the **first buyer, not the
ceiling**. This project is the inheritor of the Hearsay title from the first
three experiments — a line whose original ambition was general, knowledge-
based speech understanding, not a single courtroom product. The architecture
here — small, fully offline, model-free, flat-file, with a legible derivation
chain — is deliberately the shape that can branch out beyond legal review
into any domain where a small, inspectable, no-training-corpus recognizer has
value. Legal review is where the "show your work" property is worth money
today; it is the wedge, not the box. Nothing in this design is specific to
the courtroom — the accent signature, the provenance chain, the closed-
vocabulary classical front end, and the offline constraint all generalize to
any known-or-enrolled-speaker application (medical dictation, field
recording, personal transcription, assistive use). The honest limits in §3
(no Whisper-class open vocabulary, flat Stage 5) are limits of the *general*
approach, not limits that only apply in a courtroom.

---

## 4. Architecture

```
Stage 0   Calibration (per speaker, per session)
             ↓
Stage 1   FSE — frame-level acoustic scoring (stateless, single-pass)
             ↓
Stage 2   Arena ingestion (hash-consed phoneme-candidate facts)
             ↓
Stage 3   Local commit window — N-best competition, phonological rules,
          weighted justification sets (fabric — retraction lives here)
             ↓
Stage 4   Lexicon matching (classical dictionary lookup)
             ↓
Stage 5   Sequence plausibility (classical n-gram table)
             ↓
Output    Transcript + full provenance chain to raw acoustic evidence
```

### 4.1 Stage 0 — Calibration

A short, fixed enrollment utterance — the same phrase for every speaker, so
signatures are comparable across speakers rather than merely
self-consistent — sets a per-speaker formant/cepstral baseline. Classical
fit (cf. Reynolds & Rose §2.3), not a trained model: seconds of audio, no
gradient descent, no corpus. A well-known spoken phrase is a stronger choice
of prompt than an arbitrary one, since it carries a culturally established
reference prosody in addition to phonetic content, sharpening the initial
baseline on both axes at once.

### 4.2 Stage 1 — FSE frame scoring

Windowed audio, formant/LPC extraction, scored against the Stage 0 baseline.
Single-pass, stateless, no retraction — this is pure FSE, structurally
identical to the selector-first dispatch used elsewhere in the portfolio.
**Emits N-best candidates per frame, not argmax** — collapsing to a single
winner here would discard exactly the information Stage 3 needs.

### 4.3 Stage 2 — Arena ingestion

Candidate phonemes enter as hash-consed facts. Recurring fragments — common
syllables, function words — collapse to the same `NodeId` on reoccurrence,
giving cross-utterance memoization for free, for the same structural reason
MikeDB gets cross-query memoization for free.

### 4.4 Stage 3 — Local commit window (the core mechanism)

The resolved technical bet of this design: **discretize probability into
ordered, competing rule alternatives, bounded to a local window** (roughly
syllable-scale — an empirical parameter, not a fixed constant; see §6),
rather than either (a) committing globally and irreversibly the instant a
threshold is crossed, or (b) carrying probability as a first-class object
through every layer indefinitely (§2.4).

Within the window: candidate phoneme facts compete via multi-support
justification sets (de Kleer, §2.3) under phonological combination rules
(Chomsky & Halle, §2.2), using confidence-combination math ported from
Hearsay-II (§2.1). A candidate is promoted from Tier 1 to Tier 2 — "committed"
— when it wins the local competition. The losing candidate is not
discarded; it is **retracted with its provenance intact**, exactly the
Hearsay-III departure this project makes (§2.1 table).

This is the same compositional move that makes the Risch algorithm's
transcendental towers tractable: once a lower layer is committed, higher
layers treat it as given rather than re-deriving it.

### 4.5 Stage 4 — Lexicon

Committed syllable sequences matched against a classical pronunciation
dictionary (CMUdict-shaped — a lookup table, not a model). Word candidates
inherit justification from the syllable facts beneath them.

### 4.6 Stage 5 — Sequence plausibility

A classical n-gram table over word sequences. Deliberately flat, by design
(§3) — this is where a neural language model would add long-range semantic
disambiguation this architecture does not attempt to replicate.

### 4.7 Cross-cutting — the accent signature

Not a separate subsystem: a derived Tier 2 fact, built from the same
committed phoneme facts Stage 4 consumes, representing the systematic offset
between this speaker's realized phonemes and the canonical reference —
methodologically the same move forensic and sociolinguistic phoneticians use
to characterize an accent (e.g. describing a vowel shift as measured formant
drift from a reference position).

This fact **persists and sharpens across the session** and feeds back into
Stage 1 scoring for subsequent utterances from the same speaker — the
carry-forward property a stateless per-utterance model does not have by
default. Mid-session drift (fatigue, a mic shift, rising background noise) is
handled as a live retraction-and-recascade event: the calibration fact is
re-derived, and the cascade automatically revisits whatever depended on the
old baseline. This is the one piece with no real precedent in the Hearsay
lineage — both Hearsay-II and III were single-utterance batch systems with
no notion of recalibrating mid-stream.

---

## 5. Open questions (unresolved — not yet claims)

- **Commit-window width (§4.4)** is an empirical dial, not a settled design
  parameter. Too narrow reintroduces the brittleness of global hard
  thresholding; too wide reintroduces the combinatorial load the fabric
  exists to eliminate. To be swept directly against the prototype's error
  curve, not decided on paper.
- **Long-session memory and provenance growth is unmeasured.** The
  small-footprint precedent from the fire-control deployment (state smaller
  than a photograph) does not transfer by analogy — that system's state
  space is small because it is bounded and known at compile time. Speech is
  an open, continuous stream, closer in shape to MikeDB's fact volume than
  to the interceptor's. `provenance_stats()` — already built for exactly
  this purpose in the fabric — is the correct instrument to answer this, not
  an assumption.
- **Whether the amalgamation is greater than its parts is untested.** Every
  component above has independent precedent. Whether they compose the way
  this document expects is the actual research question, not a foregone
  conclusion.

---

## 6. Proposed fail-fast experiment

Before building the full pipeline, isolate the one claim actually in
question: **does session-persistent accent-signature carry-forward do real
work, independent of everything else?**

**Setup:** one speaker, closed vocabulary (~30 words, small enough to
hand-instrument). Classical front end only — no pretrained anything. Record
one natural session with real drift in it (fatigue, mic distance, ambient
noise change over ~20 minutes).

**Two conditions, same audio:**

- **A — control.** Every utterance scored fresh against a generic reference.
  No memory across utterances.
- **B — treatment.** Accent-signature fact persists and updates
  utterance-over-utterance within the session (§4.7).

**Measure:** error rate over the course of the session, A vs. B.

**Success criterion:** B's error rate should separate downward from A's as
the session progresses. If it does, the carry-forward mechanism is doing
real, measurable work and the rest of the pipeline is worth building at
scale. If A and B don't separate, that's the answer too — cheaply, in a
weekend, before Stages 3–5 are built on an assumption that didn't hold.

---

## 7. Relationship to the rest of the portfolio

This is not a new engine — it is a fourth domain pack on the same
architecture already carrying dzero-cas, MikeDB, and the fire-control system:

- **Multi-support justification sets with survival semantics** — the same
  fix required in dzero's and MikeDB's fact stores (currently overwritten
  on second derivation in both) is the exact mechanism Stage 3 needs for
  weighted hypothesis competition. One correct implementation, four
  consumers.
- **Alpha-indexed dispatch** replaces Hearsay-III's control blackboard here
  the same way it replaces a conventional query planner in MikeDB.
- **The FSE / fabric split** is the same two-layer shape as everywhere
  else in this portfolio: FSE for the stateless, single-pass, per-frame
  scoring (§4.2); fabric for everything that chains, competes, and can be
  retracted (§4.4 onward).

---

## 8. Note on citations

The Hearsay-lineage citations in §2.1 were verified against primary and
secondary bibliographic sources during this research synthesis. The
remaining classical references in §2.2–2.4 are standard, canonical citations
in their respective subfields, drawn from general knowledge rather than
verified line-by-line in this session — author, year, title, and venue
should be treated as reliable, but exact page ranges should be re-checked
against the primary source before use in any formal citation context.

This document is a working synthesis, not a peer-reviewed artifact. Treat
every claim outside §2 (prior art) and §7 (portfolio mechanism, already
implemented elsewhere) as a hypothesis pending the experiment in §6, not as
an established result.
