# Hearsay-IV — submission writeup (lane L, from lane F draft)

Entry for the Meta Global AI Developer Hackathon 2026 (event Oct 26 –
Nov 4, 2026; submission due Nov 4, 9:00 PM ET; internal target Nov 1).
Every number below is either quoted with its on-disk source or marked
TBD with the exact source that will fill it. TBDs resolve as Lane A
(bead `hearsay-four-8znj--main-model-pick-tune-gate`) and the 16-mix
overnight (bead `hearsay-four-r3pg--sixteen-mix-overnight`) land.

Supersedes the wave-1 draft (`DRAFT.md`, ledger rows 1–6 only) with:
the CMVN verdict (landed Nov 2), the full CMVN tune table (landed
Nov 2), and the in-progress headline gate status.

## 1. Problem

Dictation systems produce errors with no reason attached: a wrong word
appears and neither the user nor the developer can ask why. Neural
transcription stacks concentrate that failure — the output is a function
of millions of learned weights, so an error has no inspectable cause and
no targeted fix. For a user dictating text they cannot re-read
(accessibility) or material that must be quoted exactly (court records,
medical notes), an unexplained error is not a minor defect, it is the
product failing at its one job.

## 2. Approach

Hearsay-IV is Hearsay-III (1983) re-implemented with the blackboard
swapped for the patent-pending saturation fabric: a fully mechanistic,
model-free, offline speech transcription device. No neural nets, no
embeddings, no learned weights anywhere — enforced in-repo by
`scripts/no_model_guard.sh` (bead `hearsay-four-ivs`). Accent tracking
plus classical techniques throughout (HMM-GMM acoustic models,
Kneser-Ney n-gram language models, DTW baselines, pronunciation
lexicons). Every decoded word carries provenance — acoustic score,
losing alternatives, LM probability, lexicon source — so an error
points at the component that caused it. Thesis and event log live in
`docs/hackathon/OPENSTATE-WHITE-PAPER.md` and
`docs/hackathon/META-HACKATHON.md`.

The demo (`bash web/demo.sh`, bead
`hearsay-four-wgit--why-panel-demo-script`) transcribes 3 fixed
LibriSpeech utterances and renders a click-a-word why panel
(`web/transcribe.html`) showing all five provenance elements per word.

## 3. Built during the event vs pre-existing

Pre-existing (before Oct 26): initial commit Aug 15, 2026 (`6808cc4`,
verified in git history); 11 crates; 204 tests passing; the
saturation-fabric core including the D-1 survival fix and the
subsumption-minimality work; ~78 beads closed. The fabric core is
Aug 2026 work, not hackathon work.

Built during the event (Oct 26 – Nov 2): the top of the stack. Oct 26:
3 new crates (`lexicon`, `sequence`, `output`), 2 new bins
(`corpus-fetch`, `hearsay-demo`), the no-model guard, the
lexer→ngram→output chain, the stage3↔accent feedback loop, the
conclusion demo, and all orchestration machinery (39 new tests,
204 → 243). Oct 28–31: 6 more crates for the acoustic-model track
(`hmm`, `lm`, `lattice`, `eval`, `decoder`, `a2`; crate-add commits
`7c67e66`, `09e84aa`, `07b2321`, `ffc2cda`, `36cd4eb`, `449558a`),
the R1 language-model bins, R3 HMM training (1-2-4-8 mixture
schedule), the dev-clean WER harness, LM tuning grids, and the
phase-test scoring — all on main (`50a3e09` train_si stages;
`a2c5821` decoder + WER harness; `3c8a4ba` LM bins). Nov 1–2 wave 1:
8 lane beads (A, B×2, C, D×2, E, F) covering the CMVN verdict run,
triphone tying + 1h pipeline, decoder guard + streaming, file
transcription + why panel, adaptation plumbing + runbook, and the
number ledger — 6 of 8 merged to main Nov 2 (merges `729f7d9`,
`1c8e74c`, `cd37544`, `2fba434`, `c940b45`; Lane A gate run in
progress). Workspace now 20 members (was 11 at event start).

## 4. Results

Landed values first; TBDs carry their filling source. Full per-row
provenance (command, data, size, plausibility, CI) for rows 1–6 is in
`docs/results/NUMBER-LEDGER.md`; rows 9–10 below are new in this
writeup (lane F will ledger them on merge).

- CMVN verdict (NEW, landed Nov 2 16:48 UTC): CMVN_WINS — CMVN 8-mix
  iter4 dev PER 0.6137 vs non-CMVN 8-mix 0.6416, paired-bootstrap
  mean_diff −0.009119, 95% CI [−0.014685, −0.003553], n=2557,
  unpaired=0 (source: `data/models/r3v2-full93-cmvn/cmvn_per.log`
  verdict line; bead `hearsay-four-qi9u--cmvn-retrain-run-verdict`).
  Per-iter CMVN dev PER: iter1 0.6416, iter2 0.6262, iter3 0.6179,
  iter4 0.6137 — monotone improvement, every iter staged to the
  per-log. This is the first paired-bootstrap CI in the repo.
- Full tune on the CMVN winner (NEW, landed Nov 2): 14 cells
  (lm4–lm8 × wip, plus beam-2x check), stratified 200-utt dev-clean
  subset (4579 ref words). Best lm6/wip0 WER 0.6624
  (S=2413 D=338 I=282 hyp=4523; arithmetic rechecks
  (2413+338+282)/4579 = 0.6624). Beam doubling (150→300) gives the
  identical 0.6624 — beam-adequate. Independent WER re-implementation
  agreed on the best cell (harness==mine, mismatches=0). Observed
  cell RTFs 0.62–0.74 (source:
  `data/wer71g/tunefull/summary.tsv`, `mode=cmvn` in
  `lm6_wip0/result.txt`; bead 8znj step 3). Interior-adjacent
  optimum: lm5/wip0 0.6759 and lm7/wip0 0.6676 both worse.
- full93 acoustic model (26727 utts / 93.887 h / 251 speakers,
  train-clean-100 subset), dev-clean PER: 4-mix 0.6707, 8-mix 0.6416
  (ledger rows 2–3). Mixture doubling improved PER; training
  likelihood improved every iter.
- Fallback insurance table (non-CMVN 8-mix, landed before the
  verdict): best lm6/wip0 WER 0.7233 on the same 4579-word subset
  (ledger row 1). Superseded by the CMVN tune above; kept as the
  insurance record.
- sub20 tune record: best lm8/wip0 WER 0.8332 (ledger row 6).
- 30-word cross-channel characterization (isolated words, WSL-chain
  queries vs Windows-chain templates): top-1 1.0000, 29/29 scored
  (1 take QC-rejected), zero confusions (ledger row 5).
  Characterization only, not a pass/fail gate; says nothing about
  open-vocabulary accuracy.
- Headline WER on full dev-clean (2703 utts) with the CMVN winner at
  ACCURATE (lm6/wip0): TBD. Source that will fill it:
  `data/wer71g/tunefull/fullgate/cell.tsv` scored by
  `data/wer71g/tune200/score_sdi.py` when the Lane A gate run
  completes (bead 8znj step 4; run in progress, ~1300/2703 utts at
  last read, running WER ≈ 0.68, RTF ≈ 0.62 — running values, not
  the headline). Gate bar: WER < 1.0, insertions not dominant.
- 16-mix swap verdict: TBD. Source that will fill it: the verdict
  line + 3 stage PERs appended to the main-model per-log by the
  overnight bead `hearsay-four-r3pg--sixteen-mix-overnight`
  (scheduled night of Nov 2, after the Lane A gate + triphone probe
  re-measure; swaps only on paired-bootstrap improvement, else
  "no swap, 8-mix stands").
- Triphone 1h probe: TBD pending re-measure. The wave-1 probe
  numbers are retired — the eval had leaks, fixed on main
  (`d8c1dc4`: disjoint PER split, same-path decode, --per-manifest).
  Source that will fill it: bead
  `hearsay-four-3n2g--triphone-probe-remeasure`.
- LIVE setting (best WER with quiet-box RTF ≤ 0.8): TBD, Lane A
  pick on gate completion (bead 8znj step 3). Observed: every
  tunefull cell RTF ≤ 0.74, so ACCURATE is the LIVE candidate
  pending quiet-box confirmation.
- Live-mic RTF: TBD (ledger row 8); offline tune RTFs are not live
  measurements.
- Current test total: TBD (Oct 26 baseline 243 green; wave-1 lanes
  added tests). Source: `cargo test --workspace` on main after the
  Lane A merge.
- No paired-bootstrap CIs on WER rows yet — CI status TBD on every
  WER row (the CMVN verdict CI above is on PER).

## 5. Muse Code multi-lane build story

Two scored laps plus the current wave, all one pattern: planned in
beads first, executed concurrently, gates verify, green work merges
to main.

Oct 26 lap (from `META-HACKATHON.md` work log): ~4.4 h wall,
11 subagents, 15 task beads + 5 epics closed, suite 204 → 243
green, $0 of $150 Meta API credits spent (all local cargo + Muse
subscription; portal holds the real balance).

Nov 1–2 wave 1: 8 lane beads created Nov 1 — A1
`hearsay-four-8znj` (main-model pick/tune/gate, in progress), B1a
`hearsay-four-ivpu` + B1b `hearsay-four-zs89` (triphone, merged
`c940b45`), C1 `hearsay-four-0htm` (frontend guard, merged
`cd37544`), D1a `hearsay-four-xels` + D1b `hearsay-four-wgit`
(demo + why panel, merged `2fba434`), E1 `hearsay-four-9ud8`
(adaptation/runbook, merged `1c8e74c`), F1 `hearsay-four-7zuh`
(ledger/writeup, merged `729f7d9`) — plus the CMVN retrain run
`hearsay-four-qi9u` (verdict landed, bead in progress). 6 of 8
wave-1 beads closed Nov 2; all merges same-day.

Nov 2 wave 2 (in flight): headline gate completion (8znj), 16-mix
overnight (`r3pg`), triphone re-measure (`3n2g`), error analysis +
charts (`0u3z`, lane K), this submission package (`xehu`, lane L).

Agent-caught defects and cross-checks (on-disk evidence only):

- Bad training attempts discarded, not patched over:
  `train.log.attempt2-badroot`, `train.log.attempt3-fresh`, and
  `subset.tsv.pre-crash` sit next to the landed `train.log` ending
  in `train_si DONE` — the full93 run restarted fresh after a bad
  root and a crash.
- Independent WER scorer cross-check: both tune summaries record
  `harness==mine` with `mismatches=0` — a second WER implementation
  agreed with the harness on every cell.
- QC gates firing as designed: phase-test take001 rejected by the
  canonical 150 ms speech-span floor (115 ms observed), leaving
  29/29 scored takes.
- Watchers surviving restarts: `stage_per.log` shows three
  STAGE-EVAL relaunches before the 4-mix and 8-mix PERs landed.
- Leaky evals get fixed, not quoted: the triphone probe leaks were
  repaired on main (`d8c1dc4`) and the probe re-queued (`3n2g`)
  instead of landing the flattering numbers.
- Beam-adequacy proven, not assumed: the best tune cell re-ran at
  2x beam with identical WER.

## 6. LibriSpeech attribution

Acoustic-model training and all WER/PER measurements use LibriSpeech
(open SLR, SLR12 via openslr.org): `train-clean-100` (training
subset source; used subset 26727 utts / 93.887 h / 251 speakers),
`dev-clean` (tuning, PER, and headline measurements; 2703 utts),
and the normalized LM text (SLR11). LibriSpeech is licensed
CC BY 4.0, recorded per-split in
`docs/results/D0-DATA-INVENTORY.md` with archive md5s verified
against the published values (train 2a93770f…, dev 42e2234b…,
test 32fa31d2…, LM-text gzip-tested — SLR11 publishes no md5sum).
Full paper citation — TBD, owner to confirm whether the submission
format needs it (see `OWNER-QUESTIONS.md`).
