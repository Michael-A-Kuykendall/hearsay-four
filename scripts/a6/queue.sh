#!/usr/bin/env bash
# A6 16-worker reniced resumable queue (bead hearsay-four-7i2o).
#
# usage: scripts/a6/queue.sh --shard-list <file> --lm <lm.json>
#   --lat-dir <dir> [--workers 16]
#
# The shard list has one `shard_file out_dir` pair per line. Each line runs
# one `cell_decode` worker (LM loaded once per worker); outputs are
# per-(utt,budget,arm) JSON files and existing outputs skip (resume).
# The whole xargs fan-out runs at nice 19. Any worker failure prints
# `SHARD FAIL` and the exit status is nonzero (xargs 123), but all other
# shards still complete.
set -euo pipefail

usage() {
    echo "usage: scripts/a6/queue.sh --shard-list <file> --lm <lm.json> --lat-dir <dir> [--workers N]" >&2
    exit 2
}

SHARD_LIST=""
LM=""
LATDIR=""
WORKERS=16
while [ $# -gt 0 ]; do
    case "$1" in
        --shard-list) SHARD_LIST="${2:?--shard-list needs a value}"; shift 2 ;;
        --lm) LM="${2:?--lm needs a value}"; shift 2 ;;
        --lat-dir) LATDIR="${2:?--lat-dir needs a value}"; shift 2 ;;
        --workers) WORKERS="${2:?--workers needs a value}"; shift 2 ;;
        -h|--help) usage ;;
        *) echo "unknown argument: $1" >&2; usage ;;
    esac
done
[ -n "$SHARD_LIST" ] && [ -n "$LM" ] && [ -n "$LATDIR" ] || usage

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target-shared}"
CELL_BIN="$CARGO_TARGET_DIR/release/examples/cell_decode"
[ -x "$CELL_BIN" ] || { echo "queue: missing $CELL_BIN (cargo build --release -p experiment --example cell_decode)" >&2; exit 1; }
export CELL_BIN LM LATDIR

N="$(grep -c . "$SHARD_LIST")"
echo "queue: $N shards xargs -P$WORKERS nice 19 (resume: existing task JSONs skip)"
# shellcheck disable=SC2016
nice -n 19 xargs -P "$WORKERS" -n 2 -a "$SHARD_LIST" sh -c '
  SHARD="$1"; OUTDIR="$2"
  "$CELL_BIN" --lm "$LM" --lat-dir "$LATDIR" --shard "$SHARD" --out-dir "$OUTDIR" >"$SHARD.log" 2>&1 || echo "SHARD FAIL: $SHARD (see $SHARD.log)"
' _
echo "queue: done ($N shards)"
