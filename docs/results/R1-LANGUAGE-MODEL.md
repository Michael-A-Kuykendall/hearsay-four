# R1b — LM interpolation, entropy pruning, persistence, perplexity gate (bead hearsay-four-2a0)

Crate: `src/lm` (`lm`), on top of the R1a KN trigram core
([R1A-LM-CORE.md](R1A-LM-CORE.md)). New modules: `interpolate`
(two-component linear interpolation + DEV weight tuning), `persist`
(deterministic archive save/load); `KnTrigram` gains `perplexity`,
`entropy_pruned`, `from_inspectable`, and per-history stat caches
(internal only — R1a golden probabilities unchanged at 1e-9).

Measured run (this doc's numbers, one run):

```
cargo run --release -p lm --bin lm-report -- . /home/michael/repos/better-voice /tmp/lm-r1b-out
```

> RELABEL 2026-10-31 (REFOCUS §3): valid, toy scale. NOTE: `sessions/oyez-pilot/transcript.json` and `sessions/oyez-bulk/10-218_2011/transcript.json` are the same Oyez argument (id 21627), counted twice in LM training — remove the alias.

## Held-out basis (DEV perplexity)

DEV text is scored, never trained on. `load_dev_evaluation_lines`
selects from `data/partitions/dev.json`: the DEV Oyez case
`sessions/oyez-bulk/24-889_2025/transcript.json` (596 sentences) plus
the better-voice `quality_test` TSVs (10 single-word sentences);
`data/session/session.tsv` is the same DEV case segmented and is not
selected, so nothing is double-counted. The loader errors if a
selected path appears in `lm-train-text.json`. Training text still
comes only from `load_lm_train_sources` (D1 LM-train partition, 9
files), so no DEV/PHASE-TEST/LOCKBOX transcript enters LM training.

Basis totals: 606 sentences, 10,566 predicted tokens (each token plus
its sentence-final `</s>`). Perplexity is
`exp(-log_prob / predicted_tokens)` under the R1a normalization.

## Source mixes and DEV perplexity

Component split of the 9 LM-train sources (by path): the
personal/adapted component is the Oyez TRAIN cases (3 files, 1,841
sentences); the general component is the TTS sets (6 files, 2,073
sentences). Pooled trains one KN model on all 3,914 sentences
(20,596 trigram entries, vocab 4,218).

| Source mix | DEV perplexity |
|---|---|
| general-only (TTS) | 2658.6698 |
| personal-only (Oyez TRAIN) | 372.9291 |
| pooled general+personal | 415.9316 |
| interpolated, personal weight tuned on DEV (0.80) | 337.3083 |
| pooled, entropy-pruned to 10,000 trigram entries | 482.7361 |

Weight grid on DEV (`P = (1-λ)·general + λ·personal`), full sweep:
λ=0.00 → 2658.6698; 0.05 → 980.1492; 0.10 → 739.7153; 0.15 → 620.4773;
0.20 → 546.4278; 0.25 → 495.2384; 0.30 → 457.5779; 0.35 → 428.7585;
0.40 → 406.1399; 0.45 → 388.1169; 0.50 → 373.6625; 0.55 → 362.1007;
0.60 → 352.9853; 0.65 → 346.0346; 0.70 → 341.0983; 0.75 → 338.1506;
**0.80 → 337.3083 (minimum)**; 0.85 → 338.8944; 0.90 → 343.6077;
0.95 → 353.0506; 1.00 → 372.9291. The tuned mix beats both components
and the pooled model on DEV. Endpoints reproduce the standalone
component perplexities exactly, as the grid shows at λ=0 and λ=1.

## Entropy pruning

`KnTrigram::entropy_pruned(max_trigram_entries)` scores each trigram
by its total log-likelihood gain over its backed-off bigram
probability on the training counts,
`score = c · (ln P(w|w1,w2) − ln P(w|w2))`, removes the lowest scores
first (deterministic token-key tie-break), then re-estimates
continuation counts, discounts, and history stats. Measured: 20,596 →
10,000 trigram entries; deterministic archive 998,835 → 666,703 bytes
(−33%); DEV perplexity 415.9316 → 482.7361 for the pooled model.

## Persistence

`persist` writes compact JSON archives (`format: hearsay-lm`,
`version: 1`, `kind: kn-trigram | interpolated`) over the inspectable
snapshot; sorted counts/vocabulary make re-training and re-saving
byte-identical. Measured in the run above:

| Archive | Bytes | Load time | Check |
|---|---|---|---|
| pooled KN (`lm-pooled.json`) | 998,835 | 0.0801 s | rebuild byte-identical: true; snapshot equal after reload |
| interpolated (`lm-interpolated.json`) | 1,000,668 | 0.0770 s | DEV perplexity equal after reload (< 1e-9) |

Both loads are under the 2 s gate. sha256 of the measured archives:
pooled `de1584c2b0c9045836f5e0038b568917ffa7b870c865955ccf5effdfbdaa732d`,
interpolated `2b8c343199a2d0394eadb8512c100e37ecb6116031abd2e64c5f529936b512de`
(regenerate with the command above; archives were written to
`/tmp/lm-r1b-out`, not committed).

## Gate

```
cargo test -p lm                                          # 15 passed, 0 failed (8 R1a golden + 7 R1b)
cargo clippy -p lm --all-targets -- -D warnings           # clean
cargo fmt -p lm -- --check                                # clean
cargo run --release -p lm --bin lm-report -- . /home/michael/repos/better-voice /tmp/lm-r1b-out
# DEV perplexity per source mix in the table above; LM loads < 2s;
# rebuild byte-identical; serialize/reload round-trip green
```

R1b tests (`src/lm/tests/r1b.rs`): convex interpolation with exact
endpoints, bad-weight rejection, DEV tuning preferring the matching
component in both directions, perplexity definition, entropy pruning
(budget respected, deterministic, distributions still sum to 1,
smaller archive), persistence round-trip and byte-identical rebuild,
DEV-evaluation/training disjointness.

## Design decisions beyond the bead text (explicit)

1. **Personal component source.** The bead calls the second component
   the personal LM (owner's own writing). No owner-writing text exists
   in the D1 partitions (the only owner session is D0 NOT-VERIFIED
   with audio absent), so the personal slot is trained on the Oyez
   TRAIN cases — the only non-TTS LM-train text — and the general slot
   on the TTS sets. The interpolation mechanism is source-agnostic:
   when owner writing joins `lm-train-text.json`, the same code
   retrains the personal component with no change.
2. **Interpolation convention.** Components keep their own
   vocabularies; each maps OOV words to `<unk>` (standard LM
   interpolation). Endpoints therefore equal the standalone models
   exactly. (A shared-union-vocabulary variant was measured and
   rejected: it gave cross-component words probability 0, scoring
   perplexity 2.2e9 at weight 0 instead of the general model's
   2658.6698.)
3. **Pruning budget and scope.** The bead names no numeric budget;
   the measured run uses 10,000 trigram entries (≈half) and the API
   takes any budget. Only trigram entries are pruned; unigram/bigram
   raw counts stay and lower-order statistics are re-estimated.
4. **DEV basis selection.** DEV perplexity uses the DEV Oyez case
   transcript plus the `quality_test` TSVs as selected from
   `dev.json`; the duplicate segmented `session.tsv` for the same
   case is excluded to avoid double-counting.
