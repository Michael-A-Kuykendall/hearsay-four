# Cloud architecture audit: A1 acoustic correction (received 2026-10-28)

> Provenance (local addition, not part of the audit): recorded verbatim
> from the cloud reviewer's report. Audited commit: `8d1c559` (main),
> whose session writeup is
> `docs/results/SESSION-2026-10-28-A1-ACOUSTIC-CORRECTION.md`.
> Local opinion on this audit was delivered in chat, not in this file.

---

# Audit verdict

**Do not record another PHASE-TEST yet.** The local work in `8d1c559` fixed real defects and made the A1 measurement more honest, but the conclusion “we have hit an acoustic/physics wall, now the owner must rescope” goes farther than the evidence supports. I audited current `main`; it is indeed at `8d1c559`.

The overall Hearsay-IV research program is still sound. I would **not abandon DTW, not abandon the Saturation Fabric experiment, and not pivot the project.** I would change the experiment plumbing and a couple of gates before proceeding.

## What the local agent got right

The `0.0333` result absolutely should be voided. The causal chain is solid: duration mismatch made the Sakoe-Chiba endpoint unreachable, every DTW distance became infinity, and the previous strict argmin silently converted that into `congealed`. The new `Option`/degenerate handling and `NOT COMPARABLE` behavior are the correct fixes.

Endpoint trimming is also the right thing to add. An isolated-word recognizer should not get eight percentage points merely because you habitually say particular words at particular positions inside a three-second recording window. Keeping the trimmed `0.8583` rather than preserving the prettier `0.9417` was scientifically correct.

Stopping instead of lowering the threshold after seeing the number was also correct.

Where I disagree is what `0.8583` means. A 30-way classifier has chance accuracy of about **3.3%**. Getting 103/120 correct says the MFCC/DTW representation very clearly contains word-discriminative information. But it does **not** prove ≥90% cross-session robustness, and it does **not** prove that 85.8% is the physical ceiling.

Your A1 description asks, essentially, “do classical MFCC features discriminate these 30 words at all?”, while its acceptance criterion says ≥90%. Those are not the same scientific question.

## The things I would change now

| Priority | Finding | Decision |
|---|---|---|
| **BLOCKER** | A4 allows dormant tokens into `best_tilings()` | Fix before A6 |
| **BLOCKER** | A4 declares `live_budget` but does not enforce it | Fix before A6 |
| **BLOCKER** | `run1` is still formally NOT-VERIFIED/excluded under D0/D1 | Do not call A1 a formal gate result yet |
| **HIGH** | gh1 requires 8/8 diagnostic ablations ≥90% | Replace that acceptance criterion |
| **HIGH** | A1 ≥90 top-1 is poorly aligned with the actual Track-A question | Rescope the gate, not the result |
| **HIGH** | Formal recording/scoring instrument is not sealed enough | Harden before consuming another PHASE-TEST |
| **MEDIUM** | Current DTW is too simple to justify “unreachable by tuning” | One bounded classical-baseline pass |
| **MEDIUM** | A1→A2→A5 currently blocks R5 | Split the lattice-format dependency and unblock Track R |
| **DOC** | Historical/research narrative still contradicts the new plan | Clean it before publishing |

### The A4 defect is more important than A1

This is the strongest code finding in my audit.

Your frozen contract explicitly says a dormant hypothesis is retained but **excluded from tilings** until later evidence revives it.

But `best_tilings()` selects candidates based on `fabric.is_active(t.fact)`, not `is_live(t.fact)`. Dormant facts are active by definition. Therefore **a dormant C-arm candidate can currently win without a revival at all**.

That would manufacture exactly the effect A6 is supposed to measure.

Likewise, `DecisionParams.live_budget` exists, but search shows it is only declared/defaulted; `enforce_budgets()` enforces dormant and combined budgets by evicting dormant hypotheses. There is no actual live-budget enforcement.

The present test doesn't catch the first defect because it explicitly revives `reef` before calling `commit_region()`.

This is fortunately a **pre-experiment bug**, not a ruined-result bug. A6 hasn't run yet.

Add two contract tests: a dormant token must never appear in `best_tilings()` before revival, and total/live budgets must actually constrain C to the same resource budget used for B.

## A1 itself needs a small scientific redesign

There is a more fundamental process problem: `run1` still does not satisfy your own D1 regime.

D1 says owner recordings are **16 kHz mono**, that nonconforming sessions are not silently admitted, and specifically identifies the old 48 kHz `run1` as NOT-VERIFIED/excluded.

D0 also records that `run1` has **no checksum file** and is pending use-verification.

So `0.8583` is a valuable exploratory DEV result, but under the rules you wrote yourself it isn't yet a formal A1 result.

I would not throw away the audio. Either explicitly grandfather it as legacy DEV with hashes and provenance, or make a deterministic derived 16-kHz set and document that transformation. But do that consciously rather than having A1 quietly bypass D1.

There is also a feature-basis reason to standardize this. `frontend.rs` builds the Mel bank to the input file's Nyquist when no high cutoff is specified. A 48-kHz file and a 44.1-kHz file therefore literally get different filterbank frequency extents.

The new recorder compensates by asking PulseAudio for 48 kHz, which is reasonable for matching legacy `run1`, but now the recorder contradicts the binding 16-kHz protocol.

Pick one canonical feature rate. I favor **16 kHz everywhere in the formal pipeline**.

## I would change the gh1 gate

Requiring **8/8 configurations to score ≥90%** is simply the wrong acceptance criterion for endpoint trimming.

Several of those eight configurations are deliberate ablations. `no_lifter`, `no_deltas`, `no_cmn`, etc. exist partly to become worse so you can learn what matters. The writeup itself notes `no_lifter` was only 0.8333 even before trimming.

gh1 should answer:

**“Does endpointing correctly remove irrelevant padding without truncating speech or creating degeneracy?”**

That gate can close on component correctness, padding invariance, sensitivity/stability tests and an endpoint audit. The **accuracy number belongs to A1**, not gh1.

That is not lowering a bar after failure; it is repairing a category error in the sub-bead's acceptance criterion.

## I also would not call 85.8% the DTW ceiling yet

Your DTW is a legitimate simple baseline, but it uses three unrestricted horizontal/vertical/diagonal steps inside a global band and divides accumulated cost by `n + m`.

Classical DTW work treated normalization, local-path constraints and slope constraints as meaningful recognition choices; Sakoe and Chiba specifically reported improved word-category discrimination from slope constraints and compared symmetric versus asymmetric normalization formulations. [IEEE Xplore](https://ieeexplore.ieee.org/abstract/document/1163055/authors?utm_source=chatgpt.com)

So I would allow **one bounded classical-baseline bead**, entirely on DEV, before freezing A1:

- proper symmetric/path-normalized DTW step pattern;
- explicit slope/local path constraint;
- feature variance normalization/CMVN rather than only CMN;
- report top-1 **and top-K word inclusion**;
- no open-end DTW for this isolated-word experiment.

Do not spend weeks squeezing 120 clips. The goal isn't 98%. The goal is to establish whether the acoustic candidate generator is sufficiently rich for the lifecycle experiment.

And that brings us to the most important gate change.

## A1 should not be the thing deciding whether Track A lives or dies

The actual research experiment in your master plan is excellent:

B and C receive the **same lattice, evidence, scoring and budget**; B permanently prunes weak hypotheses while C can retain/dormant/revive them. That cleanly isolates hypothesis lifecycle.

For that experiment, **top-1 accuracy is not the load-bearing quantity**.

What matters is whether the correct answer survives somewhere in the N-best candidate set often enough for later evidence to rescue it. Your own A5 oracle-WER gate already understands this: if the lattice's oracle is not materially better than B, there is nothing for the Fabric to rescue.

Therefore I would reinterpret the gates as:

**A1:** characterize acoustic discrimination honestly. Cross-session top-1 is reported, not massaged.

**A2/A5:** determine whether the acoustic front end creates a useful N-best lattice. Oracle/top-K is the true proceed/fail criterion for A6.

**A6:** test the research hypothesis.

I would remove the hard dependency “A1 must be ≥90 or A2 cannot begin.” Preserve the original ≥90 number in the historical record; don't pretend it was achieved. Just state that review found it was a product-quality threshold masquerading as an architectural-falsification threshold.

## The padding-fingerprint explanation needs one more experiment

The local inference is plausible but not yet proven.

“Hangover 2 and hangover 5 produce the same accuracy” demonstrates that **those two endpoint expansions don't matter**. It does not establish that the entire eight-point delta was caused specifically by word-dependent utterance timing.

You can prove that cheaply on DEV.

Randomly re-pad every trimmed speech span with label-independent random leading/trailing silence and rerun the **untrimmed** pipeline. Separately build a stupid timing-only classifier from leading silence, trailing silence and speech centroid.

If timing-only predicts labels above chance and randomized re-padding destroys the old 94.2% advantage, you have direct evidence for the fingerprint. No new owner recording is necessary.

## Answers to the eight questions in the session writeup

For **CMN**, the `no_cmn` result is supporting evidence, not airtight. Run the 2×2 trimmed/untrimmed × CMN/no-CMN comparison on DEV. Cheap, then close it.

For **delta edges**, clamping is defensible and probably not your eight-point problem. `no_deltas` changed only roughly one classification. One reflection/one-sided-edge ablation is enough; don't make it a research project.

For the **±2 support rule**, it is an engineering heuristic, which is fine. What makes it defensible is a sensitivity plateau, not an argument that ±2 is theoretically privileged. Test radius 1/2/3 and a few threshold values and choose a stable region rather than the numerical optimum.

For **raw-energy versus post-preemphasis thresholding**, keep endpointing config-independent in the raw/log-energy domain. If low-energy fricatives are actually being clipped, add a spectral criterion rather than allowing each feature configuration to create a different speech span.

For **fingerprint proof**, use randomized re-padding plus the timing-only classifier described above.

For **baseline choice**, keep endpoint-constrained DTW as A1/A2's classical baseline. Don't jump to open-end DTW merely because it might restore the attractive number. Track R's HMM/trie recognizer is already the long-term recognition architecture.

For **symmetric 50-ms hangover**, leave it alone unless a manual endpoint audit demonstrates loss of frication/prevoicing. There is no current evidence justifying asymmetry.

For the **retired 190133 session**, never use it for tuning, evidence or a claim. I have no philosophical objection to a prominently labeled non-evidentiary machinery smoke run, but you already have synthetic probes, so strict retirement is cleaner.

## There is also a historical framing correction worth making

Hearsay-III was **not a failed speech experiment whose blackboard you are repairing**. Its own 1981 paper says Hearsay-III was a domain-independent expert-system framework and explicitly says it was not a speech-understanding system. [IJCAI](https://ijcai.org/proceedings/1981/79?utm_source=chatgpt.com)

Hearsay-II is the speech system. And its retrospective does not describe the blackboard as a failed or incoherent idea; Lesser and Erman describe successfully applying the architecture to speech, coming close to the ARPA performance goals, and identify the global blackboard as one of the things that made knowledge-source integration convenient. [IJCAI](https://www.ijcai.org/Proceedings/77-2/Papers/055.pdf?utm_source=chatgpt.com)

There were real control/scheduling problems—focus-of-attention work explicitly studied them—but the historical story is subtler than “blackboard too slow, experiment failed.” [IJCAI](https://ijcai.org/proceedings/1977/7?utm_source=chatgpt.com)

That actually makes **your research question better**:

> Can a provenance-backed, indexed saturation substrate support a dormant/revivable hypothesis lifecycle more effectively than conventional permanent pruning under an equal hypothesis budget?

That is falsifiable and does not depend on claiming that Hearsay-II failed.

One more distinction: if C beats B in A6, you have demonstrated that **the lifecycle policy helps**. You have not yet demonstrated that Saturation Fabric is uniquely or computationally superior as a substrate. I would eventually add a second systems experiment comparing the same lifecycle implemented over a straightforward reference hypothesis store versus Saturation Fabric—RTF, retraction cost, memory, update propagation and provenance overhead.

## The project docs also need one cleanup pass

The new master plan correctly says “non-neural, fully inspectable classical models” and explicitly bans the old “model-free/no training corpus” description because you now have HMM training, Kneser-Ney and MAP adaptation.

But the README still opens by calling Hearsay-IV “model-free,” and the older research synthesis makes the same claim.

The synthesis also still attributes a formal confidence formula to Hearsay-II even though your later primary-source audit correctly discovered that Hearsay-II's combination was ad hoc and the formal calibrated-Bayes treatment belonged to HWIM.

I would mark that synthesis historical/superseded or rewrite it. Right now a cold reviewer can read two incompatible definitions of the project.

## What I would have the local agent do next

1. **Do not record PHASE-TEST #2 yet.** Fix A4's dormant-tiling and budget bugs first, because they are genuine research-integrity defects.
2. **Repair gh1's acceptance criteria and close it as an instrument-correction bead**, preserving the 0.8583 result exactly as observed.
3. **Bring A1 data under D0/D1 discipline:** hash the owner WAVs, settle the 48k legacy status, choose canonical 16k processing, and make the scorer reject malformed/incomplete sessions.
4. **Harden `record_phase_test.sh`/`a1_gate`:** complete 30/30 vocabulary required, randomized precommitted prompt order, no skips/partial scoring, sample-format/rate validation, session metadata + SHA256, frozen front-end/config hash, and nonzero exit for a failed formal gate. Right now a partial session can explicitly “still score,” which should not be true for evidence.
5. **Run only DEV experiments:** direct padding-fingerprint test and one bounded classical-DTW improvement pass; add top-K reporting.
6. **Rewrite A1/A2/A5 gating:** A1 reports cross-session acoustic discrimination; A5 oracle/top-K determines whether there is enough recoverable hypothesis diversity to run A6. Split “lattice file format” out of A5 if necessary so the HMM/trie R5 work is not needlessly blocked by A1—the current graph has `A1 → A2 → A5 → R5a`.
7. **Then record one proper, randomized, sealed PHASE-TEST session and score it once.** Whatever number comes out is the answer.

So the inflection point is real, but I would characterize it as **“the experiment design has matured enough that some early gates no longer match the real research question,” not “the acoustic approach has failed.”** The most important immediate correction is actually A4, not A1. After those changes, the central Hearsay-IV A/B/C experiment remains worth doing.
