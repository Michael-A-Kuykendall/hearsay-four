# A6 Fabric Experiment — Fixture Demo Verdict

Bead: `hearsay-four-ic1w--fabric-experiment-arms` (lane
`bead/ic1w-fabric-experiment-arms`). Status: fixture demo GREEN; TEST-200
scoring pending on main (decoder K-best lattices + audio externals).

## Population

4 synthetic utterances, 11 reference words. All words are generic
vocabulary invented for this crate; no corpus, book, or owner text. This
doc claims fixture behavior only — it is NOT a Librispeech result.

| id | reference | lattice | role |
|---|---|---|---|
| u1-revival | the cat sat down | 9 tokens, `cat` rank 5/6 acoustically | B prunes `cat`, C revives it |
| u2-clean | up and away | 3 tokens, single path | all arms 0 errs |
| u3-bulk | alpha | 101 same-span tokens | budgets bind, peaks track caps |
| u4-absent | red green blue | `blue` absent | every arm + oracle err (generation miss) |

## Method

- Arm A: max-`acoustic_llr` path (Viterbi, `experiment::scoring`).
- Arms B/C: real bridge (`hearsay::decode_lattice`) with native
  `hearsay::Arm`, fixture bigram as the `SequenceScorer`, identical
  `DecisionParams` except the arm flag and budget triple.
- Oracle: exact DP oracle reused from `a2::oracle` (`sweep_k`, K=3/5/10);
 no second oracle was written.
- WER via `output::wer`; delta CI via `eval::corpus_delta_ci` (10k
  resamples, pinned seed, 95 pct — bit-identical across runs).
- Budgets: {50, 100, 250, 500} (contract §12).

Arm parameters (fixture defaults; DEV tuning on real data is main work):

| param | value |
|---|---|
| w_acoustic / w_sequence | 1.0 / 1.0 |
| theta_live / delta | 0.0 / 0.0 |
| H / H_max / J | 10 / 20 / 3 |
| B budgets | live=B, dormant=0, combined=B (counts live only) |
| C budgets | live=B, dormant=B, combined=B (live+dormant share the total) |

Reproduce: `cargo test --release -p experiment` (9 gates) and
`cargo run --release -p experiment --example fixture_table`.

## Oracle sweep (fixtures, K=3/5/10)

| K | oracle errs | oracle WER | tokens | arcs |
|---|-------------|------------|--------|------|
| 3 | 2 | 0.1818 | 15 | 11 |
| 5 | 1 | 0.0909 | 19 | 15 |
| 10 | 1 | 0.0909 | 25 | 17 |

Attribution: at K=3 the oracle misses `cat` (rank 5 acoustically — K too
small; K>=5 recovers it). `blue` (u4) is absent from the lattice at every
K: a generation miss, fixable only at lattice generation, not by search.
Oracle never exceeds 1-best errors per utterance (backbone property,
tested).

## Budget table (fixtures)

RTF is wall-clock bridge time over lattice span from one run (varies run
to run; the gate does not pin it). CI values are pinned-seed exact.

| budget | arm | WER | S | D | I | RTF | peak live | peak dormant | late rescues | delta-WER(C-B) + 95% CI |
|---|---|---|---|---|---|---|---|---|---|---|
| 50 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 50 | B | 0.1818 | 2 | 0 | 0 | 0.0731 | 50 | 0 | -- | -- |
| 50 | C | 0.0909 | 1 | 0 | 0 | 0.0893 | 50 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |
| 100 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 100 | B | 0.1818 | 2 | 0 | 0 | 0.1269 | 100 | 0 | -- | -- |
| 100 | C | 0.0909 | 1 | 0 | 0 | 0.1584 | 100 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |
| 250 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 250 | B | 0.1818 | 2 | 0 | 0 | 0.1310 | 101 | 0 | -- | -- |
| 250 | C | 0.0909 | 1 | 0 | 0 | 0.1670 | 101 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |
| 500 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 500 | B | 0.1818 | 2 | 0 | 0 | 0.1300 | 101 | 0 | -- | -- |
| 500 | C | 0.0909 | 1 | 0 | 0 | 0.1663 | 101 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |

## Per-utterance diffs (B vs C)

- u1: B commits `the bat sat down` (1 sub), C commits `the cat sat
  down` (0). B emits `Pruned{cat}` at intake; C holds `cat` dormant,
  emits `Revived{cat}` when `sat` arrives (`cat -> sat` +4.0), and
  commits the cat tiling 21.5 vs 20.0. Late rescue: `cat` at every
  budget. Fixture-design note: the commit DP keeps top-2 partials per
  frame, so the distractors carry adversarial left context (-5.0) to stay
  dormant — otherwise the revived token's partial is dropped before its
  right context arrives.
- u2/u3: identical transcripts, 0 errs, all budgets. u3 binds the small
  caps (peak live 50/100, then 101 unbound).
- u4: both commit `red green yellow` (1 sub). The correct word is absent
  from the lattice: nothing to revive, no cascade possible. Not a fabric
  defect; no rerun warranted.

## Plausibility gate

- WER below 1.0: holds (max 0.1818).
- Training size stated: the fixture demo uses no trained models
  (synthetic lattices + fixture bigram). Main-run models per the bead: AM
  `data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json`, LM
  `data/lm-text/r1v2-full/lm-r1v2-interpolated.json`.
- Test population matches the claim: the claim is the 4-utterance
  fixture population (11 words), stated above — not test-clean.
- Paired bootstrap CI present: holds; the interval includes 0 at n=4
  (one rescued word in 11), so C's fixture gain is not significant —
  reported as measured.

## Main-workspace steps (not in this lane)

1. Sample TEST-200 on main (lane has no Librispeech manifest):
   `cargo run --release -p corpus --bin sample-manifest -- \
   data/librispeech/wav/test-clean.manifest.tsv \
   data/wer71g/test200s/strat200.manifest.tsv 5 20261103`
   then verify 200 rows via `experiment::sampling::sample_test200_rows`.
2. Tune w_acoustic, w_sequence, theta_live, delta, H on DEV for arm B
   (arm C identical); needs decoder K-best lattices (bead noqu, open).
3. Generate TEST lattices once into `data/lattices/test200/`, score
   A/B/C once at {50, 100, 250, 500} with the real LM
   (`hearsay::InterpolatedScorer`).
4. Architect review of this doc before the bead closes (coordinator).
