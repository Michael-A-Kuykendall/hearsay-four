#!/usr/bin/env bash
# owner-pipeline.sh — single post-recording owner-data pipeline chain.
#
# Bead: hearsay-four-2cmx--owner-pipeline-dryrun (Lane M).
# Chain: canonicalize -> QC -> R4-v2 MAP -> owner DEV WER ->
#        read PHASE-TEST (scored ONCE) -> P4 fly benchmark.
#
# Usage:
#   owner-pipeline.sh --check [--date YYYYMMDD]
#       Validate wiring/args/paths WITHOUT audio. Green in lanes.
#   owner-pipeline.sh [--date YYYYMMDD] [--raw-root DIR] [--canon-root DIR]
#       [--only STEPS] [--skip STEPS] [--strict]
#       Run the chain on main, where session audio exists.
#
# Steps whose binary is pending print PENDING with the exact gate spec and
# do not fail the chain (unless --strict). No step ever synthesizes audio,
# fixtures, or scores: missing input fails loudly.
set -u

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATE="20261101"
RAW_ROOT="sessions"
CANON_ROOT="sessions/canon"
MODE="run"
ONLY=""
SKIP=""
STRICT=0

usage() {
    cat >&2 <<'EOF'
usage: owner-pipeline.sh --check [--date YYYYMMDD]
   or: owner-pipeline.sh [--date YYYYMMDD] [--raw-root DIR] [--canon-root DIR]
                         [--only STEPS] [--skip STEPS] [--strict]
  --check         validate wiring/args/paths without audio (lane-safe)
  --date D        owner sitting date stamp (default 20261101)
  --raw-root DIR  raw session root (default sessions)
  --canon-root D  canonical session root (default sessions/canon)
  --only STEPS    comma list: canon,qc,map,devwer,phasetest,p4
  --skip STEPS    comma list of steps to skip
  --strict        pending steps fail the chain (default: pending is not failure)
EOF
    exit 2
}

# Session table: id-suffix | prompt file | takes | kind (go|optional).
session_table() {
    cat <<'EOF'
harvard-TRAIN-01|sessions/prompts-harvard-train-01.txt|50|go
harvard-DEV-01|sessions/prompts-harvard-dev-01.txt|30|go
fly-DEV-01|sessions/prompts-fly-dev-01.txt|15|go
read-PHASE-TEST-01|sessions/prompts-read-phase-test-01.txt|50|go
fly-PHASE-TEST-01|sessions/prompts-fly-phase-test-01.txt|15|go
words30-PHASE-TEST-01|sessions/script-wave3.txt|30|optional
EOF
}

session_id() {
    # $1 = suffix -> owner-<date>-<suffix>
    echo "owner-${DATE}-$1"
}

want_step() {
    # $1 = step name -> 0 when selected by --only/--skip
    case ",${SKIP}," in
        *,"$1,"*) return 1 ;;
    esac
    if [ -n "$ONLY" ]; then
        case ",${ONLY}," in
            *,"$1,"*) return 0 ;;
            *) return 1 ;;
        esac
    fi
    return 0
}

# ---------------------------------------------------------------- check mode

CHECK_FAIL=0

check_ok() { echo "CHECK-OK: $1"; }
check_red() { echo "CHECK-RED: $1"; CHECK_FAIL=1; }
check_info() { echo "CHECK-INFO: $1"; }

check_file() {
    # $1 = repo-relative path, $2 = role description
    if [ -f "$REPO/$1" ]; then
        check_ok "$1 ($2)"
    else
        check_red "missing $1 ($2)"
    fi
}

check_grep() {
    # $1 = file, $2 = pattern, $3 = description
    if [ -f "$REPO/$1" ] && grep -q -- "$2" "$REPO/$1"; then
        check_ok "$1 carries $3"
    else
        check_red "$1 lacks $3 (pattern: $2)"
    fi
}

check_prompt() {
    # $1 = prompt file, $2 = expected content lines
    local f="$1" want="$2" got="?"
    if [ ! -f "$REPO/$f" ]; then
        check_red "missing prompt file $f (want $want lines)"
        return
    fi
    got="$(grep -vc '^#' "$REPO/$f")"
    if [ "$got" = "$want" ]; then
        check_ok "$f content lines $got/$want"
    else
        check_red "$f content lines $got, want $want"
    fi
}

run_check() {
    echo "owner-pipeline --check: repo=$REPO date=$DATE (no audio, no build)"
    echo "--- 1. pipeline sources exist ---"
    check_file "src/corpus/src/bin/canon-session.rs" "canon step binary"
    check_file "src/corpus/src/canonical.rs" "RS-1 canonicalizer + QC writer"
    check_file "src/hmm/src/adapt.rs" "MAP adaptation overlay (R4 machinery)"
    check_file "src/hmm/tests/owner_enroll_gate_tests.rs" "enrollment gate harness"
    check_file "src/hmm/tests/phase_test_scoring_tests.rs" "phase-test scoring harness"
    check_file "src/decoder/examples/wer_devclean.rs" "R5a WER harness (DEV WER recipe)"
    check_file "src/runner/src/bin/hearsay-transcribe.rs" "file transcription binary"
    check_file "src/a2/src/bin/a2_dev.rs" "connected-word DEV runner"
    check_file "src/eval/src/lib.rs" "paired bootstrap CI (STAT-1)"
    echo "--- 2. runbook + gate docs exist ---"
    check_file "docs/OWNER-RECORDING-RUNBOOK.md" "GO session order"
    check_file "docs/RECORDING-READINESS-CHECKLIST.md" "GO readiness gates"
    check_file "docs/results/PHASE-TEST-SCORING.md" "phase-test method + once rule"
    check_file "docs/design/recording-protocol.md" "P4 target sentences section 3.3"
    check_file "docs/audit/REFOCUS-2026-10-31-GO.md" "GO overrides (R4-v2, P4 n=15/15)"
    echo "--- 3. prompt files cut with exact line counts ---"
    while IFS='|' read -r _suffix prompt takes _kind; do
        check_prompt "$prompt" "$takes"
    done <<EOF
$(session_table)
EOF
    echo "--- 4. contract surfaces present (static, no build) ---"
    check_grep "src/corpus/src/bin/canon-session.rs" "usage: canon-session <raw-dir> <out-dir>" "canon arg surface"
    check_grep "src/corpus/src/canonical.rs" 'CANON_VERSION' "canon version const"
    check_grep "src/corpus/src/canonical.rs" 'RESAMPLER_ID' "pinned resampler id"
    check_grep "src/corpus/src/canonical.rs" 'PAD_MS' "250ms re-pad const"
    check_grep "src/corpus/src/canonical.rs" 'take\\tpeak_dbfs' "qc.tsv header"
    check_grep "src/hmm/src/adapt.rs" "AdaptOverlay" "MAP overlay type"
    check_grep "src/hmm/src/adapt.rs" "select_stage" "tau/stage selection"
    check_grep "src/eval/src/lib.rs" "paired_bootstrap_ci" "paired bootstrap CI fn"
    check_grep "src/eval/src/lib.rs" "corpus_delta_ci" "corpus delta CI fn"
    check_grep "src/decoder/examples/wer_devclean.rs" "usage: wer_devclean" "R5a WER arg surface"
    check_grep "src/runner/src/bin/hearsay-transcribe.rs" "usage: hearsay-transcribe" "transcribe arg surface"
    check_grep "src/a2/src/bin/a2_dev.rs" "usage: a2_dev" "a2 DEV arg surface"
    check_grep "docs/audit/REFOCUS-2026-10-31-GO.md" "P4 runs at n=15/15" "P4 n=15/15 rule"
    check_grep "docs/audit/REFOCUS-2026-10-31-GO.md" "enrollment = harvard-TRAIN-01" "R4-v2 enrollment spec"
    echo "--- 5. session mapping for --date $DATE ---"
    while IFS='|' read -r suffix _prompt takes kind; do
        check_info "$(session_id "$suffix") takes=$takes kind=$kind raw=$RAW_ROOT/$(session_id "$suffix") canon=$CANON_ROOT/$(session_id "$suffix")"
    done <<EOF
$(session_table)
EOF
    echo "--- 6. toolchain presence (no build runs) ---"
    if command -v cargo >/dev/null 2>&1; then
        check_ok "cargo on PATH ($(cargo --version 2>/dev/null || echo unknown))"
    else
        check_red "cargo not on PATH (run mode needs it on main)"
    fi
    echo "--- 7. would-run chain (run mode on main) ---"
    check_info "canon: cargo run -p corpus --bin canon-session -- $RAW_ROOT/<id> $CANON_ROOT/<id> (6 GO sessions)"
    check_info "qc: manifest.tsv + qc.tsv + provenance.tsv + SHA256SUMS per canon dir; qc.tsv status column"
    check_info "map: PENDING — R4-v2 harness (enrollment=harvard-TRAIN-01, gate: owner DEV WER adapted < base, paired bootstrap CI below 0)"
    check_info "devwer: PENDING — R5a decoder owner DEV WER (wer_devclean recipe on canon harvard-DEV-01)"
    check_info "phasetest: PENDING — owner read-PHASE-TEST-01 scored ONCE at level A (word-timestamped); marker-guarded"
    check_info "p4: PENDING — fly DEV/PHASE-TEST n=15/15, descriptive + bootstrap CI, no significance claim"
    if [ "$CHECK_FAIL" -eq 0 ]; then
        echo "OWNER-PIPELINE CHECK GREEN: wiring/args/paths valid, audio untouched."
    else
        echo "OWNER-PIPELINE CHECK RED: see CHECK-RED lines above."
    fi
    exit "$CHECK_FAIL"
}

# ---------------------------------------------------------------- run mode

RUN_FAIL=0
RUN_PENDING=0

step_ok() { echo "STEP $1: OK — $2"; }
step_failed() { echo "STEP $1: FAILED — $2"; RUN_FAIL=1; }
step_pending() {
    echo "STEP $1: PENDING — $2"
    RUN_PENDING=1
    if [ "$STRICT" -eq 1 ]; then
        RUN_FAIL=1
    fi
}
step_skip() { echo "STEP $1: SKIP — $2"; }

run_canon() {
    local suffix id raw out missing=0
    while IFS='|' read -r suffix _prompt _takes kind; do
        id="$(session_id "$suffix")"
        raw="$REPO/$RAW_ROOT/$id"
        out="$REPO/$CANON_ROOT/$id"
        if [ ! -d "$raw" ]; then
            if [ "$kind" = "optional" ]; then
                echo "STEP canon: SKIP optional $id (no $raw)"
                continue
            fi
            echo "STEP canon: FAILED — missing raw session $raw"
            missing=1
            continue
        fi
        echo "STEP canon: canon-session $raw -> $out"
        if (cd "$REPO" && cargo run --quiet -p corpus --bin canon-session -- "$raw" "$out"); then
            echo "STEP canon: OK $id"
        else
            echo "STEP canon: FAILED $id (canon-session exit $?)"
            missing=1
        fi
    done <<EOF
$(session_table)
EOF
    if [ "$missing" -ne 0 ]; then
        RUN_FAIL=1
        echo "STEP canon: FAILED — one or more sessions did not canonicalize"
    else
        echo "STEP canon: OK — all present sessions canonicalized"
    fi
}

run_qc() {
    local suffix id dir missing=0 f ok rej total
    while IFS='|' read -r suffix _prompt _takes kind; do
        id="$(session_id "$suffix")"
        dir="$REPO/$CANON_ROOT/$id"
        if [ ! -d "$dir" ]; then
            if [ "$kind" = "optional" ]; then
                echo "STEP qc: SKIP optional $id (no $dir)"
                continue
            fi
            echo "STEP qc: FAILED — missing canon dir $dir (run canon first)"
            missing=1
            continue
        fi
        for f in manifest.tsv qc.tsv provenance.tsv SHA256SUMS; do
            if [ ! -f "$dir/$f" ]; then
                echo "STEP qc: FAILED — $id lacks $f"
                missing=1
            fi
        done
        if [ -f "$dir/qc.tsv" ]; then
            total="$(($(wc -l <"$dir/qc.tsv") - 1))"
            ok="$(awk -F'\t' 'NR>1 && $NF=="OK" {n++} END {print n+0}' "$dir/qc.tsv")"
            rej="$(awk -F'\t' 'NR>1 && $NF!="OK" {n++} END {print n+0}' "$dir/qc.tsv")"
            echo "STEP qc: $id takes=$total accepted=$ok rejected=$rej"
            if [ "$ok" -eq 0 ]; then
                echo "STEP qc: FAILED — $id has zero accepted takes"
                missing=1
            fi
        fi
    done <<EOF
$(session_table)
EOF
    if [ "$missing" -ne 0 ]; then
        RUN_FAIL=1
        echo "STEP qc: FAILED — see lines above"
    else
        echo "STEP qc: OK — canon outputs present with accepted takes"
    fi
}

run_map() {
    step_pending "map" "R4-v2 harness not built yet — spec: enrollment=harvard-TRAIN-01 canon (50), tau/stage frozen on owner DEV, gate owner DEV WER adapted < base via R5a decoder with paired bootstrap CI below 0 (GO overrides). Machinery: src/hmm/src/adapt.rs (AdaptOverlay/select_stage). No stub run: the run1-based owner_enroll_gate_tests split is stale for Harvard sessions."
}

run_devwer() {
    step_pending "devwer" "owner DEV WER binary not built yet — spec: R5a decoder (wer_devclean recipe) on canon harvard-DEV-01, base vs adapted, paired bootstrap CI (src/eval). Needs --am/--lm model paths from L2/L3/L4 lanes."
}

run_phasetest() {
    local id marker
    id="$(session_id "read-PHASE-TEST-01")"
    marker="$REPO/docs/results/${id}.SCORED"
    if [ -f "$marker" ]; then
        step_failed "phasetest" "$id already scored ONCE ($marker exists) — refusing re-score. Marker removal is auditable and needs coordinator approval."
        return
    fi
    step_pending "phasetest" "$id unscored (no marker) — spec: score ONCE at level A (word-timestamped reference), milestone claims use it, then touch $marker. Harness pending: phase_test_scoring_tests scores the D1 SCOTUS partition (level B), not owner sessions."
}

run_p4() {
    step_pending "p4" "fly benchmark binary not built yet (bead b6y, after P3b) — spec: fly DEV + PHASE-TEST n=15/15, descriptive with bootstrap CI, no significance claim (GO overrides). Inputs: canon fly-DEV-01 + fly-PHASE-TEST-01."
}

run_chain() {
    echo "owner-pipeline run: repo=$REPO date=$DATE raw=$RAW_ROOT canon=$CANON_ROOT strict=$STRICT"
    if want_step "canon"; then run_canon; else step_skip "canon" "--only/--skip"; fi
    if want_step "qc"; then run_qc; else step_skip "qc" "--only/--skip"; fi
    if want_step "map"; then run_map; else step_skip "map" "--only/--skip"; fi
    if want_step "devwer"; then run_devwer; else step_skip "devwer" "--only/--skip"; fi
    if want_step "phasetest"; then run_phasetest; else step_skip "phasetest" "--only/--skip"; fi
    if want_step "p4"; then run_p4; else step_skip "p4" "--only/--skip"; fi
    echo "owner-pipeline: done fail=$RUN_FAIL pending=$RUN_PENDING"
    exit "$RUN_FAIL"
}

# ---------------------------------------------------------------- args

while [ "$#" -gt 0 ]; do
    case "$1" in
        --check) MODE="check" ;;
        --date)
            shift
            [ "$#" -eq 0 ] && usage
            DATE="$1"
            ;;
        --raw-root)
            shift
            [ "$#" -eq 0 ] && usage
            RAW_ROOT="$1"
            ;;
        --canon-root)
            shift
            [ "$#" -eq 0 ] && usage
            CANON_ROOT="$1"
            ;;
        --only)
            shift
            [ "$#" -eq 0 ] && usage
            ONLY="$1"
            ;;
        --skip)
            shift
            [ "$#" -eq 0 ] && usage
            SKIP="$1"
            ;;
        --strict) STRICT=1 ;;
        --help|-h) usage ;;
        *) usage ;;
    esac
    shift
done

case "$DATE" in
    [0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]) ;;
    *) echo "owner-pipeline: --date must be YYYYMMDD (got $DATE)" >&2; exit 2 ;;
esac

if [ "$MODE" = "check" ]; then
    run_check
else
    run_chain
fi
