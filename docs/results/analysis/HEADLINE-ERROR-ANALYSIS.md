# Headline gate error analysis (LANE K)

Gate: `data/wer71g/tunefull/fullgate/` — 2703 dev-clean utts, lm6/wip0, CMVN AM
(`ckpt-8mix-iter2`), r1v2-full LM. Method: join `cell.tsv` hyps with dev-clean
manifest refs, replicate `score_sdi.py` normalization + Levenshtein backtrace;
recompute matches harness exactly (S=28859 D=4620 I=3296, mismatches=0).

## Headline

WER **0.6760** (36775 / 54402; 95% CI [0.6691, 0.6828] per run.log) · RTF **0.649**
(cell recompute 0.6488) · ref OOV 0.0318 · hyp 53078 words.

## Error mix (S/D/I)

| Type | Count | % of errors |
|---|---|---|
| Substitutions | 28859 | 78.5% |
| Deletions | 4620 | 12.6% |
| Insertions | 3296 | 9.0% |

Substitutions dominate: fixing confusions moves the number, not endpointing.

## Function vs content words

Ref tokens: 26810 function / 27592 content (closed-class list, 100 types).
S+D error rate on function refs 58.8% (15762/26810) vs content refs 64.2%
(17717/27592). Insertions by hyp class: 2019 function / 1277 content.
Top deletions: the, and, a, to, in; top insertions: the (506), and (243), in.
Short function words are confused AND hallucinated — LM-weight territory.

## Top confusion pairs

`the→a` 174, `a→the` 103, `and→then` 78, `and→in` 68, `the→of` 55 —
the top 20 are all function↔function swaps (see `chart1_top20_confusions.png`).
Determiner/conjunction mass confusion, consistent with weak acoustic separation
on short words at this PER.

## Per-speaker spread

40 speakers (id = utt prefix before first dash): min 0.527 (spk 422),
median 0.675, max 0.826 (spk 1272) — a 30-point spread (see
`chart2_speaker_wer_spread.png`). Worst-5: 1272, 6313, 2803, 2412, 652.
Best-5: 422, 6319, 2428, 2086, 3576. Speaker-adaptive front end is the
obvious next lever after LM.

## 30-word visual

`visual_30word_29_29.png` — cross-channel top-1 1.0000, 29/29 scored
(30 attempted, take001 QC-rejected). Numbers from `NUMBER-LEDGER.md` row 5.

## Files

`chart1_top20_confusions.png` (matplotlib barh) ·
`chart2_speaker_wer_spread.png` (matplotlib hist, 12 bins) ·
`visual_30word_29_29.png` (matplotlib text card).
