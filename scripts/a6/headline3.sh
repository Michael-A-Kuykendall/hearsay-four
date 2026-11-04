#!/usr/bin/env bash
# A6 headline 3-theta sweep (owner order 2026-11-04): TEST-200 x A/B/C x
# budgets {50,100,250,500} at post-fix DEV liveness percentiles (ws10 row
# of recal_postfix.json) p25/p50/p75. B and C share theta per run.
# 3 theta x 1600 = 4800 runs. Insurance sweep stays Run 1 (theta=0 file
# sweep_insurance.tsv, old 13-col schema); these files use the 15-col
# schema with dorm_oracle/rev_winner.
#
# Usage: scripts/a6/headline3.sh [--smoke] [--workers N]
#   --smoke: 2 utts x theta p50 x budget 100 x B/C (4 tasks), fast verify.
set -euo pipefail

SMOKE=0
WORKERS=16
while [ $# -gt 0 ]; do
    case "$1" in
        --smoke) SMOKE=1; shift ;;
        --workers) WORKERS="${2:?--workers needs a value}"; shift 2 ;;
        -h|--help) echo "usage: scripts/a6/headline3.sh [--smoke] [--workers N]" >&2; exit 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target-shared}"
cd "$ROOT"

BUDGETS="50,100,250,500"
TEST_MAN="data/wer71g/test200s/strat200.manifest.tsv"
TEST_LAT="data/lattices/test200"
LM="data/lm-text/r1v2-full/lm-r1v2-interpolated.json"
TEST_CELL="data/wer71g/armA/test200-cell.tsv"
A6OUT="scripts/a6/out"

# Frozen v7fa otherwise: w_acoustic 1.0, w_sequence 10.0, delta 0.0,
# h 10, h_max 20, J 100. Thetas = recal_postfix.json liveness_ws10
# [p5,p25,p50,p75,p95] indices 1,2,3.
TH25="-4.2967437502168"
TH50="0.0"
TH75="750.6713080377012"

cargo build --quiet --offline --release -p experiment \
    --example cell_decode --example sweep_collect
COLLECT="$CARGO_TARGET_DIR/release/examples/sweep_collect"
CELL_BIN="$CARGO_TARGET_DIR/release/examples/cell_decode"

run_theta() {
    # run_theta <tag> <theta> <manifest> <cell> <budgets_csv> <workers>
    local tag="$1" theta="$2" man="$3" cell="$4" budgets_csv="$5" workers="$6"
    local cells="$A6OUT/headline_cells_$tag"
    local shards="$A6OUT/headline_shards_$tag"
    mkdir -p "$cells" "$shards"
    cut -f1 "$man" > "$A6OUT/headline_ids_$tag.txt"
    : > "$A6OUT/headline_shard_list_$tag.txt"
    while IFS= read -r id; do
        local s="$shards/sweep_$id.txt"
        : > "$s"
        for b in $(echo "$budgets_csv" | tr ',' ' '); do
            for a in B C; do
                echo "$id $b $a 1.0 10.0 $theta 0.0 10 20 100" >> "$s"
            done
        done
        echo "$ROOT/$s $ROOT/$cells" >> "$A6OUT/headline_shard_list_$tag.txt"
    done < "$A6OUT/headline_ids_$tag.txt"
    bash scripts/a6/queue.sh --shard-list "$A6OUT/headline_shard_list_$tag.txt" \
        --lm "$LM" --lat-dir "$TEST_LAT" --workers "$workers"
    local out="$A6OUT/headline_$tag.tsv"
    "$COLLECT" --mode sweep --manifest "$man" --cell "$cell" \
        --task-dir "$cells" --budgets "$budgets_csv" --out-tsv "$out" \
        --per-utt "$A6OUT/headline_${tag}_perutt.tsv"
    echo "WROTE $out"
    cat "$out"
}

if [ "$SMOKE" = 1 ]; then
    head -n 2 "$TEST_MAN" > "$A6OUT/headline_smoke_manifest.tsv"
    cut -f1 "$A6OUT/headline_smoke_manifest.tsv" > "$A6OUT/headline_smoke_ids.txt"
    { head -n 1 "$TEST_CELL"; awk 'NR==FNR {id[$1]; next} $1 in id' "$A6OUT/headline_smoke_ids.txt" "$TEST_CELL"; } > "$A6OUT/headline_smoke_cell.tsv"
    run_theta smoke "$TH50" "$A6OUT/headline_smoke_manifest.tsv" "$A6OUT/headline_smoke_cell.tsv" "100" 4
    python3 - "$A6OUT/headline_cells_smoke" <<'EOF'
import json, glob, sys
files = glob.glob(sys.argv[1] + "/*.json")
assert len(files) == 4, f"want 4 task JSONs, got {len(files)}"
for f in files:
    v = json.load(open(f))
    assert "dormant_oracle_touch" in v and "revivals_in_winner" in v, f
print(f"SMOKE FIELDS OK ({len(files)} task JSONs carry both diagnostics)")
EOF
    head -n 1 "$A6OUT/headline_smoke.tsv"
    exit 0
fi

# Order: th50 first (theta=0 replicates the insurance run with diagnostics:
# a guaranteed complete row), th25 second, th75 last (high-theta dormant
# load is the runtime risk; partial rows are dropped from the doc).
run_theta th50 "$TH50" "$TEST_MAN" "$TEST_CELL" "$BUDGETS" "$WORKERS"
run_theta th25 "$TH25" "$TEST_MAN" "$TEST_CELL" "$BUDGETS" "$WORKERS"
run_theta th75 "$TH75" "$TEST_MAN" "$TEST_CELL" "$BUDGETS" "$WORKERS"
echo "HEADLINE DONE: $A6OUT/headline_th{25,50,75}.tsv + perutt"
