# R1a — LM core (bead hearsay-four-baj)

Crate: `src/lm` (`lm`): partition-guarded ingestion, explicit
normalization, trigram modified Kneser-Ney with inspectable counts.
R1b (interpolation, pruning, persistence, perplexity) is separate.

## Ingested sources (D1 `lm-train-text.json`, 9 files)

All sets are D0 VERIFIED in `docs/results/D0-DATA-INVENTORY.md`
(local, free, no payment/account/password/API key; role licence
recorded there). LM text contains TRAIN-partition transcripts only;
`load_lm_train_sources` errors if any listed path appears in
`dev.json`, `phase-test.json`, or `lockbox.json`.

| Source | Lines | D0 basis |
|---|---|---|
| better-voice `data/bulk_tts_1k/train_1k.tsv` + `val_1k.tsv` | 900 + 100 | TTS, VERIFIED local/free |
| better-voice `data/medical_tts/training_manifest.tsv` | 50 | TTS (espeak), VERIFIED |
| better-voice `data/phonetics_audio/phonetics_training_manifest.tsv` | 20 | TTS, VERIFIED |
| better-voice `data/quality_checked_1k/train.tsv` + `val.tsv` | 900 + 100 | TTS, VERIFIED |
| repo `data/oyez-bulk/10-218_2011/transcript.json` | 643 | Oyez CC BY-NC + attribution, VERIFIED |
| repo `data/oyez-bulk/70-112_1971/transcript.json` | 444 | Oyez CC BY-NC + attribution, VERIFIED |
| repo `data/oyez-pilot/transcript.json` | 643 | same case as 10-218 (TRAIN alias) |

Measured run (`train-lm`): 3914 sentences, vocab 4218, 37056 tokens,
trigram discounts (0.370, 1.968, 1.5), bigram discounts
(0.839, 1.205, 1.625). Inspectable JSON written by the binary holds
sorted unigram/bigram/trigram counts, bigram continuation counts,
both discount sets, vocabulary, and total tokens.

## Normalization (src/lm/src/normalize.rs)

Lowercase; `.`/`!`/`?` split sentences; dashes and all punctuation
except letter-internal apostrophes are separators; boundary
apostrophes stripped; any digit-containing token becomes `<num>`.

## Gate

```
cd src/lm && cargo test        # 8 passed, 0 failed
cargo clippy -p lm --all-targets -- -D warnings   # clean
cargo fmt -p lm -- --check                        # clean
```

Golden corpus ("the cat sat / the cat ran / the dog sat / a cat sat")
asserts seen trigram 0.4475524475524475, backed-off trigram
0.14685314685314682, unseen word 0.03146853146853147, unseen history
0.4242424242424242 (epsilon 1e-9), and distributions summing to 1.
