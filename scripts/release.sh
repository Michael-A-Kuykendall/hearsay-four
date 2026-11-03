#!/usr/bin/env bash
# CLI release matrix (bead hearsay-four-dyy): build --release --bin hearsay-demo,
# run the JSON smoke on data/demo/sample.wav, and archive the no-model guard
# proof plus sha256 checksums into dist/.
#
# Usage: bash scripts/release.sh --target host               (this machine; gate leg)
#        bash scripts/release.sh --target linux               (x86_64-unknown-linux-gnu)
#        bash scripts/release.sh --target mac                 (aarch64-apple-darwin)
#        bash scripts/release.sh --target windows             (x86_64-pc-windows-gnu)
# A bare Rust target triple is also accepted: --target <triple>.
#
# The host/Linux legs run anywhere with the toolchain; the macOS/Windows legs
# are documented here and executed verbatim in owner sessions (no Apple/Windows
# toolchains in this environment). Cross-leg evidence is recorded under bead
# hearsay-four-say during submission review.
#
# Outputs in dist/: smoke.json (JSON smoke), no_model_guard.txt (guard proof),
# SHA256SUMS (checksums of the release binary + both proof files).
set -uo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

TARGET_ALIAS="${1:-}"
TARGET_ARG="${2:-}"
if [ "$TARGET_ALIAS" = "--target" ]; then
  WANT="$TARGET_ARG"
else
  echo "usage: bash scripts/release.sh --target host|linux|mac|windows|<triple>"
  exit 2
fi

case "$WANT" in
  host) TRIPLE="" ;;
  linux|x86_64-unknown-linux-gnu) TRIPLE="x86_64-unknown-linux-gnu" ;;
  mac|aarch64-apple-darwin) TRIPLE="aarch64-apple-darwin" ;;
  windows|x86_64-pc-windows-gnu) TRIPLE="x86_64-pc-windows-gnu" ;;
  *) TRIPLE="$WANT" ;;
esac

echo "== versions =="
rustc --version
cargo --version
rustup target list --installed 2>/dev/null || true

echo "== build (--release --bin hearsay-demo, target: ${TRIPLE:-host}) =="
TDIR="${CARGO_TARGET_DIR:-target}"
if [ -z "$TRIPLE" ]; then
  cargo build --offline --release --bin hearsay-demo
  BIN="$TDIR/release/hearsay-demo"
else
  cargo build --offline --release --target "$TRIPLE" --bin hearsay-demo
  BIN="$TDIR/$TRIPLE/release/hearsay-demo"
fi
case "$BIN" in
  *windows*) BIN="$BIN.exe" ;;
esac
test -x "$BIN" || { echo "release: binary missing at $BIN"; exit 1; }
echo "binary: $BIN"

mkdir -p dist
FIXTURE="data/demo/sample.wav"
echo "== human smoke ($FIXTURE) =="
"$BIN" --input "$FIXTURE" --max-secs 8

echo "== JSON smoke (-> dist/smoke.json) =="
"$BIN" --input "$FIXTURE" --max-secs 8 --json > dist/smoke.json
python3 -c "import json; d=json.load(open('dist/smoke.json')); assert d['transcript']['words'], 'empty transcript'; print('smoke.json words:', len(d['transcript']['words']))"

echo "== no-model guard (-> dist/no_model_guard.txt) =="
if bash scripts/no_model_guard.sh > dist/no_model_guard.txt 2>&1; then
  cat dist/no_model_guard.txt
  echo "no_model_guard: PASS"
else
  status=$?
  cat dist/no_model_guard.txt
  echo "no_model_guard: FAIL (exit $status)"
  exit 1
fi

echo "== checksums (-> dist/SHA256SUMS) =="
sha256sum "$BIN" dist/smoke.json dist/no_model_guard.txt > dist/SHA256SUMS
cat dist/SHA256SUMS
echo "release: DONE (target: ${TRIPLE:-host})"
