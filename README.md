# Hearsay-IV

An offline, model-free speech transcription research prototype. Hearsay-IV
transcribes speech using directly encoded phonetic/phonological rules, bounded
local competition, and provenance-tracked justification sets — no trained
neural models, no black box. Fully offline, inspectable transcription targeted
at forensic and legal transcript review.

**No-AI claim:** this pipeline contains no neural network, transformer,
embedding model, or learned-weight artifact — enforced by
[`scripts/no_model_guard.sh`](scripts/no_model_guard.sh), which fails red on
any ML crate, weight file, model API, STT-text-in-scoring-path, or network
client in the service path.

## The idea in one line

Instead of a learned acoustic model, Hearsay-IV keeps competing phonetic
hypotheses alive as **multi-support justification sets** in a saturation
fabric, and lets locally-windowed phonological rules commit the winner — so
every word in the output carries a *why it was heard that way* chain back to
the raw acoustic evidence.

## Quickstart (one command)

```sh
cargo run --release --bin hearsay-demo -- --input data/demo/sample.wav --max-secs 8
```

Zero-config: `data/demo/sample.wav` is a committed public-domain fixture (8 s
of U.S. Supreme Court oral argument; see `data/demo/README.md`). Expect exit 0
with a 4-word transcript (`PANHANDLING HORRIBLY HORRIBLY UNCORROBORATED`) and
provenance chains terminating at frame-level acoustic evidence. Verify the
model-free claim with `bash scripts/no_model_guard.sh`.

## Results

Frozen paper numbers from [the writeup](docs/writeup/WRITEUP.md); each value
sits next to its on-disk source. Tuning history stays in the
[number ledger](docs/results/NUMBER-LEDGER.md).

| Headline | Value | Source |
|---|---|---|
| Decoder WER, TEST-200 (200 utts, 4530 words) | 64.3% | `docs/results/A6-FABRIC-EXPERIMENT.md` Run 1 (arm A 0.6430) |
| Decoder WER, full dev-clean (2703 utts, 54402 words) | 67.6% (95% CI 66.9–68.3%) | `docs/results/analysis/HEADLINE-ERROR-ANALYSIS.md` (0.6760) |
| Fabric WER, TEST-200 best (budget 500) | 85.5% | `docs/results/A6-FABRIC-EXPERIMENT.md` Run 1 (B/C 0.8550) |
| Dormancy-vs-pruning delta (C−B), largest | -0.02 pts (95% CI [-0.07, 0.00]) | `docs/results/A6-FABRIC-EXPERIMENT.md` th25/budget-500 (`scripts/a6/out/headline_th25.tsv`) |
| Paired runs with identical transcripts | 1599/1600; rev_winner 0 | `docs/results/A6-FABRIC-EXPERIMENT.md` headline freeze |
| Fabric scaling exponent (CX1) | 1.07 | `src/fabric/tests/decision_cx1.rs` (commit `de334c5`, k~1.07 both profiles) |
| Fabric real-time factor | 0.08-0.26 | `docs/results/A6-FABRIC-EXPERIMENT.md` Run 1 (`scripts/a6/out/sweep_insurance.tsv`) |
| 30-word cross-channel top-1 | 29/29 | `docs/results/NUMBER-LEDGER.md` §5 |
| One-correction fixes | 7/16 fixed, 3 partial, 6 not; undo byte-identical | `docs/demo/correction-demo.md` |

Null-result framing: dormancy and pruning produced identical transcripts in
1599 of 1600 paired runs (rev_winner 0 — no revived hypothesis ever entered
a committed answer), so the experiment shows no demonstrated difference
between the two lifecycles at equal budget. The Fabric's measured value in
this test is provenance plus retractable corrections, not accuracy.

## Data attribution

Acoustic-model training and tuning use **LibriSpeech** (Panayotov et al.),
distributed by OpenSLR as SLR12 under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Audio is never
committed; `data/SHA256SUMS.librispeech` records the archives, hashes, and
roles. Demo fixtures are U.S. Supreme Court oral-argument clips (U.S. federal
government works, public domain).

## What predates the event vs what was built during it

- **Pre-existing (Aug 2026, initial commit `6808cc4`):** the saturation-fabric
  core — multi-support justification sets, subsumption minimality, survival
  semantics — plus the Stage 0–3 pipeline skeleton, design docs, and research
  synthesis.
- **Built during the event (Oct 27 – Nov 2, 2026):** full93 acoustic-model
  training (4-mix → 8-mix → CMVN), the R1/R1b language model, two-tier lexicon,
  WER tuning grids, the number ledger, CLI release matrix, demo/serve/stream
  binaries, and the audit + results documentation.

## Architecture

```
Stage 0  Calibration          per-speaker baseline fit (diagonal-covariance GMM)
Stage 1  FSE                  classical front end — 39-dim MFCC features,
                              per-phone codebook scoring, N-best emission
Stage 2  Arena                hash-consed fact ingestion (phoneme candidates)
Stage 3  Local commit window  N-best competition under SPE phonological rules
                              executed as Kaplan & Kay FST cascades; winner
                              committed on window close; losers survive via
                              independent support
Stage 4  Lexicon              two-tier CMUdict + LibriSpeech-G2P gap fill
Stage 5  Sequence             classical n-gram plausibility
Stage 10 Accent signature     session-persistent per-speaker displacement,
                              fed back into scoring; drift handled as live
                              retraction-and-recascade
```

Cross-cutting: the **saturation-fabric** fact store with multi-support
justification sets and survival semantics (the D-1 fix), always-on
provenance, and a PPT/C-Test invariant framework.

## Repository layout

```
src/
  calibration/  Stage 0 — baseline fit, enrollment
  fse/          Stage 1 — feature extraction, score, N-best
  arena/        Stage 2 — hash-consed fact arena
  stage3/       Stage 3 — commit window, SPE rules, FST cascade, Viterbi window
  accent/       Fail-fast experiment + Stage 10 accent signature
  fabric/       Saturation fact store (multi-support + survival)
  invariant_ppt/  runtime invariants + contract test framework
  corpus/       audio decode, corpora manifests (ingestion only)
  lm/           classical n-gram LM (R1/R1b)
  lexicon/      two-tier dictionary matching
  sequence/     n-gram sequence scoring
  runner/       CLI binaries (demo, serve, stream, transcribe)
data/
  demo/         committed zero-config fixtures (public domain)
  partitions/   committed train/dev/lockbox partition manifests
docs/
  design/       single-source design specifications (per module)
  research/     research notes and reference verification
  results/      NUMBER-LEDGER.md + per-gate evidence
  audit/        audit summaries (bulk transcripts excluded from release)
```

## Build & test

```sh
cargo build --release --bin hearsay-demo   # or: bash scripts/release.sh --target host
bash scripts/no_model_guard.sh             # model-free proof (must print GREEN)
sh scripts/personal_guard.sh               # privacy proof (must print OK)
```

See `HEARSAY-IV-RESEARCH-SYNTHESIS.md` for the full thesis,
`docs/design/*` for pinned design decisions, and
[`docs/RELEASE-CHECKLIST.md`](docs/RELEASE-CHECKLIST.md) for the public-release
audit record.

## Why "Hearsay"

The name borrows from the Hearsay-II / Hearsay-III speech-understanding
systems (Erman, Hayes-Roth, Lesser, Reddy — 1980). Hearsay-IV adds the
departure Hearsay-III pointed to: a losing hypothesis is retracted with
provenance intact, and survives if it retains independent support.
