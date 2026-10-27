# Hearsay-Four — External Audit Package

Written 2026-10-27 for an external auditor reviewing this repository cold.
Everything below is stated as claim + evidence. Where a number could not be
reproduced from committed artifacts, it is labeled as recorded-only.

## 1. The bounded idea

Re-run the Hearsay line of speech-understanding experiments (HEARSAY-II,
Erman et al., *Computing Surveys* 1980; Hearsay-III, IJCAI-81) with the
blackboard replaced by a saturation fabric (patent-pending by the owner,
treated here only as a claim about a faster support/retraction substrate),
carrying forward the classical, mechanistic components, in order to build a
non-AI, model-free, offline transcription service, and to produce a working
single-speaker transcription application by the Meta Hackathon deadline
(Nov 1 target, Nov 4 hard). There is no "1983 experiment"; that date appears
in session talk and is not supported by the literature.

## 2. The word-finding question, answered directly

**Do we know how to isolate words? The classical technique is known and
standard. The current code does not implement it. That is the central
finding of this audit.**

- What is established: segment audio into sub-word acoustic units
  (syllable-like units from energy dips and spectral onsets), score units
  against phone templates, then decode a *word sequence* by searching the
  pronunciation dictionary jointly with n-gram sequence scores. Every
  pre-neural recognizer, and Hearsay's lexical knowledge source, worked
  this way. Word boundaries are an output of that search, never an input.
  Fluent-speech word boundaries mostly have no acoustic mark
  (coarticulation), so any plan that requires detecting them acoustically
  is unbuildable, and no part of this architecture requires it.
- What the code does instead: `process_chunk`
  (src/runner/src/lib.rs:436) collapses each fixed 2-second chunk to a
  top-1 phone sequence, aligns the *whole* phone sequence to *one*
  dictionary word (`scored[0]`, one word fact per chunk), then applies
  bigram/trigram scoring and a verification hook to the resulting
  one-word-per-chunk sequence. Lengthening the chunk cannot help: a
  perfect 5-second phrase chunk still emits exactly one word. Words
  straddling a chunk cut are unrecoverable because no search ever sees
  across the cut.
- Consequence: maximum output density is 0.5 words/sec against ~2.5 in
  real speech. The measured WER = 1.0 on all 9 Oyez spans (Section 5) is
  fully explained by this single simplification, before any question of
  acoustic quality.

The missing component — multi-word lexical hypothesization inside a
segment — is not research. It is a known mechanism that was simplified
away during bead implementation, and no bead recorded that decision.

## 3. Architecture map: original experiment vs this code

| Hearsay component | Status | Evidence |
|---|---|---|
| Blackboard with leveled hypotheses (frame/phone/word/sequence) | Partial | src/runner/src/lib.rs process_chunk stages; fabric facts with premises |
| Support/retraction substrate (the fabric claim) | Present | src/fabric/src/lib.rs: multi-support justifications, deliberate-retraction markers, commit(); D-1 fix tested |
| N-best front end (never argmax early) | Present | src/fse/src/nbest.rs emit_nbest; but runner collapses to top-1 phone per frame before lexicon |
| Lexical hypothesization (word sequences over units) | **Missing** | One winning word per chunk (Section 2) |
| Sequence scoring (syntax stand-in) | Partial | src/sequence bigram/trigram + verify hook; scores the one-word-per-chunk stream |
| Verification that flags, does not drop | Present | C2 verify hook in process_chunk; flagged records keep provenance |
| Focus/opportunistic scheduling | Missing | Fixed pipeline order in runner |
| Island driving (anchor confident regions, expand) | Missing | No code |
| Score combination | Ad hoc | clamp01 and min-of-premises ratings, not calibrated log-odds |
| Speaker adaptation / accent | Partial, standalone | src/accent, src/calibration enrollment baseline; not integrated into served path |
| Segmentation into syllable-like units | Present, standalone | src/fse/src/boundaries.rs (SEG-1); not integrated (SEG-2 open) |
| Per-word provenance to frame evidence | Present | WordRecord::validate; demo transcript chains terminate at frames |

Era-specific items correctly *not* carried forward: the 1,000-word closed
semantic grammar as a design ceiling, PDP-10 batch single-utterance
processing, the heuristic priority-scheduler formula, and the 1983 date.

## 4. Contradiction and error log (session record)

1. **Word vs syllable conflation (agent error, repeated).** Session text
   claimed both "acoustic word segmentation is established" and "word
   boundaries have no acoustic mark," in adjacent turns, without
   distinguishing grain. The SEG-1 bead (hearsay-four-uth) and SEG-2 bead
   (hearsay-four-jf6: "SEG-1 detects word boundaries") embed the
   conflation. Resolution: Section 2 of this document.
2. **False data premise.** SEG-1 was filed assuming Oyez session.tsv
   carried word timings. Measured 2026-10-27: all 1,589 utterance
   boundaries across the 3 bulk cases tile at gap 0.0s; transcript.json
   blocks are likewise contiguous. No reference pauses exist in the
   committed data. SEG-1's gate was weakened to a boundary-rate
   plausibility band and closed. A rate without a denominator is not
   recall or precision.
3. **Phantom lane greens.** PKG-1 and SEG-1 lane branches were reported
   green with zero unique commits. Caught via `git merge-base
   --is-ancestor` plus empty diff stat; PKG-1 re-proven by running its
   gate on main; SEG-1 implemented fresh. Process now: no bead closes on
   a lane's report.
4. **OpenState state vs repo state divergence.** Against the accumulated
   store, capsule showed 74 issues vs 144 actual and 0 blockers vs 4.
   Causes separated: OpenState does not sync beads/git (tooling limit,
   owner-developed, fixable); single-use preview/apply violations were
   agent misuse. Corrected 2026-10-27 by procedure-B replay (reset,
   assess 4 OpenCode sessions = 3,464 records, capsule, checkpoint).
5. **CHUNK-1 epic (hearsay-four-urd) tests the wrong property.** Its
   pre-registered pass bar (≥70% boundary recall at 0ms gaps) gates
   SEG-2 on acoustic word-boundary detection, which Section 2 shows is
   neither achievable nor required. The epic's own pivot text already
   describes the correct architecture; its pass bar contradicts it.
6. **"1983 experiment" framing.** Unsupported; see Section 1.
7. **Dictation artifacts in the record.** OpenState replay froze
   `resume_cursor.current_task` as "beads heard as beats" — speech-to-text
   corruption of the work queue itself, flagged by the owner in-session.

## 5. Metrics: claim, measurement, caveat

| Claim | Measured | Caveat |
|---|---|---|
| WER = 1.0, 9 Oyez spans | Supported arithmetically (N=711; hyp 15 words/30s vs ref 61–99) | Structurally floored by one-word-per-chunk (Section 2); span-sliced, not full-file |
| SEG-1 detector works | Synthetic: exact 2 boundaries ±50ms; silence 0; tone 0; sample.wav 10/8s; Oyez 82/60s (1.37/s) | Real-audio figures are output rates; no precision/recall denominator exists in the data (error 2) |
| PERF-1 ~50x speedup (1075s→20s) | Fix real in code (premise index + worklist + quiet log in src/fabric) | Headline ratio has no retained artifact; scaling test is single-N wall-clock |
| RTF mean 0.047 | Math exact | One clip, one run, recorded in prose only |
| Accent fail-fast PASS (multiple runs) | Exit 0, margins recorded in hackathon log | Operates on mean-NLL proxy; multi-speaker Oyez runs violate the one-speaker design; default driver exits 0 even on FAIL verdict (defect) |
| Test suites green | This session verified: runner 48, fse 27 (incl. 5 new), workspace 367/0 per independent skeptic re-run | Older counts (204, 243) are bead-recorded, not all re-verified |

## 6. State a cold auditor can verify from git alone

- HEAD 2d3fa8a on main, clean tree, pushed to private origin.
- Beads: 144 total, 133 closed, 9 open, 0 in progress (export:
  BEADS-EXPORT.md in this directory; live DB is .beads/ Dolt, export is
  the git-visible copy).
- OpenState: 4 OpenCode sessions replayed (3,464 records); state file
  .openstate/state.jsonl (git-visible via symlink target in repo).
- Guard: scripts/no_model_guard.sh green at HEAD (no ML crates, weights,
  model APIs, STT-in-scoring, or network clients in the service path).

## 7. Questions for the auditor (falsifiable)

1. Is the fabric's multi-support survival semantics (src/fabric/src/lib.rs)
   a correct and sufficient substrate for blackboard-style hypothesis
   competition, or does the single-winner-per-chunk collapse make the
   substrate irrelevant in the current pipeline?
2. Is restoring multi-word lexical hypothesization (dictionary search over
   the phone sequence, n-gram rescoring, flag-not-drop verification) a
   bounded (≤8pt) implementation, or does it hide further missing
   components?
3. Given WER = 1.0 today, is any Nov 1–4 transcription-app claim
   supportable, and if so in what reduced form (closed-vocab phrase demo?
   measurement apparatus + fabric evidence?)?
4. Which of the missing components in Section 3 (focus scheduling, island
   driving, calibrated score combination) are load-bearing for a
   single-speaker demo, and which are deferrable without misrepresenting
   the experiment?
5. Are the bead gates in BEADS-EXPORT.md binary and reproducible, or do
   any still encode assumptions an implementer would have to guess?

## 8. Recommended corrected path (for auditor comment, not yet filed)

1. One bead: restore multi-word hypothesization inside a segment
   (Section 2). Gate: word-sequence output on a fixture with known
   multi-word reference; density no longer capped at one word per chunk.
2. Segmentation becomes a secondary variable: SEG-1 phrase groups vs
   whole-span processing, measured against absolute WER on the same
   reference spans. No comparison against the 2s chunk as a success bar.
3. CHUNK-1 epic (hearsay-four-urd): rewrite or close as superseded; its
   pivot text is the correct design, its pass bar is not.
4. Fix the fail-fast driver exit code before quoting any verdict from it.
5. File or explicitly defer: focus scheduling, island driving, calibrated
   score combination. Silence in the queue is how components were lost.
