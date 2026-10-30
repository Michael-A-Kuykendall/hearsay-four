#!/usr/bin/env python3
"""Novelty checker for eval scripts (b4z gate).

Every non-comment line of SCRIPT must be absent (normalized substring)
from every CORPUS file, and lines must be unique within the script.
Normalization: lowercase, keep [a-z0-9 ], collapse whitespace.

Usage (from repo root):
    python3 scripts/check_novelty.py SCRIPT CORPUS [CORPUS ...]
"""
import re
import sys


def norm(s):
    return re.sub(r"\s+", " ", re.sub(r"[^a-z0-9 ]", "", s.lower())).strip()


def main():
    if len(sys.argv) < 3:
        print("usage: check_novelty.py SCRIPT CORPUS [CORPUS ...]", file=sys.stderr)
        return 2
    script = sys.argv[1]
    lines = [
        l.strip()
        for l in open(script)
        if l.strip() and not l.strip().startswith("#")
    ]
    print(f"script_lines={len(lines)}")
    if len(set(lines)) != len(lines):
        print("DUPLICATE lines within script")
        return 1
    blobs = {}
    for path in sys.argv[2:]:
        try:
            blobs[path] = norm(open(path, errors="replace").read())
        except OSError as e:
            print(f"cannot read {path}: {e}", file=sys.stderr)
            return 2
    bad = 0
    for line in lines:
        n = norm(line)
        for path, blob in blobs.items():
            if n and n in blob:
                print(f"MATCH in {path}: {line}")
                bad += 1
    print(f"matches={bad}")
    if len(lines) < 100:
        print(f"only {len(lines)} lines, need >=100")
        return 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
