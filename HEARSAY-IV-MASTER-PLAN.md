# Hearsay-IV — Master Plan v3 (Claude + GPT converged, 2026-10-27)

Supersedes `CONVERGED-PLAN.md` v1/v2. This is the source document for
bead decomposition. No dates: every work package (WP) ends in a binary
gate; dependencies are explicit so lanes can run concurrently.

---

## 1. The goal

**A single-speaker transcription application built entirely from
non-neural, fully inspectable classical models.** It takes live mic or
saved audio (WAV/MP3) and shows provisional then stable text. Every word
can be clicked to see why it was heard that way, which alternatives
lost, and what defeated them. A correction is stored as a retractable
fact that permanently and visibly changes future behaviour.

Motivating case: the owner says "fly" and current services write
"family". Hearsay-IV must (a) show the evidence that produced the error,
(b) let one correction fix it, (c) prove the fix with numbers, and
(d) prove it caused no collateral damage.

Two results, pursued in parallel tracks:
- **Architectural result (Track A):** given identical acoustic
  alternatives, evidence, scoring and compute budget, does the Saturation
  Fabric's hypothesis lifecycle (survive / go dormant / revive /
  cascade-retract) beat ordinary beam pruning? This is the Hearsay-II
  premature-pruning question, answered on a controlled vocabulary.
- **Transcriber result (Track R + Track P):** a large-vocabulary, then
  open-vocabulary, speaker-adapted classical recognizer, delivered as the
  application.

## 2. Terminology (binding for code, docs, beads, writeups)

- **Classical model:** an explicit mathematical object a person can read
  (Gaussian means/variances, HMM transition probabilities, n-gram counts
  with discounts, exemplar templates, decision rules). Hearsay-IV uses
  these.
- **Forbidden:** neural networks, embeddings, pretrained ASR, gradient-
  trained weights, any model API in the recognition path.
- Say "non-neural, fully inspectable classical models". Never say
  "model-free" or "no training corpus".
- Rename `no_model_guard.sh` → `no_neural_guard.sh`; keep its checks.
- **Large-vocabulary (LVCSR):** finite lexicon (CMUdict + extensions).
- **Open vocabulary:** reserved for WP R8 capabilities (§9).
- No placeholder may carry the name of the component it stands in for:
  use a `PLACEHOLDER_` prefix plus a bead to replace it.
- Stage 3 `CommitWindow` is a bounded commit mechanism. Remove
  "Viterbi" from its name and docs.

## 3. Ground truth at `7213203` (verified in code)

| Finding | Evidence | Consequence |
|---|---|---|
| Phone templates are arithmetic | `fse::default_codebook()`: `mean[j]=((i*7+j*3)%11)/5−1` | All phone labels are noise; used by serve, live, runner |
| Fabric observes, doesn't decide | `process_chunk`: N-best offered at top-1 weight; word from `top1`; window winner only feeds a rating | Hearsay-IV untested |
| One word per 2 s chunk | `scored[0]` | 81% WER floor (576/711) with perfect acoustics |
| Token identity destroyed | phone-only syllable facts; premises → `BTreeSet` | No lattice possible |
| N-grams untrained, post-commit | `BigramTable::new()`, no `train`; scored after `scored[0]` | Sequence stage ceremonial |
| Accent "PASS" circular | `ConditionB` shifts baseline toward the audio it scores | Claim narrows to "adaptation tracks persistent acoustic displacement" |
| Vocab phone coverage | 35/39 lexicon phones; missing DH G UH ZH; AW OW OY TH W Y appear in one word each (4 acoustic takes) | Insufficient for phone models |
| Local data (owner machine, not in repo) | `run1` 120 owner words; `better-voice`: `phrase_training` 3,500 phrases ≈5.7 h, `bulk_tts_1k`, `quality_checked_1k` (MP3 in `.wav`), `accent_datasets/*` incl. `librispeech_accents`, `common_voice*` | Inventory first (WP D0). Assume TTS until proven human |

**Keep:** fabric multi-support/retraction/reverse index, provenance
validation, WAV/MP3 decode, live mic capture, streaming loop, PART-1
provisional display, TXT/SRT/VTT export, `record-session`, guard, PPT
invariants, MFCC front end (pending A1).
**Demote:** default codebook, Stage-3 "Viterbi", one-word lexical
choice, post-commit n-gram, C2 "VERIFY" naming.
**Freeze/close:** WER-2, SEG-2, CHUNK-1 epic (superseded by this plan).

---

## 4. Evaluation discipline (applies to every WP)

### 4.1 Data roles

| Role | Use | Rule |
|---|---|---|
| TRAIN | templates, fitted statistics, LM counts | anything |
| DEV | all tuning and selection: front end, band, beam, weights, penalties, thresholds, K, adaptation keep/reject, iteration stopping | look as often as needed |
| PHASE-TEST | one-time gate for a frozen WP | scored once; then becomes REGRESSION |
| REGRESSION | known-result benchmark, rerun forever | never called "test" in claims |
| LOCKBOX | final milestone claims only | opened at most once per major milestone (M2, M4, M5 in §11); each opening is logged |

- Partitions are committed (file lists + checksums) **before** any
  scoring that uses them.
- Splits are by **recording session** for owner audio and by **case**
  (chronological) for Oyez. Never by sentence within a session or case.
- LM training text never contains any DEV/PHASE-TEST/LOCKBOX transcript.

### 4.2 Metrics

- **WER** with S/D/I breakdown (existing `output::wer`).
- **PER** (phone error rate): decoded phone string vs the canonical
  CMUdict pronunciation of a held-out sentence, from a phone-loop decode
  with a phone bigram. Forced-alignment agreement is **not** PER.
- **Boundary error:** median |hyp−ref| word-boundary ms against
  hand-checked spans.
- **RTF**, **peak live hypotheses**, **peak dormant hypotheses**,
  **working-set RAM**.
- **Lattice oracle WER** (§6, A5).

### 4.3 Statistics

Any comparative claim (C vs B, adapted vs base, before vs after
correction) is **paired by utterance** and reports:
ΔWER, a 95% paired bootstrap CI (10,000 resamples over utterances),
and utterances improved / worsened / unchanged.
- "X improves accuracy" requires the CI to lie entirely below 0.
- CI includes 0 → "**no demonstrated difference**". Never "equivalent".
- "**Equivalent**" requires a pre-registered equivalence margin ε (default
  ε = 1.0 WER point) and the whole CI inside [−ε, +ε].
- "**Not worse**" (non-inferiority) requires the CI's upper bound ≤ +ε.

### 4.4 Memory wording

Gate on **bounded active working set**: RAM for live/dormant
hypotheses and decoder state is independent of input duration.
Committed transcript + provenance go to an append-only archive whose
size grows linearly with committed words. Never claim "flat memory".

---

## 5. Work-package map

```
D0 data inventory ──► D1 partitions + recording protocol ──► (all eval)

Track A (architecture)                 Track R (recognizer)
A1 acoustic gate (DTW)                 R1 LM text + n-gram
A2 connected DTW decoder               R2 HMM training subsystem
A3 token identity ──┐                  R3 base acoustic model (SI)   [optional, recommended]
A4 fabric decision contract ─┐         R4 owner enrollment + MAP (retractable stats)
A5 lattice + oracle + K sweep│         R5 phone-lexicon decoder (trie, LM inside) ◄── A3, A4
A6 A/B/C experiment ◄────────┘         R6 triphones + state tying
A7 bound gate                          R7 LVCSR evaluation
                                       R8 open vocabulary (OOV)
Track P (product)
P1 streaming + VAD/endpointing ◄── R5
P2 "why this word" view ◄── A3, A4
P3 correction loop → retractable facts ◄── A4, R4
P4 targeted-confusion benchmark ("fly") ◄── P3
P5 packaging + release
```

Concurrency: after D1, A1–A2, R1, R2, and P1-scaffolding can start at
once. A3/A4 are prerequisites for R5's fabric layer and for P2/P3.

---

## 6. Track A — Architectural result

### A1 ACOUSTIC-GATE
- DEV: leave-one-take-out DTW on `run1` (fixed distance on MFCC frames,
  Sakoe-Chiba band, length-normalized cost). Tune the front end here
  (CMN, deltas/delta-deltas, pre-emphasis, liftering, band).
- PHASE-TEST: new owner session, a 5th take of all 30 words; templates =
  original 4 takes.
- Artifact: `docs/results/A1-ACOUSTIC-GATE.md` + CSV + confusion matrix.
- **Gate:** PHASE-TEST top-1 ≥ 90%. Fail: fix the front end on DEV; a
  rerun needs a new PHASE-TEST session.

### A2 CONNECTED-DTW
Specialized one-stage connected-word DTW / token-passing decoder (Ney
family). DTW cells are **not** fabric facts.
- Token state: `(unit ∈ {word, SIL, SP, GARBAGE}, template_pos,
  start_frame, path_cost, backpointer, lm_history)`.
- Required: SIL/SP templates (from recording lead/trail), optional
  garbage state, word-insertion penalty, min/max duration per word,
  explicit path-length normalization, SIL endpoints, beam width,
  max-live tokens, max template expansion.
- Data: connected 3–7-word phrases of the 30-word vocab. DEV may use D0
  data (TTS allowed for DEV). **PHASE-TEST is owner human speech**
  (≥ 100 phrases, own session).
- **Gate:** PHASE-TEST WER ≤ 25%; boundary error ≤ 100 ms median on 20
  hand-checked phrases; RTF < 1; peak live ≤ configured bound.

### A3 TOKEN-IDENTITY
- **Type facts** (hash-consed globally): `WORD_TYPE`, `PRONUNCIATION`,
  n-gram entries, template ids, model parameters.
- **Token facts** (never merged across time):
  `WORD_TOKEN{word, start_frame, end_frame, source}`,
  `PHONE_TOKEN{phone, start_frame, end_frame}`, `SIL_TOKEN{…}`.
- Provenance preserves order and multiplicity.
- **Gate:** same word twice in one utterance → two facts, distinct spans,
  independent retraction, provenance chains distinct.

### A4 FABRIC-DECISION-CONTRACT (mathematical heart; freeze before A6)

A written spec (`docs/design/fabric-decision-contract.md`) + reference
implementation + property tests. Proposed defaults below. Amend on DEV
only, then freeze.

**Evidence.** Each justification has a boolean premise set (activation)
**and** a numeric weight: a log-likelihood ratio `llr` against a
declared null, in a **source class** c ∈ {ACOUSTIC, LEXICAL, SEQUENCE,
SPEAKER, VERIFY, USER}.
- ACOUSTIC null = garbage/SIL model over the same frames.
- SEQUENCE llr = log P(w | history) − log P_unigram(w).
- USER = owner correction (see P3), large fixed llr, retractable.

**Two scores, never mixed (prevents double-counting):**

1. **Tiling score** (what decides commits). For a candidate tiling
   T = (t₁ … tₙ) of a region:
   `S(T) = Σ_i [ Σ_{c ∈ unary} w_c · max_{j ∈ active_c(tᵢ)} llr_j ]
        + Σ_i w_SEQ · llr_SEQ(tᵢ₋₁ → tᵢ)`.
   Unary classes = ACOUSTIC, LEXICAL, SPEAKER, VERIFY, USER. SEQUENCE is a
   **pairwise** term on adjacent tokens, counted **once per adjacent
   pair**, never attached to either token. Within a unary class only the
   strongest active justification counts, so weak supports can't stack.
   `w_c` are tuned on DEV.
2. **Token liveness score** (what decides live vs dormant only):
   `L(h) = unary(h) + max_left w_SEQ·llr_SEQ(p → h) + max_right w_SEQ·llr_SEQ(h → s)`
   over currently available neighbours p, s. A max-marginal
   approximation used **only** for thresholding and the hypothesis
   budget. It is never summed into S(T) and never published as
   evidence.

**LM accounting across decoder and fabric.** The decoder (A2/R5) may use
the LM inside its search for pruning and lookahead (its internal
`path_score`). That internal score is **never published**. Each
lattice token carries its ACOUSTIC llr alone, computed from frames only
with no LM term. LM evidence enters the fabric only as the pairwise
SEQUENCE term above, computed from the same LM, once per adjacent pair.
A5 includes a unit test: a lattice built with LM weight 0 and with LM
weight 1 yields byte-identical published token llrs.

**Conflict.** Two tokens conflict iff their spans overlap by > 50% of
the shorter span. A committed region must be a non-overlapping tiling
of WORD/SIL tokens covering the region.

**Activation.** Existing rule: active iff ≥ 1 active justification.
**Weakening:** losing a justification lowers the score.
**Dormant:** liveness L(h) below the live threshold. The hypothesis is
retained with all justifications but excluded from extension; it counts
against the dormant budget.

**Where right-context evidence for a dormant hypothesis comes from.**
A dormant token is not extended, so its successors must be produced
independently of it:
- *Lattice mode (A6):* the lattice already contains all K alternatives
  per region, generated once by the decoder (A5). The successors of a
  dormant h are the lattice tokens starting within ±J frames of
  end(h) (J pre-registered, default 3). They exist whether or not h is
  live.
- *Streaming mode (R5/P1):* the fabric keeps a **dormant-predecessor
  index** keyed by end frame. Whenever the decoder creates a new token s
  (from any live path), the fabric looks up dormant tokens with
  end ∈ [start(s) − J, start(s) + J]. It computes llr_SEQ(h → s) from the
  LM and attaches it to h as a right-context justification. The decoder
  never extends h; it only supplies successors that live paths already
  produced.
- Other right-context sources (VERIFY, USER) attach the same way, by
  span lookup.

**Revival:** if a newly attached right-context justification raises
L(h) above the live threshold before h's region commits, h rejoins the
live set and becomes eligible for the tiling DP (and, in streaming,
for extension from its end frame). The revival event is recorded in
provenance (which successor and which justification revived it).
**Cascade:** if a premise token retracts (e.g. a left-context word
changes), dependent SEQUENCE justifications deactivate and scores are
recomputed through the reverse index.

**Commit.** A region [s,e) is eligible when the decoding frontier
passes `e + H` frames. It commits when the best tiling beats the
runner-up by ≥ δ, or unconditionally at `e + H_max`. The best tiling is
found by DP over tokens (identical DP in B and C). Non-winning tokens in
a committed region are **defeated**: deliberately retracted, with
provenance naming the winning tiling and score margin.

**Ties.** Higher score → earlier start → lexicographic label → lower
token id. Fully deterministic.

**The A/B/C treatment difference (this is what A6 measures):**

| | A acoustic 1-best | B conventional | C Hearsay-IV |
|---|---|---|---|
| Evidence | ACOUSTIC only | all classes | all classes |
| Score function | ACOUSTIC term | **identical** Σ_c w_c·max llr | **identical** |
| Commit DP | identical | identical | identical |
| Below threshold | discarded | **discarded forever** (beam prune) | **dormant, revivable** |
| Left-context change | n/a | not revisited | cascade re-score |
| Budget counted | live | live | live + dormant |

Holding evidence, score and commit identical isolates the hypothesis
lifecycle. That is the Saturation Fabric claim, not "more information".

### A5 LATTICE + ORACLE
- Lattice format on disk: timed tokens + per-class llr + arcs; versioned.
- Generated **once** per acoustic configuration; A, B, C read the same
  file.
- **K sweep:** K ∈ {3, 5, 10, 20} alternatives per region. Report oracle
  WER (best achievable WER selecting from the lattice) vs K vs compute.
- **Gate:** oracle WER at chosen K is ≥ 5 points below B's WER on DEV.
  Otherwise the bottleneck is acoustic generation: fix A2/R5 before A6.

### A6 FABRIC-EXPERIMENT
- Same lattice, evidence, `w_c`, H, δ for B and C.
- **Budget frontier:** total hypothesis budget ∈ {50, 100, 250, 500}
  (B: live; C: live + dormant). Per point: WER, RTF, peak
  live/dormant, working-set RAM, late-rescue count (C wins with a
  token that B had pruned), errors C fixes vs B, errors C introduces.
- Statistics per §4.3 at each budget.
- Constructed unit test (component correctness only): acoustics slightly
  favour the wrong word, later context reverses it, record shows both
  supports, the dormancy/revival, and the defeat.
- **Pre-registered readings:**
  - C better than B (CI excludes 0) at equal budget, with late rescues →
    **architectural result**.
  - C equals B's WER at a smaller budget → **efficiency result**.
  - CI includes 0 → **no demonstrated difference** in accuracy. C is
    still the transcriber's decision layer for explainability and
    corrections. "Equivalent" may be claimed only under the §4.3 margin
    rule.
- Run first on the A2 whole-word lattice, rerun on the R5 phone
  lattice (R5 gate).

### A7 BOUND-GATE
- 30-min and 3-h inputs: working-set RAM within ±10% between them;
  archive grows linearly; RTF < 1.

---

## 7. Track R — Recognizer

### R1 LANGUAGE MODEL
- Text sources (owner approval required per source): public-domain
  general English (e.g. Project Gutenberg), CC BY-SA encyclopedic text,
  Oyez **LM-train cases only**, and the owner's own writing (personal
  LM; the biggest single lever for personal dictation).
- Trigram (→ 4-gram later) with **modified Kneser-Ney** discounting
  (counts and discounts stored, inspectable). Entropy pruning to a
  size budget. Personal LM interpolated with general LM, weight tuned on
  DEV.
- Text normalization rules (numbers, punctuation, casing) explicit and
  tested.
- **Gate:** DEV perplexity reported per source mix; LM file loads in
  < 2 s; deterministic.

### R2 HMM TRAINING SUBSYSTEM
A real classical acoustic-model trainer (HTK-style), not a decoder mode.
- 3-state left-to-right monophone HMMs, diagonal Gaussians, 1 mixture
  to start; SIL (3-state) and SP (1-state, tied to SIL centre).
- Flat start from global mean/variance; forced state alignment from
  transcript + CMUdict (optional SP between words); Viterbi then
  Baum-Welch embedded re-estimation of means, variances, transitions;
  variance floors; mixture splitting (1→2→4→8) driven by DEV.
- **Selection:** save a checkpoint each iteration and keep the **best DEV
  PER/WER checkpoint**. Likelihood is a training diagnostic only;
  non-monotone DEV PER across iterations is expected and not a failure.
- Every parameter stores **sufficient statistics** (count, Σx, Σx²) and
  a provenance pointer to its source frames/utterances.
- Model file: text-readable (or JSON) with a lossless binary twin for
  speed.
- **Gate:** on a synthetic fixture with known generating HMMs, recovered
  parameters within tolerance; on real data, the selected checkpoint's
  DEV PER is lower than the flat-start (iteration-0) DEV PER, with a
  paired CI below 0.

### R3 BASE ACOUSTIC MODEL (speaker-independent; recommended)
- If D0 confirms approved **human** read speech (e.g. LibriSpeech
  subsets, CC BY 4.0), train an SI base model on it with R2.
  Classical guidance: ~1 h for single-speaker command/control, ~10 h for
  single-speaker dictation. An SI base plus adaptation beats a small
  owner-only model.
- If owner rejects external audio: skip. R4 trains owner-only and the
  owner's recording target rises (R4).
- **Gate:** SI DEV WER reported (no threshold; it is a starting point).

### R4 OWNER ENROLLMENT + MAP ADAPTATION
- **Recording protocol** (`record-session`): same mic class, quiet room,
  16 kHz mono, prompt-read sessions of public-domain phonetically
  balanced sentences (Harvard sentences, then further PD prompts).
  Session-tagged.
- Data targets: initial 1 h (falsification corpus); grow to 5–10 h as
  gates demand. Owner-only path (no R3): 1 h minimum before the first
  gate.
- **MAP adaptation via retractable sufficient statistics** (per Gaussian,
  per dimension; n = occupancy count, Σx and Σx² occupancy-weighted
  sums of the adaptation frames, τ = prior weight tuned on DEV):
  - **Stage 1 (gate here): means only.**
    `μ̂ = (τ·μ_base + Σx) / (τ + n)`; variance stays at `σ²_base`.
  - **Stage 2 (optional, after Stage 1 passes):** variance from the
    second moment.
    `E[x²]̂ = (τ·(σ²_base + μ²_base) + Σx²) / (τ + n)`,
    `σ̂² = max(E[x²]̂ − μ̂², variance_floor)`.
    Kept only if it improves DEV WER over Stage 1 (paired CI).
  Each contribution
  (count, Σx, Σx², source frames, supporting hypothesis id) is a
  **fabric fact**. Retracting the fact subtracts its statistics and
  recomputes. The base model is immutable; adaptation is an overlay.
- **Gate:** owner DEV WER (adapted) < owner DEV WER (base) with paired CI
  excluding 0. Overlay keep/reject decided on DEV; report once on
  PHASE-TEST.

### R5 PHONE-LEXICON DECODER
- Token passing over a pronunciation **trie** of HMM states. State =
  `(time, trie_node, phone, phone_state, lm_history, acoustic_score,
  path_score, backpointer)`. LM applied inside the search (lookahead at
  trie nodes), not after commit.
- Emits word-token lattices in the A5 format into the fabric (A3/A4).
  Decisions are made by system C.
- Replaces whole-word DTW for all vocabularies. DTW remains as a
  regression baseline.
- **Gate:** on the 30-word REGRESSION phrases, WER ≤ 1.5× the A2 WER;
  RTF < 1; A6 rerun on this lattice and reported.

### R6 TRIPHONES + STATE TYING
- Word-internal then cross-word triphones; decision-tree state tying
  with **hand-written phonetic questions** (nasal? stop? front vowel?
  …), split by likelihood gain with a minimum occupancy; each tied state
  traces to the questions that formed it.
- Mixture splitting on tied states.
- **Gate:** owner DEV WER improves over best monophone system (paired CI).

### R7 LVCSR EVALUATION
Full CMUdict trie + R1 LM + best acoustic system, through system C.
Comparisons on owner-read PHASE-TEST (new sentences, never aligned):
chunk baseline · phone decoder no LM · phone decoder + n-gram (B) ·
phone decoder + fabric (C).

**Tiered gates (pre-registered):**
| Tier | Owner-read WER | Meaning |
|---|---|---|
| T1 | ≤ 50% | genuinely transcribing |
| T2 | ≤ 25% | usable with corrections |
| T3 | ≤ 12% | daily-driver competitive |
All tiers: RTF < 1 live, bounded working set (A7).
Reported, not gating: owner spontaneous speech (5 min, hand-transcribed),
Oyez locked-eval cases (multi-speaker, era audio).

### R8 OPEN VOCABULARY
Two separate capabilities, claimed separately:
1. **Dynamic lexicon extension:** user supplies a new spelling →
   rule-based inspectable G2P produces pronunciation(s) → word is
   recognizable. Each generated pronunciation is a retractable fact.
   Gate: recall on a set of words removed from the lexicon then re-added
   by spelling.
2. **Unknown spoken words:** an OOV token (phone-loop path with an OOV
   cost) competes in the fabric. Output `[OOV: N AY T]` plus ranked
   candidate spellings from an inspectable P2G. The correct spelling is
   **not** promised (P2G is ambiguous: night/knight/nite).
   Gate: OOV detection precision/recall on a locked set with held-out
   words; spelling candidates reported, not gated.

---

## 8. Track P — Product

### P1 STREAMING + ENDPOINTING
- Energy/spectral VAD with hangover for live mic; endpoint-driven region
  closure; provisional (live/dormant) vs stable (committed) text.
- File and mic paths share one decoder (existing live/batch parity test
  extended).
- **Gate:** live/batch word parity on REGRESSION files; mic latency from
  end of word to provisional text ≤ 500 ms; RTF < 1.

### P2 "WHY THIS WORD" VIEW
Per committed word: acoustic llr and frames, LM probability and history,
speaker/adaptation contribution, alternatives with scores, the defeating
tiling and margin, any revival events, user facts involved. Exportable
as JSON with the transcript.
- **Gate:** every committed word in a REGRESSION run renders a complete
  chain (provenance validator passes 100%).

### P3 CORRECTION LOOP → RETRACTABLE FACTS
A user correction (hyp "family" → ref "fly" at span) creates USER facts:
1. USER justification on the corrected token (immediate fix);
2. personal-LM count increments for the corrected n-grams;
3. pronunciation-variant evidence if the acoustics disagree with all
   lexicon pronunciations;
4. MAP sufficient-statistic contributions from the corrected frames
   (R4).
Every item is a fact with provenance to the correction; **undoing the
correction retracts all of them exactly**.
- **Gate:** property test: apply then undo N random corrections → model
  state byte-identical to before.

### P4 TARGETED-CONFUSION BENCHMARK (the "fly" proof)
- Owner lists real recurring errors (fly→family, etc.).
- Per pair: record ≥ 20 DEV sentences and ≥ 20 PHASE-TEST sentences
  containing the target word, in natural contexts, plus contexts where
  the competitor is correct ("my family").
- Protocol: measure → apply corrections from DEV only → re-measure
  PHASE-TEST target accuracy **and** REGRESSION WER (collateral damage).
- **Gate:** target-word accuracy improves (paired CI); competitor-correct
  sentences **not worse** and REGRESSION WER **not worse**, both by the
  §4.3 non-inferiority rule (CI upper bound ≤ +ε).

### P5 PACKAGING
CLI + `hearsay-serve` + desktop-runnable app (mic, file, export,
why-view, corrections, enrollment wizard running the R4 protocol),
release matrix, `no_neural_guard.sh` green, reproducible model build
from recorded data with checksums.

---

## 9. Claims allowed at each milestone

| Milestone | Requires | Claim |
|---|---|---|
| M1 | A1 | "Classical features discriminate the owner's words (X% held-out)." |
| M2 | A2+A5+A6 on DTW lattice (LOCKBOX opened once) | Architectural result as per the A6 pre-registered reading |
| M3 | R2+R4+R5 | "Speaker-adapted classical phone recognizer, WER X on owner read speech" |
| M4 | R7 tier reached + P1–P4 (LOCKBOX) | "Non-neural single-speaker transcription app at tier T_n, with per-word provenance and retractable corrections; fly-class errors fixed with no collateral damage" |
| M5 | R8 | Open-vocabulary claims, split per §7 R8 |

Never claim: "model-free"; "open vocabulary" before M5; accent PASS as
recognition improvement; any number from REGRESSION as a test result.

## 10. Risk register (tripwires)

| Risk | Tripwire | Response |
|---|---|---|
| Front end lacks discrimination | A1 < 90% after DEV tuning | Add deltas/CMN/PLP-style features; do not proceed |
| Lattice is the bottleneck | A5 oracle gap < 5 pts | Raise K / improve acoustics before A6 |
| No demonstrated C vs B difference | A6 CI includes 0 at all budgets | Report honestly; keep C for provenance/corrections; try lifecycle variants on DEV only |
| Too little owner audio | R4 gate fails at 1 h | Record more (to 5–10 h) or approve R3 SI base |
| Synthetic data contamination | Any TTS audio in a test role | Remove; rerun gate on human data |
| Leakage | Any test file in TRAIN/DEV/LM text | Invalidate the result; new PHASE-TEST |
| Self-poisoning adaptation | Unsupervised overlay raises DEV WER | Restrict to user-corrected/high-confidence; bounded update; rollback |
| Real-time miss | RTF ≥ 1 live | Tighten beam/K on DEV; profile; LM pruning |
| Placeholder promoted again | Any `PLACEHOLDER_` on a served path at a milestone | Milestone blocked |

## 11. Process rules

1. Each WP's top gate is a held-out number. Unit tests are component
   correctness (test hierarchy: L1 components → L2 acoustic validity →
   L3 temporal decoding → L4 later evidence changes ranking → L5 fabric
   ablation → L6 bounds/RTF).
2. Every implementation simplification is written in its bead.
3. No lane closes on its own report; gates are rerun on main.
4. Partitions are committed before scoring; LOCKBOX openings are logged.
5. Data-source rule stands: no source is used until the owner approves
   it, with licence and human/TTS status recorded in D0.
6. Results live in `docs/results/<WP>.md` with the command to reproduce
   and the artifact checksums.

## D0 / D1 (start here)

**D0 DATA-INVENTORY:** for every `better-voice` set and `accent_datasets/*`:
exists, files, hours, rate, checksum, **human or TTS** (scripts/reports
+ listen to 10 clips), speakers, licence, 30-word coverage, proposed
role. → `docs/results/D0-DATA-INVENTORY.md`, owner-approved.

**D1 PARTITIONS + PROTOCOL:** committed split manifests for every
approved set; recording protocol doc; prompt lists (Harvard sentences,
30-word phrases, P4 target sentences); session naming;
LOCKBOX sessions recorded and sealed (checksums committed, files not
read by any pipeline until opened).
