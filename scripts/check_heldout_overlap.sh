#!/usr/bin/env bash
# Held-out zero-overlap gate (bead hearsay-four-vtce--seal-fresh-heldout-test-set):
# TEST-FRESH must contain none of the TEST-200 utt IDs (TEST-200 is consumed by
# A6 and is now a REGRESSION set; ladder rungs score the fresh sets instead).
# Fails red (exit 1) on any overlap; green prints the verdict line.
# DATA_ROOT override: lanes point at the main checkout, e.g.
#   DATA_ROOT=/home/michael/repos/hearsay-four/data scripts/check_heldout_overlap.sh
set -u
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA="${DATA_ROOT:-$REPO/data}"
FRESH="$DATA/librispeech/wav/test-fresh.manifest.tsv"
T200="$DATA/wer71g/test200s/strat200.manifest.tsv"

[ -s "$FRESH" ] || { echo "OVERLAP-RED: missing TEST-FRESH manifest: $FRESH"; exit 1; }
[ -s "$T200" ] || { echo "OVERLAP-RED: missing TEST-200 manifest: $T200"; exit 1; }

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
LC_ALL=C cut -f1 "$FRESH" | LC_ALL=C sort -u > "$tmpdir/fresh_ids.txt"
LC_ALL=C cut -f1 "$T200" | LC_ALL=C sort -u > "$tmpdir/t200_ids.txt"
overlap="$(LC_ALL=C comm -12 "$tmpdir/t200_ids.txt" "$tmpdir/fresh_ids.txt" | wc -l)"
fresh_n="$(wc -l < "$tmpdir/fresh_ids.txt")"
t200_n="$(wc -l < "$tmpdir/t200_ids.txt")"

if [ "$overlap" -ne 0 ]; then
  echo "OVERLAP-RED: $overlap TEST-200 utt IDs found in TEST-FRESH (fresh=$fresh_n t200=$t200_n)"
  LC_ALL=C comm -12 "$tmpdir/t200_ids.txt" "$tmpdir/fresh_ids.txt" | head -10
  exit 1
fi
echo "OVERLAP-GREEN: zero TEST-200 IDs in TEST-FRESH (fresh=$fresh_n t200=$t200_n)"
