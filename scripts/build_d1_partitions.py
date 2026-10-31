#!/usr/bin/env python3
"""Build D1 partition manifests (bead hearsay-four-7ms).

Writes committed split manifests under data/partitions/ for every
D0 use-verified set (docs/results/D0-DATA-INVENTORY.md). Splits are by
recording session (owner audio), by generation batch (TTS sets, whole
set to one partition so no utterance family crosses partitions), and
by case chronological (Oyez/SCOTUS). Never by sentence within a
session or case. Re-running reproduces the manifests byte-for-byte
except the seal timestamp; checksums are recomputed from disk, so a
changed source file changes the manifest loudly.

Usage: python3 scripts/build_d1_partitions.py [--better-voice-root PATH]
"""
import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "data" / "partitions"

# dataset -> (partition, group). Group is the indivisible split unit:
# a recording session, a TTS generation batch, or a court case.
BETTER_VOICE_TRAIN = ["bulk_tts_1k", "bulk_tts_1k_trimmed",
                      "quality_checked_1k", "medical_tts", "tts_audio",
                      "phonetics_audio"]
BETTER_VOICE_DEV = ["quality_test", "libri_mini_spaced"]

# Oyez / SCOTUS cases in chronological order -> partition blocks.
# (group, chronological key, [(root, path), ...])
CASES = [
    ("case:70-112_1971", "1971", "TRAIN",
     [("repo", "sessions/oyez-bulk/70-112_1971/session.tsv"),
      ("repo", "sessions/oyez-bulk/70-112_1971/transcript.json")]),
    ("case:10-218_2011", "2011", "TRAIN",
     [("repo", "sessions/oyez-bulk/10-218_2011/session.tsv"),
      ("repo", "sessions/oyez-bulk/10-218_2011/transcript.json"),
      ("repo", "sessions/oyez-pilot/session.tsv"),
      ("repo", "sessions/oyez-pilot/transcript.json")]),
    ("case:24-889_2025", "2025", "DEV",
     [("repo", "sessions/oyez-bulk/24-889_2025/session.tsv"),
      ("repo", "sessions/oyez-bulk/24-889_2025/transcript.json"),
      ("repo", "data/session/session.tsv"),
      ("repo", "data/session/oyez_media_25951.json")]),
    ("case:106765_25-170", "2026-10-05", "PHASE-TEST",
     [("repo", "data/corpus/court/scotus/106765/manifest.json"),
      ("repo", "data/corpus/court/scotus/106765/transcript.json"),
      ("repo", "data/corpus/court/scotus/106765/transcript.txt")]),
    ("case:106764_25-498", "2026-10-06", "PHASE-TEST",
     [("repo", "data/corpus/court/scotus/106764/manifest.json"),
      ("repo", "data/corpus/court/scotus/106764/transcript.json"),
      ("repo", "data/corpus/court/scotus/106764/transcript.txt")]),
    ("case:106804_25-579", "2026-10-07", "PHASE-TEST",
     [("repo", "data/corpus/court/scotus/106804/manifest.json"),
      ("repo", "data/corpus/court/scotus/106804/transcript.json"),
      ("repo", "data/corpus/court/scotus/106804/transcript.txt")]),
    ("case:106844_25-5343", "2026-10-13", "LOCKBOX",
     [("repo", "data/corpus/court/scotus/106844/manifest.json"),
      ("repo", "data/corpus/court/scotus/106844/transcript.json"),
      ("repo", "data/corpus/court/scotus/106844/transcript.txt")]),
    ("case:106874_25-459", "2026-10-14", "LOCKBOX",
     [("repo", "data/corpus/court/scotus/106874/manifest.json"),
      ("repo", "data/corpus/court/scotus/106874/transcript.json"),
      ("repo", "data/corpus/court/scotus/106874/transcript.txt")]),
]

# LM training text: TRAIN-partition transcript files only.
LM_FILES = [
    ("better-voice", "data/bulk_tts_1k/train_1k.tsv"),
    ("better-voice", "data/bulk_tts_1k/val_1k.tsv"),
    ("better-voice", "data/quality_checked_1k/train.tsv"),
    ("better-voice", "data/quality_checked_1k/val.tsv"),
    ("better-voice", "data/medical_tts/training_manifest.tsv"),
    ("better-voice", "data/phonetics_audio/phonetics_training_manifest.tsv"),
    ("repo", "sessions/oyez-bulk/70-112_1971/transcript.json"),
    ("repo", "sessions/oyez-bulk/10-218_2011/transcript.json"),
    ("repo", "sessions/oyez-pilot/transcript.json"),
]

# D0 sets that may not enter any partition (visible exclusions).
EXCLUDED = [
    ("phrase_training", "NOT-VERIFIED", "provenance unknown (D0/5qw)"),
    ("phrase_training_test", "NOT-VERIFIED", "provenance unknown; stats mismatch"),
    ("quality_checked_tts", "NOT-VERIFIED", "audio absent (manifests only)"),
    ("quality_checked_tts_full", "NOT-VERIFIED", "audio absent; stale checksums"),
    ("quality_checked_tts_smoke", "NOT-VERIFIED", "audio absent (manifests only)"),
    ("quality_test_gtts", "NOT-VERIFIED", "audio absent (manifests only)"),
    ("tts_smoke", "NOT-VERIFIED", "audio absent (manifests only)"),
    ("accent_datasets/common_voice", "NOT-VERIFIED", "audio absent (not downloaded)"),
    ("accent_datasets/common_voice_actual", "NOT-VERIFIED", "audio absent (script only)"),
    ("accent_datasets/english_dialects", "NOT-VERIFIED", "audio absent (manifest only)"),
    ("accent_datasets/english_dialects_actual", "NOT-VERIFIED", "audio absent (scripts only)"),
    ("accent_datasets/english_dialects_fixed", "NOT-VERIFIED", "audio absent (script only)"),
    ("sessions/run1", "NOT-VERIFIED", "owner WAVs absent in repo (gitignored)"),
    ("synthetic_audio", "VERIFIED", "excluded: 47 zero-frame placeholder files, no audio payload"),
    ("combined_training_dataset", "VERIFIED", "excluded: text-only Arrow shards, per-row origin unlabelled"),
    ("accent_datasets/english_dialects_simulation", "VERIFIED", "excluded: labels only, audio absent"),
    ("accent_datasets/librispeech_accents", "VERIFIED", "excluded: labels only, audio absent"),
    ("manifests (better-voice)", "VERIFIED", "excluded: text index only, no audio payload"),
    ("phonetics + phonetic databases", "VERIFIED", "excluded: lexicon text only, no audio payload"),
    ("corpus_stats / multi_domain_stats / curation_reports", "VERIFIED", "excluded: statistics text only"),
    ("curated_triggers / licensing / production_integration", "VERIFIED", "excluded: licensing working files only"),
    ("live_demo.wav + vocab text files", "VERIFIED", "excluded: single demo clip, D0 role NONE"),
]


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def resolve(root_name, rel, bv_root):
    if root_name == "repo":
        return REPO / rel
    return Path(bv_root) / rel


def entry(root_name, rel, dataset, group, bv_root):
    full = resolve(root_name, rel, bv_root)
    if not full.is_file():
        sys.exit(f"build-d1: referenced file missing: {full}")
    return {"root": root_name, "path": rel, "dataset": dataset,
            "group": group, "bytes": full.stat().st_size,
            "sha256": sha256_of(full)}


def collect_dir(bv_root, dataset, group):
    base = Path(bv_root) / "data" / dataset
    if not base.is_dir():
        sys.exit(f"build-d1: dataset dir missing: {base}")
    return [entry("better-voice", str(p.relative_to(bv_root)), dataset,
                  group, bv_root)
            for p in sorted(base.rglob("*")) if p.is_file()]


def dump(name, payload):
    text = json.dumps(payload, indent=1, sort_keys=True) + "\n"
    (OUT / name).write_text(text)
    return hashlib.sha256(text.encode()).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--better-voice-root", default=os.environ.get(
        "HEARSAY_BETTER_VOICE_ROOT", os.environ.get(
            "BETTER_VOICE_ROOT", str(REPO.parent / "better-voice"))))
    ap.add_argument("--sealed-at", default="2026-10-28")
    args = ap.parse_args()
    bv = args.better_voice_root
    OUT.mkdir(parents=True, exist_ok=True)

    parts = {k: [] for k in ("TRAIN", "DEV", "PHASE-TEST", "LOCKBOX")}
    for ds in BETTER_VOICE_TRAIN:
        parts["TRAIN"] += collect_dir(bv, ds, f"batch:{ds}")
    for ds in BETTER_VOICE_DEV:
        parts["DEV"] += collect_dir(bv, ds, f"batch:{ds}")
    for name in sorted((REPO / "data" / "demo").iterdir()):
        if name.is_file() and name.name != "README.md":
            parts["DEV"].append(entry("repo", f"data/demo/{name.name}",
                                      "data/demo", "session:demo", bv))
    for group, _key, part, files in CASES:
        for root_name, rel in files:
            parts[part].append(entry(root_name, rel, group, group, bv))

    manifest_sha = {}
    for part, fname in (("TRAIN", "train.json"), ("DEV", "dev.json"),
                        ("PHASE-TEST", "phase-test.json"),
                        ("LOCKBOX", "lockbox.json")):
        entries = sorted(parts[part], key=lambda e: (e["root"], e["path"]))
        manifest_sha[part] = dump(fname, {
            "bead": "hearsay-four-7ms", "partition": part,
            "split_rule": "by recording session / generation batch / case (chronological); never by sentence",
            "entries": entries})
        print(f"{part}: {len(entries)} files")

    lm = [entry(r, p, "lm-train-text", "lm-train", bv) for r, p in LM_FILES]
    manifest_sha["LM-TRAIN-TEXT"] = dump("lm-train-text.json", {
        "bead": "hearsay-four-7ms",
        "rule": "LM training text contains TRAIN-partition transcripts only",
        "entries": sorted(lm, key=lambda e: (e["root"], e["path"]))})
    manifest_sha["EXCLUDED"] = dump("excluded.json", {
        "bead": "hearsay-four-7ms",
        "rule": "sets listed here enter no partition; see D0-DATA-INVENTORY.md",
        "excluded": [{"dataset": d, "d0_use_verification": v, "reason": r}
                     for d, v, r in EXCLUDED]})

    lock = sorted(parts["LOCKBOX"], key=lambda e: (e["root"], e["path"]))
    combined = hashlib.sha256(
        "\n".join(e["sha256"] for e in lock).encode()).hexdigest()
    dump("manifest-index.json", {
        "bead": "hearsay-four-7ms",
        "source_inventory": "docs/results/D0-DATA-INVENTORY.md",
        "manifest_sha256": manifest_sha,
        "counts": {k: len(v) for k, v in parts.items()},
        "lockbox_seal": {
            "sealed_at": args.sealed_at, "opened": False, "openings": [],
            "cases": sorted({e["group"] for e in lock}),
            "file_count": len(lock), "combined_sha256": combined,
            "manifest_sha256": manifest_sha["LOCKBOX"],
            "rule": "files not read by any pipeline until a logged milestone opening (M2/M4/M5)"}})
    print(f"LOCKBOX seal: {len(lock)} files combined_sha256={combined}")


if __name__ == "__main__":
    main()
