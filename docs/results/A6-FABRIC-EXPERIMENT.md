# A6 Fabric Experiment — Results (Template + Synthetic Sample)

> Status: TEMPLATE — every number in §§ Question–Diagnosis below is SYNTHETIC SAMPLE (4 utts, 11 ref words) except where marked REALDATA. No synthetic number is a Librispeech result.

Bead: `hearsay-four-7ibb--a6-results-template`. Scope: `docs/results/A6-*` only. Charts regenerate from the TSV via one command (see Reproduce). TSV schema (fixed): `budget arm WER S D I RTF peak_live peak_dormant late_rescues dWER CI_lo CI_hi`.

## Question

Does dormant-hold + late revival (arm C) beat hard prune (arm B) on WER at fixed budgets, without RTF blowup, on lattices where the correct word is present but acoustically low-rank?

<!-- REALDATA: restate the frozen real-data question population (DEV-200/TEST-200) + success rule here -->

## Method

### Arms A/B/C

- Arm A: max-`acoustic_llr` path (Viterbi 1-best, `experiment::scoring`). No budget. Baseline.
- Arm B: real bridge (`hearsay::decode_lattice`) with native `hearsay::Arm::B`, fixture bigram as `SequenceScorer` in the sample; hard prune at the live cap, dormant 0.
- Arm C: same bridge with `hearsay::Arm::C`; pruned candidates held dormant up to the dormant cap and revived on late right-context.

<!-- REALDATA: canonical AM/LM/lexicon shas + tunefull winner cell for all real runs -->

### Budgets

Budgets: {50, 100, 250, 500} live units (contract §12). Arm B: live=B, dormant=0, combined=B (counts live only). Arm C: live=B, dormant=B, combined=B (live+dormant share the total). Sample peaks bind at 50/100 (peak live 50/100) and saturate at 101 for 250/500.

### Commit rule

Per-utterance commit is the max-score tiling under the arm's budgets (acoustic + sequence scores, `DecisionParams`: w_acoustic 1.0, w_sequence 1.0, theta_live 0.0, delta 0.0, H 10 / H_max 20 / J 3 in the sample). B commits the best live tiling; C may commit a tiling containing a revived token if its full-context score wins. Ties break toward fewer errors, then lower arm letter (deterministic).

## Oracle sweep

Sample oracle (exact DP `a2::oracle::sweep_k`, K=3/5/10) over the 4-utt synthetic lattices:

| K | oracle errs | oracle WER | tokens | arcs |
|---|-------------|------------|--------|------|
| 3 | 2 | 0.1818 | 15 | 11 |
| 5 | 1 | 0.0909 | 19 | 15 |
| 10 | 1 | 0.0909 | 25 | 17 |

Attribution (sample): at K=3 the oracle misses `cat` (rank 5 — K too small; K>=5 recovers it). `blue` (u4) is absent at every K: generation miss, fixable only at lattice generation.

<!-- REALDATA: DEV-200 oracle errs/WER/tokens/arcs at K=3/5/10 from sweep_real + per-utt TSV path -->

## Results table

Sample synthetic budget table (from `A6-sample.tsv`, 11 ref words; RTF is one-run wall time, not pinned):

| budget | arm | WER | S | D | I | RTF | peak live | peak dormant | late rescues | delta-WER(C-B) + 95% CI |
|---|---|---|---|---|---|---|---|---|---|---|
| 50 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 50 | B | 0.2727 | 3 | 0 | 0 | 0.0731 | 50 | 0 | 0 | -- |
| 50 | C | 0.1818 | 2 | 0 | 0 | 0.0893 | 50 | 2 | 0 | -0.0909 [-0.2500, +0.0500] |
| 100 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 100 | B | 0.1818 | 2 | 0 | 0 | 0.1269 | 100 | 0 | 0 | -- |
| 100 | C | 0.0909 | 1 | 0 | 0 | 0.1584 | 100 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |
| 250 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 250 | B | 0.1818 | 2 | 0 | 0 | 0.1310 | 101 | 0 | 0 | -- |
| 250 | C | 0.0909 | 1 | 0 | 0 | 0.1670 | 101 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |
| 500 | A | 0.1818 | 2 | 0 | 0 | 0.0000 | n/a | n/a | -- | -- |
| 500 | B | 0.1818 | 2 | 0 | 0 | 0.1300 | 101 | 0 | 0 | -- |
| 500 | C | 0.0909 | 1 | 0 | 0 | 0.1663 | 101 | 5 | 1 | -0.0909 [-0.2000, +0.0000] |

Story in the sample: at budget 50 the cap binds and C cannot rescue (`cat` stays dormant); at >=100 C revives `cat` on `sat` right-context (+4.0) and holds the 1-err line while B stays at 2 errs.

<!-- REALDATA: DEV-200/TEST-200 arm x budget WER/S/D/I/RTF/peaks from real_table (same columns) -->
<!-- REALDATA: delta-WER(C-B) + paired-bootstrap 95% CI per budget (10k resamples, pinned seed) -->

## Per-budget chart (WER vs budget by arm)

Sample charts generated from `A6-sample.tsv` (stdlib-only, no matplotlib):

![A6 WER vs budget by arm](A6-wer-vs-budget.svg)

![A6 RTF vs budget by arm](A6-rtf-vs-budget.svg)

Regenerate: `python3 docs/results/A6-charts.py --sample` (writes both SVGs next to the TSV). Real TSVs with the same schema render via `--input real.tsv --outdir docs/results`.

<!-- REALDATA: regenerate both SVGs from the real TSV and replace the sample embeds (same filenames) -->

## Late-rescue analysis

Stub (sample definitions; real counts drop in below):

- Definition: a late rescue is a committed word that arm B pruned (`Pruned{w}`) at intake but arm C held dormant and later emitted (`Revived{w}`) before commit.
- Sample: 1 rescue (`cat` in u1) at every budget >=100; 0 at budget 50 (cap binds); u2/u3 0 (no prune); u4 0 (absent — nothing to revive).
- Per-utterance diff B vs C (sample): u1 B `the bat sat down` (1 sub) vs C `the cat sat down` (0); u2/u3 identical 0 errs; u4 both `red green yellow` (1 sub, generation miss).

<!-- REALDATA: per-utterance late-rescue counts + B-vs-C transcript diffs on DEV-200/TEST-200 -->

## Diagnosis

Stub (sample attribution; real diagnosis drops in below):

- Generation miss: `blue` absent from the lattice at every K — no search (B, C, or oracle) can recover it. Fixable only at lattice generation (AM/trie), not by budgets.
- K depth: K=3→5 recovers `cat` (rank 5); sample headroom is 1 word in 11.
- Budget binding: peaks track caps at 50/100, saturate at 101 above — the fabric honors budgets; the 50-cap rescue failure is expected cap behavior, not a defect.
- Significance: C's sample gain (-0.0909) has CI including 0 at n=4 — reported as measured, not significant.

<!-- REALDATA: generation-miss rate + K-depth + backbone/SIL + budget-binding attribution on real lattices -->

## Reproduce

From the repo root:

```bash
python3 docs/results/A6-charts.py --sample
# → writes docs/results/A6-wer-vs-budget.svg + docs/results/A6-rtf-vs-budget.svg
grep -c REALDATA docs/results/A6-FABRIC-EXPERIMENT.md  # >= 5
```

Real-data render (same schema): `python3 docs/results/A6-charts.py --input path/to/real.tsv --outdir docs/results`.

---

## Preserved record (wave-2 partial, kept verbatim)

# A6 Fabric Experiment — Wave-2 Real-Data Verdict (PARTIAL)

Bead: `hearsay-four-ic1w--fabric-experiment-arms` (wave 1 lane
`bead/ic1w-fabric-experiment-arms`; wave 2 lane
`bead/ic1w-fabric-experiment-real`). Status: PARTIAL — arm A, the DEV
oracle sweep, and lattice generation are REAL (measured 2026-11-03,
this doc); arms B/C and DEV tuning are BLOCKED on bridge cost
(measured below) pending a decision-engine cost-model fix.

## Canonical config (all real runs)

Every real number below uses one frozen search, the tunefull winner
(`data/wer71g/tunefull/lm6_wip0`, DEV WER 0.6624):

| item | value |
|---|---|
| AM | `data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json`, sha256 `6620a3dd9b8520bdbb756007e606ab9c734b593b7d1c95589d50d4d7ff416bab` |
| LM | `data/lm-text/r1v2-full/lm-r1v2-interpolated.json`, sha256 `1b31acf661ea0b35b73796a78e803b5011add3c5b3f78614663757856471b6f6` |
| lexicon | tier1 better-voice `Master_English_Phonetics_FINAL_COMPLETE.txt` + tier2 `data/lexicon/librispeech-lexicon.txt` |
| vocab | top_n 20000 + book types: kept 22125 words / 22135 prons (tier1 20753, tier2 1372, oov_lm 0, oov_book_only 180) |
| search | lm-weight 6, word-penalty 0, beam 150, max-tokens 2000, CMVN on |

## Data sets (real)

- DEV-200: `data/wer71g/tune200s/strat200.manifest.tsv` (existing,
  stratified-200 dev-clean, 200 rows, 4579 ref words).
- TEST-200: `data/wer71g/test200s/strat200.manifest.tsv` (NEW, sampled
  2026-11-03): `sample-manifest
  data/librispeech/wav/test-clean.manifest.tsv
  data/wer71g/test200s/strat200.manifest.tsv 5 20261103` →
  `in_rows=2620 speakers=40 per_speaker=5 seed=20261103 out_rows=200`
  (no SHORT speakers), 4530 ref words. Rows are `id, relpath, dur,
  UPPERCASE transcript`, same shape as DEV.

## Lattice generation (once per set)

New example `src/decoder/examples/lattices_manifest.rs`: it mirrors the
`wer_devclean` setup exactly (same vocab build, lexicon, LM handling,
config flags) so arm A and arms B/C share one search, then runs
`decode_kbest` per manifest row, sets `utterance` to the utt id, and
writes `<utt>.lat.jsonl`. K per end frame is the decoder const 10
(`KBEST_PER_END_FRAME`, `src/decoder/src/moore.rs`); each word token
gets a SIL twin (llr 0.0).

| set | files | tokens | arcs | bytes | shard RTF |
|---|---|---|---|---|---|
| DEV-200 `data/lattices/dev200/` | 200 | 3,285,300 | 3,002,548 | 421M | 1.14–1.25 |
| TEST-200 `data/lattices/test200/` | 200 | 3,461,782 | 3,185,950 | 445M | 1.12–1.20 |

Six shards each (`--skip 0/34/68/102/136/170 --limit 34`, niced, ≤6
procs), exit 0, no panics, no tiny files. Both sets are generated
exactly once and frozen; neither is committed to git (lane worktree
only, for coordinator extraction).

## Arm A (1-best baseline, real)

`wer_devclean` with the canonical config over each manifest; S/D/I via
the project scorer `data/wer71g/tune200/score_sdi.py`; CI is the
binary's own paired bootstrap (10k resamples, pinned seed).

| set | WER | 95% CI | S | D | I | ref | hyp | RTF | ref_oov |
|---|---|---|---|---|---|---|---|---|---|
| DEV-200 | 0.6624 | [0.6383, 0.6868] | 2413 | 338 | 282 | 4579 | 4523 | 0.963 | 0.0306 |
| TEST-200 | 0.6430 | [0.6182, 0.6687] | 2306 | 291 | 316 | 4530 | 4555 | 1.085 | 0.0302 |

DEV reproduces the recorded tunefull cell bit-for-bit (3033 errs,
`mismatches=0`), validating the whole sampling → decode → score
pipeline. TEST-200 arm A is a first-ever score for this population.

## Oracle sweep (DEV-200, real lattices)

Exact tested oracle (`experiment::oracle_sweep` → `a2::oracle::sweep_k`,
example `src/experiment/examples/sweep_real.rs`) over DEV-200 with the
filter_k 1-best carve-out (`a2::oracle::acoustic_best_path_ids`,
bead hearsay-four-7i2o--real-wiring-tuning-sweep). 1-best column is
`one_best_path` over the same lattices (acoustic-max path: 3116 errs,
0.6805 — +83 over the decoder 1-best 3033, as expected: the lattice
stores acoustic LLRs only, so the acoustic path cannot reproduce the
LM-weighted search best). The oracle is at most the acoustic 1-best on
all 200 utterances at every K (0 exceedances; pre-fix: 129/200 at K=3).

| K | oracle errs | oracle WER | tokens | arcs |
|---|-------------|------------|--------|------|
| 3 | 2775 | 0.6060 | 494,993 | 314,606 |
| 5 | 2731 | 0.5964 | 822,782 | 597,635 |
| 10 | 2637 | 0.5759 | 1,642,926 | 1,501,260 |

Provenance: `sweep_real --manifest
data/wer71g/tune200s/strat200.manifest.tsv --lat-dir data/lattices/dev200`
in lane bead/7i2o-real-wiring-tuning-sweep; per-utterance table
`scripts/a6/out/oracle_dev200_perutt.tsv` (lane worktree, coordinator
extraction; `data/wer71g/dev200sweep/per_utt.tsv` is the superseded
pre-fix run).

Gap attribution:

- Generation miss: 700/4579 ref words (15.3%) never appear in the
  lattice at any rank. Only 140 are OOV — ~560 in-vocab words are never
  hypothesized, i.e. the weak AM never surfaces them. The tunefull
  beam-300 cell is byte-identical to beam-150, so search is saturated:
  the miss is AM/trie-pruning under weak acoustics, not beam width.
- K depth: K=3 → K=10 recovers 138 errs (correct words ranked 4–10
  acoustically). Small: with the carve-out all three rows are fair
  oracles, and the K=10 row (0.5759 vs decoder 1-best 0.6624) is the
  real ~0.09 gap.
- Backbone/SIL (FIXED this lane): `filter_k` previously kept top-K per
  end frame by acoustic LLR with no 1-best carve-out for decoder
  lattices (the carve-out only matched source `a2-1best`), and SIL
  twins at llr 0.0 displaced negative-LLR words — the oracle exceeded
  1-best on 129/200 utterances at K=3. The acoustic-max-path carve-out
  plus regression test `a2::oracle::tests::oracle_at_most_1best`
  closes this; exceedances are now 0/200 at every K.
- Verdict: the gap is real but AM-bound. No decoder/search fix is
  available in-lane (beam saturated, AM is the project baseline), so
  no regen loop was run; TEST is reported as-is with this diagnosis,
  per the bead's defect-loop rule.

## Arms B/C and DEV tuning — BLOCKED (measured)

No B/C or tuning numbers exist: `hearsay::decode_lattice` does not
complete on real lattices. Scaling probe (new example
`src/hearsay/examples/bridge_scale.rs`, release, 1 core, lattice
`data/lattices/dev5/1272-128104-0001.lat.jsonl` with 9510 tokens /
7396 arcs, real 72 MB LM loaded in 6.5 s) over token-prefix
truncations with ZERO arcs kept:

| K tokens | wall |
|---|---|
| 10 / 25 / 50 | ≤0.02 s |
| 100 | 0.27 s |
| 150 | 1.24 s |
| 200 | 4.00 s |
| 300 | 18.24 s |
| 500 | 137.08 s |
| 9510 (full) | TIMEOUT >900 s |

That is ~O(n⁴) (2× tokens → ~15× wall), so a full 9.5k-token
utterance extrapolates to ~1e7 s — against a need of 400 utterances ×
2 arms × 4 budgets. Corroboration: an independent `hearsay-bridge`
run on the same lattice passed 18 min CPU with no output, and
`cargo test -p hearsay` (release, this lane, tiny fallback LM) fails
to finish the two dev5 gate tests in 900 s — the hang is
LM-independent engine asymptotics, not data size alone.

Root-cause pointers (`src/fabric/src/decision.rs`): nested linear
scans — `best_tilings` filters frames × candidates and runs
`active_sequence_max` / `active_unary_max` (each a scan over ALL
evidence) per DP extension, once per region-end tried;
`liveness` is O(tokens × evidence) inside `min_by` comparators; and
`enforce_budgets` runs after every intake step (W3 contract). A fix
is a cost-model redesign (evidence indexing, incremental liveness,
batched eviction), owned by the fabric/bridge beads — not this
runner lane, so per orders the fabric work stops here.

Why wave 1 stayed green: `git merge-base --is-ancestor` proves the
dev5 commit (72857ad) is NOT in wave-1 history (6b15ded) — wave 1
tested against the `synthetic_five` fallback (tiny lattices), and no
lane or main run has ever completed the bridge over real lattices.
Wave 2 is the first attempt.

Post-fix assets (ready in this lane): the frozen TEST-200/DEV-200
lattice sets above; the `real_table` example
(`src/experiment/examples/real_table.rs`, builds green — takes
manifest + lat-dir + LM + wer_devclean cell.tsv + tuned params,
reuses the fixture-table scoring/CI/rescue definitions, and reports
cell-vs-lattice 1-best agreement); the `sweep_real` example; and the
tuning plan — coordinate descent on DEV arm B at budget 250 over
w_acoustic × w_sequence × theta_live × delta × H (arm C identical),
verified across all four budgets. Suggested gate for the fix bead:
the dev5 lattice decodes in <60 s release and the hearsay suite goes
green on main with the shipped LM.

## Wave-1 fixture demo (kept, synthetic)

Everything below this line is the wave-1 fixture demo, kept verbatim
for the record. It claims the 4-utterance synthetic population only —
it is NOT a Librispeech result.

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

## Reproduce (real data)

From the wave-2 lane root (`bead/ic1w-fabric-experiment-real`), release
profile, shared target dir (never `--target-dir`), niced, ≤8 threads;
`$MAIN` is the main workspace (audio, models, LM are main externals,
read-only from lanes):

```bash
# 1. TEST-200 sampling
cargo build --release -p corpus --bin sample-manifest
sample-manifest $MAIN/data/librispeech/wav/test-clean.manifest.tsv \
  data/wer71g/test200s/strat200.manifest.tsv 5 20261103
# → in_rows=2620 speakers=40 per_speaker=5 seed=20261103 out_rows=200

# 2. Lattice generation (one shard shown; all six: skip 0/34/68/102/136/170)
cargo build --release -p decoder --example lattices_manifest
lattices_manifest --am $MAIN/data/models/r3v2-full93-cmvn/ckpt-8mix-iter2.json \
  --lm $MAIN/data/lm-text/r1v2-full/lm-r1v2-interpolated.json \
  --tier1 /home/michael/repos/better-voice/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt \
  --tier2 $MAIN/data/lexicon/librispeech-lexicon.txt \
  --wav-root $MAIN/data/librispeech/wav \
  --manifest $MAIN/data/wer71g/tune200s/strat200.manifest.tsv \
  --out-dir data/lattices/dev200 --skip 0 --limit 34 \
  --lm-weight 6 --word-penalty 0   # TEST: manifest data/wer71g/test200s/…, out data/lattices/test200

# 3. Arm A (DEV shown; TEST swaps manifest/out)
wer_devclean --am …/ckpt-8mix-iter2.json --lm …/lm-r1v2-interpolated.json \
  --tier1 … --tier2 … --wav-root $MAIN/data/librispeech/wav \
  --manifest $MAIN/data/wer71g/tune200s/strat200.manifest.tsv \
  --out data/wer71g/armA/dev200-cell.tsv --lm-weight 6 --word-penalty 0
python3 $MAIN/data/wer71g/tune200/score_sdi.py data/wer71g/armA/dev200-cell.tsv \
  $MAIN/data/wer71g/tune200s/strat200.manifest.tsv   # S/D/I + cross-check

# 4. DEV oracle sweep
cargo build --release -p experiment --example sweep_real
sweep_real --manifest $MAIN/data/wer71g/tune200s/strat200.manifest.tsv \
  --lat-dir data/lattices/dev200 --out data/wer71g/dev200sweep/sweep.md \
  --per-utt data/wer71g/dev200sweep/per_utt.tsv

# 5. Quality gates (observed green)
cargo test --release -p experiment   # 9 passed, 0 failed
cargo clippy --release -p experiment -p decoder --all-targets -- -D warnings
cargo fmt -p decoder -p experiment -p hearsay --check
```

Post-fix, arms B/C run via `real_table --manifest … --lat-dir … --lm …
--cell … --out … --per-utt … --params w_ac,w_seq,theta,delta,h,hmax`
(builds green; hangs pre-fix like every bridge run).

## Plausibility gate (real data)

- WER below 1.0: holds (DEV-A 0.6624, TEST-A 0.6430, oracle K=10 0.5809).
- Training size stated: AM `r3v2-full93-cmvn` trained on LibriSpeech
  train-clean-100 (100 h by corpus definition), 93-phone set; LM
  `r1v2-full` interpolated (general + book). Model shas in the config
  table above.
- Test population matches the claim: TEST claims cover the stratified
  TEST-200 test-clean set (5/speaker × 40, seed 20261103); DEV claims
  cover the stratified DEV-200 dev-clean set. No other population is
  claimed.
- Paired bootstrap CI present: holds for arm A (10k resamples, pinned
  seed, tabulated). The delta-WER(C−B) + CI cannot exist until the
  bridge runs — the B/C half of the gate is BLOCKED, not waived.

Architect review of this doc is still required before the bead closes
(coordinator).
