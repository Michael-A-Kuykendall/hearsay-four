# WER-1: Absolute WER of the full system on Oyez bulk (span-sliced)

Bead: `hearsay-four-4b7`. The full system (`hearsay-serve` pipeline:
windowed MFCC, phone scoring, hash-consed arena, commit window,
edit-distance lexicon lookup over the vendored closed vocabulary,
counted bigrams) transcribes bounded audio spans of each bulk case's
`audio/run1.wav` — each span on a FRESH pipeline so fabric cost
stays bounded — and each span hypothesis is scored against its
MATCHING reference span (timed words from `extract_words` whose
word-start falls in the span) with `output::wer`.
Model-free, Oyez audio + synthetic fixtures only, fully offline.

This is a SPAN-SLICED measurement, not full-file: full-file
transcription is kept as the overnight driver path (`--span-secs 0`).

Reproduce (from `src/runner`):
`cargo run --release --bin wer-bulk -- --root ../../sessions/oyez-bulk --span-secs 30 --num-spans 3`

Pipeline: chunk_secs=2 (one committed word per chunk), generic
reference baseline, SPAN-SLICED: 30s spans per case, span starts spread start/middle/end across each file (3 spans/case).

| case | span start (s) | span (s) | ref words (N) | hyp words | S | D | I | WER |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 70-112_1971 | 0 | 30 | 61 | 15 | 15 | 46 | 0 | 1.0000 |
| 70-112_1971 | 1773 | 30 | 87 | 15 | 15 | 72 | 0 | 1.0000 |
| 70-112_1971 | 3546 | 30 | 81 | 15 | 15 | 66 | 0 | 1.0000 |
| 10-218_2011 | 0 | 30 | 82 | 15 | 15 | 67 | 0 | 1.0000 |
| 10-218_2011 | 1816 | 30 | 99 | 15 | 15 | 84 | 0 | 1.0000 |
| 10-218_2011 | 3631 | 30 | 95 | 15 | 15 | 80 | 0 | 1.0000 |
| 24-889_2025 | 0 | 30 | 63 | 15 | 15 | 48 | 0 | 1.0000 |
| 24-889_2025 | 1795 | 30 | 74 | 15 | 15 | 59 | 0 | 1.0000 |
| 24-889_2025 | 3590 | 30 | 69 | 15 | 15 | 54 | 0 | 1.0000 |

Overall span micro-average (total errors / total span ref words): S=135 D=576 I=0 N=711 hyp=135 WER=1.0000.

S = substitutions, D = deletions, I = insertions, WER = (S+D+I)/N.
