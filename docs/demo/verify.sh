#!/bin/bash
# Submission demo from the fabric path (bead hearsay-four-sq9l--fabric-path-demo).
#
# File transcription where the Fabric commits the transcript (hearsay-bridge
# decision-fabric decode + hearsay-transcribe --fabric-input render), a
# why-panel showing per-word alternatives, the defeating tiling + margin, and
# dormancy/revival/defeat events from the DecisionEvent log, plus correction
# + byte-identical-exact undo (hearsay-correct fabric-demo).
#
# Demo utterances (TEST-200, disclosed as chosen in docs/demo/fabric-path-demo.md):
#   1995-1836-0004  (long: 105 committed words)
#   7021-79730-0004 (A6's sole B/C-diff row @ budget 500)
#   61-70968-0060   (revival showcase: 4 revived events, none committed)
# Bridge config: arm C, budget 500 (an A6 C-cell).
#
# Gate: CARGO_TARGET_DIR=$HOME/.cargo/target-demo bash docs/demo/verify.sh
# (from the lane worktree root; exit 0 with all three assertion groups PASS).
# Reads model/lattice/lexicon inputs read-only from the main checkout; writes
# only inside this worktree (isolated target dir + target/fabric-demo/).
# NEVER run with the shared target dir (bead hearsay-four-xk1k).
set -euo pipefail

WORKTREE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$WORKTREE"
ROOT=/home/michael/repos/hearsay-four
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cargo/target-demo}"
case "$CARGO_TARGET_DIR" in
  *"/target-demo") ;;
  *) echo "FAIL: CARGO_TARGET_DIR must be the isolated lane dir (\$HOME/.cargo/target-demo), got $CARGO_TARGET_DIR" >&2; exit 1 ;;
esac
OUT="$WORKTREE/target/fabric-demo"
BRIDGE="$OUT/bridge"
WORK="$OUT/work"
UTTS="1995-1836-0004 7021-79730-0004 61-70968-0060"
LM="$ROOT/data/lm-text/r1v2-full/lm-r1v2-interpolated.json"
TIER1=/home/michael/repos/better-voice/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt
TIER2="$ROOT/data/lexicon/librispeech-lexicon.txt"

echo "==> [1/6] checking read-only inputs"
for f in "$LM" "$TIER1" "$TIER2"; do
  [ -f "$f" ] || { echo "FAIL: missing input $f" >&2; exit 1; }
done
for u in $UTTS; do
  [ -f "$ROOT/data/lattices/test200/$u.lat.jsonl" ] || { echo "FAIL: missing lattice $u" >&2; exit 1; }
done
echo "inputs OK (main checkout read-only; target $CARGO_TARGET_DIR)"

echo "==> [2/6] building demo binaries (release)"
nice -n 15 cargo build --release -p hearsay --bin hearsay-bridge \
  -p runner --bin hearsay-transcribe --bin hearsay-correct -j4
HB="$CARGO_TARGET_DIR/release/hearsay-bridge"
HT="$CARGO_TARGET_DIR/release/hearsay-transcribe"
HC="$CARGO_TARGET_DIR/release/hearsay-correct"

echo "==> [3/6] bridge decode (arm C, budget 500) + fabric render"
rm -rf "$BRIDGE" "$WORK"
mkdir -p "$BRIDGE" "$WORK"
for u in $UTTS; do
  nice -n 15 "$HB" --lattice "$ROOT/data/lattices/test200/$u.lat.jsonl" \
    --lm "$LM" --out "$BRIDGE" --arm C --budget 500
done
nice -n 15 "$HT" \
  --fabric-input "$ROOT/data/lattices/test200/1995-1836-0004.lat.jsonl" \
  --fabric-input "$ROOT/data/lattices/test200/7021-79730-0004.lat.jsonl" \
  --fabric-input "$ROOT/data/lattices/test200/61-70968-0060.lat.jsonl" \
  --fabric-dir "$BRIDGE" --json --out-dir "$OUT" \
  --lm "$LM" --tier1 "$TIER1" --tier2 "$TIER2" \
  > "$OUT/demo-transcripts.json"

echo "==> [4/6] asserting (a) fabric-commit provenance + falsification"
python3 - "$OUT/demo-transcripts.json" "$BRIDGE" "$ROOT/data/lattices/test200" <<'EOF'
import json, sys

tpath, bdir, latdir = sys.argv[1], sys.argv[2], sys.argv[3]
items = json.load(open(tpath))
want_ids = ["1995-1836-0004", "7021-79730-0004", "61-70968-0060"]

def check_provenance(items):
    assert isinstance(items, list) and len(items) == 3, f"want 3 transcripts, got {len(items)}"
    got_ids = [t.get("utt_id") for t in items]
    assert got_ids == want_ids, f"utt ids {got_ids} != demo list {want_ids}"
    for t in items:
        uid = t.get("utt_id")
        assert t.get("path") == "fabric", f"{uid}: path != fabric (1-best path?)"
        assert "disclosed as chosen" in t.get("chosen", ""), f"{uid}: chosen label missing"
        assert t.get("text"), f"{uid}: empty transcript text"
        fs = t.get("fabric_summary")
        assert isinstance(fs, dict), f"{uid}: fabric_summary missing (1-best path?)"
        for key in ("revived", "defeated", "evicted", "pruned", "committed"):
            assert isinstance(fs.get(key), int) and fs[key] >= 0, f"{uid}: bad summary {key}"
        assert fs["events_total"] == fs["revived"] + fs["defeated"] + fs["evicted"] + fs["pruned"] + fs["committed"], \
            f"{uid}: summary counts do not add up"
        assert fs["committed"] >= 1, f"{uid}: nothing committed"
        # Independent re-derivation from the raw handoff files: committed
        # winner ids (emission order) joined to lattice words must equal the
        # transcript text. A decoder 1-best transcript has no commit records
        # and fails here.
        lat_words = {}
        for line in open(f"{latdir}/{uid}.lat.jsonl"):
            o = json.loads(line)
            if "t" in o:
                lat_words[o["t"]["id"]] = o["t"]["word"]
        winners = []
        for line in open(f"{bdir}/{uid}.events.jsonl"):
            o = json.loads(line)
            if o["event"] == "committed":
                for w in o["winner"]:
                    assert "id" in w, f"{uid}: winner without lattice id {w!r}"
                    winners.append(lat_words[w["id"]])
        assert winners, f"{uid}: commit records are empty"
        assert winners == t["text"].split(" "), f"{uid}: commits != transcript text"
        words = t.get("words")
        assert words and len(words) == len(winners), f"{uid}: words != commits"
        assert [w["text"] for w in words] == winners, f"{uid}: word texts != commits"
    return True

check_provenance(items)
print("PASS (a): 3 transcripts re-derived from DecisionEvent commit records")

# Falsification: a decoder-1-best-shaped transcript (no commit provenance)
# must FAIL the same check.
forged = json.loads(json.dumps(items))
for t in forged:
    t["path"] = "decoder"
    t.pop("fabric_summary", None)
try:
    check_provenance(forged)
except AssertionError as e:
    print(f"PASS (a-falsify): 1-best-shaped transcript rejected ({e})")
else:
    raise SystemExit("FAIL (a-falsify): 1-best-shaped transcript passed provenance")
EOF

echo "==> [5/6] asserting (b) why panels (alternatives, tiling+margin, events)"
python3 - "$OUT/demo-transcripts.json" <<'EOF'
import json, math, sys

items = json.load(open(sys.argv[1]))
nwords = 0
for t in items:
    uid = t["utt_id"]
    fs = t["fabric_summary"]
    assert fs["defeated"] >= 1, f"{uid}: no defeat events"
    assert fs["late_rescues"] == [], f"{uid}: late rescue claimed {fs['late_rescues']}"
    saw_alt = False
    for w in t["words"]:
        nwords += 1
        why = w.get("why")
        assert why, f"{uid} {w.get('text')}: why panel missing"
        assert isinstance(why.get("acoustic_score"), (int, float)) and \
            math.isfinite(why["acoustic_score"]), f"{w}: bad acoustic_score"
        assert why.get("acoustic_note"), f"{w}: acoustic_note missing"
        assert isinstance(why.get("alternatives"), list), f"{w}: alternatives missing"
        if why["alternatives"]:
            saw_alt = True
        assert why.get("alternatives_note"), f"{w}: alternatives_note missing"
        p = why.get("lm_probability")
        assert isinstance(p, (int, float)) and 0.0 < p <= 1.0, f"{w}: bad lm_probability {p}"
        assert isinstance(why.get("lm_history"), list) and \
            len(why["lm_history"]) == 2, f"{w}: bad lm_history"
        assert why.get("lexicon_source") in ("cmudict", "librispeech-g2p"), \
            f"{w}: bad lexicon_source"
        ev = why.get("evidence")
        assert isinstance(ev, dict) and \
            isinstance(ev.get("acoustic"), (int, float)), f"{w}: evidence missing"
        fe = why.get("fabric_events")
        assert isinstance(fe, dict), f"{w}: fabric record must be an object"
        assert isinstance(fe.get("revivals"), list), f"{w}: revivals missing"
        assert isinstance(fe.get("alternatives_beaten"), list), f"{w}: beaten missing"
        com = fe.get("committed")
        assert isinstance(com, dict) and com.get("tiling"), f"{w}: defeating tiling missing"
        assert com.get("margin") is None or isinstance(com.get("margin"), (int, float)), \
            f"{w}: bad commit margin"
        db = fe.get("defeated_by")
        assert db is None or (isinstance(db, dict) and isinstance(db.get("winner"), list)), \
            f"{w}: bad defeated_by"
        ve = fe.get("evicted")
        assert ve is None or (isinstance(ve, dict) and isinstance(ve.get("liveness"), (int, float))), \
            f"{w}: bad evicted record"
        assert why.get("fabric_note"), f"{w}: fabric_note missing"
    assert saw_alt, f"{uid}: no word shows beaten alternatives"
    print(f"  {uid}: {t['text'][:60]}... "
          f"[words={len(t['words'])} defeated={fs['defeated']} revived={fs['revived']} "
          f"committed={fs['committed']} rescues={fs['late_rescues']}]")
show = [t for t in items if t["utt_id"] == "61-70968-0060"][0]["fabric_summary"]
assert show["revived"] >= 1, "showcase utt must log revival events"
print(f"PASS (b): {nwords} why panels (alternatives, tiling+margin, events); "
      f"showcase revivals={show['revived']} with late_rescues=[] (machinery shown, no answer changed)")
EOF

echo "==> [6/6] correction + byte-identical undo (c)"
CU=61-70968-0060
nice -n 15 env \
  HEARSAY_LM="$LM" HEARSAY_TIER1="$TIER1" HEARSAY_TIER2="$TIER2" \
  "$HC" --workdir "$WORK" fabric-demo --utt $CU --fabric-dir "$BRIDGE" \
  --lattice "$ROOT/data/lattices/test200/$CU.lat.jsonl" --index 8 --word SUIT \
  | tee "$OUT/fabric-demo.log" | grep -E "^(utt|before|applied|after|undo verified|FABRIC-DEMO OK)" | cut -c1-160
grep -q "FABRIC-DEMO OK" "$OUT/fabric-demo.log"
[ -f "$WORK/lm-corrected-$CU.json" ] || { echo "FAIL (c): corrected LM missing" >&2; exit 1; }
cmp "$WORK/fabric-ledger-$CU.before.bin" "$WORK/fabric-ledger-$CU.after.bin"
echo "PASS (c): correction/undo round-trip restores byte-identical ledger state (cmp)"
echo "==> informational: re-bridge with the corrected LM"
RB="$OUT/rebridge"
mkdir -p "$RB"
nice -n 15 "$HB" --lattice "$ROOT/data/lattices/test200/$CU.lat.jsonl" \
  --lm "$WORK/lm-corrected-$CU.json" --out "$RB" --arm C --budget 500
if cmp -s "$BRIDGE/$CU.transcript.txt" "$RB/$CU.transcript.txt"; then
  echo "re-bridge with corrected LM: transcript UNCHANGED (commit stands; acoustic-dominated region)"
else
  echo "re-bridge with corrected LM: transcript CHANGED"
  diff "$BRIDGE/$CU.transcript.txt" "$RB/$CU.transcript.txt" || true
fi

echo ""
echo "GATE GREEN: fabric-path submission demo verified in $OUT"
echo "  (a) transcripts from fabric commits (1-best falsified)"
echo "  (b) why panels: alternatives, defeating tiling + margin, DecisionEvent log"
echo "  (c) correction/undo byte-identical (cmp)"
