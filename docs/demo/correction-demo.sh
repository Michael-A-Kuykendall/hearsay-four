#!/bin/bash
# Correction demo without owner audio (lane G, bead hearsay-four-j1i0).
# One command: builds hearsay-correct and runs decode -> why -> correct ->
# re-decode -> undo end to end on 4 fixed dev-clean utterances, plus the
# 20-utterance collateral report. Exits nonzero on any failure.
#
# Reads models/audio/transcripts read-only from $HEARSAY_MAIN_ROOT
# (default: this checkout); writes only inside this worktree
# (build output + target/correction-demo/).
set -u
cd "$(dirname "$0")/../.." || exit 1
ROOT="${HEARSAY_MAIN_ROOT:-$PWD}"
BVROOT="${BETTER_VOICE_ROOT:-$ROOT/../better-voice}"
TIER1="$BVROOT/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt"
[ -f "$TIER1" ] || { echo "FAIL: tier-1 lexicon not found at $TIER1 (set BETTER_VOICE_ROOT)" >&2; exit 1; }
nice -n 15 cargo build --release -p runner --bin hearsay-correct -j4 || exit 1
exec nice -n 15 env \
  HEARSAY_AM="$ROOT/data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json" \
  HEARSAY_LM="$ROOT/data/lm-text/r1v2-full/lm-r1v2-interpolated.json" \
  HEARSAY_TIER1="$TIER1" \
  HEARSAY_TIER2="$ROOT/data/lexicon/librispeech-lexicon.txt" \
  HEARSAY_WAV_ROOT="$ROOT/data/librispeech/wav" \
  HEARSAY_TRANS_ROOT="$ROOT/data/librispeech" \
  "${CARGO_TARGET_DIR:-$ROOT/target}/release/hearsay-correct" --workdir target/correction-demo demo
