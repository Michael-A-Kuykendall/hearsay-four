# Submission form fields — drafts (lane L)

One section per field. Bracketed items are owner fills (see
`OWNER-QUESTIONS.md`). Numbers match `WRITEUP.md` §4 exactly.

## Title

Hearsay-IV: Transcription You Can Argue With

## Tagline

A fully mechanistic, model-free, offline speech transcriber — every
word carries its provenance, so every error has an address.

## Problem fit (Build for Your World track)

Dictation errors are silent and sourceless: a wrong word appears and
neither the user nor the developer can ask why. For people who
dictate text they cannot re-read, and for records that must be quoted
exactly — court proceedings, medical notes — that is the product
failing at its one job. Hearsay-IV is built for that world: a
transcription device where every word ships with its reason (acoustic
score, losing alternatives, language-model probability, lexicon
source), so an error points at the component that caused it and the
fix has an aim. It runs fully offline with zero learned weights, so
it works where the cloud doesn't reach and keeps speech on-device.
[Confirm track name "Build for Your World" against the submission
portal — owner.]

## Description

Hearsay-IV re-implements Hearsay-III (1983) with the blackboard
swapped for our patent-pending saturation fabric: a fully
mechanistic, model-free, offline speech transcription device. No
neural nets, no embeddings, no learned weights — enforced by a guard
in the repo (`scripts/no_model_guard.sh`) that fails the build if
any appear. Classical speech science throughout: HMM-GMM acoustic
models, Kneser-Ney n-gram language model, pronunciation lexicons,
DTW baselines.

What we built during the event (Oct 26 – Nov 2): the top of the
stack on a pre-existing fabric core — 9 new crates (20 workspace
members total), the lexer→LM→output chain, the dev-clean WER
harness, the full93 acoustic model (26,727 utterances / 93.9 h /
251 speakers), a one-command demo (`bash web/demo.sh`) with a
click-a-word why panel for every decoded word.

Measured results (LibriSpeech, CC BY 4.0; frozen paper numbers from
`docs/writeup/WRITEUP.md`, each with its on-disk source): decoder 64.3%
WER on TEST-200 (200 utts, 4530 words;
`docs/results/A6-FABRIC-EXPERIMENT.md` Run 1, arm A 0.6430) and 67.6% on
full dev-clean (2703 utts, 54402 words, 95% CI 66.9–68.3%;
`docs/results/analysis/HEADLINE-ERROR-ANALYSIS.md`); Fabric 85.5% at its
best budget-500 point (`docs/results/A6-FABRIC-EXPERIMENT.md` Run 1);
dormancy-vs-pruning delta (C−B) largest -0.02 pts, 95% CI [-0.07, 0.00]
(`docs/results/A6-FABRIC-EXPERIMENT.md` th25,
`scripts/a6/out/headline_th25.tsv`); identical transcripts in 1599/1600
paired runs with rev_winner 0
(`docs/results/A6-FABRIC-EXPERIMENT.md` headline freeze) — no demonstrated
difference between the lifecycles at equal budget, so the Fabric's measured
value here is provenance plus retractable corrections, not accuracy;
Fabric scaling exponent CX1 1.07 (`src/fabric/tests/decision_cx1.rs`,
commit `de334c5`) at 0.08-0.26x real time
(`scripts/a6/out/sweep_insurance.tsv`); 30-word cross-channel check 29/29
(`docs/results/NUMBER-LEDGER.md` §5); one correction fixed 7/16
single-word errors with byte-identical undo
(`docs/demo/correction-demo.md`).

## Meta tooling usage

Built with Muse Code as a multi-lane agent build. All work was
planned as small verifiable tasks (beads) and executed concurrently
by parallel agents: Oct 26 lap closed 15 task beads + 5 epics in
~4.4 h wall time (test suite 204 → 243 green); the Nov 1–2 wave
ran 8 lanes at once — acoustic-model tuning, triphone support,
decoder hardening, file-transcription demo + why panel, adaptation
plumbing, and the number ledger this entry quotes — with 6 of 8
beads merged to main same-day. Verification gates ran on every
lane: the agents discarded bad training runs instead of patching
them, fixed a leaky evaluation instead of quoting its flattering
numbers, and double-checked every word-error count with an
independent second scorer. Meta API credits spent: $0 of $150 as
of Oct 26 (all local compute + Muse subscription; [owner to
confirm current balance]). Humans owned the microphone, the model
verdicts, and the submission; the machine owned the parallelism.

## Links

- Repository: [TBD — public repo URL at submission time; repo goes
  public per plan, patent internals excluded per
  `docs/hackathon/META-HACKATHON.md`]
- Demo video (3 min): [TBD — hosted video URL; script in
  `docs/video/SCRIPT.md`]
- Live demo: [TBD — owner decides: none / WASM page / hosted clip.
  Note: `2sh` WASM demo exists in-repo; live-mic RTF is unmeasured]
- Writeup: [repo path `docs/writeup/WRITEUP.md` at the submission
  tag — fill tag on freeze]
- Event page: https://cerebralvalley.ai/e/meta-ai-developer-hackathon
- Data: LibriSpeech SLR12 via openslr.org (CC BY 4.0)
