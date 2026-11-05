# Held-out seal log: fresh ladder test sets

Bead: `hearsay-four-vtce--seal-fresh-heldout-test-set`
Date sealed (TEST-FRESH): 2026-11-05
Sealer: lane vtce (post-hackathon swarm worker)

TEST-200 (`data/wer71g/test200s/strat200.manifest.tsv`) is consumed by A6 and
is now a REGRESSION set. Ladder rungs must score fresh held-out sets instead.
Manifests live in gitignored `data/` (owner disk); this log is the seal: it
carries paths + shas + recipes, never audio. Counts and shas only, no
utterance content.

## TEST-FRESH: SEALED

- Path (owner disk): `data/librispeech/wav/test-fresh.manifest.tsv`
- sha256: `cd2553d5ae2dc2d74c51b9fd8bd31282865861a02a674e744504a24af5555a8c`
- Rows: 2420 (2620 test-clean minus 200 TEST-200; order of test-clean kept)
- Shape: no header, 4 tab columns, utt-id first (matches existing manifests)

Derivation (utt-id join subtraction, exact column-1 match):

```
awk 'NR==FNR{a[$1];next} !($1 in a)' \
  data/wer71g/test200s/strat200.manifest.tsv \
  data/librispeech/wav/test-clean.manifest.tsv \
  > data/librispeech/wav/test-fresh.manifest.tsv
```

Input pins (read-only; TEST-200 untouched):

- `data/librispeech/wav/test-clean.manifest.tsv`
  sha256 `0f6178a3aabfe1679f3d3f7b68fe3faa8adcabbcbf094aa03beefb7e38529bd7`
- `data/wer71g/test200s/strat200.manifest.tsv`
  sha256 `1f4a36fcfc63521c6517d9c3609c8015ac135255edfdd20b70159ce01d3b845e`
  (matches the frozen value in `scripts/a6/frozen_real.json`: TEST-200
  untouched)

Verification:

- `scripts/check_heldout_overlap.sh` → `OVERLAP-GREEN` (0 TEST-200 IDs in
  TEST-FRESH; exactly the 200 TEST-200 IDs absent from test-clean order;
  0 duplicate IDs in TEST-FRESH)
- Falsification: any TEST-200 utt ID in TEST-FRESH fails the gate (red)

## TEST-STRESS: UNSEALED (blocked, owner action required)

Status: NOT sealed. No manifest written; nothing scored against this name.

Reason: the `test-other` corpus is absent from owner disk. Verified 2026-11-05:
no `test-other.tar.gz` under `data/librispeech/`, no `test-other/` audio dir,
no `test-other` manifest anywhere under `data/` (only `dev-clean` and
`test-clean` corpora are present). The lane DATA RULE permits the seal lane to
write only its own new manifest files into main `data/` and never audio, so
corpus acquisition cannot happen in-lane.

Required owner action (on main, before any rung scores a stress set):

1. Acquire `test-other.tar.gz` from OpenSLR resources/12 (freely
   downloadable, no account) into `data/librispeech/`, verifying tarball sha.
2. Extract + convert to `data/librispeech/wav/test-other/` mirroring the
   `test-clean/` layout, and build `data/librispeech/wav/test-other.manifest.tsv`
   in the same 4-column shape (no header, utt-id first).
3. Follow-up seal: fixed sample with recorded seed + count at the reserved
   path `data/librispeech/wav/test-stress.manifest.tsv`, then append its
   sha + recipe to this log.

Reserved path (do not use for anything else):
`data/librispeech/wav/test-stress.manifest.tsv`

## Scored-once rule (M5 lockbox discipline)

Each sealed set is scored at most once per rung. A miss is recorded, never
re-tuned and re-opened: no rung may tune on a sealed set, inspect per-utterance
results to adjust, then re-score the same set. Scores reference the manifest
sha above so any re-cut is detectable. TEST-200 stays a regression set and is
never re-opened as a held-out.
