"""Build the remaining recording waves from an existing run1 manifest.

The already-recorded session has 64 scripted utterances. This generator adds
56 utterances so every word in ``fse::CLOSED_VOCABULARY`` ends with exactly
four exposures:

- Wave 2 (26 lines): brings every word to at least three exposures.
- Wave 3 (30 lines): records every word once more.

Within each generated wave, words are ordered greedily by the longest time
since their previous occurrence. That avoids the scientifically weak close
repeats produced by an unconstrained shuffle. The canonical ``script.txt``
is rewritten as the recorded prefix plus these two waves; the original
160-line version is preserved as ``script-160.txt`` when present.
"""

from collections import Counter
from pathlib import Path

WORDS = [
    "impeachment", "methodical", "survival", "process", "intense",
    "reflection", "kinship", "resented", "congealed", "uncorroborated",
    "highboy", "hotbed", "ripple", "predestine", "byzantium",
    "genetic", "constitution", "interpersonal", "enchantment", "perfume",
    "submissions", "overboard", "knockdown", "sleepwalking", "unrealized",
    "panhandling", "horribly", "salivating", "rhubarb", "staked",
]

TARGET_EXPOSURES = 4
WAVE2_EXPOSURES = 3


def read_recorded(manifest_path: Path) -> list[str]:
    """Return expected transcripts for ``utt*.wav`` rows in manifest order."""
    recorded = []
    for line in manifest_path.read_text(encoding="utf-8").splitlines():
        columns = line.split("\t")
        if columns and columns[0].startswith("utt") and len(columns) >= 2:
            recorded.append(columns[1])
    return recorded


def _order_spaced(history: list[str], needed: Counter) -> list[str]:
    remaining = Counter({word: count for word, count in needed.items() if count > 0})
    last_seen = {word: index for index, word in enumerate(history)}
    ordered = []

    while remaining:
        global_index = len(history) + len(ordered)
        candidates = list(remaining)
        if len(candidates) > 1 and ordered:
            without_immediate_repeat = [word for word in candidates if word != ordered[-1]]
            if without_immediate_repeat:
                candidates = without_immediate_repeat
        word = max(
            candidates,
            key=lambda candidate: (
                global_index - last_seen.get(candidate, -10_000),
                remaining[candidate],
                -WORDS.index(candidate),
            ),
        )
        ordered.append(word)
        last_seen[word] = global_index
        remaining[word] -= 1
        if remaining[word] == 0:
            del remaining[word]

    return ordered


def plan_waves(recorded: list[str]) -> tuple[list[str], list[str]]:
    """Plan Wave 2 and Wave 3 for a partial session with counts at most 3."""
    counts = Counter(recorded)
    unknown = sorted(set(counts) - set(WORDS))
    if unknown:
        raise ValueError(f"recorded words outside closed vocabulary: {unknown}")
    if any(count > WAVE2_EXPOSURES for count in counts.values()):
        raise ValueError("plan_waves expects no word recorded more than 3 times")

    wave2_needed = Counter(
        {
            word: WAVE2_EXPOSURES - counts[word]
            for word in WORDS
            if counts[word] < WAVE2_EXPOSURES
        }
    )
    wave2 = _order_spaced(recorded, wave2_needed)
    wave3 = _order_spaced(recorded + wave2, Counter({word: 1 for word in WORDS}))

    final_counts = Counter(recorded + wave2 + wave3)
    if set(final_counts.values()) != {TARGET_EXPOSURES}:
        raise ValueError(f"wave plan does not reach {TARGET_EXPOSURES} exposures: {final_counts}")
    return wave2, wave3


def _write_lines(path: Path, lines: list[str]) -> None:
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    sessions_dir = root / "sessions"
    recorded = read_recorded(sessions_dir / "run1" / "manifest.tsv")
    wave2, wave3 = plan_waves(recorded)
    full_script = recorded + wave2 + wave3

    script_path = root / "script.txt"
    if script_path.exists():
        previous = script_path.read_text(encoding="utf-8").splitlines()
        backup_path = root / "script-160.txt"
        if previous != full_script and len(previous) == 160 and not backup_path.exists():
            _write_lines(backup_path, previous)

    _write_lines(script_path, full_script)
    _write_lines(sessions_dir / "script-wave2.txt", wave2)
    _write_lines(sessions_dir / "script-wave3.txt", wave3)
    print(
        f"recorded={len(recorded)} wave2={len(wave2)} wave3={len(wave3)} "
        f"total={len(full_script)} counts={sorted(set(Counter(full_script).values()))}"
    )


if __name__ == "__main__":
    main()
