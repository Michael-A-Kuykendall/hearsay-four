# R3 — Speaker-Independent Base Acoustic Model (bead `hearsay-four-30y--speaker-independent-model`)

Measured: 2026-10-28 in lane `/home/michael/repos/hearsay-four-lane-r3`
(branch `bead/30y-speaker-independent`).
Status: GREEN — `test result: ok. 1 passed; 0 failed; finished in 507.35s`
(release, explicit run; the test is `#[ignore]`d by default).

## Use-verification basis (train path, not skipped)

D0 (`docs/results/D0-DATA-INVENTORY.md`) use-verifies HUMAN audio, so the
bead's train condition holds:

- `libri_mini_spaced`: human (LibriSpeech readers 103-1240/103-1241),
  40 WAVs, CC BY 4.0 upstream, `VERIFIED — local, free, no account/key`,
  D1 role DEV. This is the bead's named example (LibriSpeech subset).
- Oyez TRAIN cases (`10-218_2011`, `70-112_1971`): human (SCOTUS oral
  argument), CC BY-NC + attribution, `VERIFIED`, D1 role TRAIN.
- Oyez DEV case (`24-889_2025`): human, `VERIFIED`, D1 role DEV.

No LOCKBOX / PHASE-TEST / NOT-VERIFIED audio is touched: the gate asserts
every consumed path against `data/partitions/{train,dev,excluded}.json`
and fails on any forbidden fragment (`lockbox`, `phase-test`, sealed
SCOTUS case ids) or any D1-excluded dataset.

## Data

SI TRAIN (human, multi-speaker, D1 TRAIN): Oyez `10-218_2011` (642
segments, 11 speakers) + `70-112_1971` (444 segments, 8 speakers).
`session.tsv` transcripts from the lane checkout (committed text);
`audio/run1.wav` (mono 16 kHz, gitignored) read-only from the main
workspace `/home/michael/repos/hearsay-four` (`HEARSAY_MAIN_ROOT`
override). Segments sliced by `[start_sec, stop_sec)`; lexicon-OOV
segments (case cites, numbers, `--`) skipped for TRAIN.

SI DEV (human read speech, unseen speaker, D1 DEV): `libri_mini_spaced`
(40 utterances, LibriSpeech speaker 103, transcripts from
better-voice `data/manifests/libri_mini_spaced.tsv`). The lexicon-covered
subset feeds PER + likelihoods; all 40 feed WER (OOV reference words
count as errors, OOV rate reported alongside).

Secondary held-out check: Oyez `24-889_2025` covered segments, forced
alignment likelihoods only (no decoding, no selection).

## Method

Trainer: merged R2 machinery — `HmmSet::flat_start` from the TRAIN-only
phone inventory, then `train_checkpoints` (3 log-domain Baum-Welch
iterations — R2b used 6 — checkpoint each iteration, DEV-PER
selection, mixture splits 1->2->4->8 accepted only on DEV-PER
improvement, per-parameter sufficient statistics + provenance).
`var_floor_ratio 0.01`, `optional_sp` as in R2b. TRAIN budget (R3
scope): segments <= 12 s, 350 total round-robin over speakers (175
per case, balanced eras) — the R2 E-step visits every (frame, state)
pair so cost is ~quadratic in utterance length. All 19 speakers and
all 39 phones survive the budget (see gate log).

Word decoder (R3 starting point; R5 builds the trie/LM decoder): uniform
word-loop Viterbi beam search over the closed DEV vocabulary + SIL loop
word, implemented in the gate harness (`build_word_net` /
`decode_words`). Within-phone transitions mirror the R2a decode
convention (self/next 0.5). Fixed a priori, NOT tuned on DEV: beam 200,
max-active 3000, insertion penalty 0, no LM. Fully deterministic
(BTreeMap order, strict-`>` max). WER scored with the canonical
`output::wer` (corpus S+D+I over N).

## Results (gate log `/tmp/r3_gate_final.log`, lane run)

- TRAIN: 350 segs, 14469 phones, 139177 frames, 19 speakers
  (skipped: lexicon_oov 289, too_long 174). Speakers: Kennedy,
  Scalia, Kneedler, Kagan, Garre, Blackmun, Roberts, Clement,
  Stewart, Ginsburg, Alito, Sotomayor, Breyer, Tinglum, Marshall,
  Burger, Brennan, Coffey, unknown (10-218_2011 + 70-112_1971).
- DEV libri: 40/40 loaded, 24 lexicon-covered (PER/likelihood subset).
- DEV oyez (`24-889_2025`): 233 segs, 11 speakers (likelihoods only).
- Phones: TRAIN inventory 39 (full ARPAbet set incl ZH), model 41
  (39 + SIL + SP), DEV phones unseen in TRAIN: none.
- Checkpoints (DEV PER selects iter 4, 2 mixtures):

```
flat PER=0.9943 selected PER=0.8490 (iter 4, 2 mixtures)
split 1->2 before=0.9088 after=0.8490 accepted=true
split 2->4 before=0.8490 after=0.8608 accepted=false
split 2->8 before=0.8490 after=0.8613 accepted=false
ckpt iter=0 mix=1 train_ll=-701110.73 dev_per=0.9943
ckpt iter=1 mix=1 train_ll=-679723.67 dev_per=0.9465
ckpt iter=2 mix=1 train_ll=-627326.40 dev_per=0.9720
ckpt iter=3 mix=1 train_ll=-611013.80 dev_per=0.9088
ckpt iter=4 mix=2 train_ll=-573314.58 dev_per=0.8490 SELECTED
```

- SI DEV PER: flat 0.9943, selected 0.8490 (paired diff mean
  -0.1447, hi -0.1380, n=24 — improvement with CI below 0; reported,
  not gated).
- **SI DEV WER = 1.4363** (S=1799, D=4, I=893, N=1877; decoder vocab
  652 incl SIL, 8733 states, lexicon OOV 25/1877 = 1.3%). High, as
  expected for a starting point: monophones, no LM, uniform word
  loop, spontaneous-courtroom TRAIN vs read-speech DEV. No threshold
  per the plan; this number is the floor R4/R5 improve on.
- Likelihoods: finite on all 24 covered libri + 233 oyez DEV
  utterances (min ll -140487.93).
- Phone-decode smoke: 174/149/147 phones on 3 sampled DEV utterances.
- Word-decode smoke: all 40 DEV utterances decode without error
  (8733-state beam search, deterministic).
- Provenance: 33664 real-TRAIN ids, 2100 flat-residue ids; no foreign
  ids; `validate_provenance` green.
- Model: `models/si_base/model.json` (3096141 bytes,
  sha256 `16f3ccc34ca41ec3e1f4a49c102ab61f795677d57d9cdaa525f9ca9b19f1dbbd`)
  + `models/si_base/model.bin` (1504170 bytes,
  sha256 `fc642360f92f0ac20712eac4cffc78adcb30e53348fc469d6de5b8b8ed89440a`);
  text and binary round-trips assert equal in-test.

## Gate checklist (bead text)

- All expected phones represented in trained model: GREEN (39/39 +
  SIL/SP; DEV-unseen set empty).
- Model loads: GREEN (JSON + binary round-trip equality in-test).
- Held-out utterances produce finite likelihoods: GREEN (24 + 233).
- Decoder smoke succeeds: GREEN (phone decode + 40/40 word decode).
- SI DEV WER reported here: GREEN (1.4363, S/D/I/N above).

## Beyond the bead text (explicit)

1. TRAIN is Oyez spontaneous courtroom speech, not read speech: the only
   D1-TRAIN human audio with transcripts. LibriSpeech read speech is D1
   DEV by batch rule, so training on it would violate the partition.
2. Checkpoint selection uses the same DEV set the WER is reported on
   (40 utterances are too few to split select/eval); the WER is
   therefore mildly optimistic and is reported as a starting point with
   no threshold, per the plan.
3. Decoder has no LM and no learned-transition consumption (matches the
   R2a decode convention); SP is trained but not used in the word loop.
4. TRAIN budget (350 segs <= 12 s, 3 BW iters instead of R2b's 6): the
   R2 E-step visits every (frame, state) pair, ~quadratic in utterance
   length. Full-data scale-up (695 segs, 6 iters) is follow-up work.
5. The gate is `#[ignore]`d by default (~9 min release; debug would
   take hours) so `cargo test --workspace` stays fast. Run explicitly.
6. E-step allocation fix in `src/hmm/src/lib.rs` (`baum_welch_ids`,
   this branch): the gamma loop cloned a full `Mixture` (Gaussians +
   stats + provenance utterance lists) per (frame, state) and
   re-cloned accumulator keys and utterance ids per visit — real-data
   training took hours. The fix hoists per-state mixture borrows,
   accumulator keys, and the emit denominator (bit-identical values:
   emissions are frozen during the E-step) and inserts each
   (component, utterance) provenance id once (same set content).
   Numerics verified IDENTICAL: R2b gate rerun in debug prints the
   recorded main numbers exactly (flat PER 0.9938, selected 0.8127,
   iter-0 LL -273025.12), and all 12 hmm unit tests pass.

## Reproduce

```
cargo test --release -p hmm --test r3_si_base_tests -- --nocapture --test-threads=1 --include-ignored
```

Requires `/home/michael/repos/better-voice` (libri audio, transcripts,
phonetics table) and `/home/michael/repos/hearsay-four` (Oyez
`audio/run1.wav`, read-only) or `HEARSAY_MAIN_ROOT` set. Deterministic:
same inputs give bit-identical models and scores.
