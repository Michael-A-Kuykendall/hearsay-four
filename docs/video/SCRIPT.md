# Hearsay-IV — 3-minute video: final script (lane L)

Total 180 s. Voiceover ≈ 425 words at ≈ 150 wpm (≈170 s speech +
pauses/cards). All numbers are the
landed values from `../writeup/WRITEUP.md` §4; bracketed TBDs fill
from the stated sources before recording. Record screens at 1920×1080,
terminal font ≥ 18 pt, browser zoom 125%.

## Timing map

| # | Segment | Time | Dur | Screen |
|---|---------|------|-----|--------|
| 1 | Hook: the unexplained error | 0:00–0:20 | 20 s | A. Wrong word, no reason |
| 2 | Approach: mechanistic, model-free | 0:20–0:50 | 30 s | B. Guard + repo map |
| 3 | Demo: transcribe + why panel | 0:50–1:20 | 30 s | C. `web/demo.sh` + click-a-word |
| 4 | Results: landed numbers | 1:20–1:50 | 30 s | D. Verdict line + tune table |
| 5 | Build story: Muse Code lanes | 1:50–2:25 | 35 s | E. Timeline graphic |
| 6 | What it proves + attribution | 2:25–2:55 | 30 s | F. 29/29 visual + credits |
| 7 | Close / tagline | 2:55–3:00 | 5 s | Title card |

## Full voiceover text (read verbatim)

**[1 — 0:00–0:20, 53 words]**
"Every dictation system fails the same way. A wrong word appears —
with no reason attached. No cause to inspect. No fix to aim. For a
court record, a medical note, or a user who can't re-read the screen,
an unexplained error isn't a defect. It's the product failing at its
one job."

**[2 — 0:20–0:50, 67 words]**
"Hearsay-Four is our answer: Hearsay-Three, the 1983 Carnegie Mellon
blackboard system, re-implemented on our patent-pending saturation
fabric. Fully mechanistic, model-free, offline. No neural nets. No
embeddings. No learned weights — a repo guard refuses to build if any
appear. Just classical speech science: acoustic models, a language
model, a pronunciation lexicon. Every word it outputs carries its
provenance — which evidence, which entry, which score produced it."

**[3 — 0:50–1:20, 64 words]**
"Watch. One command transcribes real speech — and every word is
clickable. Click any word: the why panel opens with its acoustic
score, the alternatives it beat and why they lost, its language-model
probability, and its lexicon source. When Hearsay-Four errs, it shows
exactly which component to blame — acoustics, language model, or
lexicon. An error with an address. Transcription you can argue with."

**[4 — 1:20–1:50, 69 words]**
"The numbers, all measured on LibriSpeech, all reproducible from the
repo. Our controlled shootout picked the CMVN acoustic model: phone
error rate oh-point-six-one, beating the baseline oh-point-six-four —
a statistically significant win. Tuning landed word error rate
oh-point-six-six on the development subset. [Headline TBD: full
dev-clean word error rate — fills from the Lane A gate run.] And on a
thirty-word cross-channel check: twenty-nine out of twenty-nine,
zero confusions."

**[5 — 1:50–2:25, 88 words — BUILD STORY]**
"How was it built? With Muse Code, in parallel lanes. The work was
planned as verifiable tasks — beads — and agents built them
simultaneously: one tuned acoustics, one built triphone support, one
hardened the decoder, one built this demo, one kept the number ledger
this video quotes. Gates verified everything; only green work merged.
The agents caught defects: bad training runs discarded, a leaky
evaluation fixed instead of quoted, every word-error count
double-checked by a second scorer. Humans owned microphone, verdicts,
submission. The machine owned the parallelism."

**[6 — 2:25–2:55, 77 words]**
"Hearsay-Four doesn't chase the leaderboard — it chases the audit
trail. Every claim in this video traces to a file in the repo: the
model, the tuning grid, the verdict line. Speech recognition you can
inspect, reproduce, and fix — running fully offline, on classical
methods, with no model at all. Data: LibriSpeech, open corpus, CC BY
4.0. [16-mix TBD: overnight verdict line — fills night of Nov 2.]
Built for the Meta Global AI Developer Hackathon."

**[7 — 2:55–3:00, 6 words]**
"Hearsay-Four. Transcription you can argue with."

## Shot list + screens to record

- **A (0:00–0:20).** Screen recording: a plain text editor with one
  dictated sentence containing a wrong word, cursor blinking on it,
  no explanation anywhere. Slow zoom on the wrong word. (Staged —
  any editor; no repo dependency.)
- **B (0:20–0:50).** Terminal: run `bash scripts/no_model_guard.sh`
  (green exit 0), then `ls src/` showing the 20 workspace crates.
  Overlay title card: "mechanistic · model-free · offline".
  Verbatim guard command from repo root.
- **C (0:50–1:20).** Terminal: `bash web/demo.sh` → "DEMO GREEN"
  line. Then browser: `python3 -m http.server 8000`, open
  `http://localhost:8000/web/transcribe.html`; scroll the 3 fixed
  transcripts (utts 777-126732-0081, 2035-147960-0015,
  1919-142785-0021); click 2 words to open why panels showing
  ACOUSTIC / LOSING ALTERNATIVES / LM / lexicon-source rows.
  Pre-record once; reuse the take (deterministic output).
- **D (1:20–1:50).** Terminal, large font, three beats:
  1. `tail -3 data/models/r3v2-full93-cmvn/cmvn_per.log` — verdict
     line `verdict=CMVN_WINS cmvn=0.6137 noncmvn=0.6416 …`.
  2. `head -12 data/wer71g/tunefull/summary.tsv` — best row
     `6 0 150 0.6624 …`; note the `mode=cmvn` best-cell line.
  3. [Headline TBD: `cat data/wer71g/tunefull/fullgate/cell.tsv`
     score line — record after the gate lands.]
  Freeze each beat ≥ 4 s for readability.
- **E (1:50–2:25).** Full-screen `timeline.svg` (this directory,
  generated from bead/lane data by `timeline.py`): slow pan
  left→right across Oct 26 → Nov 2 lanes as the VO names them.
  Overlay lower-third with live bead counts at record time.
- **F (2:25–2:55).** Lane K chart PNGs when they land
  (`docs/results/analysis/`: confusion chart + 29/29 clean visual;
  bead `hearsay-four-0u3z--headline-error-analysis`): hold the
  29/29 visual full-screen ≥ 5 s. Fallback if K hasn't landed:
  `docs/results/A1-phasetest-20261031-100240/` diagonal confusion
  matrix + `top1=1.0000 n=29` score line. End card: "Data:
  LibriSpeech (SLR12) · CC BY 4.0 · openslr.org".
- **Title card (0:00 under, 2:55–3:00).** "HEARSAY-IV —
  Transcription you can argue with · Meta Global AI Developer
  Hackathon 2026".

## Recording notes

- TBD inserts (2): headline-WER VO line + shot D3 (source: Lane A
  gate cell, bead 8znj); 16-mix VO clause (source: r3pg verdict
  line, night of Nov 2). Everything else is recordable now.
- Lane K dependency: shot F prefers K's PNGs; fallback needs no
  new artifacts.
- No live-mic recording in the video (live RTF is TBD; offline
  numbers only).
- Keep the verdict-line and tune-table shots verbatim — they are
  the audit trail the script promises.
