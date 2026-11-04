#!/usr/bin/env bash
# A6 sweep gate (beads hearsay-four-fmyc--sweep-drivers-scorer,
# hearsay-four-7i2o--real-wiring-tuning-sweep).
#
# Usage:
#   scripts/a6/run_all.sh --synthetic [--out <tsv>]
#   scripts/a6/run_all.sh --real [--smoke] [--out <tsv>] [--workers N]
#     [--tune-budgets 100,250]
#
# --synthetic: dry run on synthetic lattices (DEV-tune arm B, sweep TEST
#   over budgets {50,100,250,500} x arms {B,C} through the real fabric
#   bridge, scorer self-checks). Exit 0 = gate green.
# --real: full real-data pipeline (DEV-200 tune on the 16-worker queue,
#   freeze + sha256, TEST-200 A/B/C x budgets sweep, score once):
#   writes scripts/a6/out/sweep_real.tsv (12 rows) + frozen config.
# --real --smoke: same code path on 4 TEST utts with fixture params
#   (no tuning): schema-valid 12-row smoke TSV in minutes.
set -euo pipefail

usage() {
    echo "usage: scripts/a6/run_all.sh (--synthetic [--out <tsv>] | --real [--smoke] [--out <tsv>] [--workers N] [--tune-budgets CSV] [--tune-manifest TSV] [--tune-cell TSV] [--dev-man P] [--dev-lat P] [--dev-cell P] [--test-man P] [--test-lat P] [--test-cell P] [--lm P])" >&2
    exit 2
}

MODE=""
SMOKE=0
TUNE_ONLY=0
OUT=""
WORKERS=16
TUNE_BUDGETS=""
TUNE_MANIFEST=""
TUNE_CELL=""
DEV_MAN_OV=""
DEV_LAT_OV=""
DEV_CELL_OV=""
TEST_MAN_OV=""
TEST_LAT_OV=""
TEST_CELL_OV=""
LM_OV=""
while [ $# -gt 0 ]; do
    case "$1" in
        --synthetic) MODE="synthetic"; shift ;;
        --real) MODE="real"; shift ;;
        --smoke) SMOKE=1; shift ;;
        --out) OUT="${2:?--out needs a value}"; shift 2 ;;
        --workers) WORKERS="${2:?--workers needs a value}"; shift 2 ;;
        --tune-budgets) TUNE_BUDGETS="${2:?--tune-budgets needs a value}"; shift 2 ;;
        --tune-manifest) TUNE_MANIFEST="${2:?--tune-manifest needs a value}"; shift 2 ;;
        --tune-cell) TUNE_CELL="${2:?--tune-cell needs a value}"; shift 2 ;;
        --dev-man) DEV_MAN_OV="${2:?--dev-man needs a value}"; shift 2 ;;
        --dev-lat) DEV_LAT_OV="${2:?--dev-lat needs a value}"; shift 2 ;;
        --dev-cell) DEV_CELL_OV="${2:?--dev-cell needs a value}"; shift 2 ;;
        --test-man) TEST_MAN_OV="${2:?--test-man needs a value}"; shift 2 ;;
        --test-lat) TEST_LAT_OV="${2:?--test-lat needs a value}"; shift 2 ;;
        --test-cell) TEST_CELL_OV="${2:?--test-cell needs a value}"; shift 2 ;;
        --lm) LM_OV="${2:?--lm needs a value}"; shift 2 ;;
        --tune-only) TUNE_ONLY=1; shift ;;
        -h|--help) usage ;;
        *) echo "unknown argument: $1" >&2; usage ;;
    esac
done
[ -n "$MODE" ] || usage
if [ "$SMOKE" = 1 ] && [ "$MODE" != "real" ]; then
    echo "--smoke needs --real" >&2; usage
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target-shared}"
cd "$ROOT"

BUDGETS="50,100,250,500"
# Lane runs point these at the main workspace (read-only) via the --*-ov flags;
# defaults are the in-checkout data paths.
DEV_MAN="${DEV_MAN_OV:-data/wer71g/tune200s/strat200.manifest.tsv}"
TEST_MAN="${TEST_MAN_OV:-data/wer71g/test200s/strat200.manifest.tsv}"
DEV_LAT="${DEV_LAT_OV:-data/lattices/dev200}"
TEST_LAT="${TEST_LAT_OV:-data/lattices/test200}"
LM="${LM_OV:-data/lm-text/r1v2-full/lm-r1v2-interpolated.json}"
DEV_CELL="${DEV_CELL_OV:-data/wer71g/armA/dev200-cell.tsv}"
TEST_CELL="${TEST_CELL_OV:-data/wer71g/armA/test200-cell.tsv}"
A6OUT="scripts/a6/out"

check_schema() {
    # check_schema <tsv> <want_rows> <arms...>: exact header, row count,
    # every budget x arm exactly once.
    local tsv="$1" want="$2"; shift 2
    local expected
    expected="$(printf 'budget\tarm\tWER\tS\tD\tI\tRTF\tpeak_live\tpeak_dormant\tlate_rescues\tdWER\tCI_lo\tCI_hi\tdorm_oracle\trev_winner')"
    local header
    header="$(head -n 1 "$tsv")"
    if [ "$header" != "$expected" ]; then
        echo "SCHEMA FAIL: header mismatch" >&2
        echo "got:  $header" >&2
        echo "want: $expected" >&2
        exit 1
    fi
    local rows
    rows="$(($(wc -l < "$tsv") - 1))"
    if [ "$rows" -ne "$want" ]; then
        echo "SCHEMA FAIL: want $want data rows, got $rows" >&2
        exit 1
    fi
    for b in 50 100 250 500; do
        for a in "$@"; do
            local n
            n="$(awk -F'\t' -v b="$b" -v a="$a" '$1==b && $2==a {n++} END {print n+0}' "$tsv")"
            if [ "$n" -ne 1 ]; then
                echo "SCHEMA FAIL: budget $b arm $a appears $n times" >&2
                exit 1
            fi
        done
    done
}

if [ "$MODE" = "synthetic" ]; then
    OUT="${OUT:-$ROOT/$A6OUT/sweep.tsv}"
    mkdir -p "$(dirname "$OUT")"
    cargo run --quiet --offline -p experiment --example scorer_selfcheck
    cargo run --quiet --offline -p experiment --example sweep_synth -- --out "$OUT"
    check_schema "$OUT" 8 B C
    echo "GATE PASS: $OUT (8 rows)"
    cat "$OUT"
    exit 0
fi

# ---------------- real path ----------------
OUT_WANTED="$OUT"
mkdir -p "$A6OUT"
cargo build --quiet --offline --release -p experiment \
    --example cell_decode --example tune_grid --example sweep_collect --example scorer_selfcheck
cargo run --quiet --offline --release -p experiment --example scorer_selfcheck
COLLECT="$CARGO_TARGET_DIR/release/examples/sweep_collect"

if [ "$SMOKE" = 1 ]; then
    # 4 TEST utts, fixture params, all budgets x A/B/C -> 12-row smoke TSV.
    SMOKE_DIR="$A6OUT/smoke"
    mkdir -p "$SMOKE_DIR/shards" "$SMOKE_DIR/cells"
    head -n 4 "$TEST_MAN" > "$SMOKE_DIR/manifest.tsv"
    cut -f1 "$SMOKE_DIR/manifest.tsv" > "$SMOKE_DIR/ids.txt"
    { head -n 1 "$TEST_CELL"; awk 'NR==FNR {id[$1]; next} $1 in id' "$SMOKE_DIR/ids.txt" "$TEST_CELL"; } > "$SMOKE_DIR/cell.tsv"
    : > "$SMOKE_DIR/shards/s0.txt"
    while IFS= read -r id; do
        for b in 50 100 250 500; do
            for a in B C; do
                echo "$id $b $a 1 1 0 0 10 20" >> "$SMOKE_DIR/shards/s0.txt"
            done
        done
    done < "$SMOKE_DIR/ids.txt"
    echo "$ROOT/$SMOKE_DIR/shards/s0.txt $ROOT/$SMOKE_DIR/cells" > "$SMOKE_DIR/shard_list.txt"
    bash scripts/a6/queue.sh --shard-list "$SMOKE_DIR/shard_list.txt" \
        --lm "$LM" --lat-dir "$TEST_LAT" --workers 4
    SMOKE_TSV="${OUT_WANTED:-$ROOT/$A6OUT/sweep_real_smoke.tsv}"
    mkdir -p "$(dirname "$SMOKE_TSV")"
    "$COLLECT" --mode sweep --manifest "$SMOKE_DIR/manifest.tsv" --cell "$SMOKE_DIR/cell.tsv" \
        --task-dir "$SMOKE_DIR/cells" --budgets "$BUDGETS" --out-tsv "$SMOKE_TSV" \
        --per-utt "$SMOKE_DIR/per_utt.tsv"
    check_schema "$SMOKE_TSV" 12 A B C
    echo "GATE PASS (real smoke): $SMOKE_TSV (12 rows, 4 utts)"
    cat "$SMOKE_TSV"
    exit 0
fi

# ---------------- full real pipeline ----------------
OUT="${OUT_WANTED:-$ROOT/$A6OUT/sweep_real.tsv}"
mkdir -p "$(dirname "$OUT")"
# 1. Tune: 18 coarse candidates x DEV-200 x tuning budgets, one shard per
# candidate. Default tunes on all four budgets; --tune-budgets subsets for
# time-boxed lanes (the winner still sweeps all budgets; the frozen config
# records which budgets tuned it).
TUNE_BUDGETS="${TUNE_BUDGETS:-$BUDGETS}"
TUNE_BL="$(echo "$TUNE_BUDGETS" | tr ',' ' ')"
TUNE_MAN="${TUNE_MANIFEST:-$DEV_MAN}"
TUNE_CL="${TUNE_CELL:-$DEV_CELL}"
echo "tune set: $TUNE_MAN ($(grep -c . "$TUNE_MAN") utts) budgets $TUNE_BUDGETS"
"$CARGO_TARGET_DIR/release/examples/tune_grid" > "$A6OUT/tune_params.txt"
TUNE_DIR="$A6OUT/tune_cells"
SHARD_DIR="$A6OUT/shards"
mkdir -p "$TUNE_DIR" "$SHARD_DIR"
cut -f1 "$TUNE_MAN" > "$A6OUT/tune_ids.txt"
: > "$A6OUT/tune_shard_list.txt"
while read -r k wa ws th d h hm; do
    [ -z "$k" ] && continue
    S="$SHARD_DIR/tune_cand$k.txt"
    : > "$S"
    while IFS= read -r id; do
        for b in $TUNE_BL; do
            echo "$id $b B $wa $ws $th $d $h $hm" >> "$S"
        done
    done < "$A6OUT/tune_ids.txt"
    echo "$ROOT/$S $ROOT/$TUNE_DIR/cand$k" >> "$A6OUT/tune_shard_list.txt"
done < "$A6OUT/tune_params.txt"
if ! bash scripts/a6/queue.sh --shard-list "$A6OUT/tune_shard_list.txt" \
        --lm "$LM" --lat-dir "$DEV_LAT" --workers "$WORKERS"; then
    echo "tune queue had shard failures; collecting anyway (failed candidates score None)"
fi
"$COLLECT" --mode tune --manifest "$TUNE_MAN" --params-file "$A6OUT/tune_params.txt" \
    --task-dir "$TUNE_DIR" --budgets "$TUNE_BUDGETS" --out "$A6OUT/frozen_raw.json" | tee "$A6OUT/tune.log"
# 2. Freeze: winner + sha256 of every input -> committed config.
FREEZE_CMD="scripts/a6/run_all.sh --real --tune-budgets $TUNE_BUDGETS --tune-manifest $TUNE_MAN --tune-cell $TUNE_CL --workers $WORKERS"
[ -n "$DEV_MAN_OV" ] && FREEZE_CMD="$FREEZE_CMD --dev-man $DEV_MAN_OV"
[ -n "$DEV_LAT_OV" ] && FREEZE_CMD="$FREEZE_CMD --dev-lat $DEV_LAT_OV"
[ -n "$DEV_CELL_OV" ] && FREEZE_CMD="$FREEZE_CMD --dev-cell $DEV_CELL_OV"
[ -n "$TEST_MAN_OV" ] && FREEZE_CMD="$FREEZE_CMD --test-man $TEST_MAN_OV"
[ -n "$TEST_LAT_OV" ] && FREEZE_CMD="$FREEZE_CMD --test-lat $TEST_LAT_OV"
[ -n "$TEST_CELL_OV" ] && FREEZE_CMD="$FREEZE_CMD --test-cell $TEST_CELL_OV"
[ -n "$LM_OV" ] && FREEZE_CMD="$FREEZE_CMD --lm $LM_OV"
python3 - "$A6OUT/frozen_raw.json" "scripts/a6/frozen_real.json" "$FREEZE_CMD" "$DEV_MAN" "$TEST_MAN" "$LM" "$DEV_CELL" "$TEST_CELL" "$TUNE_MAN" "$TUNE_CL" <<'EOF'
import hashlib, json, sys, time
raw, dest, cmd = sys.argv[1], sys.argv[2], sys.argv[3]
inputs = sys.argv[4:]
f = json.load(open(raw))
f["sha256"] = {p: hashlib.sha256(open(p, "rb").read()).hexdigest() for p in inputs}
f["frozen_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
f["freeze_cmd"] = cmd
json.dump(f, open(dest, "w"), indent=2)
print("froze", dest, "winner cand", f["winner_cand"], "dev_errors", f["dev_errors"])
EOF
cp -f scripts/a6/frozen_real.json "$A6OUT/frozen_real.json"
if [ "$TUNE_ONLY" = 1 ]; then
    echo "TUNE DONE: $A6OUT/frozen_raw.json + scripts/a6/frozen_real.json (TEST sweep skipped by --tune-only)"
    exit 0
fi
# 3. Sweep: TEST-200 x budgets x B/C with winner params, one shard per utt.
# shellcheck disable=SC2016
WPARAMS="$(python3 -c 'import json; p=json.load(open("scripts/a6/frozen_real.json"))["params"]; print(p["w_acoustic"], p["w_sequence"], p["theta_live"], p["delta"], int(p["h"]), int(p["h_max"]))')"
SWEEP_DIR="$A6OUT/sweep_cells"
mkdir -p "$SWEEP_DIR"
cut -f1 "$TEST_MAN" > "$A6OUT/test_ids.txt"
: > "$A6OUT/sweep_shard_list.txt"
while IFS= read -r id; do
    S="$SHARD_DIR/sweep_$id.txt"
    : > "$S"
    # shellcheck disable=SC2086
    set -- $WPARAMS
    for b in 50 100 250 500; do
        for a in B C; do
            echo "$id $b $a $1 $2 $3 $4 $5 $6" >> "$S"
        done
    done
    echo "$ROOT/$S $ROOT/$SWEEP_DIR" >> "$A6OUT/sweep_shard_list.txt"
done < "$A6OUT/test_ids.txt"
bash scripts/a6/queue.sh --shard-list "$A6OUT/sweep_shard_list.txt" \
    --lm "$LM" --lat-dir "$TEST_LAT" --workers "$WORKERS"
"$COLLECT" --mode sweep --manifest "$TEST_MAN" --cell "$TEST_CELL" \
    --task-dir "$SWEEP_DIR" --budgets "$BUDGETS" --out-tsv "$OUT" \
    --per-utt "$A6OUT/sweep_real_perutt.tsv"
check_schema "$OUT" 12 A B C
echo "GATE PASS (real): $OUT (12 rows) + scripts/a6/frozen_real.json"
cat "$OUT"
