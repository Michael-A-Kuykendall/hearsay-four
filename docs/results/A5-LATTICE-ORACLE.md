# A5 Lattice Oracle + K Sweep — DEV Verdict

Bead: `hearsay-four-p3o--lattice-format-oracle` (worked in lane L5 Track A
alongside A2; status change left to parent). Oracle + K sweep on the A2
whole-word lattices from `docs/results/A2-CONNECTED-DTW.md` (same DEV-A:
104 concat phrases, 422 tokens, cross-take). Lattices generated ONCE per
acoustic config (K=20 + `a2-1best` backbone); all K conditions read the
same files. Format: v1 `hearsay-word-lattice` JSONL (reader/writer:
`src/lattice`, bead `hearsay-four-g0m--word-lattice-file-format`).

## Method

Exact oracle = min edit distance to the reference over all lattice paths
(DP over DAG × reference positions with open entry/exit; `src/a2/src/oracle.rs`).
K filter: top-K by `acoustic_llr` per end frame + always-kept backbone.
Sweep command (no decode; reads `/tmp/a2-dev-full/lattices/deva/`):

```
cargo run --release -p a2 --bin a2_dev -- \
  --resweep /tmp/a2-dev-full/lattices/deva \
  --phrase-list prompts/script-a2-phrases.txt \
  --onebest-csv /tmp/a2-dev-full/per_phrase_deva.csv
```

1-best reference: 24 errs / 422 refs = **0.0569**.

## Oracle WER vs K vs compute (DEV-A, n=104)

| K | oracle errs | oracle WER | gap vs 1-best (pts) | mean tokens | mean arcs |
|---|-------------|------------|---------------------|-------------|-----------|
| 3 | 19 | 0.0450 | 1.19 | 924 | 58,028 |
| 5 | 19 | 0.0450 | 1.19 | 1,373 | 136,977 |
| 10 | 19 | 0.0450 | 1.19 | 2,197 | 367,412 |
| 20 | 19 | 0.0450 | 1.19 | 2,994 | 681,271 |

- Sweep is FLAT: top-3 already contains every oracle-fixable error; K>3
  adds only lattice bulk (arcs grow 12× from K=3 to K=20 for zero gain).
- Gap: oracle−1best = **−0.0118**, 95% CI **[-0.0234, −0.0024]**
  (`eval::corpus_delta_ci`, B=2000, seed 20261031): significant, tiny.
- 5 of 24 1-best errors are lattice-recoverable; 19 persist under perfect
  selection (correct word absent from the lattice at K=20).
- Chosen K: **3** (same oracle as K=20 at 1/12th the arcs).

## Verdict: NO-GO for A6 (acoustic-bottleneck per p3o gate)

Gate (p3o acceptance): chosen K has oracle WER ≥5 points below
condition-B WER, else record acoustic-bottleneck verdict and block
`hearsay-four-cjj--fabric-pruning-experiment`.

- Observed gap: **1.19 points** (0.0569 → 0.0450) < 5 points. **RED.**
- Attribution: ceiling effect + generation misses. 1-best is already 0.057,
  so a 5-point gap needs oracle ≤0.007 (≤3 errors of 422); the lattice
  misses 19. Fabric rescoring (A6) can gain at most 1.2 points on this DEV.
- `hearsay-four-cjj--fabric-pruning-experiment` stays BLOCKED behind this
  verdict (status change left to parent).
- Robustness: search saturates max_live 8000, so wider search might add a
  few more alternatives — but closing a 3.8-point shortfall (19→3 errors)
  by search budget alone is implausible; the misses are acoustic
  (genetic/rhubarb/kinship hubs, same as A1).

## Canonical-audio confirmation (main, 2026-10-31)

Same sweep on canon-template lattices (`/tmp/a2-dev-canon/lattices/`):
1-best 0/422, oracle 0/422 at K=3/5/10/20, gap 0.0000, CI [0,0].
NO-GO verdict confirmed and strengthened (zero recoverable errors —
there is nothing for fabric rescoring to fix on this DEV).

Architect review 2026-10-31: APPROVED. K=3 choice stands (moot at zero
gap; keep for future non-ceiling data).

## Notes

- Oracle ≤ 1-best holds by construction (backbone + open entry/exit;
  regression tests `oracle_enters/exits_mid_lattice_without_prefix/suffix_penalty`).
  An earlier restricted-entry bug made DEV-B oracle (0.19) exceed 1-best
  (0.00); fixed, DEV-B oracle is now 0 at all K.
- Arc density is extreme (681k mean arcs at K=20; 2.0 GB for 104 lattices):
  gap tolerance admits near-duplicate spans. Future lattice work should
  prune arcs (span dedup / posterior threshold) before any A6 rerun.
- Lattices + CSV in `/tmp/a2-dev-full/` (NOT in repo); regenerate via the
  A2 command, then the sweep command above.
