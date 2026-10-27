#!/usr/bin/env python3
"""Oyez transcript JSON -> session.tsv manifest (bead zl3 pilot, reused by c1g bulk).

Reads a case_media oral_argument_audio JSON (api.oyez.org, CC-BY-NC, no auth),
walks sections/turns/text_blocks in document order, and emits a scorer
manifest: bucket, idx, start_sec, stop_sec, speaker, drift_tags, text.

Bucket rule (pre-registered per run, defaults match the zl3 pilot):
first K-1 buckets take floor(n/K) blocks each, the last bucket takes the
remainder, so every block lands in exactly one bucket.

Usage:
  scripts/oyez_ingest.py transcript.json --out-tsv session.tsv \\
      --buckets 8 --drift-tags 'source:oyez,verified:api'
"""

import argparse
import csv
import json
import sys


def load_blocks(path):
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    blocks = []
    dropped = []
    for section in doc["transcript"]["sections"]:
        for turn in section.get("turns", []):
            speaker = ((turn.get("speaker") or {}).get("name") or "unknown").strip()
            for block in turn.get("text_blocks", []):
                text = (block.get("text") or "").strip()
                if not text:
                    continue
                start = float(block["start"])
                stop = float(block["stop"])
                if stop <= start:
                    # Zero/negative-duration blocks carry no audio; no scorer
                    # can score them. Drop loudly (counted below), never
                    # fabricated, never silently shifted.
                    dropped.append((speaker, start, stop, text[:60]))
                    continue
                blocks.append(
                    {
                        "start": start,
                        "stop": stop,
                        "speaker": speaker,
                        "text": text,
                    }
                )
    return blocks, dropped


def assign_buckets(n, buckets):
    if buckets <= 0:
        raise ValueError("buckets must be positive")
    per = n // buckets
    if per == 0:
        raise ValueError(f"fewer blocks ({n}) than buckets ({buckets})")
    return [min(i // per, buckets - 1) for i in range(n)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("transcript_json")
    parser.add_argument("--out-tsv", required=True)
    parser.add_argument("--buckets", type=int, default=8)
    parser.add_argument("--speaker-default", default="unknown")
    parser.add_argument("--drift-tags", default="source:oyez,verified:api")
    args = parser.parse_args()

    blocks, dropped = load_blocks(args.transcript_json)
    if not blocks:
        print("no text blocks found", file=sys.stderr)
        return 1
    for speaker, start, stop, text in dropped:
        print(f"dropped zero-duration block: {speaker} {start:.3f}-{stop:.3f} {text!r}")
    print(f"dropped {len(dropped)} zero-duration block(s)")
    bucket_of = assign_buckets(len(blocks), args.buckets)

    with open(args.out_tsv, "w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["bucket", "idx", "start_sec", "stop_sec", "speaker", "drift_tags", "text"])
        for idx, (block, bucket) in enumerate(zip(blocks, bucket_of)):
            speaker = block["speaker"] or args.speaker_default
            writer.writerow(
                [
                    bucket,
                    idx,
                    f"{block['start']:.3f}",
                    f"{block['stop']:.3f}",
                    speaker,
                    args.drift_tags,
                    block["text"],
                ]
            )
    print(f"oyez-ingest: OK ({len(blocks)} blocks, {args.buckets} buckets)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
