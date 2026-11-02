#!/bin/bash
# Gate for bead hearsay-four-tcv2--demo-web-ui (Lane I).
# Serves web/tcv2-demo.html over http and validates the gate:
#   1. one command serves the page (HTTP 200)
#   2. all six why-panel elements render data for every fixture word
#
# Usage: bash web/tcv2-gate.sh [port]   (from the repo root; exit 0 on PASS)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PORT="${1:-8123}"
PAGE="web/tcv2-demo.html"

echo "==> [1/3] serving $PAGE on :$PORT"
python3 -m http.server "$PORT" >/dev/null 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null || true' EXIT
sleep 1

echo "==> [2/3] fetching page (expect HTTP 200)"
CODE="$(curl -s -o /tmp/tcv2-gate-page.html -w '%{http_code}' "http://127.0.0.1:$PORT/$PAGE")"
[ "$CODE" = "200" ] || { echo "FAIL: HTTP $CODE for $PAGE"; exit 1; }
echo "PASS: HTTP 200, $(wc -c < /tmp/tcv2-gate-page.html) bytes"

echo "==> [3/3] validating six panel elements on the fixture transcript"
python3 - /tmp/tcv2-gate-page.html <<'EOF'
import json, math, re, sys

html = open(sys.argv[1]).read()

# Six panel sections must exist as render targets (data-sec markers).
for sec in ["alternatives", "scores", "lm", "lexicon", "why", "correct"]:
    assert f'data-sec="{sec}"' in html, f"panel section missing: {sec}"
assert "Correct to" in html and ">Undo<" in html, "Correct/Undo button labels missing"
assert 'id="drop"' in html and 'id="file"' in html, "file drop / picker missing"
assert 'id="sentence"' in html and 'chipHTML' in html, "clickable transcript missing"
print("PASS: page carries transcript chips, file drop, and all six data-sec panel sections")

# Embedded fixture must parse, and every word must carry all six elements.
m = re.search(r'<script type="application/json" id="tcv2-fixture">\s*(\{.*?\})\s*</script>',
              html, re.S)
assert m, "embedded fixture block missing"
fix = json.loads(m.group(1))
words = fix["words"]
assert len(words) >= 6, f"want >=6 fixture words, got {len(words)}"

for w in words:
    why = w.get("why") or {}
    tag = w.get("text")
    # 1+2. alternatives with scores
    alts = why.get("alternatives")
    assert isinstance(alts, list) and alts, f"{tag}: alternatives missing/empty"
    for a in alts:
        assert isinstance(a.get("text"), str) and a["text"], f"{tag}: alt text bad"
        assert isinstance(a.get("score"), (int, float)) and math.isfinite(a["score"]), \
            f"{tag}: alt score bad"
    # 2. scores block: finite acoustic score + note
    assert isinstance(why.get("acoustic_score"), (int, float)) and \
        math.isfinite(why["acoustic_score"]), f"{tag}: acoustic_score bad"
    assert why.get("acoustic_note"), f"{tag}: acoustic_note missing"
    # 3. LM probability in (0, 1] + 2-word history
    p = why.get("lm_probability")
    assert isinstance(p, (int, float)) and 0.0 < p <= 1.0, f"{tag}: lm_probability bad"
    assert isinstance(why.get("lm_history"), list) and len(why["lm_history"]) == 2, \
        f"{tag}: lm_history bad"
    # 4. lexicon source tag
    assert isinstance(why.get("lexicon_source"), str) and why["lexicon_source"], \
        f"{tag}: lexicon_source missing"
    # 5. plain-English why sentence slot
    assert isinstance(why.get("why_sentence"), str) and len(why["why_sentence"]) > 20, \
        f"{tag}: why_sentence missing/too short"
    # 6. Correct/Undo support: word text + >=1 alternative to correct to
    assert w.get("text"), f"{tag}: word text missing"

print(f"PASS: {len(words)} fixture words, every word carries all six panel elements")
print("  " + " ".join(w["text"] for w in words))
EOF

echo ""
echo "TCV2-DEMO GATE GREEN: one command serves the page; six panel elements render."
echo "View: python3 -m http.server 8000  # then /web/tcv2-demo.html"
