#!/usr/bin/env bash
#
# record_phase_test.sh -- interactive 5th-take PHASE-TEST recorder for A1.
#
# Records one fresh owner take of each of the 30 CLOSED_VOCABULARY words
# (docs/design/recording-protocol.md section 3.2, src/fse/src/lib.rs) into a
# new timestamped directory that `a1_gate --phase-test-dir <dir>` scores.
#
# Layout written (exactly what src/fse/src/bin/a1_gate.rs load_session reads):
#   <out>/manifest.tsv    header line + one "<file>\t<word>\t<rate>\t<frames>" row per kept take
#   <out>/utt000.wav ...  one mono PCM16 WAV per kept take (number = prompt position)
# a1_gate skips manifest line 1 unconditionally (lines().skip(1)), so line 1
# MUST be a header, never a take row (run1 relies on this too: its line 1 is
# the enroll row). Take index is per-word occurrence order in the manifest.
# No enroll.wav and no session.tsv are written: a1_gate reads neither from
# the phase-test dir (session.tsv start/stop timing would be meaningless
# for one-utterance-per-file takes, so it is omitted rather than faked).
#
# Sample-rate reconciliation (measured 2026-10-28, not assumed):
#   run1 DEV templates : 48000 Hz mono (WAV headers + manifest.tsv col 3)
#   WSLg mic native    : 44100 Hz mono (pactl: RDPSource s16le 1ch 44100Hz)
#   protocol standard  : 16000 Hz mono (recording-protocol.md section 1)
# Default RATE=48000 matches the run1 DEV templates so MFCC filterbanks
# compare like-for-like in the DTW gate (PulseAudio resamples the 44.1 kHz
# source on request; the true rate is in every WAV header + manifest col 3,
# so nothing is resampled silently). Override with RATE=16000 if run1 is
# ever re-recorded at the protocol rate. The fse front end accepts any
# rate >= 8 kHz, but same-rate templates/queries is the cleaner gate.
#
# Take length is EXACT (head -c byte cap, not wall clock): the RDP source can
# burst buffered audio faster than realtime after idle (measured 6.13s of
# audio in a 1s wall-clock window, 2026-10-28), so wall-clock recording gives
# variable-length takes. A 2s discard warm-up runs before word 1.
#
# Deps: bash + parec (+ pactl for the source check) + python3 + coreutils
# (head/timeout) only.
# Platform: Linux/WSL2 with WSLg (fixed PulseServer socket path below).
#
# Usage (owner, from repo root, quiet room, same mic/room/distance as run1):
#   bash scripts/record_phase_test.sh --check   # 2s silent pipeline check, /tmp only
#   bash scripts/record_phase_test.sh           # interactive 30-word session
#   bash scripts/record_phase_test.sh <out-dir> # explicit dir (must be missing/empty)
# Env overrides: SRC=... RATE=... DUR=... (seconds per take, integer)
# Then, separately and EXACTLY ONCE per fresh dir (a fail means fixing the
# front end on DEV and recording a NEW dir, never re-scoring this one):
#   cargo run -p fse --bin a1_gate -- --run1 sessions/run1 --phase-test-dir <out-dir>
# This script NEVER scores anything.
#
# Safe to re-run: the default out-dir is timestamped, and Ctrl-C keeps the
# partial session valid (manifest + WAVs written so far still score).

set -u
export PULSE_SERVER="${PULSE_SERVER:-unix:/mnt/wslg/PulseServer}"
SRC="${SRC:-RDPSource}"
RATE="${RATE:-48000}"
DUR="${DUR:-3}"
WORDS=(impeachment methodical survival process intense reflection kinship resented congealed uncorroborated highboy hotbed ripple predestine byzantium genetic constitution interpersonal enchantment perfume submissions overboard knockdown sleepwalking unrealized panhandling horribly salivating rhubarb staked)

fail() { echo "FAIL: $1" >&2; exit "${2:-1}"; }
info() { echo "--- $1"; }

# --- toolchain + environment checks (fail fast) ---
command -v parec >/dev/null || fail "parec not found (needs pulseaudio-utils; see scripts/wsl_mic_setup.sh)"
command -v python3 >/dev/null || fail "python3 not found"
[ -S /mnt/wslg/PulseServer ] || fail "no WSLg PulseServer socket at /mnt/wslg/PulseServer (is this WSL2 with WSLg?)"
[ -d sessions ] && [ -d scripts ] || fail "run from the repo root (needs ./sessions and ./scripts)"
if command -v pactl >/dev/null; then
  pactl list short sources 2>/dev/null | cut -f2 | grep -qxF "$SRC" \
    || fail "source '$SRC' not listed by pactl (override with SRC=<name>)"
else
  echo "WARN: pactl missing, skipping source check (capture will fail fast if wrong)"
fi

# --- vocabulary guard: embedded 30 must equal run1 manifest labels ---
RUN1_MANIFEST="sessions/run1/manifest.tsv"
if [ -f "$RUN1_MANIFEST" ]; then
  python3 - "$RUN1_MANIFEST" "${WORDS[*]}" <<'PYEOF' || fail "vocabulary drift: embedded 30-word list != run1 manifest labels"
import sys
manifest, embedded = sys.argv[1], set(sys.argv[2].split())
labels = set()
for i, line in enumerate(open(manifest, encoding='utf-8').read().splitlines()):
    if i == 0:
        continue  # run1 line 1 is the enroll row (a1_gate skips it too)
    parts = line.split('\t')
    if len(parts) >= 2 and parts[0] != 'enroll.wav' and parts[1]:
        labels.add(parts[1])
assert labels == embedded, f"run1={sorted(labels)} embedded={sorted(embedded)}"
print(f"vocab guard OK: embedded 30 == run1 distinct {len(labels)}")
PYEOF
else
  echo "WARN: $RUN1_MANIFEST absent, skipping vocab guard"
fi

# --- capture: raw s16le from parec; echoes bytes; false unless EXACTLY rate*secs*2 ---
# Length is capped by byte count (head -c), not wall clock: the RDP source can
# burst buffered audio faster than realtime after idle, so wall-clock recording
# gives variable-length takes. timeout bounds the wall time if it stalls.
capture_raw() { # $1=rawfile $2=seconds
  local rawfile="$1" secs="$2" exact
  exact=$((RATE * secs * 2))
  timeout $((secs + 10)) parec --device="$SRC" --rate="$RATE" --channels=1 --format=s16le 2>"$rawfile.err" \
    | head -c "$exact" > "$rawfile"
  local bytes
  bytes="$(stat -c%s "$rawfile" 2>/dev/null || echo 0)"
  echo "$bytes"
  [ "$bytes" -eq "$exact" ]
}

# --- wrap: raw -> WAV, re-read header to prove validity; echoes "nframes peak dbfs status" ---
wrap_wav() { # $1=raw $2=wav
  python3 - "$1" "$2" "$RATE" <<'PYEOF'
import math, struct, sys, wave
raw, wav, rate = sys.argv[1], sys.argv[2], int(sys.argv[3])
data = open(raw, 'rb').read()
data = data[:len(data) // 2 * 2]  # whole samples only
w = wave.open(wav, 'wb')
w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
w.writeframes(data); w.close()
r = wave.open(wav, 'rb')  # re-read: prove the file is a valid WAV
assert (r.getnchannels(), r.getsampwidth(), r.getframerate()) == (1, 2, rate), r.getparams()
n = r.getnframes()
peak = abs(max(struct.unpack('<%dh' % n, r.readframes(n)) or [0], key=abs)) / 32768.0
db = 20 * math.log10(peak) if peak > 0 else float('-inf')
status = 'CLIP' if peak >= 0.999 else 'HOT' if peak >= 0.708 else 'QUIET' if peak < 0.02 else 'OK'
print(f'{n} {peak:.3f} {db:.1f} {status}')
PYEOF
}

# --- non-interactive pipeline check: /tmp only, never session data ---
if [ "${1:-}" = "--check" ]; then
  info "2s silent pipeline check (writes /tmp only, scores nothing)"
  raw="$(mktemp /tmp/phase-test-check-XXXXXX.raw)"
  wav="/tmp/phase-test-check.wav"
  bytes="$(capture_raw "$raw" 2)" || fail "capture short ($bytes bytes, need exactly $((RATE * 2 * 2))) -- mic muted, held, or stalled?"
  echo "captured bytes: $bytes (exact $((RATE * 2 * 2)))"
  report="$(wrap_wav "$raw" "$wav")" || fail "WAV wrap/verify failed"
  echo "wrapped: $report (nframes peak dbfs status)"
  python3 - "$wav" <<'PYEOF'  # independent second read of the finished file
import sys, wave
r = wave.open(sys.argv[1])
print(f"verify: ch={r.getnchannels()} sw={r.getsampwidth()} sr={r.getframerate()} frames={r.getnframes()}")
assert (r.getnchannels(), r.getsampwidth()) == (1, 2)
PYEOF
  rm -f "$raw" "$raw.err"
  echo "PASS: pipeline writes valid WAVs. Audible check: $wav"
  exit 0
fi

# --- output dir (fresh timestamped default; explicit must be missing/empty) ---
TS="$(date +%Y%m%d-%H%M%S)"
OUT="${1:-sessions/phase-test-$TS}"
if [ -e "$OUT" ]; then
  [ -d "$OUT" ] || fail "$OUT exists and is not a directory"
  [ -z "$(ls -A "$OUT")" ] || fail "$OUT exists and is not empty (re-run without args for a fresh timestamped dir)"
else
  mkdir -p "$OUT" || fail "cannot create $OUT"
fi
MANIFEST="$OUT/manifest.tsv"
printf 'file\tword\trate_hz\tframes\n' > "$MANIFEST"  # line 1 = header (a1_gate skips it)

trap 'echo; echo "Interrupted. Partial session kept at $OUT ($(($(wc -l < "$MANIFEST") - 1)) takes). Re-run for a fresh dir; this one still scores."; exit 130' INT

record_word() { # $1=word $2=file; returns 1 on skip
  local word="$1" file="$2" wav="$OUT/$2" raw bytes report nframes peak dbfs status ans
  raw="$(mktemp /tmp/phase-test-XXXXXX.raw)"
  while :; do
    echo "  Recording ${DUR}s ... speak NOW:  $word"
    if ! bytes="$(capture_raw "$raw" "$DUR")"; then
      echo "  WARN: capture short ($bytes bytes, need exactly $((RATE * DUR * 2))). Mic muted, held, or stalled?"
      printf '  Enter = retry, s = skip this word: '
      read -r ans || ans="s"
      if [ "$ans" = "s" ]; then rm -f "$raw" "$raw.err"; return 1; fi
      continue
    fi
    echo "  captured bytes: $bytes (exact $((RATE * DUR * 2)))"
    if ! report="$(wrap_wav "$raw" "$wav")"; then
      echo "  WARN: WAV wrap/verify failed; retrying."
      rm -f "$wav"
      printf '  Enter = retry, s = skip this word: '
      read -r ans || ans="s"
      if [ "$ans" = "s" ]; then rm -f "$raw" "$raw.err"; return 1; fi
      continue
    fi
    read -r nframes peak dbfs status <<< "$report"
    echo "  wrote $file: $nframes frames @ ${RATE}Hz mono, peak $peak ($dbfs dBFS) [$status]"
    [ "$status" = "OK" ] || echo "  WARN: level $status (CLIP=too loud, HOT=above -3dBFS, QUIET=check mic)"
    rm -f "$raw" "$raw.err"
    printf '%s\t%s\t%s\t%s\n' "$file" "$word" "$RATE" "$nframes" >> "$MANIFEST"
    printf '  Enter = keep, r = redo this word: '
    read -r ans || ans=""
    if [ "$ans" = "r" ]; then
      rm -f "$wav"
      sed -i '$d' "$MANIFEST"  # drop the row just appended
      raw="$(mktemp /tmp/phase-test-XXXXXX.raw)"
      continue
    fi
    return 0
  done
}

info "session dir: $OUT"
echo "Mic: $SRC @ ${RATE}Hz mono via $PULSE_SERVER | ${#WORDS[@]} words x ${DUR}s"
echo "Lead/trail silence is KEPT (do not trim). Ctrl-C keeps the partial session valid."
info "warming up mic (2s discard capture absorbs wake-up burst/latency)..."
warm="$(mktemp /tmp/phase-test-warm-XXXXXX.raw)"
warmbytes="$(capture_raw "$warm" 2)" || fail "warm-up capture short ($warmbytes bytes) -- mic muted, held, or stalled?"
rm -f "$warm" "$warm.err"
echo "mic ready."
kept=0
skipped=0
idx=0
for word in "${WORDS[@]}"; do
  idx=$((idx + 1))
  file="$(printf 'utt%03d.wav' $((idx - 1)))"
  echo "=== [$idx/${#WORDS[@]}] Say:  $word   (Enter = record ${DUR}s, s = skip, q = quit) ==="
  read -r ans || ans="q"
  case "$ans" in
    s|S) echo "  skipped $word"; skipped=$((skipped + 1)); continue;;
    q|Q) echo "  quitting after $kept kept."; break;;
  esac
  if record_word "$word" "$file"; then kept=$((kept + 1)); else skipped=$((skipped + 1)); fi
done

rows=$(($(wc -l < "$MANIFEST") - 1))
info "done: $kept kept, $skipped skipped; manifest rows: $rows"
echo "Session: $OUT"
ls "$OUT"
echo "Score EXACTLY ONCE (a fail means a NEW dir, never re-scoring this one):"
echo "  cargo run -p fse --bin a1_gate -- --run1 sessions/run1 --phase-test-dir $OUT"
