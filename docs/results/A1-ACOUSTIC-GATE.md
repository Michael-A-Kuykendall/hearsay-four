# A1 — Acoustic gate (bead hearsay-four-b7p)

Question: do classical MFCC features discriminate the owner's 30 words at all?

## Gate status: NOT RUN

PHASE-TEST top-1 >= 90% on a fresh 5th-take session was **not run**.
No number is reported for it. Reason below.

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

## How to run the real gate when audio lands

1. Record run1 audio so the 120 manifest WAVs exist under `sessions/run1/`
   (per `docs/design/recording-protocol.md`), record a fresh 5th-take
   session of all 30 words in a new directory with its own `manifest.tsv`.
2. `cargo run -p fse --bin a1_gate -- --run1 sessions/run1 --phase-test-dir <fresh-dir>`
3. Record the PHASE-TEST top-1 here and in the bead notes. A fail means:
   fix the front end on DEV only and re-run against a NEW phase-test
   session (never re-score the same one).

## Artifacts

- This file: `docs/results/A1-ACOUSTIC-GATE.md`
- `docs/results/A1-dev-results.csv` (per-config DEV status + machinery probe)
- `docs/results/A1-confusion-matrix.csv` (30x30 PHASE-TEST matrix, empty
  with status NOT RUN until the fresh session is scored)

## Design decisions beyond the bead text (explicit)

1. Take index = per-word occurrence order in `manifest.tsv`, because
   `session.tsv` buckets (0/1/2 x 40 rows) do not encode 4 takes.
2. `a1_gate` is a new binary in the existing `fse` crate (which gained a
   `hound = "3"` dependency, already used elsewhere in the workspace)
   rather than a new crate.
3. The synthetic chirp probe lives only in `/tmp` and in unit tests; no
   synthetic audio was committed as owner data.
