# Hearsay-IV — Final video, recording, and submission plan (Nov 4)

Times are CST. Submit by 19:00 CST (deadline 20:00 CST / 21:00 ET).

---

## Part 1 — Script fixes (why each changed)

| Seg | Old line | Problem | New line |
|---|---|---|---|
| a | "Every dictation system fails the same way." | Overclaim | "My dictation tool fails the same way, every time." |
| b | "the one it dropped was often the right one" | Sources say ratings were unreliable, not "often" | "and sometimes the one it dropped was the right one" |
| c | Shot uses 4 synthetic fixture transcripts as if real | Misleading | Show the real showcase utterance; say plainly the transcripts are rough |
| d | "The red bar is the old bridge. The green bar is the Fabric." | Chart has three bars (red before, green release, blue debug) | "Red is the old decision layer. Green and blue are the rebuilt Fabric." |
| e | "the correct words were never the dormant ones" | The diagnostic is measured at commit time only | "at commit time, the correct words weren't among the dormant ones" |
| f | "Fix family back to fly … re-decode obeys it" | Never recorded; on the fabric path the correction did not change the commit | Use the real decoder-path demo (TRIFLE ODDS → ODD), 7 of 16 fixed, exact undo |
| end card | "mechanistic · model-free · offline" | "Model-free" is false: HMMs and n-grams are trained models | "non-neural · inspectable · offline" |
| — | No accuracy number anywhere | Judges score "whether it works" | Add the honest numbers in (e) |

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

---

## Part 4 — How to record and edit (Windows, all free)

1. **Screen capture:** OBS Studio (obsproject.com). One "Display Capture"
   source, 1920×1080, 30 fps, output MP4. Record shots A–G as separate
   clips. (Quick alternative: Win+Alt+R records the active window only.)
2. **Voiceover:** after the clips exist, record the Part 2 text in one
   or two takes. In Clipchamp (built into Windows 11), use
   Record & create → Audio, or record in OBS with only the mic source.
   Read slowly; leave 1 s gaps between segments.
3. **Edit in Clipchamp:** drop clips A–G on the timeline in order, the
   voiceover below, trim each clip to its segment, add the B title card,
   the lower-thirds (Text → Lower third) and the end card. Target 2:55–3:00.
4. **Export:** 1080p MP4.
5. **Upload:** YouTube as **Unlisted** (or wherever the submission portal
   asks). Copy the link for the form and the X post.

Time budget: shots 16:00–16:45, voiceover 16:45–17:05, edit 17:05–17:45,
export + upload 17:45–18:00.

---

## Part 5 — Packaging and submission checklist

- [ ] Paste the Part 6 block to the local agent (form/README/X fixes).
- [ ] Rotate the three live credentials; confirm none are in git history.
- [ ] Sign off `docs/RELEASE-KEEP-STRIP.md`; Oyez audio into `data/`,
      bulk JSONs out of the tree.
- [ ] Repo → Public (by 18:00).
- [ ] Video uploaded; link in the form, README and X post.
- [ ] Submission form: title, tagline, track (Indie), problem statement
      ("Build for Your World"), description, Meta tooling (Muse Code
      lanes), repo URL, video URL.
- [ ] Post on X with the clip (counts toward the viral bonus through Nov 8).
- [ ] Submit by 19:00 CST. Screenshot the confirmation.
