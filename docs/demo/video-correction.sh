#!/bin/bash
# Screen-recordable correction demo (shot F). Build once beforehand with:
#   cargo build --release -p runner --bin hearsay-correct
set -u
cd "$(dirname "$0")/../.." || exit 1
ROOT="${HEARSAY_MAIN_ROOT:-$PWD}"
BVROOT="${BETTER_VOICE_ROOT:-$ROOT/../better-voice}"
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/release/hearsay-correct"
clear
echo "loading acoustic model + 259k-word lexicon + trigram LM ..."
HEARSAY_DEMO_VIDEO=1 \
HEARSAY_AM="$ROOT/data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json" \
HEARSAY_LM="$ROOT/data/lm-text/r1v2-full/lm-r1v2-interpolated.json" \
HEARSAY_TIER1="$BVROOT/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt" \
HEARSAY_TIER2="$ROOT/data/lexicon/librispeech-lexicon.txt" \
HEARSAY_WAV_ROOT="$ROOT/data/librispeech/wav" \
HEARSAY_TRANS_ROOT="$ROOT/data/librispeech" \
"$BIN" --workdir target/correction-demo demo 2>&1 \
  | grep --line-buffered -v '^hearsay-correct demo\|^base vocab' \
  | while IFS= read -r l; do
      echo "$l"
      case "$l" in applied*) echo "re-decoding with the correction (reloading models) ...";;
                   "fix verified"*) echo "undoing, re-decoding with the original models ...";; esac
    done
echo
grep -m1 '29/29' docs/results/NUMBER-LEDGER.md
