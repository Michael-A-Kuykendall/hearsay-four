#!/usr/bin/env bash
# Per-worktree CARGO_TARGET_DIR (bead hearsay-four-xk1k--per-worktree-target-dirs).
#
# 2026-11-04 incident: main and a lane shared $HOME/.cargo/target-shared.
# Cargo fingerprints do not distinguish same-package different-source
# worktrees, so `cargo build` in main reported "Finished" without rebuilding
# and the lane's stale cell_decode failed every headline shard until a manual
# purge (cargo clean -p did NOT fix it).
#
# Usage: `. "$ROOT/scripts/cargo_target.sh"` after ROOT is set. If
# CARGO_TARGET_DIR is already set (lane workers, CI), it is respected.
# Otherwise a per-worktree dir under $HOME/.cargo/targets/ is derived from
# the worktree toplevel (basename + 12-hex path hash), so main and every lane
# build in isolation with no manual purge.
#
# This file is sourced, not executed.
if [ -z "${CARGO_TARGET_DIR:-}" ]; then
    _ct_root="${ROOT:-$(pwd)}"
    if _ct_git="$(git -C "$_ct_root" rev-parse --show-toplevel 2>/dev/null)"; then
        _ct_root="$_ct_git"
    fi
    _ct_hash="$(printf '%s' "$_ct_root" | sha256sum | cut -c1-12)"
    _ct_base="$(basename "$_ct_root")"
    _ct_base="$(printf '%s' "$_ct_base" | tr -c 'A-Za-z0-9_-' '_')"
    export CARGO_TARGET_DIR="$HOME/.cargo/targets/hearsay-four-${_ct_base}-${_ct_hash}"
    unset _ct_root _ct_git _ct_hash _ct_base
fi
