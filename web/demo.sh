#!/bin/bash
# One-command lane-D demo: file transcription + why-panel data on the fixed
# demo file list (bead hearsay-four-wgit--why-panel-demo-script). No manual
# steps: builds, transcribes all fixed files, validates the transcripts and
# the per-word why panels, and points at the click-a-word viewer.
#
# Usage: bash web/demo.sh   (from the repo root; exit 0 on full PASS)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
OUT="$ROOT/web/demo-out"
mkdir -p "$OUT"

echo "==> [1/4] building hearsay-transcribe (release)"
cargo build --release -p runner --bin hearsay-transcribe

echo "==> [2/4] transcribing fixed demo files"
./target/release/hearsay-transcribe --demo --json --out-dir "$OUT" \
  > "$OUT/demo-transcripts.json"

echo "==> [3/4] validating transcripts + why panels"
python3 - "$OUT/demo-transcripts.json" <<'EOF'
import json, math, sys

path = sys.argv[1]
items = json.load(open(path))
assert isinstance(items, list) and len(items) == 3, f"want 3 transcripts, got {len(items)}"
want_ids = ["777-126732-0081", "2035-147960-0015", "1919-142785-0021"]
got_ids = [t.get("utt_id") for t in items]
assert got_ids == want_ids, f"utt ids {got_ids} != fixed list {want_ids}"

nwords = 0
for t in items:
    assert t.get("text"), f"{t.get('utt_id')}: empty transcript text"
    words = t.get("words")
    assert words, f"{t.get('utt_id')}: no words"
    for w in words:
        nwords += 1
        why = w.get("why")
        assert why, f"{t['utt_id']} {w.get('text')}: why panel missing"
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
        # 4. lexicon source tag (decoded words always carry one)
        assert why.get("lexicon_source") in ("cmudict", "librispeech-g2p"), \
            f"{w}: bad lexicon_source"
        # 5. fabric events: the ONLY permitted TBD
        assert why.get("fabric_events") == "TBD", f"{w}: fabric_events must be explicit TBD"
        assert why.get("fabric_note"), f"{w}: fabric_note missing"

print(f"PASS: 3 files, {nwords} words, every word carries all five why elements")
for t in items:
    print(f"  {t['utt_id']}: {t['text']}")
EOF

echo "==> [4/4] checking viewer wiring"
grep -q "demo-out/demo-transcripts.json" "$ROOT/web/transcribe.html"
echo "PASS: web/transcribe.html loads demo-out/demo-transcripts.json"

echo ""
echo "DEMO GREEN: transcripts + why-panel data for all fixed files in $OUT"
echo "To view the click-a-word why panel:"
echo "  python3 -m http.server 8000   # then open http://localhost:8000/web/transcribe.html"
