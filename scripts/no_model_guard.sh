#!/usr/bin/env bash
# No-model guard (bead hearsay-four-ivs): proves the Hearsay-IV pipeline is
# model-free. Hearsay-III + saturation fabric, classical techniques only:
# no neural net, transformer, embedding model, or learned-weight artifact
# may enter the pipeline. AI-derived data (CourtListener stt_transcript,
# Oyez turns) is eval/reference material only and must never feed live
# scoring. Fails red (exit 1) on any hit; green prints the verdict line.
set -u
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail=0
hit() { echo "GUARD-RED: $1"; fail=1; }

# 1. ML crates in any manifest under src/.
if grep -rEi '(candle-core|candle-nn|candle-transformers|libtorch|\btch\b|\bburn\b|onnxruntime|\bort\b|tensorflow|hf-hub|tokenizers|safetensors)' --include='Cargo.toml' "$REPO/src" 2>/dev/null; then
  hit "ML crate dependency in a Cargo manifest (see above)"
fi

# 2. Shipped weight files (never model binaries in src/ or data/).
while IFS= read -r f; do
  hit "shipped weight file: $f"
done < <(find "$REPO/src" "$REPO/data" -type f \
  \( -iname '*.onnx' -o -iname '*.safetensors' -o -iname '*.pt' -o -iname '*.pth' -o -iname '*.bin' \) \
  -not -path '*/target/*' 2>/dev/null)

# 3. Model-loading APIs in Rust sources.
if grep -rEi '(onnxruntime|tract-pulse|candle::|\btch::|burn::|hf_hub|from_pretrained|load_model|safetensors::)' --include='*.rs' "$REPO/src" 2>/dev/null; then
  hit "model-loading API usage in Rust sources (see above)"
fi

# 4. AI-derived STT text outside ingestion: the stt_transcript API field may
# appear only under src/corpus (ingestion). Every other crate dir is live
# pipeline or its tests and must not consume it.
for d in "$REPO"/src/*/; do
  case "$d" in
    */corpus/) continue ;;
  esac
  if grep -rEi 'stt_transcript' --include='*.rs' "$d" 2>/dev/null; then
    hit "STT transcript field consumed outside src/corpus: $d (see above)"
  fi
done

# 5. Network-client crates in the service path (bead hearsay-four-aff, E3):
# the E1/E2 service path is offline by construction (no sockets opened,
# vendored dictionary only), so no live-pipeline manifest may depend on a
# direct socket/HTTP client crate. Ingestion is exempt: src/corpus
# (corpus-fetch downloads CourtListener audio at dev time) may use an HTTP
# client, exactly like the STT exemption in section 4. The unified workspace
# root (./Cargo.toml) is scanned too, so a future [workspace.dependencies]
# cannot smuggle a client into the service closure.
NETWORK_CRATES='(reqwest|hyper|\btokio\b|\bureq\b|\bcurl\b|\bminreq\b|\bisahc\b|\bawc\b|\bsurf\b|\bmio\b|\bsocket2\b|\btungstenite\b)'
while IFS= read -r manifest; do
  if grep -Ei "$NETWORK_CRATES" "$manifest" 2>/dev/null; then
    hit "network-client crate in service-path manifest: $manifest (see above)"
  fi
done < <(find "$REPO/src" -path "$REPO/src/corpus" -prune -o -name 'Cargo.toml' -print 2>/dev/null)
if grep -Ei "$NETWORK_CRATES" "$REPO/Cargo.toml" 2>/dev/null; then
  hit "network-client crate in unified workspace deps (see above)"
fi

# 6. Direct socket/HTTP client API use outside ingestion: a manifest is not
# the only way to pull the network in, so Rust sources are scanned for
# client API paths and socket types too. src/corpus is exempt (ingestion,
# same as sections 4-5). Comment-only mentions without the `::` API path
# (e.g. "no sockets are opened") do not match.
while IFS= read -r srcfile; do
  if grep -En '(reqwest::|hyper::|tokio::net|std::net::|TcpStream|TcpListener|UdpSocket|\bmio::|\bsocket2::|\bureq::|\bcurl::|tungstenite::|isahc::|minreq::|\bawc::|\bsurf::)' "$srcfile" 2>/dev/null; then
    hit "network-client API usage outside src/corpus: $srcfile (see above)"
  fi
done < <(find "$REPO/src" -path "$REPO/src/corpus" -prune -o -name '*.rs' -print 2>/dev/null)

if [ "$fail" -eq 0 ]; then
  echo "NO-MODEL GUARD GREEN: no ML crates, weight files, model APIs, STT-in-scoring-path, or network clients in the service path."
fi
exit "$fail"
