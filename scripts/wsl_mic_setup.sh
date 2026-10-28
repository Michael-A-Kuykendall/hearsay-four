#!/usr/bin/env bash
#
# wsl_mic_setup.sh -- fail-fast shared-microphone setup for WSL2 + WSLg.
#
# Run from this repo, in an admin shell:
#     sudo bash scripts/wsl_mic_setup.sh
# Then restart your agent session.
#
# What it does:
#   1. Installs audio CLIENT tools only (no daemons started, no Windows
#      settings changed, mic is NOT stolen from Windows -- WSLg RDP
#      redirection shares it).
#   2. Lists sinks/sources from the WSLg PulseAudio server.
#   3. Records a 3-second mono 16 kHz test clip to /tmp/wsl_mic_test.wav.
#   4. Prints PASS/FAIL with the exact next step.
#
# Safe to re-run. Exit 0 = mic captured audio, 1 = setup ran but no
# usable source, 2 = must run as root.

set -u
export DEBIAN_FRONTEND=noninteractive

fail() { echo "FAIL: $1"; exit "${2:-1}"; }
info() { echo "--- $1"; }

[ "$(id -u)" -eq 0 ] || fail "run as root: sudo bash scripts/wsl_mic_setup.sh" 2

# sudo strips PULSE_SERVER from the environment; WSLg's server lives at a
# fixed socket path, so restore it explicitly when present.
if [ -S /mnt/wslg/PulseServer ]; then
  export PULSE_SERVER="unix:/mnt/wslg/PulseServer"
fi

info "environment"
uname -r
echo "user: $(id -un) (SUDO_USER=${SUDO_USER:-<none>})"
echo "PULSE_SERVER=${PULSE_SERVER:-<unset>}"
ls -la /mnt/wslg/PulseServer /mnt/wslg/PulseAudioRDPSink /mnt/wslg/PulseAudioRDPSource 2>&1 || true

info "installing client tools (pulseaudio-utils, alsa-utils)"
apt-get update && apt-get install -y pulseaudio-utils alsa-utils \
  || fail "apt install failed (check network / admin rights)"

info "server"
pactl info > /tmp/wsl_pulse_info.txt 2>&1 \
  || fail "cannot talk to PulseServer at ${PULSE_SERVER:-<unset>}. From Windows PowerShell run: wsl --shutdown ; reopen Ubuntu ; re-run this script."
head -12 /tmp/wsl_pulse_info.txt

info "sinks"
pactl list short sinks || true
info "sources"
pactl list short sources || true

SRC="$(pactl get-default-source 2>/dev/null || true)"
case "$SRC" in *.monitor|"")
  SRC="$(pactl list short sources 2>/dev/null | grep -v '\.monitor$' | head -n 1 | cut -f2)" ;;
esac
[ -n "$SRC" ] || fail "no microphone source exposed by WSLg. On Windows: Settings > Privacy > Microphone > let desktop apps access it; update WSL (wsl --update); then re-run."

echo "capture source: $SRC"
RAW=/tmp/wsl_mic_test.raw
WAV=/tmp/wsl_mic_test.wav
rm -f "$RAW" "$WAV"
info "recording 3s test clip (speak now)"
timeout 5 parec --device="$SRC" --rate=16000 --channels=1 --format=s16le > "$RAW" &
REC=$!
sleep 3
kill "$REC" 2>/dev/null || true
wait "$REC" 2>/dev/null || true

BYTES=$(stat -c%s "$RAW" 2>/dev/null || echo 0)
echo "captured bytes: $BYTES (expect ~96000)"
[ "$BYTES" -gt 48000 ] || fail "capture too short ($BYTES bytes) -- mic may be muted or exclusively held"

python3 - "$RAW" "$WAV" <<'EOF'
import sys, wave
raw, wav = sys.argv[1], sys.argv[2]
data = open(raw, 'rb').read()
w = wave.open(wav, 'wb')
w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
w.writeframes(data); w.close()
print('wrote', wav, len(data)//2, 'frames')
EOF

echo "PASS: shared mic works. Test clip: $WAV (copy it to Windows to listen)."
echo "Nothing was reconfigured on the Windows side; the mic stays available there."
exit 0
