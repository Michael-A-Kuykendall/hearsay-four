#!/usr/bin/env bash
# A6 calibration gates (bead hearsay-four-v7fa--recalibrate-retune-resweep).
#
# usage: scripts/a6/gates.sh [--lat-dir <dir>] [--lm <json>]
#   [--manifest <tsv>] [--cell <armA.tsv>] [--workers N]
#   [--theta-hi <f>] [--wseq <f>] [--out <dir>]
#
# Runs the three DEV gates through the production cell_decode + queue path:
#   SANITY      arm C, infinite budgets, wseq=6, theta=0 vs arm A (|d|<=0.10),
#               plus a lattice-1best diagnostic column.
#   SENSITIVITY transcript movement across params: theta 0-vs-hi (driver),
#               wseq 4-vs-10 (open question: pair llrs are 0, boundary -0.251).
#               Delta 0-vs-1000 and the uncalibrated w1-vs-w2 falsification
#               are EXPECTED-FLAT controls (reported, not verdicts).
#   DORMANCY    peak_dormant>0 (C) and prunes>0 (B) at every budget 50-500,
#               at theta-hi; theta=0 rerun reported as contrast.
#
# Grid provenance: theta-hi default is the DEV-200 liveness p75
# (751.8986152839452); p25=p50=0.0 exactly (50% zero-mass), so the owner
# theta grid {p25,p50,p75} collapses to {0, 751.8986152839452}.
# Delta grid {0, m25} collapsed to {0}: all 3.28M DEV commit margins are
# exactly 0.0 (see bead), so delta is behaviorally vacuous here.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target-shared}"

LATDIR="data/lattices/dev200"
LM="data/lm-text/r1v2-full/lm-r1v2-interpolated.json"
MAN="data/wer71g/tune200s/strat200.manifest.tsv"
CELL="data/wer71g/armA/dev200-cell.tsv"
WORKERS=16
THI=751.8986152839452
WSEQ=6
OUT="scripts/a6/out/gates"
while [ $# -gt 0 ]; do
    case "$1" in
        --lat-dir) LATDIR="${2:?--lat-dir needs a value}"; shift 2 ;;
        --lm) LM="${2:?--lm needs a value}"; shift 2 ;;
        --manifest) MAN="${2:?--manifest needs a value}"; shift 2 ;;
        --cell) CELL="${2:?--cell needs a value}"; shift 2 ;;
        --workers) WORKERS="${2:?--workers needs a value}"; shift 2 ;;
        --theta-hi) THI="${2:?--theta-hi needs a value}"; shift 2 ;;
        --wseq) WSEQ="${2:?--wseq needs a value}"; shift 2 ;;
        --out) OUT="${2:?--out needs a value}"; shift 2 ;;
        -h|--help) sed -n '2,20p' "$0" >&2; exit 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

MAXB=18446744073709551615
cargo build --quiet --offline --release -p experiment \
    --example cell_decode --example gate_collect
COLLECT="$CARGO_TARGET_DIR/release/examples/gate_collect"

mkdir -p "$OUT/shards" "$OUT/cells"
cut -f1 "$MAN" > "$OUT/ids.txt"
: > "$OUT/shard_list.txt"

# mkshard <name> <cell-subdir> <lines...>: one shard file + list entry.
mkshard() {
    local name="$1" sub="$2"; shift 2
    local S="$OUT/shards/$name.txt" D="$OUT/cells/$sub"
    mkdir -p "$D"
    : > "$S"
    while IFS= read -r id; do
        for spec in "$@"; do
            # shellcheck disable=SC2206
            local -a f=($spec)
            echo "$id ${f[0]} ${f[1]} ${f[2]} ${f[3]} ${f[4]} ${f[5]} ${f[6]} ${f[7]}" >> "$S"
        done
    done < "$OUT/ids.txt"
    echo "$ROOT/$S $ROOT/$D" >> "$OUT/shard_list.txt"
}

# Single-config shards (B 250 unless noted).
mkshard sanity cells_sanity "18446744073709551615 C 1 $WSEQ 0 0 10 20"
mkshard sens_base cells_sens_base "250 B 1 $WSEQ 0 0 10 20"
mkshard sens_t1 cells_sens_t1 "250 B 1 $WSEQ $THI 0 10 20"
mkshard sens_w4 cells_sens_w4 "250 B 1 4 0 0 10 20"
mkshard sens_w10 cells_sens_w10 "250 B 1 10 0 0 10 20"
mkshard sens_d1000 cells_sens_d1000 "250 B 1 $WSEQ 0 1000 10 20"
mkshard fals_w1 cells_fals_w1 "250 B 1 1 0 0 10 20"
mkshard fals_w2 cells_fals_w2 "250 B 1 2 0 0 10 20"
# Dormancy: one shard per arm x budget x theta (hi + lo contrast).
for th in "$THI" 0; do
    tag=hi; [ "$th" = 0 ] && tag=lo
    for b in 50 100 250 500; do
        for a in B C; do
            mkshard "dorm_${tag}_${b}_${a}" "cells_dorm_$tag" "$b $a 1 $WSEQ $th 0 10 20"
        done
    done
done

bash scripts/a6/queue.sh --shard-list "$OUT/shard_list.txt" \
    --lm "$LM" --lat-dir "$LATDIR" --workers "$WORKERS"

pass=0; fail=0
report() { # report <name> <output>: echo output, tally PASS/FAIL token.
    echo "$2"
    case "$2" in
        *" PASS"*) pass=$((pass + 1)) ;;
        *) fail=$((fail + 1)) ;;
    esac
}

sanity_out="$("$COLLECT" --mode sanity --manifest "$MAN" --cell "$CELL" \
    --task-dir "$OUT/cells/cells_sanity" --budget "$MAXB" --arm C \
    --lat-dir "$LATDIR" --out-tsv "$OUT/sanity.tsv" 2>&1)" || true
report SANITY "$sanity_out"

sens_t_out="$("$COLLECT" --mode sensitivity \
    --task-dir-a "$OUT/cells/cells_sens_base" --task-dir-b "$OUT/cells/cells_sens_t1" \
    --name-a "B250/w$WSEQ/th0" --name-b "B250/w$WSEQ/th$THI" \
    --out-tsv "$OUT/sens_theta.tsv" 2>&1)" || true
report SENS-THETA "$sens_t_out"

sens_w_out="$("$COLLECT" --mode sensitivity \
    --task-dir-a "$OUT/cells/cells_sens_w4" --task-dir-b "$OUT/cells/cells_sens_w10" \
    --name-a "B250/w4/th0" --name-b "B250/w10/th0" \
    --out-tsv "$OUT/sens_wseq.tsv" 2>&1)" || true
report SENS-WSEQ "$sens_w_out"

delta_out="$("$COLLECT" --mode sensitivity \
    --task-dir-a "$OUT/cells/cells_sens_base" --task-dir-b "$OUT/cells/cells_sens_d1000" \
    --name-a "B250/d0" --name-b "B250/d1000" \
    --out-tsv "$OUT/check_delta.tsv" 2>&1)" || true
echo "$delta_out (EXPECTED FLAT under all-zero margins)"

fals_out="$("$COLLECT" --mode sensitivity \
    --task-dir-a "$OUT/cells/cells_fals_w1" --task-dir-b "$OUT/cells/cells_fals_w2" \
    --name-a "B250/w1/th0" --name-b "B250/w2/th0" \
    --out-tsv "$OUT/check_fals.tsv" 2>&1)" || true
echo "$fals_out (EXPECTED FLAT: uncalibrated negative control)"

dorm_hi_out="$("$COLLECT" --mode dormancy --task-dir "$OUT/cells/cells_dorm_hi" \
    --out-tsv "$OUT/dorm_hi.tsv" 2>&1)" || true
report DORM-HI "$dorm_hi_out"

dorm_lo_out="$("$COLLECT" --mode dormancy --task-dir "$OUT/cells/cells_dorm_lo" \
    --out-tsv "$OUT/dorm_lo.tsv" 2>&1)" || true
echo "$dorm_lo_out (CONTRAST at theta=0, not a verdict)"

echo "GATES tally: pass=$pass fail=$fail (SANITY + SENS-THETA + SENS-WSEQ + DORM-HI)"
[ "$fail" = 0 ]
