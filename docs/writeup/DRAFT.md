# Hearsay-IV — writeup draft (lane F, wave 1)

Draft for the Meta AI Developer Hackathon entry (event Oct 26 – Nov 4, 2026;
submission due Nov 4, 9:00 PM ET; internal target Nov 1). Every number below
comes from [the number ledger](../results/NUMBER-LEDGER.md); anything not in
the ledger is marked TBD. Wave 2 (video script + timeline graphic) is out of
scope for this bead.

## 1. Problem

Dictation systems produce errors with no reason attached: a wrong word
appears and neither the user nor the developer can ask why. Neural
transcription stacks concentrate that failure — the output is a function of
millions of learned weights, so an error has no inspectable cause and no
targeted fix. For a user dictating text they cannot re-read (accessibility)
or material that must be quoted exactly (court records, medical notes), an
unexplained error is not a minor defect, it is the product failing at its
one job.

## 2. Approach

Hearsay-IV is Hearsay-III (1983) re-implemented with the blackboard swapped
for the patent-pending saturation fabric: a fully mechanistic, model-free,
offline speech transcription device. No neural nets, no embeddings, no
learned weights anywhere — enforced in-repo by `scripts/no_model_guard.sh`.
Accent tracking plus classical techniques throughout (HMM acoustic models,
n-gram language models, DTW baselines, pronunciation lexicons). Every
hypothesis carries provenance: which acoustic evidence, which lexical entry,
which LM score produced it, so an error points at the component that caused
it. Thesis and event log live in `docs/hackathon/OPENSTATE-WHITE-PAPER.md`
and `docs/hackathon/META-HACKATHON.md`.

## 3. Built during the event vs pre-existing

Pre-existing (before Oct 26): initial commit Aug 15, 2026 (`6808cc4`,
verified in git history); 11 crates (fabric, fse, arena, stage3, accent,
calibration, capture, corpus, groebner, invariant_ppt, runner); 204 tests
passing; the saturation-fabric core including the D-1 survival fix and the
subsumption-minimality work; ~78 beads closed. The fabric core is Aug 2026
work, not hackathon work.

Built during the event (Oct 26 – Nov 1): the top of the stack — 3 new
crates (`lexicon`, `sequence`, `output`), 2 new bins (`corpus-fetch`,
`hearsay-demo`), the no-model guard, the lexer→ngram→output chain, the
stage3↔accent feedback loop, the conclusion demo, and all orchestration
machinery (Oct 26: 39 new tests, 204 → 243). The acoustic-model track in
this draft's results — R1 language-model bins, R3 HMM training
(1-2-4-8 mixture schedule), the dev-clean WER harness, LM tuning grids,
and the phase-test scoring — is event work on main (`50a3e09` train_si
stages; `a2c5821` decoder + WER harness; `3c8a4ba` LM bins).

## 4. Results (ledgered numbers only)

Source for each value: `docs/results/NUMBER-LEDGER.md`. No paired-bootstrap
CIs exist yet — CI status is TBD on every row.

- Fallback LM tune, 9 cells on the non-CMVN 8-mix final, stratified
  200-utt dev-clean subset (4579 ref words): best lm6/wip0 WER 0.7233
  (ledger row 1). Arithmetic rechecks: (2623+446+243)/4579 = 0.7233.
  Best sits on the LM grid edge — extend below lm6 in full tuning.
- full93 acoustic model (26727 utts / 93.887 h / 251 speakers,
  train-clean-100 subset), dev-clean PER: 4-mix 0.6707, 8-mix 0.6416
  (ledger rows 2-3). Mixture doubling improved PER; training likelihood
  improved every iter.
- CMVN retrain (iters=4): iter1 dev PER 0.6416 landed; iters 2-4 TBD
  (ledger row 4).
- 30-word cross-channel characterization (isolated words, WSL-chain
  queries vs Windows-chain templates): top-1 1.0000, 29/29 scored
  (1 take QC-rejected), zero confusions (ledger row 5). Characterization
  only, not a pass/fail gate; says nothing about open-vocabulary accuracy.
  For scale: exact 95% interval for 29/29 is [0.881, 1.000] (shown
  calculation, not the paired-bootstrap gate).
- sub20 tune record, 28 cells on the 4-mix sub20 model, first 200 dev-clean
  rows (3639 ref words): best lm8/wip0 WER 0.8332 (ledger row 6).
  Arithmetic rechecks: (2284+647+101)/3639 = 0.8332. Interior optimum —
  all four grid neighbors are worse.
- Pending headline WER on full dev-clean with the final AM: TBD (ledger
  row 7). Live-mic RTF: TBD (ledger row 8); offline tune RTFs (1.008
  under retrain contention, 2.357 batch) are not live measurements.

## 5. Muse Code multi-lane build story

Wave 1 ran as parallel implementation lanes, one bead per lane, planned in
beads first and executed concurrently: A1 hearsay-four-8znj, B1a
hearsay-four-ivpu, B1b hearsay-four-zs89, C1 hearsay-four-0htm, D1a
hearsay-four-xels, D1b hearsay-four-wgit, E1 hearsay-four-9ud8, F1
hearsay-four-7zuh (this writeup). The repo work log records the pattern —
lanes implement, gates verify, green work merges to main.

Agent-caught defects and cross-checks (on-disk evidence only; this lane has
no bead-database access, coordinator owns bead state):

- Bad training attempts discarded, not patched over: `train.log.attempt2-badroot`,
  `train.log.attempt3-fresh`, and `subset.tsv.pre-crash` sit next to the
  landed `train.log` ending in `train_si DONE` — the full93 run restarted
  fresh after a bad root and a crash.
- Independent WER scorer cross-check: the fallback summary records
  `harness==mine errs all cells (mismatches=0)` — a second WER
  implementation agreed with the harness on all 9 cells.
- QC gates firing as designed: phase-test take001 rejected by the canonical
  150 ms speech-span floor (115 ms observed), leaving 29/29 scored takes.
- Watchers surviving restarts: `stage_per.log` shows three STAGE-EVAL
  relaunches before the 4-mix and 8-mix PERs landed.

## 6. LibriSpeech attribution

Acoustic-model training and all WER/PER measurements use LibriSpeech
(open SLR, SLR12 via openslr.org): `train-clean-100` (training subset
source), `dev-clean` (tuning and PER measurements), and the normalized LM
text. LibriSpeech is licensed CC BY 4.0 (recorded per-split in
`docs/results/D0-DATA-INVENTORY.md` with archive md5s verified against the
published values). Full paper citation per the corpus docs — TBD in wave 2
if the submission format needs it.
