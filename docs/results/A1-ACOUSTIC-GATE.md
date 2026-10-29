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
