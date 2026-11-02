# Correction demo without owner audio (lane G)

Bead: `hearsay-four-j1i0--correction-demo`. One command:

```sh
docs/demo/correction-demo.sh
```

The script builds `hearsay-correct` (release, `-j4`), then runs the demo:
for each demo utterance, decode -> why-panel -> apply correction (fabric
USER fact + personal-LM count increment) -> re-decode shows fixed -> undo
restores a byte-identical original; then a 20-utterance collateral report.
Exit nonzero on any failure. All model/audio inputs are read read-only from
the main checkout; the worktree gains only the corrected-LM artifacts under
`target/correction-demo/` (scratch, gitignored).

## Gate headline (read-only)

- Per-utterance cells: `data/wer71g/tunefull/fullgate/cell.tsv` (2703
  dev-clean utterances)
- Columns: `utt`, `errors`, `ref_len`, `hyp_len`, `audio_sec`, `decode_sec`,
  `hyp` (no `ref` column; refs come from the shipped
  `LibriSpeech/dev-clean/*/*/*.trans.txt`)
- Gate result: WER 0.6760, RTF 0.649 (`run.log`: `utterances 2703 ref_words
  54402 errors 36775`, `audio_hours 5.39 decode_hours 3.50`)
- Gate decode recipe (reproduced exactly by the demo bin): AM
  `data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json`, LM
  `data/lm-text/r1v2-full/lm-r1v2-interpolated.json` (personal_weight 0.25),
  top_n 20000, lmw 6, wip 0, CMVN mode

## Selection rule

1. Filter `cell.tsv` to rows with `errors == 1` and `ref_len == hyp_len`
   (single-word substitution shape): 16 rows.
2. Verify each row's hyp against the shipped `.trans.txt` ref at runtime
   (the bin fails loudly on mismatch rather than trusting the filter).
3. Keep utterances where the single user correction (corrected sentence
   counted once, R=1) flips the re-decode to exactly the reference.

The intended rule was "substitution with the correct word present as a
runner-up in the lattice". That rule was **not usable**: the decoder emits
1-best only (see gap below), so there are no recorded runners-up to check.
The operative rule is single-word-substitution-vs-reference, with the
lattice gap recorded in `unresolved`.

## Demo utterances

Probe (`hearsay-correct probe`, R=1) over the 16 single-sub candidates: 7
FIXED, 3 PARTIAL (sub slot fixed, but the global LM renormalization inserts
a stray sentence-final word elsewhere: `1462-170145-0012`,
`3752-4944-0045`, `422-122949-0017` each gain a trailing `AND`/`AS`), 6
NOFLIP. The demo keeps 4 FIXED utterances (one per book chapter):

| utt | before (hyp) | after (ref) | corrected trigram 0->1 | interp. P before->after |
| --- | --- | --- | --- | --- |
| 2428-83699-0004 | ... TRIFLE ODDS | ... TRIFLE ODD | a trifle odd | 2.345e-5 -> 7.533e-2 |
| 251-118436-0001 | ... NOT POISONED SHE CRIED | ... NOT POISON SHE CRIED | is not poison | 2.928e-6 -> 1.543e-2 |
| 3170-137482-0033 | ... WERE SOUNDED | ... WERE ASTOUNDED | friends were astounded | 1.516e-5 -> 3.071e-2 |
| 3752-4944-0041 | ... THE GROUSE SMELLS | ... THE GRASS SMELLS | delightful the grass | 2.861e-4 -> 3.087e-2 |

All four corrected words were already in the 22125-word gate vocabulary
(vocab size unchanged 22125->22125); the single counted sentence supplies
the missing trigram (0->1 in the personal component) and the interpolated
probability jump flips the 1-best. Each correction also records a
`PRON_VARIANT` fact (hyp phones != corrected-word prons in all four).

## Correction machinery (existing, used as-is)

- `fabric::correction::CorrectionLedger::apply` asserts the four-item fact
  set: USER justification on the corrected `WORD_TOKEN`, personal-LM count
  increments, `PRON_VARIANT` evidence iff the observed phones disagree with
  every lexicon pronunciation, MAP deltas (empty here: no frame stats cross
  the decoder-direct path). `undo` retracts the event fact and drops the
  record; `state_bytes` returns bit-identically.
- The personal-LM increment is folded into a rebuilt interpolated LM: the
  personal component's `InspectableModel` counts plus the corrected
  sentence's unigram/bigram/trigram contributions (exact
  `KnTrigram::train` keying: `<s>` never counted, `</s>` counted once),
  rebuilt via `KnTrigram::from_inspectable` (discounts/continuations
  re-derived deterministically by `finalize`), remixed at the gate
  personal_weight 0.25, saved as a standalone LM artifact, and reloaded
  through the same `load_transcriber` path as the base decode. The
  corrected word enters the decoder vocabulary through the book union when
  it has a lexicon pronunciation.
- Observed phones for the variant check are the hyp word's first tiered-
  lexicon pronunciation (what the acoustics decoded as); lexicon variants
  are the corrected word's pronunciations.

## Why-panel

Per-word panels come from `Transcriber::transcript_json_with_why` (bead
wgit shape + `why_text`). The demo prints the wrong word's panel before
applying the correction. Note the standing caveat: `alternatives` is empty
by construction (1-best Viterbi; losing beam competitors are pruned, not
recorded).

## Results

`docs/demo/correction-demo.sh` (2026-11-02): all 4 fixes verified
(corrected hyp == ref), all 4 undos verified (post-undo lattice bytes
identical to the original; ledger `state_bytes` bit-identical and LM totals
empty after `undo`), collateral 0/20 utterances changed for every one of
the 4 corrected LMs. Gate exit 0 (`DEMO OK`).
