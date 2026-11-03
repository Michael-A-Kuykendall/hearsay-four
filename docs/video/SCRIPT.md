# Hearsay-IV — 3-minute video: final script (lane su00, fabric-centered)

Total 180 s. Voiceover ≈ 437 words at ≈ 150 wpm (≈175 s speech +
cards). Every number is a landed in-checkout value from
`../results/A6-FABRIC-EXPERIMENT.md`, `../results/NUMBER-LEDGER.md`,
or `../writeup/WRITEUP.md` §4 — no TBDs. Record screens at 1920×1080,
terminal font ≥ 18 pt, browser zoom 125%.

## Timing map

| # | Segment | Time | Dur | Screen |
|---|---------|------|-----|--------|
| 1 | Hook: the unexplained error | 0:00–0:12 | 12 s | A. Wrong word, no reason |
| 2 | Method: the 30-word check | 0:12–0:32 | 20 s | B. 29/29 score line |
| 3 | Demo: fabric transcript + why panel | 0:32–1:02 | 30 s | C. `web/demo.sh` + click-a-word |
| 4 | Revival case: the late rescue | 1:02–1:24 | 22 s | D. cat panel + event log |
| 5 | Correction + undo | 1:24–1:40 | 16 s | E. `hearsay-correct demo` |
| 6 | A6 numbers + headline WER | 1:40–2:08 | 28 s | F. Arm-A table + oracle row |
| 7 | Build story: lanes + gates | 2:08–2:42 | 34 s | G. Timeline graphic |
| 8 | What it proves + close | 2:42–3:00 | 18 s | H. End card + credits |

## Full voiceover text (read verbatim)

**[1 — 0:00–0:12, 24 words]**
"Every dictation system fails the same way. A wrong word appears —
with no reason attached. No cause to inspect. No fix to aim."

**[2 — 0:12–0:32, 46 words]**
"Before the system, the method. Thirty isolated words, recorded
through a laptop chain, matched against templates from a different
machine. Twenty-nine scored, twenty-nine right, zero confusions.
A characterization, not a headline — it says the classical pipeline
measures cleanly before we ask anything harder of it."

**[3 — 0:32–1:02, 74 words]**
"Watch. One command runs the decision fabric over a speech lattice
and renders the transcript — every word clickable. Click any word:
the why panel opens with its acoustic score, the alternatives it
beat and why they lost, its language-model probability, and its
lexicon source. Plus the fabric record: which rivals it defeated,
by what margin, and whether it was ever revived or evicted. An
error with an address. Transcription you can argue with."

**[4 — 1:02–1:24, 54 words]**
"Here is the fabric thinking. The sentence 'the cat sat down' —
'cat' starts dormant, acoustically weak. Late evidence through
'sat' revives it; it defeats 'bat' by a scored margin and commits.
The event log records each step: revived, defeated, committed. A
resurrection with a receipt — the demo asserts it on every run."

**[5 — 1:24–1:40, 40 words]**
"And when the user corrects a word, the fix lands in the fabric as
a ledgered fact — then undo restores the decode byte-identical to
the original. Correction you can take back, verified by the binary
itself, not by promise."

**[6 — 1:40–2:08, 65 words]**
"The numbers, all measured, all reproducible. The fabric experiment's
canonical search — language-model weight six, beam one-fifty, CMVN
acoustics — scores word error rate oh-point-six-six on the
two-hundred-utterance dev set. On the held-out test set:
oh-point-six-four-three-oh, with a ninety-five percent interval
oh-point-six-two to oh-point-six-seven — a first-ever score for that
population. The lattice oracle sits at oh-point-five-eight: the
remaining gap is acoustic, measured, and disclosed."

**[7 — 2:08–2:42, 85 words — BUILD STORY]**
"How was it built? With Muse Code, in parallel lanes. The work was
planned as verifiable tasks — beads — and agents built them
simultaneously: one tuned acoustics, one ran the fabric experiment,
one built this demo, one kept the number ledger this video quotes.
Gates verified everything; only green work merged. The agents caught
defects: bad training runs discarded, a leaky evaluation fixed
instead of quoted, every word-error count double-checked by a second
scorer. Humans owned microphone, verdicts, submission. The machine
owned the parallelism."

**[8 — 2:42–3:00, 49 words]**
"Hearsay-Four doesn't chase the leaderboard — it chases the audit
trail. Every claim in this video traces to a file in the repo.
Speech recognition you can inspect, reproduce, and fix — running
fully offline, on classical methods, with no model at all.
Hearsay-Four. Transcription you can argue with."

## Shot list + screens to record

- **A (0:00–0:12).** Screen recording: a plain text editor with one
  dictated sentence containing a wrong word, cursor blinking on it,
  no explanation anywhere. Slow zoom on the wrong word. (Staged —
  any editor; no repo dependency.)
- **B (0:12–0:32).** Terminal: `docs/results/NUMBER-LEDGER.md`
  row 5 (`top-1 1.0000, 29/29 scored … zero confusions`). Then the
  29/29 visual: Lane K chart PNGs when they land
  (`docs/results/analysis/`, bead
  `hearsay-four-0u3z--headline-error-analysis`), held full-screen
  ≥ 5 s; fallback if K hasn't landed:
  `docs/results/A1-phasetest-20261031-100240/` diagonal confusion
  matrix + `top1=1.0000 n=29` score line. Lower-third:
  "characterization, not a headline".
- **C (0:32–1:02).** Terminal: `bash web/demo.sh` → "DEMO GREEN"
  line plus the per-utterance `fabric_summary` count lines. Then
  browser: `python3 -m http.server 8000`, open
  `http://localhost:8000/web/transcribe.html`; scroll the 4 fixture
  transcripts (u1-revival, u2-clean, u3-bulk, u4-absent); click 2
  words to open why panels showing ACOUSTIC / LOSING ALTERNATIVES
  / LM / LEXICON / EVIDENCE / FABRIC rows.
  Pre-record once; reuse the take (deterministic output).
- **D (1:02–1:24).** Browser zoom on the `cat` why panel FABRIC
  row (`revivals: sat … beat: bat … committed … margin`). Terminal:
  `cat web/demo-out/bridge/u1-revival.events.jsonl` — hold on the
  `revived`, `defeated`, `committed` lines. Lower-third:
  "revived → defeated → committed".
- **E (1:24–1:40).** Terminal (owner checkout with externals):
  `cargo run --release -q -p runner --bin hearsay-correct -- demo`
  → `undo verified: post-undo decode byte-identical to original`
  plus `DEMO OK: all fixes + undo verified, collateral report
  printed`. Hold the undo line ≥ 4 s.
- **F (1:40–2:08).** Terminal, large font, three beats from
  `docs/results/A6-FABRIC-EXPERIMENT.md`:
  1. Arm-A table rows: DEV-200 `0.6624 [0.6383, 0.6868]`,
     TEST-200 `0.6430 [0.6182, 0.6687]`.
  2. Oracle rows: K=10 `0.5809` vs 1-best `0.6624`.
  3. Scroll to "Arms B/C and DEV tuning — BLOCKED (measured)" —
     disclosed on screen, not hidden.
  Freeze each beat ≥ 4 s for readability.
- **G (2:08–2:42).** Full-screen `timeline.svg` (this directory,
  generated from bead/lane data by `timeline.py`): slow pan
  left→right across Oct 26 → Nov 3 lanes as the VO names them.
  Overlay lower-third with live bead counts at record time.
- **H (2:42–3:00).** End card: "HEARSAY-IV — Transcription you
  can argue with · Meta Global AI Developer Hackathon 2026".
  Credits: "Data: LibriSpeech (SLR12) · CC BY 4.0 · openslr.org"
  + "mechanistic · model-free · offline".

## Recording notes

- No TBDs: every number is landed in-checkout (A6 doc, ledger,
  writeup §4). Re-verify the arm-A rows against
  `docs/results/A6-FABRIC-EXPERIMENT.md` at record time; if a newer
  verdict landed, re-read the VO numbers before recording.
- Lane K dependency: shot B prefers K's PNGs; fallback needs no
  new artifacts.
- No live-mic recording in the video (offline numbers only).
- Shot E needs the owner checkout (externals); every other shot
  records anywhere after `bash web/demo.sh` (+ `HEARSAY_*` in lanes
  without `data/`).
- Keep the event-log, arm-A table, and undo-line shots verbatim —
  they are the audit trail the script promises.
