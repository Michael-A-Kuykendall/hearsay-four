#!/usr/bin/env bash
# Deterministic formal-session fixture builder for bead kyz gates.
# Builds /tmp/good-session: 30 synthetic 16kHz/3.0s WAVs + manifest.tsv +
# order.txt (seed 42) + session.meta + SHA256SUMS.
# Flag --drop-label WORD builds the consistent-but-incomplete variant (word
# omitted from WAVs + manifest; order.txt keeps all 30; hashes recomputed).
# Usage: scripts/fixtures/make_good_session.sh [--drop-label WORD]
# Must run from the repo root (reads prompts/script-wave3.txt).
set -u

[ -f prompts/script-wave3.txt ] || { echo "FAIL: run from the repo root" >&2; exit 1; }
[ "$(wc -l < prompts/script-wave3.txt)" -eq 30 ] || { echo "FAIL: script-wave3.txt must have exactly 30 lines" >&2; exit 1; }
command -v python3 >/dev/null || { echo "FAIL: python3 not found" >&2; exit 1; }
command -v sha256sum >/dev/null || { echo "FAIL: sha256sum not found" >&2; exit 1; }

DROP=""
if [ "${1:-}" = "--drop-label" ]; then
  [ -n "${2:-}" ] || { echo "FAIL: --drop-label needs a WORD" >&2; exit 1; }
  DROP="$2"
fi

OUT=/tmp/good-session
rm -rf "$OUT"
mkdir -p "$OUT"

python3 - "$DROP" <<'PYEOF'
import math, random, struct, sys, wave
drop = sys.argv[1] or None
vocab = [l.strip() for l in open('prompts/script-wave3.txt') if l.strip()]
assert len(vocab) == 30, len(vocab)
if drop is not None:
    assert drop in vocab, f"drop word {drop} not in vocab"
rng = random.Random(42)
order = vocab[:]
rng.shuffle(order)
with open('/tmp/good-session/order.txt', 'w') as f:
    f.write('seed 42\n')
    for w in order:
        f.write(w + '\n')
rate, secs = 16000, 3.0
n = int(rate * secs)
rows = ['file\tword\trate_hz\tframes']
for i, w in enumerate(order):
    if w == drop:
        continue
    fname = f'utt{i:03d}.wav'
    freq = 300.0 + i * 20.0
    with wave.open(f'/tmp/good-session/{fname}', 'wb') as wv:
        wv.setnchannels(1)
        wv.setsampwidth(2)
        wv.setframerate(rate)
        frames = struct.pack(
            '<%dh' % n,
            *(int(8000.0 * math.sin(2.0 * math.pi * freq * k / rate)) for k in range(n)),
        )
        wv.writeframes(frames)
    rows.append(f'{fname}\t{w}\t{rate}\t{n}')
with open('/tmp/good-session/manifest.tsv', 'w') as f:
    f.write('\n'.join(rows) + '\n')
print(f"fixture: {len(rows) - 1} wavs + manifest + order (drop={drop})")
PYEOF

(cd "$OUT" && sha256sum utt*.wav manifest.tsv order.txt > SHA256SUMS)
ORDER_HASH="$(sha256sum "$OUT/order.txt" | cut -d' ' -f1)"
MAN_HASH="$(sha256sum "$OUT/manifest.tsv" | cut -d' ' -f1)"
KEPT="$(($(wc -l < "$OUT/manifest.tsv") - 1))"
cat > "$OUT/session.meta" <<EOF
device=fixture
rate_hz=16000
date_utc=2026-10-29T00:00:00Z
seed=42
order_sha256=$ORDER_HASH
manifest_sha256=$MAN_HASH
recorder_commit=fixture
kept=$KEPT
EOF
echo "fixture ready: $OUT (kept=$KEPT)"
