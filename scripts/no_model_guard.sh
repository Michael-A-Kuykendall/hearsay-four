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
if grep -rEi '(onnxruntime|tract-pulse|candle::|tch::|burn::|hf_hub|from_pretrained|load_model|safetensors::)' --include='*.rs' "$REPO/src" 2>/dev/null; then
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

if [ "$fail" -eq 0 ]; then
  echo "NO-MODEL GUARD GREEN: no ML crates, weight files, model APIs, or STT-in-scoring-path."
fi
exit "$fail"
