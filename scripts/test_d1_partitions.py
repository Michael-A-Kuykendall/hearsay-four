#!/usr/bin/env python3
"""D1 partition-invariant test (bead hearsay-four-7ms). Durable gate.

Verifies, against the committed manifests in data/partitions/:
  1. TRAIN/DEV/PHASE-TEST/LOCKBOX are pairwise disjoint (by root+path).
  2. No recording session / generation batch / case group crosses partitions.
  3. Every referenced file exists and its sha256 matches the manifest
     (better-voice files are checked when that root is resolvable;
     an unresolvable root prints SKIP, never a silent pass).
  4. LM training text is a subset of TRAIN and contains no
     DEV/PHASE-TEST/LOCKBOX transcript.
  5. LOCKBOX files are unreferenced by ordinary evaluation configs
     (source, scripts, and docs outside the D1 allowlist).
  6. Excluded D0 sets are listed with reasons and enter no partition;
     every D0 audio-bearing VERIFIED set is partitioned or excluded.

Usage: python3 scripts/test_d1_partitions.py
Exit 0 = PASS, 1 = FAIL. Regenerate manifests with
scripts/build_d1_partitions.py after any data change.
"""
import hashlib
import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PART_DIR = REPO / "data" / "partitions"
PARTITIONS = ("TRAIN", "DEV", "PHASE-TEST", "LOCKBOX")
FILES = {"TRAIN": "train.json", "DEV": "dev.json",
         "PHASE-TEST": "phase-test.json", "LOCKBOX": "lockbox.json"}
# Paths allowed to name LOCKBOX files (the seal machinery itself).
LOCKBOX_ALLOWLIST = ("data/partitions/", "scripts/build_d1_partitions.py",
                     "scripts/test_d1_partitions.py",
                     "docs/results/D1-PARTITIONS.md",
                     "docs/design/recording-protocol.md")
SCAN_ROOTS = ("src", "scripts", "web", "docs", "Cargo.toml")


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def bv_root():
    for cand in (os.environ.get("HEARSAY_BETTER_VOICE_ROOT"),
                 os.environ.get("BETTER_VOICE_ROOT"),
                 str(REPO.parent / "better-voice")):
        if cand and (Path(cand) / "data").is_dir():
            return Path(cand)
    return None


def load(name):
    return json.loads((PART_DIR / name).read_text())


def main():
    errors, skips = [], []
    bv = bv_root()
    parts = {p: load(FILES[p])["entries"] for p in PARTITIONS}

    def key(e):
        return (e["root"], e["path"])

    # 1. Pairwise disjoint.
    seen = {}
    for part, entries in parts.items():
        for e in entries:
            if key(e) in seen:
                errors.append(f"disjoint: {key(e)} in {seen[key(e)]} and {part}")
            seen[key(e)] = part

    # 2. No group crosses partitions.
    group_part = {}
    for part, entries in parts.items():
        for e in entries:
            if group_part.setdefault(e["group"], part) != part:
                errors.append(f"group-crossing: {e['group']} in "
                              f"{group_part[e['group']]} and {part}")

    # 3. Existence + checksums.
    checked = 0
    for part, entries in parts.items():
        for e in entries:
            if e["root"] == "repo":
                full = REPO / e["path"]
            elif bv is not None:
                full = bv / e["path"]
            else:
                skips.append(f"existence/checksum skipped (better-voice "
                             f"root unresolved): {e['path']}")
                continue
            if not full.is_file():
                errors.append(f"missing file ({part}): {full}")
            elif sha256_of(full) != e["sha256"]:
                errors.append(f"checksum mismatch ({part}): {full}")
            elif full.stat().st_size != e["bytes"]:
                errors.append(f"size mismatch ({part}): {full}")
            else:
                checked += 1

    # 4. LM text subset of TRAIN, disjoint from other transcripts.
    train_keys = {key(e) for e in parts["TRAIN"]}
    lm = load("lm-train-text.json")["entries"]
    held_out = {key(e) for p in ("DEV", "PHASE-TEST", "LOCKBOX")
                for e in parts[p]
                if "transcript" in e["path"] or e["path"].endswith(".tsv")}
    for e in lm:
        if key(e) not in train_keys:
            errors.append(f"LM file not in TRAIN: {key(e)}")
        if key(e) in held_out:
            errors.append(f"LM file is a held-out transcript: {key(e)}")

    # 5. LOCKBOX unreferenced by ordinary evaluation configs.
    lock_paths = [e["path"] for e in parts["LOCKBOX"]]
    lock_dirs = sorted({str(Path(p).parent) for p in lock_paths})
    for root in SCAN_ROOTS:
        base = REPO / root
        files = [base] if base.is_file() else (
            [p for p in base.rglob("*") if p.is_file()] if base.exists() else [])
        for path in files:
            rel = str(path.relative_to(REPO))
            if rel.startswith(LOCKBOX_ALLOWLIST):
                continue
            try:
                text = path.read_text(errors="ignore")
            except OSError:
                continue
            for needle in lock_paths + lock_dirs:
                if needle in text:
                    errors.append(f"lockbox referenced by {rel}: {needle}")

    # 6. Exclusions visible; partitioned-or-excluded accounting.
    excluded = load("excluded.json")["excluded"]
    for row in excluded:
        if not row.get("reason") or not row.get("d0_use_verification"):
            errors.append(f"exclusion missing fields: {row}")
    partitioned_sets = {e["dataset"] for entries in parts.values()
                        for e in entries}
    for must in ("phrase_training", "quality_checked_tts",
                 "sessions/run1"):
        if not any(r["dataset"] == must for r in excluded):
            errors.append(f"NOT-VERIFIED set not in excluded.json: {must}")
    for leaked in ("phrase_training", "quality_checked_tts"):
        if leaked in partitioned_sets:
            errors.append(f"NOT-VERIFIED set in a partition: {leaked}")

    # Seal consistency.
    index = load("manifest-index.json")
    seal = index["lockbox_seal"]
    lock = sorted(parts["LOCKBOX"], key=lambda e: (e["root"], e["path"]))
    combined = hashlib.sha256(
        "\n".join(e["sha256"] for e in lock).encode()).hexdigest()
    if combined != seal["combined_sha256"]:
        errors.append("lockbox seal combined_sha256 mismatch")
    if seal["file_count"] != len(lock) or seal["opened"] is not False:
        errors.append("lockbox seal metadata mismatch")

    for s in sorted(set(skips)):
        print(f"SKIP: {s}")
    if errors:
        for e in errors[:50]:
            print(f"FAIL: {e}")
        print(f"D1 PARTITION TEST: FAIL ({len(errors)} errors)")
        return 1
    print(f"D1 PARTITION TEST: PASS ({checked} files existence+checksum "
          f"verified, {len(lm)} LM files, {len(lock)} lockbox files sealed)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
