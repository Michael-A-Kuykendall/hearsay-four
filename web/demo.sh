#!/bin/bash
# Fabric-path demo (bead hearsay-four-4lsp--fabric-path-demo, extended by
# hearsay-four-su00--demo-video-release): bridge handoff + fabric why panels
# on the experiment fixture population (u1-u4, including the u1 late rescue
# of `cat`). No manual steps: generates bridge outputs, renders transcripts
# with fabric why panels + utterance-level DecisionEvent summaries (revived /
# defeated+margin / evicted / pruned / committed), validates structure +
# revival + summary counts, exercises the correction entry point on the
# fabric path, and checks the click-a-word viewer wiring.
#
# Binaries run via `cargo run --release` (no hardcoded target dir): correct
# under both the shared-target config and the standard layout.
# Externals: why-panel LM/lexicon enrichment reads HEARSAY_LM/HEARSAY_TIER1/
# HEARSAY_TIER2 (or the repo-relative defaults on a checkout with externals).
# The bridge itself runs the fixture scorer — no model files.
#
# Usage: bash web/demo.sh   (from the repo root; exit 0 on full PASS)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
OUT="$ROOT/web/demo-out"
BRIDGE="$OUT/bridge"
mkdir -p "$BRIDGE"

echo "==> [1/6] building demo binaries (release)"
cargo build --release -p experiment --example fabric_demo_inputs
cargo build --release -p runner --bin hearsay-transcribe --bin hearsay-correct

echo "==> [2/6] generating bridge handoff (arm-C fixture decode)"
cargo run --release -q -p experiment --example fabric_demo_inputs -- "$BRIDGE"

echo "==> [3/6] rendering fabric transcripts"
cargo run --release -q -p runner --bin hearsay-transcribe -- \
  --fabric-input "$BRIDGE/u1-revival.lat.jsonl" \
  --fabric-input "$BRIDGE/u2-clean.lat.jsonl" \
  --fabric-input "$BRIDGE/u3-bulk.lat.jsonl" \
  --fabric-input "$BRIDGE/u4-absent.lat.jsonl" \
  --fabric-dir "$BRIDGE" --json --out-dir "$OUT" \
  > "$OUT/demo-transcripts.json"

echo "==> [4/6] validating transcripts + fabric why panels"
python3 - "$OUT/demo-transcripts.json" <<'EOF'
import json, math, sys

path = sys.argv[1]
items = json.load(open(path))
assert isinstance(items, list) and len(items) == 4, f"want 4 transcripts, got {len(items)}"
want_ids = ["u1-revival", "u2-clean", "u3-bulk", "u4-absent"]
got_ids = [t.get("utt_id") for t in items]
assert got_ids == want_ids, f"utt ids {got_ids} != fixture list {want_ids}"

nwords = 0
for t in items:
    uid = t.get("utt_id")
    assert t.get("path") == "fabric", f"{uid}: path != fabric"
    assert "disclosed as chosen" in t.get("chosen", ""), f"{uid}: chosen label missing"
    assert t.get("text"), f"{uid}: empty transcript text"
    words = t.get("words")
    assert words, f"{uid}: no words"
    for w in words:
        nwords += 1
        why = w.get("why")
        assert why, f"{uid} {w.get('text')}: why panel missing"
        # 1. acoustic score: finite number + note
        assert isinstance(why.get("acoustic_score"), (int, float)) and \
            math.isfinite(why["acoustic_score"]), f"{w}: bad acoustic_score"
        assert why.get("acoustic_note"), f"{w}: acoustic_note missing"
        # 2. losing alternatives + why they lost
        assert isinstance(why.get("alternatives"), list), f"{w}: alternatives missing"
        assert why.get("alternatives_note"), f"{w}: alternatives_note missing"
        # 3. LM probability in (0, 1] + 2-word history
        p = why.get("lm_probability")
        assert isinstance(p, (int, float)) and 0.0 < p <= 1.0, f"{w}: bad lm_probability {p}"
        assert isinstance(why.get("lm_history"), list) and \
            len(why["lm_history"]) == 2, f"{w}: bad lm_history"
        # 4. lexicon source tag (fixture words are all cmudict)
        assert why.get("lexicon_source") in ("cmudict", "librispeech-g2p"), \
            f"{w}: bad lexicon_source"
        # 5. per-class evidence the bridge saw
        ev = why.get("evidence")
        assert isinstance(ev, dict) and \
            isinstance(ev.get("acoustic"), (int, float)), f"{w}: evidence missing"
        # 6. fabric record: object, never TBD
        fe = why.get("fabric_events")
        assert isinstance(fe, dict), f"{w}: fabric record must be an object, got {fe!r}"
        assert isinstance(fe.get("revivals"), list), f"{w}: revivals missing"
        assert isinstance(fe.get("alternatives_beaten"), list), f"{w}: beaten missing"
        com = fe.get("committed")
        assert isinstance(com, dict) and com.get("tiling"), f"{w}: defeating tiling missing"
        assert com.get("margin") is None or isinstance(com.get("margin"), (int, float)), f"{w}: bad commit margin"
        # 7. defeat outcome + eviction record: null-or-shaped, never TBD
        db = fe.get("defeated_by")
        assert db is None or (isinstance(db, dict) and isinstance(db.get("winner"), list)), f"{w}: bad defeated_by"
        if isinstance(db, dict):
            assert db.get("margin") is None or isinstance(db.get("margin"), (int, float)), f"{w}: bad defeat margin"
        ev = fe.get("evicted")
        assert ev is None or (isinstance(ev, dict) and isinstance(ev.get("liveness"), (int, float)) and ev.get("budget")), f"{w}: bad evicted record"
        assert why.get("fabric_note"), f"{w}: fabric_note missing"

# The late rescue: u1 commits `cat`, revived via `sat`, beating `bat`.
u1 = items[0]
assert u1["text"] == "the cat sat down", f"u1 text {u1['text']!r}"
cat = [w for w in u1["words"] if w["text"] == "cat"]
assert len(cat) == 1, "u1 must commit cat exactly once"
rev = cat[0]["why"]["fabric_events"]["revivals"]
assert len(rev) == 1 and rev[0]["via"] == "sat", f"u1 cat revival {rev!r}"
assert "bat" in cat[0]["why"]["alternatives"], "u1 cat must show bat beaten"

# Utterance-level DecisionEvent summaries (su00). Falsification: a demo
# reading the 1-best path emits no fabric_summary, so every assert below
# fails on decoder transcripts.
for t in items:
    fs = t.get("fabric_summary")
    assert isinstance(fs, dict), f"{t.get('utt_id')}: fabric_summary missing (1-best path?)"
    for key in ("revived", "defeated", "evicted", "pruned", "committed"):
        assert isinstance(fs.get(key), int) and fs[key] >= 0, f"{t['utt_id']}: bad summary {key}"
    assert fs["events_total"] == fs["revived"] + fs["defeated"] + fs["evicted"] + fs["pruned"] + fs["committed"], \
        f"{t['utt_id']}: summary counts do not add up"
    assert isinstance(fs.get("late_rescues"), list), f"{t['utt_id']}: late_rescues missing"
    assert fs["committed"] >= 1, f"{t['utt_id']}: nothing committed"
u1s = items[0]["fabric_summary"]
assert u1s["revived"] >= 1 and u1s["defeated"] >= 1, f"u1 summary {u1s!r}"
assert u1s["late_rescues"], "u1 must name its late rescue in the summary"
u3s = items[2]["fabric_summary"]
assert u3s["evicted"] >= 1, f"u3 bulk must bind evictions, got {u3s!r}"

print(f"PASS: 4 fixture transcripts, {nwords} words, fabric record on every word")
for t in items:
    fs = t["fabric_summary"]
    print(f"  {t['utt_id']}: {t['text']} "
          f"[events={fs['events_total']} revived={fs['revived']} defeated={fs['defeated']} "
          f"evicted={fs['evicted']} committed={fs['committed']} rescues={fs['late_rescues']}]")
print("PASS: u1 late rescue verified (cat revived via sat, beats bat)")
print("PASS: fabric summaries verified (u1 revival+defeat, u3 evictions, counts add up)")
EOF

echo "==> [5/6] correction entry point reads the fabric path"
cargo run --release -q -p runner --bin hearsay-correct -- \
  decode --utt u1-revival --fabric-dir "$BRIDGE" \
  --lattice "$BRIDGE/u1-revival.lat.jsonl" > "$OUT/correct-fabric.txt"
grep -q "the cat sat down" "$OUT/correct-fabric.txt"
echo "PASS: hearsay-correct decode reads bridge outputs (u1 hyp verified)"

echo "==> [6/6] checking viewer wiring"
grep -q "demo-out/demo-transcripts.json" "$ROOT/web/transcribe.html"
echo "PASS: web/transcribe.html loads demo-out/demo-transcripts.json"

echo ""
echo "DEMO GREEN: fabric transcripts + why panels for u1-u4 in $OUT"
echo "To view the click-a-word why panel:"
echo "  python3 -m http.server 8000   # then open http://localhost:8000/web/transcribe.html"
