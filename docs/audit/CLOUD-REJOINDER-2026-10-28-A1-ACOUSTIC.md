# Cloud rejoinder: A1/A4 rescope tightening (received 2026-10-28)

> Provenance (local addition, not part of the rejoinder): recorded
> verbatim from the cloud reviewer's follow-up. Follows
> `docs/audit/CLOUD-AUDIT-2026-10-28-A1-ACOUSTIC.md` and the local
> opinion delivered in chat on that audit. Audited commit: `8d1c559`
> (main). Local opinion on this rejoinder was delivered in chat,
> not in this file.

---

The rejoinder is strong, and I agree with almost all of it. More importantly, it has converged on the right interpretation: **the measurements are not being repudiated; the experimental contract around them is being repaired.**

There are only a few places where I would tighten the local response before turning this into work.

### 1. Derived 16 kHz is the right choice, but treat it as a derived view of the same DEV session

I agree with choosing deterministic 48→16 kHz conversion rather than grandfathering a permanent 48 kHz exception.

But I would be explicit about one scientific point: **resampling does not create a new dataset or a new DEV observation.** `run1-16k` inherits the same session identity, partition, speaker conditions, labels and leakage status as `run1`. It is simply the canonical feature-domain representation of that session.

So the provenance chain should be something like:

`original WAV SHA256 → fixed resampler/version/parameters → derived WAV SHA256`

Then all subsequent formal A1 numbers use only the 16 kHz derivative. Do **not** compare 48 kHz and 16 kHz, pick whichever performs better, and call that tuning—that would turn the resampling decision itself into another DEV knob.

I would fix the resampler and parameters a priori, derive once, and freeze them.

### 2. The A4 budget defect needs a little more design work than “read live_budget”

This is the one part I would not let the agent implement mechanically yet.

The dormant-tiling bug is straightforward:

> candidate tilings must contain only live hypotheses.

Changing the relevant filter from `fabric.is_active()` to the appropriate liveness eligibility is consistent with the frozen contract.

But `live_budget` is subtler. The contract currently has:

- live budget,
- dormant budget,
- optional combined budget,
- B counts live,
- C counts live + dormant against the same total experimental budget.

If the live budget is exceeded, **what exactly dies?**

Evicting a dormant token is conceptually clean: it has already fallen below threshold. Evicting a live token introduces another pruning mechanism and potentially becomes part of the B/C treatment difference.

So before implementing this, I would force a tiny A4 spec decision:

**Either**

`live_budget` is truly normative, define deterministic eviction/pruning semantics for excess live hypotheses identically across B and C;

**or**

remove `live_budget` as an independently enforced parameter and define the experimental budget solely through the arm-specific total budget rule.

Do not just add an `if live.len() > live_budget` branch and invent semantics in code. A6 is specifically supposed to measure pruning/lifecycle behavior, so budget behavior has to be mathematically frozen first.

### 3. I would make the new A1 gate wording very precise

The rejoinder correctly says the 90% threshold was measuring something narrower than the research question.

I would avoid replacing it with another vague gate like “acoustics are good enough.”

Make A1 produce **three distinct outputs**:

- cross-session top-1 accuracy;
- top-K inclusion/recall, probably K = 3/5/10;
- confusion structure.

Then A1 becomes a characterization/falsification experiment rather than a pass/fail product benchmark.

The actual Track-A proceed criterion belongs downstream:

> **Does the candidate lattice contain enough correct hypotheses that later evidence could theoretically rescue them?**

That is fundamentally an oracle/top-K question, which A5 already approximates much better than top-1.

This change preserves the 90% historical result as **FAILED/NOT ACHIEVED**, rather than rewriting history. It simply stops treating that threshold as the dependency that decides whether the Fabric experiment is scientifically meaningful.

### 4. The proposed gh1 repair is correct

I agree with the local agent here without reservation.

The new gh1 acceptance criteria it proposes are much better:

- span/blip/hangover component tests;
- padding invariance;
- support-radius sensitivity;
- manual endpoint inspection;
- no run1 degeneracy.

I would add only one thing: make the manual endpoint audit **small and predeclared**, e.g. 10–15 clips sampled by a fixed seed, and record whether initial/final speech was visibly/audibly truncated.

Otherwise “manual audit” can become an informal way to keep looking until satisfied.

And yes: **accuracy leaves gh1 entirely.**

### 5. The timing-fingerprint experiment is worth doing exactly because it can falsify our story

The rejoinder properly corrected this.

I would actually make it slightly stronger than my earlier wording:

Take the trimmed spans and generate several deterministic re-padding conditions:

- centered;
- random lead/trail offsets;
- fixed short padding;
- fixed long padding.

Then compare those to original-window accuracy.

Separately fit the deliberately stupid timing classifier using only quantities such as:

`leading_silence_ms, trailing_silence_ms, speech_duration_ms, centroid_ms`

If that classifier predicts word identity materially above chance and DTW accuracy changes when those timing relationships are randomized, the fingerprint hypothesis becomes directly supported.

If neither happens, then we were wrong, and the 94→86 point drop needs another explanation.

That's exactly the kind of experiment this project should prefer.

### 6. Instrument hardening should happen before the next human recording

Strong agreement.

I would define the PHASE-TEST recorder/scorer as an actual **measurement instrument**, not merely a script.

At minimum it should reject the session unless:

- exactly 30 expected labels exist;
- each label occurs exactly once;
- no unknown labels exist;
- order/seed was committed before capture;
- PCM16 mono 16 kHz everywhere;
- duration is within an allowed range;
- files and manifest have hashes;
- recording metadata exists;
- scorer commit/config identity is recorded;
- the frozen A1 configuration is identified before reading the PHASE-TEST;
- the entire session is scored atomically—no partial test.

One subtle correction: **randomized order is good, but the seed/order must be frozen before recording, not merely logged afterward.** Otherwise it isn't genuinely precommitted.

### 7. I agree A4 can proceed while the acoustic rescope is being written

This is a good observation by the local agent.

There are really two parallel activities:

**Architecture/document work:** rewrite the work graph/gates holistically so nobody starts executing contradictory beads.

**Implementation work:** A4 repair is independent and already has a clear defect.

So I would not force the codebase to sit idle while every A1/A2/A5 bead is rewritten. The A4 bug is known, bounded and pre-A6. Fixing it does not contaminate the acoustic experiment.

The constraint should be: **no new downstream experimental bead starts under the old graph.**

### 8. I agree the “Fabric substrate superiority” experiment is later work

Definitely.

The current A6 question is already substantial:

> Does retain/dormant/revive beat permanent discard when everything else is held equal?

If C wins, that's a legitimate architectural result.

Only afterward should you ask:

> Is Saturation Fabric a better *implementation substrate* for that policy than an ordinary purpose-built hypothesis graph/store?

That second experiment would compare things like update complexity, retraction propagation, memory, RTF, provenance cost and implementation complexity.

Trying to combine those questions now would make A6 uninterpretable.

### One thing I would add that neither response has emphasized enough

**The project is now past the point where “hackathon path” should be allowed to bend experimental design.**

The repo already contains real HMM training, MAP adaptation, a frozen decision contract, statistical evaluation machinery and a carefully partitioned research plan. This isn't a weekend demo anymore.

If November 1/4 is still relevant, submit whatever bounded artifact is defensible. But don't let that date force A1/A5/A6 into questionable gates.

The project has become more valuable as an actual experiment than as a rushed transcription demo.

### My current decision tree

I would now authorize the local agent to structure the rescope essentially as it proposes:

**A4 repair first**, with the budget semantics explicitly resolved before implementation → repair gh1's category-error gate → canonicalize `run1` into provenance-preserving 16 kHz DEV → prove/disprove the padding fingerprint → run the bounded DTW baseline investigation → rewrite A1/A2/A5 dependencies around acoustic characterization and lattice oracle capability → harden the measurement instrument → **only then consume another owner PHASE-TEST session**.

The biggest change from where we were one session ago is conceptual:

> **85.83% is now a measured fact, not a verdict.**

That is the correct place for it in the research.
