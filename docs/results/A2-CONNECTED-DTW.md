# A2 Connected-Word DTW Decoder — DEV Results

Bead: `hearsay-four-5n2--connected-word-decoder` (lane L5 Track A, 2026-10-31).
Status: DEV complete. PHASE-TEST untouched (requires owner connected recordings; none exist).

## What was built

New crate `src/a2` (workspace member): one-stage connected-word DTW
token-passing decoder (Ney family) over the frozen A1 front end.

- `src/a2/src/decoder.rs` — frame-synchronous Viterbi over word templates +
  SIL/SP; token state (unit, template_pos, start_frame, path_cost,
  backpointer, lm_history empty); word-insertion penalty, min/max word
  duration, slope caps (stay/step), max template expansion, beam + max-live
  pruning, SIL endpoints; emits 1-best + word-end events + A5-format lattices
  (1-best backbone always kept, source `a2-1best`, chain arcs added).
- `src/a2/src/templates.rs` — run1-shaped `manifest.tsv` loader (frozen
  frontend: `base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn`, dim 39);
  trimmed word models, SIL mean from untrimmed lead/trail; cross-take split
  (`excluding_take`); bulk-TTs vocab-agnostic loader (DEV-B).
- `src/a2/src/concat.rs` — DEV phrase synthesis: trimmed takes + REAL
  recording-silence lead/gaps/trail (never digital zeros); exact boundaries.
- `src/a2/src/wer.rs`, `src/a2/src/oracle.rs` — WER + exact lattice oracle
  DP + K sweep (shared with A5).
- `src/a2/src/bin/a2_dev.rs` — DEV runner (`--template-dir/--phrase-list`,
  `--bulk-dir`, `--resweep` for lattice-only re-sweeps).
- No changes to `src/fse` (reuse only: `frame_distance`, frontend, config
  conventions). No new external dependencies (fse/lattice/eval/hound only).

## DEV data (pinned in bead notes before implementation)

- DEV-A (primary, 30-vocab, human, cross-take): templates
  `/home/michael/repos/hearsay-four/sessions/run1-16k` takes 1–3
  (90 clips, 30 words, 16 kHz, read-only from main — lane has 0 WAVs);
  phrases = 104 synthetic connected phrases (422 tokens, 3–7 words) built
  in-memory from take-0 clips per `sessions/script-a2-phrases.txt`
  (sha256 `c3d03d55…b5cc`), 150 ms real-silence gaps, 200 ms lead/trail.
  Training size 90 clips; test population 104 phrases / 422 tokens.
  Caveat: 30 distinct acoustic realizations reused across contexts.
- DEV-B (TTS machinery, parent-ordered): `/home/michael/repos/better-voice/data/bulk_tts_1k`
  (1000 WAV, 22050 Hz, D0-verified; measured 0/30 closed-vocab coverage):
  10-word sub-vocab, 20 round-robin 3–5-word same-clip phrases, 79 tokens.
- Excluded: `phrase_training` (3500 16 kHz phrases, 30/30 vocab, ideal shape)
  — D0 NOT-VERIFIED provenance (0-byte manifest script, no speaker/licence);
  `quality_checked_1k` (30×1 isolated TTS) — MP3 content under `.wav` names.
- Swap design: the bin takes `--template-dir + --phrase-list`; L1 canonical
  output (same `manifest.tsv` shape) drops in with zero code change.

## Tuning (DEV-only, 5-phrase subset, cross-take)

`max_live` 2000→8000 halved errors (search saturation was dominant);
WIP {0,5,15} tied at 1/19 errs. Frozen DEV config:
`--beam 800 --wip 5 --max-live 8000` (+ defaults: min/max word 15/400
frames, max_step 2, max_stay 2, max_expansion 3.0, max_sp 30, SIL endpoints).

## Results (frozen config, full DEV)

Command (from worktree root, release):

```
cargo run --release -p a2 --bin a2_dev -- \
  --template-dir /home/michael/repos/hearsay-four/sessions/run1-16k \
  --phrase-list sessions/script-a2-phrases.txt \
  --out-dir /tmp/a2-dev-full --wip 5 --max-live 8000 --beam 800
```

DEV-A cross-take (templates takes 1–3, phrases take 0):

- 1-best WER: **24 errs / 422 refs = 0.0569**, n=104 phrases, 0 failures.
- Boundary median: **100.0 ms** (exact synth bounds; gate ≤100 ms: PASS).
- RTF mean: **0.324** (gate <1: PASS).
- Peak live tokens: 8000 (bound 8000; saturates — search budget binding).
- Confusions (24): genetic→intense ×11, rhubarb→overboard ×6,
  kinship→congealed ×4, ripple deletion ×2, rhubarb deletion ×1.
  Same hub words as A1 isolated (rhubarb/genetic/kinship) — acoustic,
  not search artifacts.
- Same-take control (`--heldout-take 99 --phrase-take 0`, 3 phrases):
  2/11 errs = 0.18 (subset; full same-take not run — cross-take is the claim).

DEV-B TTS machinery (same-clip, 10 words):

```
cargo run --release -p a2 --bin a2_dev -- \
  --bulk-dir /home/michael/repos/better-voice/data/bulk_tts_1k \
  --bulk-words 10 --bulk-phrases 20 --out-dir /tmp/a2-dev-bull2 \
  --wip 5 --max-live 8000 --beam 800
```

- 1-best WER: **0/79 = 0.0000**, n=20, boundary median 40 ms. Machinery proven
  on real TTS audio (no 30-word claim — bulk has 0/30 vocab).

## Validity caveats

- Concat phrases have clean inter-word silences (no coarticulation):
  results transfer to words-in-sequence, not natural connected speech.
- Same speaker/session throughout (cross-take only). Cross-session and
  PHASE-TEST generalization unmeasured.
- Search saturates max_live 8000 every phrase; wider search might fix some
  of the 24 errors (oracle shows at most 5 are lattice-recoverable — see A5).

## Gate checklist (bead acceptance)

- PHASE-TEST WER ≤25%: NOT RUN (no owner connected recordings exist).
- Boundary ≤100 ms median on 20 hand-checked: 100.0 ms median on 104 exact
  synth bounds (DEV proxy): PASS.
- RTF <1: 0.324 mean: PASS.
- Peak live ≤ bound: 8000 ≤ 8000: PASS (at bound).
- Results here: DONE.

## Artifacts

- Lattices (v1 `hearsay-word-lattice`, K=20 + backbone): 104 files, 2.0 GB
  total in `/tmp/a2-dev-full/lattices/deva/` (NOT in repo — too large;
  regenerate with the command above).
- Per-phrase CSV: `/tmp/a2-dev-full/per_phrase_deva.csv`
  (id,nref,nhyp,errors,boundary_med_ms,frames,peak_live,decode_ms,audio_ms,rtf,cost,norm,n_events).
- Quality gates: `cargo test -p a2` (23 passed), `cargo clippy -p a2
  --all-targets -- -D warnings` clean, `cargo fmt -p a2 --check` clean.
