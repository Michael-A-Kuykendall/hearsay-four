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

> Source: owner RECORDING-PLAN.md Parts 2-3, pasted verbatim 2026-11-04 (replaces the lane draft voiceover, shot list, and timing map).

---

## Part 2 — Final voiceover (read verbatim, ~430 words, ~3:00)

**[a — 0:00–0:18]**
"My dictation tool fails the same way, every time. I say fly. It writes
family. And nothing in it can tell me why. No cause to inspect. No fix
that sticks. Just a wrong word, again, with no reason attached."

**[b — 0:18–0:46]**
"Fifty years ago, Hearsay-Two sketched the answer: a blackboard where
independent experts post competing hypotheses about the speech. But it
had a cost problem. Ratings were unreliable, so the scheduler had to
drop hypotheses it couldn't afford to keep, and sometimes the one it
dropped was the right one. Gone for good. Its rival, HARPY, won by
searching faster and pruning later."

**[c — 0:46–1:16]**
"Hearsay-Four keeps the blackboard and changes what's underneath: a
Saturation Fabric that decides every word. Each hypothesis keeps all its
evidence. Lose one support and the others hold. Fall below threshold and
it goes dormant, kept, not deleted, so late evidence can revive it. Here
it's deciding a real test sentence. The transcript is rough, but click
any word and the why panel shows its score, the two hundred rivals it
beat, the margin, and the Fabric's record. An error with an address."

**[d — 1:16–1:36]**
"And the Fabric scales. Rebuilt as a true saturation loop, its growth
exponent fell from three-point-eight to one-point-oh-seven, near-linear.
It judges up to sixty-seven thousand competing hypotheses per sentence,
faster than real time. Red is the old decision layer. Green and blue are
the rebuilt Fabric."

**[e — 1:36–2:06]**
"Then the experiment we pre-registered: dormancy against pruning,
identical evidence, identical budgets, two hundred held-out sentences.
Fifteen-ninety-nine of sixteen hundred paired runs gave identical
transcripts. No revived hypothesis ever won. At commit time, the correct
words weren't among the dormant ones. So dormancy did not beat pruning,
and we're reporting that null result with its reason. It's honest about
accuracy too: the classical decoder gets sixty-four percent of words
wrong on this set, and the Fabric eighty-five. Scoring equivalence is the
next fix."

**[f — 2:06–2:28]**
"Corrections are facts, not overrides. One correction, trifle odds to
trifle odd, flips the decode. One correction fixed seven of sixteen
single-word errors, and undo restores the exact original state, checked
byte for byte. And on my own thirty-word vocabulary, recorded on a
different setup: twenty-nine of twenty-nine right."

**[g — 2:28–3:00]**
"How was it built? With Muse Code, in parallel lanes. The work was
planned as verifiable tasks, and agents built them at the same time:
acoustics, the Fabric, the experiment, the demo. Gates verified
everything; only green work merged. A reviewer seat caught what agents
missed: a leaky evaluation voided instead of quoted, a scaling bug found
and fixed, every number double-checked. Humans owned the voice, the
verdicts and the submission. The machine owned the parallelism."

---

## Part 3 — Shot list with exact commands

Record each shot SILENT first (screen only), then record the voiceover
once over the edit. Terminal font ≥ 18 pt; browser zoom 125%; 1920×1080.

**Prep (once, in WSL, repo root):**
```bash
cd ~/repos/hearsay-four
CARGO_TARGET_DIR=$HOME/.cargo/target-demo bash docs/demo/verify.sh   # must end GATE GREEN
bash web/demo.sh                                                      # DEMO GREEN
python3 -m http.server 8000                                           # leave running
```

| Shot | Seconds | What to record |
|---|---|---|
| A | 18 | Notepad with one sentence where "fly" came out as "family". Cursor on the wrong word. Slow zoom. |
| B | 28 | Title card (make in Clipchamp): "HEARSAY-II (1970s): THE BLACKBOARD" · "experts post competing hypotheses" · "unreliable ratings → scheduler must drop" · "dropped = gone; HARPY searched faster". |
| C | 30 | Terminal: the `verify.sh` lines for `61-70968-0060` (12 words, 4 revived, 0 late rescues). Then browser `http://localhost:8000/web/transcribe.html`, open the showcase utterance, click `WHOA` (202 beaten alternatives, margin 4.64), then one more word. Lower-third: "Real TEST-200 sentence · Fabric arm C, budget 500". If the page only shows fixtures, record the fabric-path JSON panel from `target/fabric-demo/` in the terminal instead, and label it. |
| D | 20 | `docs/video/scaling-cx1.png` full screen, slow zoom on the green bar. Lower-third: "growth exponent 3.84 → 1.07 (CX1 logs)". |
| E | 30 | `docs/results/A6-FABRIC-EXPERIMENT.md`, headline block (1599/1600, rev_winner 0, ΔWER −0.02 [−0.07, 0.00]). Hold ≥ 5 s. Lower-third: "Dormancy did not beat pruning — measured". |
| F | 22 | Terminal: `bash docs/demo/correction-demo.sh`, hold on the TRIFLE ODDS → TRIFLE ODD line and the `undo verified … byte-identical` line ≥ 4 s each. Then `docs/results/NUMBER-LEDGER.md` row 5 (29/29). |
| G | 32 | `docs/video/timeline.svg` in the browser, slow pan left → right. Last 5 s end card: "HEARSAY-IV — Transcription you can argue with" · "Meta Global AI Developer Hackathon 2026" · "Data: LibriSpeech (OpenSLR 12), CC BY 4.0" · "non-neural · inspectable · offline". |
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
