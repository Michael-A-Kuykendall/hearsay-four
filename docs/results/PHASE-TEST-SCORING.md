# Phase-Test Scoring for Enrollment Models (bead hearsay-four-pts--phase-test-scoring)

Measured: 2026-10-28 in lane `hearsay-four-lane-f3`
(branch `bead/pts-phase-test`). Follow-up to
`hearsay-four-jem--owner-enrollment-adaptation` (closed), whose gate
requires PHASE-TEST reported once.

## Reading (resolved)

D1 PHASE-TEST partition: the three SCOTUS cases under
`data/corpus/court/scotus` (groups `case:106764_25-498`,
`case:106765_25-170`, `case:106804_25-579`). Evidence:

1. The jem close reason defines this bead's phase-test as the D1
   partition: "Phase-test deferred to
   hearsay-four-pts--phase-test-scoring (no word timestamps in
   partition)". Only the D1 partition lacks word timestamps; a 5th-take
   session would carry exact per-file word labels by construction.
2. [D1-PARTITIONS.md](./D1-PARTITIONS.md): "PHASE-TEST owner sessions (A1
   5th take, A2 ≥100 phrases) do not exist yet by design". The 5th-take
   is A1's gate instrument (record via `scripts/record_phase_test.sh`,
   bead `hearsay-four-b7p--dtw-acoustic-gate`), unrecorded — recording
   it is owner manual work, not something this bead can build.
3. The bead DESIGN step 2 prescribes exactly the D1 build
   (segmentation + documented approximation level).

Framing (load-bearing): SCOTUS speakers are off-target for owner
adaptation. Adapted ≈ base is expected; collapse would be the failure
mode. This report makes NO win claim on phase-test — the win gate
lives on owner DEV. The paired numbers quantify the off-target cost
of enrollment, reported once.

## Method

`src/hmm/tests/phase_test_scoring_tests.rs`:

- Rebuilds the jem base + MAP-adapted pair with the identical
  procedure (same TRAIN subset/caps, `TrainConfig`, owner
  enroll/DEV split, tau grid). Tau + stage + keep are frozen on
  owner DEV; the jem verdict asserts reproduce exactly (see below).
- Scores the frozen pair ONCE on three deterministic segments:
  first 60 s of each case `audio.mp3` (sample-exact head,
  `22050 Hz` mono after `to_mono`) plus the duration-proportional
  head word slice (`floor(total_words * 60 / duration)`).
- OOV slice words are skipped (counted), never guessed. Lexicon:
  first pronunciation from the better-voice phonetics table, same
  as jem. Features: 39-dim MFCC+deltas, 25 ms / 10 ms.
- Scoring is `HmmSet::phone_error_rate` (free phone-loop Viterbi,
  SIL/SP stripped) — pure scoring, no tuning on phase-test.
- Guards: consumed groups must equal the partition group set;
  consumed audio paths must carry no sealed-case fragment; both
  models must score identical references (equal ref-phone totals).

## Approximation level

- **Level A**: word-timestamped reference (forced alignment to known
  boundaries). NOT available — no SCOTUS word timestamps exist.
- **Level B (this run)**: duration-proportional transcript slicing
  over a bounded head window, OOV-skipped. Absolute PER is inflated
  by unknown slice-boundary error (uniform-rate assumption); paired
  base-vs-adapted diffs share identical references, so the
  comparison is unbiased by the slicing.
- **Level C**: decode-only, no reference (no PER possible).

## Numbers

Owner-DEV recap (frozen pair; reproduces the jem close values exactly):
base PER 1.7983, adapted PER 1.6695, tau 200 (interior), paired CI
[-0.2245,-0.0162], keep=true, stage MeansOnly (stage-2-vs-1 CI
includes 0), n=30, 91 enrollment facts. Base training: TRAIN 455,
D1-DEV 32, 39 phones, flat 0.9938 / selected 0.8043 (iter 5, 2 mix).

| Case | Slice words | OOV skipped | Ref phones | Frames | Base PER | Adapted PER |
|---|---|---|---|---|---|---|
| case:106764_25-498 | 159/9762 | 24 (15.1%) | 619 | 5984 | 0.8094 | 0.8417 |
| case:106765_25-170 | 164/17552 | 17 (10.4%) | 602 | 5984 | 0.8073 | 0.8189 |
| case:106804_25-579 | 167/12659 | 18 (10.8%) | 591 | 5984 | 0.8240 | 0.8443 |
| **Total** | 490 | 59 (12.0%) | 1812 | 17952 | **0.8135** | **0.8350** |

> RELABEL 2026-10-31 (REFOCUS §3): SCOTUS numbers are NOT evidence — wrong target population for owner-adaptation claims. R4 owner MAP gate (PER 1.80 → 1.67) is machinery check only (PER > 1 = insertion-dominated garbage; enrollment on isolated words). Do not cite as adaptation evidence.

Totals: base errors 1474, adapted errors 1513 (+39 / +2.6% relative).
Paired-diff mean +0.0214, CI [+0.0097,+0.0332] — descriptive only
(n=3, normal approximation not trusted; no inferential claim).

Interpretation: adapted is slightly worse than base on all three
cases — the expected off-target direction for owner adaptation —
and within 3% of base (no collapse). Absolute PERs here (0.81) sit
below owner-DEV PERs (1.80), likely because owner clips are 3 s
isolated-word takes with kept silence (insertion-heavy) while
SCOTUS heads are fluent speech; interpretation only, not verified
(the scorer reports no insertion/deletion split).

## Gate

```
cargo test --release -p hmm --test phase_test_scoring_tests -- --nocapture --test-threads=1 --include-ignored
# Expected: 1 passed, 0 failed; PHASE-TEST base PER=0.8135 adapted PER=0.8350 (n=3 segs, 1812 ref phones)
```

Needs the better-voice sibling checkout and
the main checkout present (or `HEARSAY_MAIN_ROOT`
set); audio is read read-only from the main workspace, transcripts
from the lane checkout. Runtime 84 s release. The test is
`#[ignore]`d so the default workspace suite stays fast (r3
precedent). No-tuning statement: tau/stage/keep come only from
owner-DEV `select_stage`; phase-test values feed no decision.

## Beyond bead text

- Approximation scale (A/B/C) defined here for reuse by later
  phase-test reports (e.g. a future owner-session phase-test would
  score at level A).
- The jem bead is closed and lane rules forbid bead DB writes, so
  the paired numbers are recorded here (not on the enrollment
  bead). No model files persisted; the run log is the record.
