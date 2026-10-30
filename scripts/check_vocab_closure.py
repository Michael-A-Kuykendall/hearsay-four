#!/usr/bin/env python3
"""Vocab-closure checker for A2 phrase scripts (wb2 gate).

Every whitespace token of every non-comment line must be in
CLOSED_VOCABULARY, and every phrase must be 3-7 words. The embedded
list is cross-checked against src/fse/src/lib.rs (drift = fail).

Usage (from repo root):
    python3 scripts/check_vocab_closure.py sessions/script-a2-phrases.txt
"""
import re
import sys

EMBEDDED = """impeachment methodical survival process intense reflection kinship resented congealed uncorroborated highboy hotbed ripple predestine byzantium genetic constitution interpersonal enchantment perfume submissions overboard knockdown sleepwalking unrealized panhandling horribly salivating rhubarb staked""".split()


def main():
    if len(sys.argv) != 2:
        print("usage: check_vocab_closure.py SCRIPT", file=sys.stderr)
        return 2
    lib = open("src/fse/src/lib.rs").read()
    m = re.search(r"CLOSED_VOCABULARY: \[&str; 30\] = \[(.*?)\];", lib, re.S)
    if not m:
        print("cannot locate CLOSED_VOCABULARY in src/fse/src/lib.rs", file=sys.stderr)
        return 2
    source = re.findall(r'"([a-z]+)"', m.group(1))
    if sorted(source) != sorted(EMBEDDED):
        print(
            f"embedded vocab drifted from lib.rs: {sorted(set(source) ^ set(EMBEDDED))}",
            file=sys.stderr,
        )
        return 2
    vocab = set(EMBEDDED)
    bad = 0
    n_phrases = 0
    n_tokens = 0
    for i, line in enumerate(open(sys.argv[1]), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        n_phrases += 1
        toks = line.split()
        n_tokens += len(toks)
        if not (3 <= len(toks) <= 7):
            print(f"line {i}: {len(toks)} words, need 3-7: {line}")
            bad += 1
        for t in toks:
            if t not in vocab:
                print(f"line {i}: out-of-vocab token: {t}")
                bad += 1
    print(f"phrases={n_phrases} tokens={n_tokens} violations={bad}")
    return 1 if bad or n_phrases < 100 else 0


if __name__ == "__main__":
    sys.exit(main())
