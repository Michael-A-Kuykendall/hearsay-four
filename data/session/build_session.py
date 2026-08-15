#!/usr/bin/env python3
"""Build the 39c.2 fail-fast session manifest from a SCOTUS oral argument.

Source: Oyez speaker-labeled, time-aligned transcript (api.oyez.org) + the
argument mp3 (s3.amazonaws.com/oyez.case-media.mp3). One speaker's turns are
extracted into chronological buckets of ~40 utterances covering a ~20-minute
window, with per-bucket drift annotations (fatigue by session position; mic
distance / ambient noise are real properties of a courtroom recording).

Output: data/session/session.tsv (tab-separated, one turn per row).

Offline-friendly: pass --json with a cached Oyez media JSON to skip network.
"""

import argparse
import json
import os
import re
import sys
import urllib.request

ROOT = os.path.dirname(os.path.abspath(__file__))
DEFAULT_JSON = os.path.join(ROOT, "oyez_media_25951.json")
DEFAULT_AUDIO = os.path.join(ROOT, "audio", "24-889_20260429-argument.mp3")
OUT_TSV = os.path.join(ROOT, "session.tsv")

WS = re.compile(r"\s+")


def fetch_json(url, cache_path):
    if os.path.exists(cache_path):
        with open(cache_path, "r", encoding="utf-8") as f:
            return json.load(f)
    with urllib.request.urlopen(url, timeout=60) as resp:
        data = json.load(resp)
    with open(cache_path, "w", encoding="utf-8") as f:
        json.dump(data, f)
    return data


def flatten_turns(media):
    turns = []
    for section in media["transcript"]["sections"]:
        for turn in section.get("turns", []):
            text = " ".join(
                block.get("text", "") for block in turn.get("text_blocks", [])
            )
            text = WS.sub(" ", text).strip()
            speaker = turn.get("speaker", {}).get("name", "UNKNOWN")
            turns.append(
                {
                    "start": float(turn["start"]),
                    "stop": float(turn["stop"]),
                    "speaker": speaker,
                    "text": text,
                }
            )
    turns.sort(key=lambda t: t["start"])
    return turns


def drift_tags(bucket_idx, n_buckets):
    rel = bucket_idx / max(1, n_buckets - 1)
    if rel < 1 / 3:
        fatigue = "fresh"
    elif rel < 2 / 3:
        fatigue = "warm"
    else:
        fatigue = "fatigued"
    return f"fatigue:{fatigue},mic:variable,noise:ambient"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", default=DEFAULT_JSON, help="cached Oyez media JSON")
    ap.add_argument("--minutes", type=float, default=20.0)
    ap.add_argument("--bucket-size", type=int, default=40)
    ap.add_argument("--audio", default=DEFAULT_AUDIO, help="session mp3 path")
    args = ap.parse_args()

    media = fetch_json(
        "https://api.oyez.org/case_media/oral_argument_audio/25951", args.json
    )
    turns = flatten_turns(media)

    # Primary speaker = the one with the most turns (the session speaker).
    by_speaker = {}
    for t in turns:
        by_speaker.setdefault(t["speaker"], []).append(t)
    speaker, sp_turns = max(by_speaker.items(), key=lambda kv: len(kv[1]))
    print(f"speakers: {list(by_speaker)}", file=sys.stderr)
    print(f"primary: {speaker} ({len(sp_turns)} turns)", file=sys.stderr)

    # Window: first `minutes` minutes of the primary speaker's own clock.
    start0 = sp_turns[0]["start"]
    window_end = start0 + args.minutes * 60.0
    windowed = [t for t in sp_turns if t["stop"] <= window_end]
    print(
        f"window: {args.minutes:.0f} min ({start0:.1f}s..{window_end:.1f}s), "
        f"{len(windowed)} turns",
        file=sys.stderr,
    )

    buckets = [
        windowed[i : i + args.bucket_size]
        for i in range(0, len(windowed), args.bucket_size)
    ]
    n_buckets = len(buckets)
    print(f"buckets: {n_buckets}", file=sys.stderr)

    lines = [
        "# Hearsay-IV fail-fast session manifest (bead 39c.2)",
        "# case: Hikma Pharmaceuticals USA Inc. v. Amarin Pharma, Inc. (24-889)",
        "# date: 2026-04-29 | source: Oyez oral_argument_audio/25951",
        f"# audio: {os.path.relpath(args.audio, ROOT)}",
        f"# speaker: {speaker} | window: {start0:.1f}s..{window_end:.1f}s "
        f"| bucket_size: {args.bucket_size}",
        "# columns: bucket, idx, start_sec, stop_sec, speaker, drift_tags, text",
    ]
    lines.append("bucket\tidx\tstart_sec\tstop_sec\tspeaker\tdrift_tags\ttext")
    for b, bucket in enumerate(buckets):
        tags = drift_tags(b, n_buckets)
        for i, t in enumerate(bucket):
            lines.append(
                "\t".join(
                    [
                        str(b),
                        str(i),
                        f"{t['start']:.3f}",
                        f"{t['stop']:.3f}",
                        t["speaker"],
                        tags,
                        t["text"],
                    ]
                )
            )
    with open(OUT_TSV, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {OUT_TSV}: {n_buckets} buckets, {len(windowed)} rows")


if __name__ == "__main__":
    main()
