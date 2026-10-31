#!/bin/sh
# GO 2026-10-31 privacy guard: fails if a personal or corpora artifact is
# git-tracked. Wired into `cargo test -p corpus` via
# tests/personal_guard_tests.rs; also runnable by hand:
#   sh scripts/personal_guard.sh
set -u
root=$(git rev-parse --show-toplevel) || exit 1
# The two guard files name themselves; allowlist them exactly.
tracked=$(git -C "$root" ls-files | grep -v -x -e scripts/personal_guard.sh -e src/corpus/tests/personal_guard_tests.rs)
fail=0
for pat in 'owner-book' 'hearsay-personal' 'personal' 'data/librispeech/' 'data/lm-text/' 'Destroyer'; do
  hits=$(printf '%s\n' "$tracked" | grep -F "$pat" || true)
  if [ -n "$hits" ]; then
    echo "PERSONAL-GUARD-FAIL: tracked paths matching '$pat':"
    printf '%s\n' "$hits"
    fail=1
  fi
done
# .arpa / .lm.bin models must never be tracked (may embed personal LM counts).
lmfiles=$(printf '%s\n' "$tracked" | grep -E '\.(arpa|lm\.bin)$' || true)
if [ -n "$lmfiles" ]; then
  echo 'PERSONAL-GUARD-FAIL: tracked LM files:'
  printf '%s\n' "$lmfiles"
  fail=1
fi
# HEARSAY_PERSONAL_TEXT must resolve outside the repo.
if [ -n "${HEARSAY_PERSONAL_TEXT:-}" ]; then
  case "$(cd "${HEARSAY_PERSONAL_TEXT}" 2>/dev/null && pwd)/" in
    "$root/"*) echo 'PERSONAL-GUARD-FAIL: HEARSAY_PERSONAL_TEXT inside repo'; fail=1;;
  esac
fi
if [ "$fail" -eq 0 ]; then
  echo PERSONAL-GUARD-OK
fi
exit "$fail"
