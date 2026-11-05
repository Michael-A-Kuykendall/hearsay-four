# NUMBER-LEDGER — Hearsay-IV headline numbers (lane F, wave 1)

One row per headline number. Each row carries: command, data, size,
plausibility check, CI status. Fields with no recorded evidence say TBD.

Read 2026-11-01 from the main checkout.
`data/` is gitignored; paths below are main-checkout locations. This ledger
lives in `docs/results/` so the numbers are citable from the repo.

## Index

| # | Headline | Value | Status |
|---|---|---|---|
| 1 | Fallback 9-cell best (lm6/wip0) WER | 0.7233 | landed |
| 2 | full93 4-mix dev PER | 0.6707 | landed |
| 3 | full93 8-mix dev PER | 0.6416 | landed |
| 4 | CMVN 8-mix iters 1-4 dev PER | 0.6416 / 0.6262 / 0.6179 / 0.6137 | landed |
| 5 | 30-word cross-channel top-1 | 1.0000 (29/29) | landed |
| 6 | sub20 tune best (lm8/wip0) WER | 0.8332 | landed |
| 7 | Headline WER (final AM, full dev) | TBD | pending |
| 8 | LIVE RTF (live mic) | TBD | pending |

## 1. Fallback 9-cell best WER 0.7233

- Source: `data/wer71g/tune200s-fallback/summary.tsv` (13 lines: header +
  9 cells + 3 comment lines), best line
  `# best: lm6/wip0 WER 0.7233 (LM grid edge -> extend below lm6 in full tuning)`.
- Command: `data/wer71g/tune200s-fallback/sweep9.sh <am.json>` drives 9 cells
  (`lmw` in {6,8,10} x `wip` in {-5,0,5}) via `cell9.sh`, 4 parallel workers:
  `target/release/examples/wer_devclean --am <AM> --lm
  data/lm-text/r1v2-full/lm-r1v2-interpolated.json --tier1
  better-voice/.../Master_English_Phonetics_FINAL_COMPLETE.txt --tier2
  data/lexicon/librispeech-lexicon.txt --wav-root data/librispeech/wav
  --manifest data/wer71g/tune200s/strat200.manifest.tsv --out cell.tsv
  --lm-weight L --word-penalty W --no-cmvn`, scored by
  `data/wer71g/tune200/score_sdi.py`.
- Data: AM = `ckpt-8mix-iter4.json` (non-CMVN 8-mix final), RAW features
  (`--no-cmvn`); queries = strat200 (stratified 200-utt subset, 40x5);
  LM = r1v2-full interpolated.
- Size: 200 utts, ref = 4579 words per cell. Best cell lm6/wip0:
  S=2623 D=446 I=243 hyp=4376.
- Plausibility check: WER 0.7233 < 1.0 gate. Arithmetic rechecks:
  (2623+446+243)/4579 = 3312/4579 = 0.7233. Neighbors are worse
  (lm6/wip-5 0.7362, lm6/wip5 0.7436, lm8/wip0 0.7449), but the best sits
  on the LM grid edge (lm6), so the file's own note applies: extend the
  grid below lm6 in full tuning. Cross-check note in file:
  `harness==mine errs all cells (mismatches=0)` — independent WER
  re-implementation agreed on all 9 cells.
- CI status: TBD — no confidence interval computed (no `bootstrap` string
  in any `docs/results` file as of this read).

## 2. full93 4-mix dev PER 0.6707

- Source: `data/models/r3v2-full93/stage_per.log` line 5:
  `2026-11-01T18:04:43Z stage=4mix ckpt=ckpt-4mix-iter3.json dev_per=0.6707`.
- Command: `data/models/r3v2-full93/stage_eval.sh` (watcher; log-only, no
  gating): waits for each checkpoint named in `schedule.json`, then
  `target/release/examples/eval_si <ckpt>
  data/librispeech/wav/dev-clean.manifest.tsv data/librispeech/wav --raw
  --cache data/cache/feat`, greps `dev_per=`.
- Data: train = train-clean-100 subset via `train_si` (`train.log`:
  select rows=28539, oov_dropped=1812, kept_utts=26727, kept_h=93.887,
  speakers=251; iters=[0,0,3,4]; 4-mix iters 1-3 log-likelihood
  -885908530.4 -> -883908707.9 -> -882694577.5, monotonic). Dev = full
  dev-clean manifest.
- Size: train 26727 utts / 93.887 h / 251 speakers. Dev denominator
  (ref phones): TBD — the watcher log keeps only `dev_per`.
- Plausibility check: PER 0.6707 < 1.0 gate; consistent with improving
  training likelihood across the 3 iters.
- CI status: TBD — none computed.

## 3. full93 8-mix dev PER 0.6416

- Source: `data/models/r3v2-full93/stage_per.log` line 6:
  `2026-11-01T20:41:34Z stage=8mix ckpt=ckpt-8mix-iter4.json dev_per=0.6416`.
- Command: same `stage_eval.sh` watcher as row 2 (second spec
  `8mix:ckpt-8mix-iter4.json`).
- Data: same train as row 2, split to 8 mixes; 8-mix iters 1-4
  log-likelihood -881792958.5 -> -879903146.6 -> -874331847.2 ->
  -865038115.0, monotonic (`train.log`, ends `train_si DONE`).
  Final `model.json` = `ckpt-8mix-iter4.json` (both 748244587 bytes).
- Size: train 26727 utts / 93.887 h / 251 speakers. Dev denominator: TBD
  (same watcher limitation as row 2).
- Plausibility check: 0.6416 < 1.0 gate and < 4-mix 0.6707 — mixture
  doubling improved dev PER, consistent with the likelihood gains.
- CI status: TBD — none computed.

## 4. CMVN 8-mix per-iter dev PER (complete)

- Source: `data/models/r3v2-full93-cmvn/cmvn_per.log` (6 lines, read
  2026-11-05 from the main checkout):
  `2026-11-01T22:18:03Z stage=cmvn8-iter1 ckpt=ckpt-cmvn8-iter1.json dev_per=0.6416`
  `2026-11-01T23:15:45Z stage=cmvn8-iter2 ckpt=ckpt-cmvn8-iter2.json dev_per=0.6262`
  `2026-11-01T23:18:25Z stage=cmvn8-iter2 ckpt=ckpt-cmvn8-iter2.json dev_per=0.6262`
  (re-logged, same value)
  `2026-11-02T15:57:08Z stage=cmvn8-iter3 ckpt=ckpt-8mix-iter1.json dev_per=0.6179`
  `2026-11-02T16:48:44Z stage=cmvn8-iter4 ckpt=ckpt-8mix-iter2.json dev_per=0.6137`
  `2026-11-02T16:48:44Z verdict=CMVN_WINS cmvn=0.6137 noncmvn=0.6416
  mean_diff=-0.009119 ci95=[-0.014685,-0.003553] n=2557 unpaired=0`.
- Per-iter checkpoints (sha256 measured 2026-11-05 on the main disk):
  iter1 0.6416 `ckpt-cmvn8-iter1.json` (752142056 bytes)
  sha256 `2fbcc78316232d34e6d959d3235475bfde49aa99e41b8623ca8e1e194e93f71b`;
  iter2 0.6262 `ckpt-cmvn8-iter2.json` (741773818 bytes)
  sha256 `b38f2841e09a285e258596833de3285255a870a52836fb1c3dd2b4266df1bbda`;
  iter3 0.6179 `ckpt-8mix-iter1.json` (743925545 bytes)
  sha256 `37332b213e2f0738ebb9e1abedfbcfcdd998126f33ed2d5dd03c875af7c3d622`;
  iter4 0.6137 `ckpt-8mix-iter2.json` (747656515 bytes)
  sha256 `6620a3dd9b8520bdbb756007e606ab9c734b593b7d1c95589d50d4d7ff416bab`
  (matches the sha256 already recorded in `A6-FABRIC-EXPERIMENT.md`).
- MODEL-NAME-MISMATCH record: `data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json`
  (sha256 `6620a3dd...f416bab`, full value above) holds CMVN stage iter4
  (dev PER 0.6137), not CMVN iter2 — the `8mix-iter2` name comes from the
  `train_si_continue` lineage (`cont-iter34.log`: `target_mix=8 iters=2`,
  `mix=8 iter=2/2 ... ckpt-8mix-iter2.json`), which wrote iters 3-4 of the
  CMVN track under continue-training names. Decision: NO rename. A rename
  is owner-disk-only (`data/` is gitignored) yet 12 repo references hard-code
  the current name (2 demo scripts' `HEARSAY_AM` defaults, 1 test helper
  `am_relpath`, frozen-evidence rows in A6/D0), and the current name is the
  true continue-training provenance — renaming would destroy that lineage
  and silently break external/historical references. Cite by sha256.
- Command: TBD (eval command not yet recorded for the CMVN track; presumed
  same `eval_si` dev-clean probe — verify before citing).
- Data: `cmvn_retrain threads=24 iters=4
  frontend=hmm-fe2.cmvn-pop-floor1e-6`, reestimate frames=33752128
  (`train.log`: iters 1-2/4, ll=61593703.8 -> 89320918.5); iters 3-4/4 via
  `train_si_continue` (`cont-iter34.log`: split to 8 mixes,
  ll=100104016.1 -> 105638331.3, `CONTINUE exit: 0`). Checkpoints
  `ckpt-cmvn8-iter0/1/2.json` + `ckpt-8mix-iter1/2.json` all present.
- Size: train 33752128 frames. Dev denominator: TBD.
- Plausibility check: iters 1-4 read 0.6416 -> 0.6262 -> 0.6179 -> 0.6137,
  monotonic improvement, all < 1.0 gate; final 0.6137 < non-CMVN 8-mix
  0.6416, consistent with the log's `CMVN_WINS` verdict.
- CI status: TBD — none computed (the verdict line carries its own
  `ci95=[-0.014685,-0.003553]` from the watcher; no paired-bootstrap CI was
  recorded for this row — do not cite the watcher's interval as the row's CI).

## 5. 30-word cross-channel top-1 1.0000 (29/29)

- Source: `docs/results/A1-phasetest-20261031-100240/score.log`
  (`top1=1.0000`, `top3/top5/top10=1.0000`, `n=29`, `degenerate=0`,
  diagonal confusion matrix) and `docs/results/A1-ACOUSTIC-GATE.md`
  section `PHASE-TEST phase-test-20261031-100240`.
- Command: exact scoring argv TBD — recorded provenance is the frozen
  config `base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn+dtw_baseline+band1.0`
  (`frozen_config` in score.log), scorer commit `fd7b63b`,
  `templates=sessions/canon/run1`, `phase_test_dir=sessions/canon/phase-test-20261031-100240`,
  seal ACCEPT (seed 14206645340250353102, order `041a964f`).
- Data: cross-channel (WSL-chain queries vs Windows-chain run1 templates),
  29/30 takes; take001 (`reflection`) rejected by canonical QC
  (speech span 115 ms < 150 ms floor). Labels: characterization only,
  NOT a pass/fail gate. Session consumed, now REGRESSION in
  `sessions/CATALOG.tsv`, never re-scored.
- Size: n=29 scored takes (30 attempted, 1 QC-rejected).
- Plausibility check: 29/29 with zero confusions on a 30-word closed
  vocabulary with per-word templates is the expected ceiling outcome,
  not evidence about open-vocabulary accuracy; test population (isolated
  words, same-session templates) matches only the characterization claim.
- CI status: no paired-bootstrap CI computed (TBD). For scale only, the
  exact 95% Clopper-Pearson interval for 29/29 is [0.881, 1.000]
  (lower = 0.025^(1/29) = e^(-3.6889/29) = 0.881) — not a substitute for
  the paired-bootstrap gate on WER rows.

## 6. sub20 tune best WER 0.8332 (lm8/wip0)

- Source: `data/wer71g/tune200/results_final.tsv` line 8
  (`8 0 0.8332 2284 647 101 3639 3093 2.357` under header
  `lmw wip wer S D I ref hyp rtf`). Full grid rechecked: 28 rows
  (`lmw` in {6,8,10,12,14,16,18} x `wip` in {-10,-5,0,5}), minimum is
  this cell.
- Command: `data/wer71g/tune200/sweep.sh` (24 cells; lm6 rows added after —
  file holds 28) via `cell.sh`: `wer_devclean --am
  data/models/r3v2-sub20/model.json --lm
  data/lm-text/r1v2-full/lm-r1v2-interpolated.json --tier1 ... --tier2 ...
  --wav-root data/librispeech/wav --manifest
  data/librispeech/wav/dev-clean.manifest.tsv --out cell.tsv --limit 200
  --lm-weight L --word-penalty W`, scored by `score_sdi.py`.
- Data: AM = `data/models/r3v2-sub20/model.json` (1->2->4 mix schedule,
  final `ckpt-4mix-iter6.json`, 85191217 bytes; training-subset row counts
  TBD — no `train.log` in that dir, `subset.tsv` present at 1450621 bytes).
  Queries = first 200 rows of the dev-clean manifest (`--limit 200`).
- Size: 200 utts, ref = 3639 words. Best cell: S=2284 D=647 I=101 hyp=3093.
- Plausibility check: 0.8332 < 1.0 gate. Arithmetic rechecks:
  (2284+647+101)/3639 = 3032/3639 = 0.8332. Interior optimum: all four
  grid neighbors are worse (lm6/wip0 0.8403, lm10/wip0 0.8535,
  lm8/wip-5 0.8508, lm8/wip5 0.8384).
- CI status: TBD — none computed.

## 7. Headline WER (final AM, full dev) — TBD

- Pending: full dev-clean decode with the final (CMVN) acoustic model at
  tuned LM weight / word penalty. Rows 1 and 6 are 200-utt tune subsets,
  not the headline. Fill command / data / size / plausibility / CI when
  the run lands.

## 8. LIVE RTF (live mic) — TBD

- Pending: real-time factor on live-microphone input. Observed offline
  values are not citable as live RTF: fallback best cell RTF 1.008
  (file notes measurement under contention with the CMVN retrain,
  19 threads — live-mic will differ); sub20 best cell rtf 2.357
  (offline batch decode). Fill when a live-mic measurement exists.

## Cross-row notes

- full93 resumed from the sub20 4-mix (`train.log` line 5
  `train_si resume .../ckpt-seed-4mix.json`; seed file is 85191217 bytes,
  same size as `r3v2-sub20/model.json` — sizes observed, byte-identity
  not verified).
- Discarded attempts on disk (not ledgered as results):
  `train.log.attempt2-badroot`, `train.log.attempt3-fresh`,
  `subset.tsv.pre-crash` in `data/models/r3v2-full93/`.
- `stage_per.log` lines 1-4 are watcher start/relaunch markers, not scores.
