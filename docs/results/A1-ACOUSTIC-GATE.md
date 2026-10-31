# A1 — Acoustic gate (bead hearsay-four-b7p)

Question: do classical MFCC features discriminate the owner's 30 words at all?

## Gate status: INVALID — experiment void (not a failure of the acoustic hypothesis)

PHASE-TEST run of 2026-10-28 on main (`a1_gate --run1 sessions/run1
--phase-test-dir sessions/phase-test-20261028-190133`, pid 1360822, log
/tmp/a1_phasetest_190133.log) is VOID: the test was administered
incorrectly, so its output (top-1 = 0.0333, 1/30, gate90=false) is not a
measurement of MFCC discriminability and not a failure of the acoustic
hypothesis — the acoustic question remains unanswered. Every query collapsed
to the first template's label through a proven degenerate mechanism (below):
wrong recording protocol for the test session plus a gate instrument that
reports a score when it cannot compare. DEV in the same run passed and stands:
best config `base_cmn_d_dd_lifter22_pre097` top-1 = 0.8583 (103/120)

> RELABEL 2026-10-31 (REFOCUS §3): exploratory DEV — likely still includes leading-click padding. Re-measure on canonical run1 (A1-r, hearsay-four-mc1--a1-rerun-canonical). 190133 stays VOID (unchanged).
with endpoint trimming (untrimmed pipeline measured 0.9417; the gap is
padding fingerprint, see the DEV section).

## Data existence finding (measured 2026-10-28, lane bead/b7p-a1)

- `sessions/run1/manifest.tsv`: 121 lines = header + `enroll.wav` + 120 clips
  (`utt000.wav`..`utt119.wav`), 30 distinct words x 4 occurrences each,
  48000 Hz. Take index = per-word occurrence order (0..3).
- `sessions/run1/session.tsv`: 120 rows, same 30 words x 4.
- WAV files present under `sessions/run1/`: **0**. The directory contains
  only `manifest.tsv` and `session.tsv`. `find sessions -name '*.wav'`
  returns 0; the only WAVs in the repo are `data/demo/sample.wav` and
  `data/demo/serve-mp3-twin.wav` (fixtures, not owner takes).
- `data/partitions/*.json` (D1, bead hearsay-four-7ms): `dev.json` (64
  entries), `phase-test.json` (9), `train.json` (3216), `lockbox.json` (6)
  contain no owner/run1 audio. D1 already classified `sessions/run1` as
  NOT-VERIFIED / excluded (audio absent) — this lane re-measured the same
  result independently.
- No fresh 5th-take owner session exists anywhere in the lane.

Therefore DEV leave-one-take-out and PHASE-TEST cannot be scored on real
owner data. No synthetic or TTS stand-in was substituted for owner data,
and no 5th take was fabricated.

Update (main, 2026-10-28): owner audio landed after the lane finding.
`sessions/run1/` now holds 121 WAVs (enroll + 120 clips, 30 words x 4 takes,
48 kHz, fixed ~3.0 s windows) and `sessions/phase-test-20261028-190133/`
holds 31 WAVs (enroll + 30 5th takes, 44.1 kHz, variable 3.8-5.9 s windows).
The gate below ran on this audio.

## What was built and verified (runnable machinery)

- Tunable front end in `src/fse/src/frontend.rs` (`FrontendConfig`):
  pre-emphasis, liftering, CMN, deltas / delta-deltas, mel band
  (low/high Hz), plus `FrontendConfig::tuning_grid()` covering every knob
  the bead names (CMN on/off, deltas off / deltas-only / deltas+delta-deltas,
  pre-emphasis 0.97/0.0, lifter 22/0, bands 300-3400 and 100-4000).
  Output dim follows the config: 13 / 26 / 39.
- DTW in `src/fse/src/dtw.rs`: Euclidean frame distance, Sakoe-Chiba band
  (`band_frac` of max length), length-normalized cost `total/(n+m)`,
  infinite cost on empty input.
- Protocols in `src/fse/src/gate.rs`: `leave_one_take_out` (DEV; each take
  held out in turn, templates = remaining takes) and `phase_test`
  (templates = original takes, queries = fresh session, nearest-template
  classification), both returning predictions, accuracy and a confusion
  matrix (`confusion_csv()`).
- Runner binary `a1_gate` (`src/fse/src/bin/a1_gate.rs`, `cargo run -p fse
  --bin a1_gate -- --run1 sessions/run1 [--phase-test-dir DIR] [--band 0.2]`):
  audits data first, runs the DEV tuning grid and picks the best config,
  then scores PHASE-TEST once only when an explicit fresh-session dir is
  supplied. With absent audio it prints `A1 GATE: NOT RUN`.

## Measured machinery probe (not a gate result)

Synthetic chirp clips in `/tmp/a1synth` (3 labels x 4 takes, generated
only to exercise the WAV -> front end -> DTW -> LOTO path) scored
DEV top-1 = 1.0000 (n=12) on every tuning-grid config. This verifies the
machinery end to end; it says nothing about the owner's words.

Durable regression tests: `src/fse/src/tests/a1_tests.rs` (5 tests:
DTW identity/warp/empty, front-end dims and error paths, CMN zero-mean
statics, synthetic LOTO + phase-test discrimination).

## Measured gate result (main, 2026-10-28)

Command: `cargo run -p fse --bin a1_gate -- --run1 sessions/run1
--phase-test-dir sessions/phase-test-20261028-190133` (exit 0, ~50 min
debug build).

DEV leave-one-take-out on `sessions/run1` (n=120 each):

| config | top-1 |
|---|---|
| base_cmn_d_dd_lifter22_pre097 | 0.8583 |
| no_cmn | 0.8500 |
| no_deltas | 0.8500 |
| deltas_only | 0.8583 |
| no_preemphasis | 0.8500 |
| no_lifter | 0.8250 |
| band_300_3400 | 0.8500 |
| band_100_4000_no_cmn | 0.8500 |

Best: `base_cmn_d_dd_lifter22_pre097`, 103/120, endpoint-trimmed pipeline
(raw-domain energy trim, floor 0.001, hangover 5). Its 17 confusions:
byzantium->resented, congealed->hotbed, congealed->kinship, genetic->kinship,
genetic->ripple, highboy->hotbed, hotbed->ripple, impeachment->hotbed,
kinship->congealed, kinship->enchantment, knockdown->survival,
process->congealed, rhubarb->ripple, ripple->hotbed, salivating->hotbed,
staked->congealed, survival->knockdown.

Padding-fingerprint finding (2026-10-28): the untrimmed pipeline measured
best 0.9417 (113/120) on this same audio. Trimming to speech spans drops
LOTO to 0.8583; hangover 2 vs 5 frames gives identical grids to 4 decimals,
so spans are stable and the drop is not edge-cutting. The ~8-point gap is
within-window timing/position habit the untrimmed DTW exploited, not word
acoustics. 0.8583 is the honest within-session number; cross-session fresh
takes measure at or below it.

Re-padding proof experiment (2026-10-28, bead zco, run1-16k, base config,
untrimmed pipeline, band 0.2, one release run, 0 degenerate in all cells):

| Condition | Result |
|---|---|
| original-window-untrimmed-16k | 113/120 = 0.9417 |
| repad-centered (symmetric, same length) | 114/120 = 0.9500 |
| repad-random (seed 7, same length) | 103/120 = 0.8583 |
| repad-short (+/-50 ms) | 105/120 = 0.8750 |
| repad-long (+/-1000 ms cap) | 114/120 = 0.9500 |
| timing-only nearest-mean LOTO (4 timing feats) | 8/120 = 0.0667 |

Verdict: REFUTED (as operationalized). The pinned rule required timing-only
>= 10% absolute AND best re-padded trailing original-window by >= 4 points.
Timing-only reached 6.67% (2x chance, below the 3x bar), and the best
re-padded conditions (centered/long, 0.9500) EXCEED the original window
(0.9417) rather than trailing it. The pattern is informative anyway: random
re-padding lands EXACTLY on the trimmed level (103/120), while deterministic
symmetric re-padding preserves/exceeds the original. The padding advantage is
therefore not primarily word-dependent timing leakage; it looks like DTW
alignment geometry (centered speech aligns best, random positions add noise
down to the no-padding level). The ~8-point trim drop still needs an
explanation - candidates: CMN edge effects (2x2 trimmed/untrimmed x CMN/no-CMN
on DEV), delta-edge handling. The word "fingerprint" above is kept as the
historical label for the padding advantage, not as a proven mechanism.

Bounded classical baseline (2026-10-29, bead t5i, run1-16k, band 0.2, base
config only, each variant run ONCE, release, 0 degenerate in all runs):

| Run | top1 | top3 | top5 | top10 | vs baseline |
|---|---|---|---|---|---|
| baseline (grid row 0) | 0.8500 (102) | 0.8583 | 0.8667 | 0.8667 | - |
| V1 sym-weighted DTW | 0.8583 (103) | 0.8667 | 0.8667 | 0.8667 | +1 WIN |
| V2 slope-constrained DTW | 0.8417 (101) | 0.8583 | 0.8667 | 0.8667 | -1 LOSE |
| V3 CMVN replaces CMN | 0.8667 (104) | 0.8667 | 0.8667 | 0.8667 | +2 WIN |
| V4 V1+V2 | 0.8500 (102) | 0.8583 | 0.8667 | 0.8667 | 0 NEUTRAL |
| V5 V1+V2+V3 | 0.8667 (104) | 0.8667 | 0.8667 | 0.8667 | +2 WIN |

Winner by best top-1: V3 and V5 tie at 0.8667; pinned tie-break (fewest
moving parts) selects V3. Full 8-row baseline grid reproduced the recorded
16k top-1 values exactly (7x 0.8500, no_lifter 0.8333): zero drift through
the DTW-unification + N-best refactor. Top-K signal is WEAK: top-10 exceeds
top-1 by only 2 clips on every row - the candidate generator is not rich;
A5 oracle proceeds on this measured basis, not on hoped-for recall.
Confusion matrices were not captured for the variant runs (output was
grep-filtered at run time); per-variant correct counts above are the
record. Re-running for capture would violate once-each-literal, so the
frozen-config confusion is deferred to sealed scoring (bead b7p).

Frozen-A1-Config: base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn+dtw_baseline+band0.2

PHASE-TEST (templates = run1 takes, queries = fresh 5th takes, n=30):
output top-1 = 0.0333, gate90=false. INVALID — void run, not scored evidence.

### Root cause (proven, not acoustic)

All 30 queries were predicted `congealed`; the single correct trial is the
true-`congealed` query. Mechanism, each link verified in source:

1. `classify` (`src/fse/src/gate.rs`) is strict-`<` argmin over templates in
   manifest order. If every distance is +inf, the winner is always the FIRST
   template.
2. The first template in `sessions/run1/manifest.tsv` order is
   `utt000.wav congealed`. Hence all-+inf collapses to `congealed`, exactly
   as observed.
3. `dtw_distance` (`src/fse/src/dtw.rs`) returns +inf when no alignment path
   fits the Sakoe-Chiba band. With band 0.2 of max length, endpoint (n-1,m-1)
   requires |n-m| <= 0.2*max, i.e. frame-count ratio >= 0.8.
4. Frame counts follow clip duration (window/shift are rate-derived,
   `window_len`/`shift_len` in `src/fse/src/lib.rs`). run1 clips are fixed
   ~3.0 s (143520-144000 samples @48 kHz); phase-test clips are variable
   3.8-5.9 s (168462-261072 samples @44.1 kHz). Best-case ratio
   3.0/3.82 = 0.785 < 0.8, so EVERY query-template pair is +inf.

So 0.0333 measures the recording-protocol mismatch (fixed 3.0 s run1 windows
vs variable-window native 5th-take recording), amplified by the gate's silent
degenerate fallback — not MFCC discriminability: the experiment was administered incorrectly
(wrong test-session protocol, defective instrument), so there is no verdict
on the acoustic question either way. Two defects:

- D1: `classify`/runner never reports degenerate all-+inf; it prints a
  chance-level "score". The gate must fail loudly instead.
- D2: the 190133 session used a different recording protocol than run1, so it
  is invalid as a test session independent of D1. Per the bead fail path it is
  never re-scored; the rerun needs a NEW session recorded with run1's fixed
  ~3.0 s window protocol.

### Rerun procedure (revised path after void run)

1. Fix D1 on DEV + synthetic duration-mismatch probes in /tmp (never the
   190133 audio): all-+inf must print NOT COMPARABLE (nonzero exit), DEV grid
   must stay >= 0.90, `fse` tests green.
2. Owner records a NEW 5th-take session with run1's fixed-window protocol.
3. Score once: `cargo run -p fse --bin a1_gate -- --run1 sessions/run1
   --phase-test-dir <new-dir>`. >= 0.90 closes the bead.

## Artifacts

- This file: `docs/results/A1-ACOUSTIC-GATE.md`
- `docs/results/A1-dev-results.csv` (per-config DEV top-1 + PHASE-TEST verdict)
- `docs/results/A1-confusion-matrix.csv` (30x30 PHASE-TEST matrix, VOID
  2026-10-28: degenerate all-`congealed` output of the void run, kept so the
  run is never mistaken for evidence — see root cause above)

## Design decisions beyond the bead text (explicit)

1. Take index = per-word occurrence order in `manifest.tsv`, because
   `session.tsv` buckets (0/1/2 x 40 rows) do not encode 4 takes.
2. `a1_gate` is a new binary in the existing `fse` crate (which gained a
   `hound = "3"` dependency, already used elsewhere in the workspace)
   rather than a new crate.
3. The synthetic chirp probe lives only in `/tmp` and in unit tests; no
   synthetic audio was committed as owner data.

## Re-freeze 2026-10-31 (A1-r, hearsay-four-mc1--a1-rerun-canonical)

re-frozen 2026-10-31 on DEV evidence; supersedes band0.2 freeze.

Frozen-A1-Config:
`base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn+dtw_baseline+band1.0`

Decision record (DEV on canonical run1, `sessions/canon/run1`, n=120,
`a1_gate` release build, 2026-10-31):

- band 1.0: 7 configs 119/120 = 0.9917 (base with and without cmvn),
  `band_100_4000_no_cmn` 120/120 = 1.0000.
- band 0.2: all 8 configs 113/120 = 0.9417; exclusion rate 7/120
  unscoreable (all 7 misses have sameword_finite=0 — the band excludes
  every same-word template, lengths being config-invariant; `a1_probe`
  evidence, same 7 miss queries in every config, fallback labels differ).
- Only band changed (0.2 -> 1.0); the cmvn token is kept
  (base+cmvn = 119/120 at band 1.0, identical to base). The instrument
  requires the frozen base to equal tuning-grid row 0.

Why (item 3, closed 2026-10-31; evidence `src/fse/examples/a1_probe.rs` +
bead notes): the identical 113/120 x8 was genuine band-exclusion, not a
scorer bug. Zeroed/shuffled controls collapse (0.0333 = 4/120 exactly at
band 1.0 under zeroed features); all 7 band-0.2 misses are forced; at
band 1.0 the configs diverge exactly at the one ambiguous query.

Within-session DEV 119-120/120 at band 1.0 means classical MFCC+DTW
discriminates the 30 words on one session; the earlier 0.86/0.94 were
band-length artifacts, not acoustic limits.
