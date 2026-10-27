#!/usr/bin/env python3
"""D0 inventory validator (bead hearsay-four-0f6).

Fails (exit 1) if any inventory row is missing any validator-enforced field:
path/status, file count, duration, sample rate, human|TTS|unknown determination,
speaker info, licence, checksum/status, 30-word coverage, data role, approval.
Usage: python3 docs/results/validate_d0_inventory.py [--self-test] [path]
"""
import re
import sys

REQUIRED = ["Dataset", "Path / Status", "Files", "Duration", "Sample rate",
            "Determination (evidence)", "Speakers", "Licence",
            "Checksum / Status", "30-word coverage", "Data role", "Approval"]
DETERMINATIONS = ("human", "TTS", "unknown")


def parse_rows(text):
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("|") or "---" in line:
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        if cells and cells[0] == "Dataset":
            continue
        rows.append(cells)
    return rows


def validate(text):
    errors = []
    rows = parse_rows(text)
    if len(rows) < 20:
        errors.append(f"too few inventory rows: {len(rows)} (expected >= 20)")
    for i, cells in enumerate(rows, 1):
        if len(cells) != len(REQUIRED):
            errors.append(f"row {i}: {len(cells)} cells, expected {len(REQUIRED)}")
            continue
        for field, value in zip(REQUIRED, cells):
            if not value:
                errors.append(f"row {i} ({cells[0]}): missing field '{field}'")
        det = cells[5]
        if not any(det.startswith(d) for d in DETERMINATIONS):
            errors.append(f"row {i} ({cells[0]}): determination must start with human|TTS|unknown, got '{det[:40]}'")
        if "(" not in det or ")" not in det:
            errors.append(f"row {i} ({cells[0]}): determination lacks parenthesised evidence")
        if not re.fullmatch(r"\d+/30.*|not measurable.*|0/30.*|4/30.*|1/30.*|30/30.*", cells[9]):
            errors.append(f"row {i} ({cells[0]}): bad 30-word coverage '{cells[9][:40]}'")
    if "phrase_training provenance" not in text.lower():
        errors.append("phrase_training provenance section missing")
    return errors


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    path = args[0] if args else "docs/results/D0-DATA-INVENTORY.md"
    with open(path) as f:
        text = f.read()
    if "--self-test" in sys.argv:
        broken = text.replace("| PENDING |", "|  |", 1)
        errs = validate(broken)
        if not errs:
            print("SELF-TEST FAIL: validator accepted a row with a missing field")
            return 1
        print(f"SELF-TEST PASS: validator rejected broken inventory ({len(errs)} error(s))")
        return 0
    errors = validate(text)
    if errors:
        print("FAIL")
        for e in errors:
            print(" -", e)
        return 1
    print(f"PASS: {len(parse_rows(text))} rows, all {len(REQUIRED)} fields present")
    return 0


if __name__ == "__main__":
    sys.exit(main())
