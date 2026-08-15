# Hearsay-IV

An offline, model-free speech transcription research prototype. Hearsay-IV
transcribes speech using directly encoded phonetic/phonological rules, bounded
local competition, and provenance-tracked justification sets — no trained
neural models, no black box. The system is designed for fully offline,
inspectable transcription targeted at forensic and legal transcript review.

## The idea in one line

Instead of a learned acoustic model, Hearsay-IV keeps competing phonetic
hypotheses alive as **multi-support justification sets** in a saturation
fabric, and lets locally-windowed phonological rules commit the winner —
so every word in the output carries a *why it was heard that way* chain back
to the raw acoustic evidence.

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
Stage 4  Lexicon            (planned) CMUdict-shaped dictionary matching
Stage 5  Sequence            (planned) classical n-gram plausibility
Stage 10 Accent signature     session-persistent per-speaker displacement,
                              fed back into scoring; drift handled as live
                              retraction-and-recascade
```

Cross-cutting: a **saturation-fabric** fact store with multi-support
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
  corpus/       audio decode, corpora manifests
data/
  session/      fails-fast experiment session manifest + build tooling
docs/
  design/       single-source design specifications (per module)
  research/     research notes and reference verification
```

## Build & test

Each crate is an ordinary Cargo crate under `src/`. Build and test in the
workspace:

```sh
cargo build --manifest-path src/fabric/Cargo.toml
cargo test --manifest-path src/stage3/Cargo.toml --all-features
```

The system is in-progress. See `HEARSAY-IV-RESEARCH-SYNTHESIS.md` for the
full thesis, and `docs/design/*` for the pinned design decisions.

## Why "Hearsay"

The name borrows from the Hearsay-II / Hearsay-III speech-understanding
systems (Erman, Hayes-Roth, Lesser, Reddy — 1980). Hearsay-IV adds the
departure Hearsay-III pointed to: a losing hypothesis is retracted with
provenance intact, and survives if it retains independent support.

## Status

Stages 0–3 and the accent-signature loop are implemented with a TDD gate per
module (see `docs/design/`). The behavioral science question — "does
session-persistent carry-forward actually separate error rates downward?" —
is the fail-fast experiment living in `src/accent`.