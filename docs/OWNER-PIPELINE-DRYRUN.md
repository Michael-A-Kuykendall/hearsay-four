# Owner-pipeline dry run (Lane M, bead hearsay-four-2cmx--owner-pipeline-dryrun)

> **NOT EVIDENCE.** This doc and `scripts/owner-pipeline.sh --check` validate
> wiring only. No audio was touched, no model was trained, no score was
> produced. Nothing here supports any accuracy claim.

## What exists

`scripts/owner-pipeline.sh` — one post-recording command chaining the full
owner-data path:

```
canonicalize -> QC -> R4-v2 MAP -> owner DEV WER ->
read PHASE-TEST (scored ONCE) -> P4 fly benchmark
```

## Lane check (no audio, this worktree)

```bash
./scripts/owner-pipeline.sh --check [--date YYYYMMDD]
```

`--check` is static: file presence, prompt line counts, contract constants,
arg surfaces, session mapping, and the would-run command list. It runs no
build and reads no audio. Gate: exit 0 (`OWNER-PIPELINE CHECK GREEN`) plus
a shellcheck-clean (or `sh -n`) script.

Observed in lane (no-audio worktree, 2026-11-02): CHECK GREEN; see the
bead return notes for the exact gate commands.

## Real dry run (coordinator, on main — audio lives only there)

Lane worktrees carry no session audio, so the coordinator runs the real
chain on main after the GO sitting lands in `sessions/`:

```bash
nice -n 15 ./scripts/owner-pipeline.sh --date <YYYYMMDD>
```

CPU etiquette: Lane A owns the box; prefix with `nice -n 15`, keep cargo
at `-j4`, single-utterance decodes only for spot checks.

Per-step selection: `--only canon,qc` / `--skip p4` (steps:
`canon,qc,map,devwer,phasetest,p4`). `--strict` turns PENDING steps into
failure; default exit 0 means every *executed* step passed.

## Step table (existing pieces, discovered read-only)

| Step | Runs today | Piece |
|---|---|---|
| canon | yes | `cargo run -p corpus --bin canon-session -- <raw> <canon>` per GO session (`src/corpus/src/bin/canon-session.rs`, RS-1 §4) |
| qc | yes | `manifest.tsv` + `qc.tsv` + `provenance.tsv` + `SHA256SUMS` per canon dir; `qc.tsv` status column (`OK` / `REJECTED: …`); zero-accepted fails |
| map | PENDING | R4-v2 harness (enrollment = canon harvard-TRAIN-01, 50). Spec: gate owner DEV WER adapted < base via R5a decoder, paired bootstrap CI below 0 (GO overrides). Machinery: `src/hmm/src/adapt.rs` (`AdaptOverlay`, `select_stage`). The run1-based `owner_enroll_gate_tests` split is stale for Harvard sessions, so the script does not run it as a stub. |
| devwer | PENDING | Owner DEV WER via the R5a recipe (`src/decoder/examples/wer_devclean.rs`) on canon harvard-DEV-01, base vs adapted, paired bootstrap CI (`src/eval`). Needs `--am`/`--lm` model files from the L2/L3/L4 lanes. |
| phasetest | PENDING | Owner read-PHASE-TEST-01 scored ONCE at level A (word-timestamped), milestone claims use it. Guard is live: `docs/results/owner-<date>-read-PHASE-TEST-01.SCORED` refuses re-score; removal is auditable, coordinator-only. Note `phase_test_scoring_tests` scores the D1 SCOTUS partition (level B), not owner sessions. |
| p4 | PENDING | Fly benchmark binary (bead b6y, after P3b). Spec: fly DEV + PHASE-TEST n=15/15, descriptive with bootstrap CI, no significance claim. Inputs: canon fly-DEV-01 + fly-PHASE-TEST-01. |

Session IDs follow `docs/OWNER-RECORDING-RUNBOOK.md`
(`owner-<date>-harvard-TRAIN-01`, `-harvard-DEV-01`, `-fly-DEV-01`,
`-read-PHASE-TEST-01`, `-fly-PHASE-TEST-01`, optional
`-words30-PHASE-TEST-01`); `--date` defaults to `20261101`.

## Out of scope for Lane M

Read-only everywhere except the script + this doc: no changes to
`adapt/`, `corpus/`, `hmm/`, prompts, or partitions. No `bd` writes, no
merge — the coordinator owns beads and merges.
