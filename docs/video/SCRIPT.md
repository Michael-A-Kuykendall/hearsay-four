# Hearsay-IV — 3-minute video: final script (centerpiece order, lane 0nqa)

Total 180 s. Voiceover 424 words at ≈ 150 wpm (≈ 170 s speech +
chart/card holds). Segment order follows the centerpiece exactly:
(a) unexplained dictation error; (b) Hearsay-II blackboard
cost/commitment problem; (c) Saturation Fabric + why panel; (d) scaling
chart; (e) A6 dormancy-vs-pruning result; (f) correction + exact undo
and the 30-word check; (g) Muse Code multi-lane build story. Every
number traces to `../writeup/WRITEUP.md`, `../results/A6-FABRIC-EXPERIMENT.md`,
`../results/NUMBER-LEDGER.md`, or the CX1 logs cited in
`scaling-cx1.dat` — no TBDs. Record screens at 1920×1080, terminal
font ≥ 18 pt, browser zoom 125%.

## Timing map

| # | Segment | Time | Dur | Screen |
|---|---------|------|-----|--------|
| a | Hook: fly becomes family, no reason | 0:00–0:18 | 18 s | A. Wrong word, no reason |
| b | Hearsay-II blackboard problem | 0:18–0:48 | 30 s | B. Blackboard diagram card |
| c | Fabric decides every word, why panel | 0:48–1:18 | 30 s | C. `web/demo.sh` + click-a-word |
| d | Scaling chart | 1:18–1:38 | 20 s | D. `scaling-cx1.png` full-screen |
| e | A6: 1599/1600 identical | 1:38–2:05 | 27 s | E. A6 headline block |
| f | Correction + undo, 29/29 check | 2:05–2:28 | 23 s | F. `hearsay-correct demo` + ledger row 5 |
| g | Build story: Muse Code lanes | 2:28–3:00 | 32 s | G. Timeline graphic + end card |

## Full voiceover text (read verbatim)

**[a — 0:00–0:18, 40 words]**
"Every dictation system fails the same way. You say fly. It writes
family. Every time. And nothing in the system can say why. No cause
to inspect. No fix that sticks. Just a wrong word, again, with no
reason attached."

**[b — 0:18–0:48, 69 words]**
"Fifty years ago, Hearsay-Two named the shape of the answer: a
blackboard where independent experts post competing hypotheses about
the speech. But the blackboard had a cost problem. Ratings were
unreliable, so the scheduler had to drop hypotheses it could not
afford to keep — and the one it dropped was often the right one.
Once dropped, gone forever. Its rival HARPY won by searching faster
and pruning later."

**[c — 0:48–1:18, 79 words]**
"Hearsay-Four keeps the blackboard and changes what sits under it: a
Saturation Fabric that decides every word. Each hypothesis keeps all
of its evidence. Lose one support, the rest hold. Fall below
threshold, it goes dormant — kept, not deleted — and revives when
late evidence arrives. Watch: every word of this transcript is
clickable. Click one, and the why panel shows its score, the rivals
it beat, the margin, and the fabric record. An error with an
address."

**[d — 1:18–1:38, 49 words]**
"And the Fabric scales. Rebuilt as a true saturation loop, its
growth exponent fell from three-point-eight to one-point-oh-seven —
near-linear — and it adjudicates sixty-seven thousand competing
hypotheses per utterance at a fraction of real time. The red bar is
the old bridge. The green bar is the Fabric."

**[e — 1:38–2:05, 60 words — 1599/1600]**
"Then the experiment: dormancy against pruning, identical evidence,
identical budgets. Fifteen-ninety-nine of sixteen hundred paired
runs produced identical transcripts. No revived hypothesis ever
entered a committed answer. The logs say why: the correct words were
never the dormant ones. So dormancy did not beat pruning. We report
the null result — and the measured reason — instead of a win."

**[f — 2:05–2:28, 54 words — 29/29]**
"Corrections are facts, not overrides. Fix family back to fly, and
the Fabric records the correction — re-decode obeys it, and undo
restores the original byte-identical. Verified by the binary itself,
not by promise. And the pipeline measures cleanly first: thirty
isolated words, twenty-nine scored, twenty-nine right, zero
confusions. A characterization, not a headline."

**[g — 2:28–3:00, 73 words — BUILD STORY]**
"How was it built? With Muse Code, in parallel lanes. The work was
planned as verifiable tasks, and agents built them simultaneously:
acoustics, the Fabric experiment, the demo, the number ledger this
video quotes. Gates verified everything; only green work merged. A
reviewer seat caught what agents missed: bad training runs
discarded, a leaky evaluation fixed instead of quoted, every count
double-checked. Humans owned microphone, verdicts, and submission.
The machine owned the parallelism."

## Shot list + screens to record

- **A (0:00–0:18).** Screen recording: a plain text editor with one
  dictated sentence where "fly" came out as "family", cursor
  blinking on the wrong word, no explanation anywhere. Slow zoom on
  the wrong word. (Staged — any editor; no repo dependency.)
- **B (0:18–0:48).** Title card: "HEARSAY-II (1970s): THE BLACKBOARD"
  with three bullets — "experts post competing hypotheses" /
  "ratings unreliable: scheduler must drop" / "dropped = gone;
  HARPY won by searching faster". Hold full segment; crossfade at
  "HARPY" to the paper's Section 2 heading on screen.
- **C (0:48–1:18).** Terminal: `bash web/demo.sh` → "DEMO GREEN"
  line plus the per-utterance `fabric_summary` count lines. Then
  browser: `python3 -m http.server 8000`, open
  `http://localhost:8000/web/transcribe.html`; scroll the 4 fixture
  transcripts; click 2 words to open why panels showing ACOUSTIC /
  LOSING ALTERNATIVES / LM / LEXICON / EVIDENCE / FABRIC rows.
  Pre-record once; reuse the take (deterministic output).
- **D (1:18–1:38).** Full-screen `scaling-cx1.png` (this directory,
  generated from CX1 log data by `scaling-cx1.py`): hold ≥ 8 s.
  Slow zoom onto the green bar as the VO names it. Lower-third:
  "growth exponent 3.84 → 1.07 (CX1 logs)".
- **E (1:38–2:05).** Terminal, large font, `docs/writeup/WRITEUP.md`
  Section 8 + `docs/results/A6-FABRIC-EXPERIMENT.md` "A6 headline
  (13:00 freeze)": 1599/1600 identical paired runs; the single
  th25-budget-500 cell differing by one insertion (CI touches 0,
  not significant); `rev_winner = 0` in all 24 C cells. Freeze the
  headline block ≥ 5 s. Lower-third: "dormancy did not beat
  pruning — measured".
- **F (2:05–2:28).** Terminal (owner checkout with externals):
  `cargo run --release -q -p runner --bin hearsay-correct -- demo`
  → `undo verified: post-undo decode byte-identical to original`.
  Hold the undo line ≥ 4 s. Then `docs/results/NUMBER-LEDGER.md`
  row 5 (`top-1 1.0000, 29/29 scored … zero confusions`).
  Lower-third: "characterization, not a headline".
- **G (2:28–3:00).** Full-screen `timeline.svg` (this directory,
  generated from bead/lane data by `timeline.py`): slow pan
  left→right across Oct 26 → Nov 4 lanes as the VO names them.
  Overlay lower-third with live bead counts at record time. Final
  5 s: end card "HEARSAY-IV — Transcription you can argue with ·
  Meta Global AI Developer Hackathon 2026" + "Data: LibriSpeech
  (SLR12) · CC BY 4.0 · openslr.org" + "mechanistic · model-free ·
  offline".

## Recording notes

- No TBDs: every number is landed in-checkout (writeup §8–9, A6
  doc, ledger, `scaling-cx1.dat` sources). Re-verify the A6
  headline rows against `docs/results/A6-FABRIC-EXPERIMENT.md` at
  record time; if a newer verdict landed, re-read the VO numbers
  before recording.
- No live-mic recording in the video (offline numbers only).
- Shot F needs the owner checkout (externals); every other shot
  records anywhere after `bash web/demo.sh` (+ `HEARSAY_*` in lanes
  without `data/`).
- Keep the chart, headline-block, and undo-line shots verbatim —
  they are the audit trail the script promises.
- Chart provenance: `scaling-cx1.png` is generated by
  `scaling-cx1.py` from `scaling-cx1.dat`; the script fits k from
  the logged timing points and aborts if the fits differ from the
  logged k values. Re-run `python3 docs/video/scaling-cx1.py`
  rather than editing the PNG.
