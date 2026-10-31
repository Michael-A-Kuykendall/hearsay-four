#!/usr/bin/env python3
"""Owner-book eval-novelty check (DATA-1 support, bead hearsay-four-79f--data-intake-corpora).

Counts book sentences matching an eval prompt verbatim (after
normalization). Prints COUNTS ONLY — no book text is written anywhere
inside the repo, ever.

Normalization here (lowercase, strip punctuation, collapse whitespace)
is an approximation; L3 finalizes with lm::normalize + the exact RS1-d
cut lists. Prompt side is a SUPERSET (whole prompt files, not the
50/15-line cuts), so the drop count is an upper bound.
"""

import os
import re
import string
import sys
from pathlib import Path

PROMPT_FILES = [
    "sessions/script-c0r-read.txt",
    "sessions/script-p4-fly-dev.txt",
    "sessions/script-p4-fly-ptest.txt",
    "sessions/script-harvard.txt",
]


def norm(s: str) -> str:
    s = s.lower()
    s = s.translate(str.maketrans("", "", string.punctuation))
    return " ".join(s.split())


def main() -> int:
    repo = Path(__file__).resolve().parent.parent
    personal = os.environ.get("HEARSAY_PERSONAL_TEXT", str(Path.home() / "hearsay-personal"))
    book = Path(personal) / "owner-book.txt"
    if not book.exists():
        print(f"FAIL: no book text at {book} (HEARSAY_PERSONAL_TEXT)")
        return 1
    prompts = set()
    for rel in PROMPT_FILES:
        for line in (repo / rel).read_text().splitlines():
            n = norm(line)
            if n:
                prompts.add(n)
    text = book.read_text()
    sents = [s for s in re.split(r"[.!?]+", text) if len(s.split()) >= 3]
    words = len(text.split())
    drops = sum(1 for s in sents if norm(s) in prompts)
    print(f"book_words={words}")
    print(f"book_sentences={len(sents)}")
    print(f"eval_prompts_superset={len(prompts)}")
    print(f"novelty_drops_upper_bound={drops}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
