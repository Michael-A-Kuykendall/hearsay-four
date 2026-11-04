# Submission demo from the fabric path

Bead: `hearsay-four-sq9l--fabric-path-demo`. One command (from the repo root):

```sh
CARGO_TARGET_DIR=$HOME/.cargo/target-demo bash docs/demo/verify.sh
```

The script builds `hearsay-bridge`, `hearsay-transcribe`, and
`hearsay-correct` (release, isolated lane target dir), decodes three
TEST-200 lattices through the decision fabric (arm C, budget 500),
renders transcript JSON with per-word why panels, asserts the three
gate groups, and exits 0 on full PASS (`GATE GREEN`). All model,
lattice, and lexicon inputs are read read-only from the main checkout;
the worktree gains only build output and scratch under
`target/fabric-demo/` (gitignored).

SCIENCE FROZEN: demo only. No `src/fabric` changes, no experiment
reruns. The only code changes on this lane are the `fabric-demo`
subcommand in `src/runner/src/bin/hearsay-correct.rs` (correction +
undo on the fabric path, sharing lane G's apply path) and a generalized
`chosen` label in `src/runner/src/fabric_path.rs` (the old text named
only the fixture population).

## Gate headline (read-only, cited)

- A6 result (`docs/results/A6-FABRIC-EXPERIMENT.md`, 13:00 freeze):
  B/C WER 0.8547-0.8550 at budget 500 vs decoder 1-best (arm A) 0.6430.
  The demo transcripts below are garbled accordingly; this is the
  measured research operating point, not a demo bug.
- `rev_winner = 0` in all 24 A6 C-cells: no revived token ever entered a
  committed tiling. B==C on 1599/1600 rows; the sole differing row is
  7021-79730-0004 @ budget 500, theta -4.2967 (one insertion, live-set
  pressure, not revival; CI touches 0, not significant).
- The demo runs `theta_live 0.0` (bridge default), matching A6's th50
  row, where B==C on all 800 rows. The A6 numbers are cited, not
  re-measured; the demo's own measurements are stated as measured.

## Selection rule

1. Start from the bead's suggested TEST-200 utterances at arm C,
   budget 500 (an A6 C-cell).
2. `7021-79730-0006` (suggested "dense") is not in
   `data/lattices/test200/` (only `-0003` and `-0004` exist for that
   chapter), so the third slot went to a revival showcase instead (rule
   3). The bead's other two suggestions are kept.
3. Full-200 scan (`hearsay-bridge --arm C --budget 500` over every
   TEST-200 lattice, event kinds counted, scratch deleted after): 12
   utterances log `revived` events (15 total), 0 log evictions.
   `61-70968-0060` has the most (4) and is the smallest log, so it is
   the dormancy/revival showcase.
4. Correction demo uses the showcase utterance (12 words, readable
   output) with a disclosed simulated correction (rule below).

## Demo utterances (disclosed as chosen)

| utt | lattice tokens | words | commits | defeat events | revived | role |
| --- | --- | --- | --- | --- | --- | --- |
| 1995-1836-0004 | 67740 | 105 | 20 | 28437 | 0 | long |
| 7021-79730-0004 | 35820 | 48 | 11 | 14720 | 0 | A6 sole-diff row |
| 61-70968-0060 | 7400 | 12 | 3 | 4018 | 4 | revival showcase |

Measured 2026-11-04 (bridge arm C, budget 500). Token counts are
`{"t":` lines in the shipped lattices.

Transcripts (fabric commits, garbled at WER ~0.85 — see gate headline):

- 1995-1836-0004: `THE ASHY WAY TICKER GASH E SURVEYED THE TABLES ...`
  (105 words)
- 7021-79730-0004: `THE BUM OVER THE SUPER J CITIES SO FOR TURN ...`
  (48 words)
- 61-70968-0060: `WHOA THANKS I M GLAD TO KICK A SOOT EASY HAPPY MESSED`
  (12 words)

## Honesty constraint (locked by assertions, not prose)

- The showcase's 4 revival events (`AH` via `SH`, `EH` via `EH`, `OH`
  via `WELL`, `I` via `KNOW`) are shown in the event log and the
  utterance summary — and every one of those tokens was later
  *defeated*, none committed. The gate asserts `late_rescues == []` on
  all three utterances: the demo shows the machinery and must NOT claim
  a revival changed an answer.
- B==C measured on all three demo utterances under the demo bridge
  config (arm B re-run at the same budget; transcripts byte-identical).
  Consistent with A6 th50 (B==C on all 800 rows at theta 0.0); the
  bead's sole-diff row is the th25 cell, which the demo does not run.
- Correction is *simulated*: no test-clean `.trans.txt` refs ship in
  the checkout, so there is no reference to fix toward. The corrected
  sentence is stated exactly (next section), never implied to be a
  ground-truth fix.

## Correction machinery (existing, used as-is)

- `fabric::correction::CorrectionLedger::apply` asserts the four-item
  fact set: USER justification on the corrected `WORD_TOKEN`,
  personal-LM count increments, `PRON_VARIANT` evidence, MAP deltas
  (empty here: no frame stats cross the bridge path). `undo` retracts
  the event fact and drops the record; `state_bytes` returns
  bit-identically. Same apply path as lane G (`hearsay-four-j1i0`);
  the only fabric-specific piece is the 1-best lattice built from
  committed winners (the ledger needs (word, span) pairs, and the
  fabric transcript is the hyp).
- Simulated correction, stated exactly: `61-70968-0060`, index 8,
  `SOOT` -> `SUIT` (both tier-1 cmudict words; observed `S UH1 T` vs
  `S UW1 T`, so a `PRON_VARIANT` fact is recorded: `variant=Some(3)`).
- Measured effect (R=1): `tri[kick a suit] 0->1`,
  `p 2.346e-4 -> 3.082e-2` (131x); corrected LM saved as a standalone
  artifact and reloaded through the bridge path (informational
  re-bridge: transcript UNCHANGED — the commit stands, acoustic
  evidence dominates with SOOT at acoustic LLR 755; also probed R=10,
  `p -> 0.21`, commit still stands).
- Undo: post-undo ledger state bit-identical to pre-apply (`cmp` in
  the gate), LM totals empty.

## Why-panel

Per-word panels come from `runner::fabric_path` (five-element shape:
acoustic score, losing alternatives + why they lost, LM probability,
lexicon source tag, fabric record). On real data the panels are dense:
e.g. showcase word 0 (`WHOA`) shows 202 beaten alternatives with the
defeating 7-token tiling at margin 4.64. The fabric record carries
revivals (empty here — no committed token revived, honestly shown),
the committed tiling + margin, defeat outcome, and the eviction record
(null on these utterances: 0 evictions at budget 500). LM/lexicon
enrichment is the interpolated R1-v2 LM + tiered lexicon (164 cmudict
+ 1 librispeech-g2p tags across the 165 words).

## Results

`docs/demo/verify.sh` (2026-11-04): exit 0 (`GATE GREEN`).

- (a) All 3 transcripts re-derived from DecisionEvent commit records
  (independent join of committed winner ids to lattice words from the
  raw handoff files); 1-best-shaped forgery rejected
  (`path != fabric`).
- (b) 165/165 why panels carry alternatives, defeating tiling + margin,
  and shaped event records; defeat events on all 3 utts; showcase
  `revived=4`, `late_rescues=[]` on all 3.
- (c) Correction/undo round-trip restores byte-identical ledger state
  (`cmp`); corrected-LM artifact present; informational re-bridge
  reports UNCHANGED.
