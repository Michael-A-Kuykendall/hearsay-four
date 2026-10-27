#!/usr/bin/env python3
"""Oyez bulk fetcher (bead c1g; scales the zl3 pilot lane to N cases).

For each ``TERM/DOCKET`` argument: fetch the case JSON (api.oyez.org, no
auth), walk its ``oral_argument_audio`` segments in order, take the FIRST
segment whose media object carries a transcript object, and download the
MP3 (first ``audio/mpeg`` media file) plus the full media JSON into the
staging dir. Cases with no transcript-bearing segment abort loudly
(exit nonzero) -- missing transcripts are never silently skipped.

Layout per case: ``<staging>/<label>/audio-source.mp3`` + ``<label>.json``.
Downstream: ``scripts/oyez_ingest.py`` turns ``<label>.json`` into a
``session.tsv`` scorer manifest; ``mp3-to-wav`` makes ``audio/run1.wav``;
``failfast_run --root`` scores it.

Usage:
  scripts/oyez_bulk_fetch.py --out-dir <staging> 1971/70-112 2011/10-218 2025/24-889

Writes ``<staging>/bulk_manifest.json``: per-case media ids, byte sizes,
durations, block counts, plus the CC-BY-NC attribution line (Oyez audio
and transcripts are Creative Commons non-commercial + attribution;
see docs/research/oyez-ingest-evaluation.md section 1).
"""

import argparse
import json
import sys
import urllib.request
from pathlib import Path

API = "https://api.oyez.org"
ATTRIBUTION = (
    "Audio and transcripts courtesy of Oyez (https://www.oyez.org), "
    "used under Creative Commons non-commercial + attribution license."
)


def get_json(url):
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.load(r)


def get_bytes(url, dest):
    with urllib.request.urlopen(url, timeout=300) as r, open(dest, "wb") as f:
        total = 0
        while True:
            chunk = r.read(1 << 20)
            if not chunk:
                break
            f.write(chunk)
            total += len(chunk)
    return total


def count_blocks(media):
    n_turns = 0
    n_blocks = 0
    for section in media["transcript"]["sections"]:
        for turn in section.get("turns", []):
            n_turns += 1
            n_blocks += len(turn.get("text_blocks", []))
    return n_turns, n_blocks


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("cases", nargs="+", help="TERM/DOCKET, e.g. 1971/70-112")
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    entries = []
    for spec in args.cases:
        term, _, docket = spec.partition("/")
        if not term or not docket:
            print(f"bad case spec {spec!r}: want TERM/DOCKET", file=sys.stderr)
            return 1
        case = get_json(f"{API}/cases/{term}/{docket}")
        if not isinstance(case, dict) or "oral_argument_audio" not in case:
            print(f"no case object at /cases/{term}/{docket}", file=sys.stderr)
            return 1
        segments = case.get("oral_argument_audio") or []
        picked = None
        for seg in segments:
            href = (seg.get("href") or "").replace(
                "https://api.oyez.org", API
            )
            media = get_json(href)
            if media.get("transcript"):
                picked = (seg, media)
                break
            print(f"  {spec}: segment {href} has no transcript object, continuing")
        if picked is None:
            print(
                f"LOUD SKIP REFUSED: {spec} ({case.get('name')}) has "
                f"{len(segments)} segment(s) and none carries a transcript "
                "object -- aborting, not silently skipping",
                file=sys.stderr,
            )
            return 1
        seg, media = picked
        media_id = media.get("id")
        label = f"{docket}_{term}"
        case_dir = out / label
        case_dir.mkdir(parents=True, exist_ok=True)

        mp3_url = None
        mp3_size = None
        for f in media.get("media_file", []):
            if f.get("mime") == "audio/mpeg":
                mp3_url = f["href"]
                mp3_size = f.get("size")
                break
        if mp3_url is None:
            print(f"{spec}: media {media_id} has no audio/mpeg file", file=sys.stderr)
            return 1

        mp3_dest = case_dir / "audio-source.mp3"
        media_dest = case_dir / f"{label}.json"
        print(f"{spec} ({case.get('name')}): media {media_id}, fetching MP3 ...")
        got = get_bytes(mp3_url, mp3_dest)
        with open(media_dest, "w", encoding="utf-8") as f:
            json.dump(media, f, indent=1)
        n_turns, n_blocks = count_blocks(media)
        duration = (media.get("transcript") or {}).get("duration")
        print(
            f"  OK: {got} bytes audio (advertised {mp3_size}), "
            f"duration {duration}s, {n_turns} turns, {n_blocks} blocks"
        )
        entries.append(
            {
                "label": label,
                "term": term,
                "docket": docket,
                "name": case.get("name"),
                "media_id": media_id,
                "mp3_bytes": got,
                "mp3_advertised_bytes": mp3_size,
                "duration_sec": duration,
                "turns": n_turns,
                "blocks": n_blocks,
            }
        )

    manifest = {"attribution": ATTRIBUTION, "cases": entries}
    with open(out / "bulk_manifest.json", "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=1)
    print(f"oyez-bulk-fetch: OK ({len(entries)} cases -> {out})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
