#!/usr/bin/env bash
# A6 sweep gate (bead hearsay-four-fmyc--sweep-drivers-scorer).
#
# Usage: scripts/a6/run_all.sh --synthetic [--out <tsv>]
#
# Dry run on synthetic lattices: DEV-tunes arm B (arm C reuses the identical
# params), sweeps TEST-200 over budgets {50,100,250,500} x arms {B,C} through
# the real fabric bridge, writes the fixed-schema TSV, and runs the scorer
# self-checks. Exit 0 = gate green.
set -euo pipefail

usage() {
    echo "usage: scripts/a6/run_all.sh --synthetic [--out <tsv>]" >&2
    exit 2
}

MODE=""
OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --synthetic) MODE="synthetic"; shift ;;
        --out) OUT="${2:?--out needs a value}"; shift 2 ;;
        -h|--help) usage ;;
        *) echo "unknown argument: $1" >&2; usage ;;
    esac
done
[ "$MODE" = "synthetic" ] || usage

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${OUT:-$ROOT/scripts/a6/out/sweep.tsv}"
mkdir -p "$(dirname "$OUT")"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target-shared}"

cd "$ROOT"
cargo run --quiet --offline -p experiment --example scorer_selfcheck
cargo run --quiet --offline -p experiment --example sweep_synth -- --out "$OUT"

# Falsification gate: exact header, 8 data rows, every budget x arm once.
EXPECTED="$(printf 'budget\tarm\tWER\tS\tD\tI\tRTF\tpeak_live\tpeak_dormant\tlate_rescues\tdWER\tCI_lo\tCI_hi')"
HEADER="$(head -n 1 "$OUT")"
if [ "$HEADER" != "$EXPECTED" ]; then
    echo "SCHEMA FAIL: header mismatch" >&2
    echo "got:  $HEADER" >&2
    echo "want: $EXPECTED" >&2
    exit 1
fi
ROWS="$(($(wc -l < "$OUT") - 1))"
if [ "$ROWS" -ne 8 ]; then
    echo "SCHEMA FAIL: want 8 data rows, got $ROWS" >&2
    exit 1
fi
for b in 50 100 250 500; do
    for a in B C; do
        n="$(awk -F'\t' -v b="$b" -v a="$a" '$1==b && $2==a {n++} END {print n+0}' "$OUT")"
        if [ "$n" -ne 1 ]; then
            echo "SCHEMA FAIL: budget $b arm $a appears $n times" >&2
            exit 1
        fi
    done
done
echo "GATE PASS: $OUT ($ROWS rows)"
cat "$OUT"
