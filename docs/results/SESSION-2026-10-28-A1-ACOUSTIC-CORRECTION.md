# A1 acoustic correction session (2026-10-28): from void run to honest 0.8583

Purpose: single writeup of this session's central thesis, narrative,
methods, changes, and measurements, for cloud architecture review.
Source of record for work state is beads; this document narrates and
cites, it does not replace bead text.

## 0. Central thesis

The A1 isolated-word DTW-MFCC experiment was administered incorrectly
(wrong test-session protocol, defective instrument), so its 0.0333 was a
void run, never a hypothesis failure. Fixing the instrument correctly —
loud degenerate handling, then speech-span endpoint trimming — removed
an ~8-point padding/timing fingerprint the untrimmed pipeline exploited,
leaving honest within-session LOTO at best 0.8583 (103/120). The trim is
verified correct and stable; the 8/8 >= 0.90 DEV gate is therefore
unreachable by tuning and the A1 bar-vs-physics question is an owner
rescope decision, not an implementation deficit.

## 1. Provenance (OpenState capsule, 2026-10-29T04:28Z)

- Store was reset mid-session and replayed via procedure B (4 sessions,
  ~940 records, idempotent); `total_live` 683 at capsule time.
  Several capsule propositions arrived truncated (`openst…`), so the
  capsule contributes the change frontier and the bead-operation log,
  while session substance below is reconstructed from bead text,
  diffs, and executed commands — all cited.
- Capsule change frontier matches `git status` exactly: 7 modified
  files (3 docs/results, 4 src/fse), 4 untracked (3 phase-test
  session dirs with owner WAVs, 1 trajectory JSON). Audio and the
  trajectory file stay out of git (see section 6).
- Capsule `completed` log records the bead surgery: creation of
  `hearsay-four-4ew--loud-degenerate-gate-failure` and
  `hearsay-four-gh1--endpoint-trimming-padding-invariant`, their
  design/acceptance pinning, the dep edge b7p -> 4ew, renames to
  slugged IDs, and the b7p void-correction note.

## 2. Session narrative (in order)

1. **Handoff in.** Prior session recorded `sessions/phase-test-20261028-190133/`
   (31 WAVs, 5th takes) and left the A1 PHASE-TEST gate running detached.
2. **Void verdict.** Gate output: top-1 0.0333, all 30 queries predicted
   `congealed`. Root-caused in source, each link verified: run1 clips are
   fixed ~3.0 s windows while 190133 clips are variable 3.8-5.9 s, so the
   best frame-count ratio (0.785) sits below the Sakoe-Chiba band-0.2
   reachability floor (0.8); `dtw_distance` returns +inf for every pair;
   strict-`<` argmin over manifest order collapses to the first template
   (`utt000.wav congealed`). Conclusion: invalid experiment (wrong
   protocol + silent degenerate fallback), labeled VOID/INVALID
   everywhere — doc, CSVs, beads, memory. Retired session, never re-score.
3. **Architecture into beads.** The fix was designed up front into two
   self-contained beads with settled mechanics, blast radius, probe
   recipes, and binary gates (section 4 quotes them): 4ew (fail loudly)
   first as safety, then gh1 (endpoint trimming) as correctness.
4. **4ew landed and closed.** `classify -> Option`, skip-and-count
   `EvalResult.degenerate`, `is_degenerate()`, runner `NOT COMPARABLE`
   exit 1. All gates green (section 5).
5. **gh1 implemented.** `speech_span()` + integration, trim unit tests,
   padding-invariance tests, then a measurement crisis: first constants
   caused `EmptyInput` on run1, so instead of blind-cutting a scratch
   energy probe (`span_probe`, since removed) founded the constants from
   run1 statistics. Key probe finding: silence is near-digital-zero
   (raw p5 <= 8e-5) while the failure mode is isolated transient blips,
   which motivated the support-rule hysteresis design.
6. **Inflection.** Trimmed DEV best 0.8583 vs untrimmed 0.9417 on identical
   audio. Hangover 2-vs-5 stability experiment returned bit-identical
   grids, eliminating edge-cutting; elimination leaves padding/timing
   fingerprint as the ~8-point gap. Bead gate unreachable by tuning;
   workstream stopped per architectural-inflection protocol, artifacts
   re-recorded to trimmed truth, state saved (bead notes, `mem:core`,
   OpenState checkpoint).

## 3. Methodology ledger (every method, and what it established)

- **Beads as sole work source.** All architecture lives in bead
  description/design/acceptance, written to >95% cold-executable before
  coding. No separate markdown plans. One bead `in_progress` at a time;
  re-architecture is holistic and never mid-bead.
- **Failing-first tests.** 4ew: all-+inf regression test observed
  collapsing to first-template red, then green after the fix. gh1: trim
  unit tests + 2x-padding classification test written pre-fix (padding
  case observed NOT COMPARABLE under post-4ew behavior), green after.
- **Loud failure over sentinel values.** `build_result` indexes
  `Prediction.predicted` into the label map, so a degenerate sentinel
  string would panic — hence skip-and-count (`degenerate: usize`) plus
  runner abort on any degenerate split. Any degenerate query aborts the
  run; there is no threshold to game.
- **DEV-only tuning.** All threshold exploration used `sessions/run1`
  LOTO only. The retired 190133 audio was never read or scored after
  voiding (constraint pinned in both beads).
- **Probe-founded constants, not guessed.** `span_probe` measured all 120
  run1 clips (raw vs production-domain energies, threshold grid): room
  silence p5 <= 8e-5, speech peaks >= 0.0226 -> floor 0.001 sits 12x
  above worst silence and 22x below weakest peak. K 0.01, hangover 5.
  Probe removed after use; numbers recorded in gh1 notes.
- **Stability experiment to distinguish cause.** Hangover 2 vs 5 frames:
  identical grids to 4 decimals across all 8 configs. A span-placement
  bug or edge-cutting would move with hangover; invariant output proves
  spans cover speech fully in both cases.
- **Elimination argument for the fingerprint.** Candidates: edge-cutting
  (killed by stability experiment), framing-misalignment bug (would
  crater accuracy far below 0.86 and would move with hangover —
  observed 0.8583 with clean 3-4/4 diagonals), CMN-shift artifact
  (argued against: `no_cmn` config dropped equally to 0.8500; open
  reviewer question 1 below). Remaining: untrimmed DTW exploited
  within-window timing/position habit. Inferred, not directly measured
  (reviewer question 5).
- **Gates, all binary.** `cargo test -p fse`; `cargo clippy -p fse
  --all-targets -- -D warnings`; `cargo fmt -p fse --check`; release
  DEV grid; /tmp synthetic probe exit codes. 4ew additionally required
  an exact-match DEV grid (behavior preservation proof).
- **OpenState symbolic workflow.** All code search/read/edit through
  `openstate_*` (ast_search -> read_symbol/read_text_region ->
  preview -> single-use apply); bash reserved for build/test/gates and
  the `bd` CLI. Native read/grep/edit tools were not used on the repo.
- **Evidence language.** No attitude words; every claim carries the
  command run and the number observed (section 5).

## 4. Bead gates (quoted from bead text)

- `hearsay-four-4ew--loud-degenerate-gate-failure` (CLOSED): cargo test
  green incl. degenerate regression; release DEV exit 0 with 8/8 >= 0.90
  (behavior-preservation); synthetic 2x probe NOT COMPARABLE exit 1;
  clippy + fmt clean. All observed green; close reason recorded.
- `hearsay-four-gh1--endpoint-trimming-padding-invariant` (IN_PROGRESS,
  gate red): cargo test green (39/39 observed); release DEV exit 0 with
  8/8 >= 0.90 (observed 0/8, best 0.8583 — unreachable, see inflection);
  CSV + doc table re-recorded (done); clippy + fmt clean (observed).
  Note: `no_lifter` measured 0.8333 pre-trim, so 8/8 was already red
  before trimming — the bar miscounts diagnostic ablations as candidates.

## 5. Numbers ledger (command -> observed result)

- `a1_gate --run1 sessions/run1 --phase-test-dir
  sessions/phase-test-20261028-190133` (pre-fix): top-1 0.0333, 30/30 ->
  `congealed`, gate90=false. VOID (section 2, link-by-link mechanism).
- Post-4ew `cargo test -p fse`: 34/34. Post-gh1: 39/39 (4 trim unit
  tests incl. transient-blip test, padding-invariance test, degenerate
  tests). Clippy `-D warnings` clean; fmt clean (after scratch-probe
  removal; the probe was clippy-dirty, which is why scratch leaves).
- `a1_gate --run1 sessions/run1` (trimmed, hangover 5): base 0.8583,
  no_cmn 0.8500, no_deltas 0.8500, deltas_only 0.8583, no_preemphasis
  0.8500, no_lifter 0.8250, band_300_3400 0.8500,
  band_100_4000_no_cmn 0.8500 (n=120 LOTO each).
- Same command pre-trim: best 0.9417 (113/120); full grid in doc history.
- Hangover 2 vs 5: grids identical to 4 decimals, all configs.
- `/tmp/a1deg` 2x synthetic probe post-4ew: NOT COMPARABLE, exit 1.
- Trimmed best-config 17 confusions: byzantium->resented,
  congealed->hotbed, congealed->kinship, genetic->kinship,
  genetic->ripple, highboy->hotbed, hotbed->ripple, impeachment->hotbed,
  kinship->congealed, kinship->enchantment, knockdown->survival,
  process->congealed, rhubarb->ripple, ripple->hotbed, salivating->hotbed,
  staked->congealed, survival->knockdown. Spread across short/plosive
  words, no degenerate sink.

## 6. Changes committed (7 files, +407/-75)

- `src/fse/src/gate.rs` (+45/-): `EvalResult.degenerate`, `classify ->
  Option` (None on non-finite best), skip-and-count in
  `leave_one_take_out`/`phase_test`, `is_degenerate()`.
- `src/fse/src/bin/a1_gate.rs` (+10/-): NOT COMPARABLE + exit(1) on any
  degenerate DEV or phase-test split.
- `src/fse/src/frontend.rs` (+51/-): `TRIM_K/FLOOR/HANGOVER`,
  `speech_span()` with support-rule hysteresis, raw-domain frame
  energies, span slicing in `extract_with_config` before
  `static_features` (None -> `EmptyInput` before the panic-prone call).
- `src/fse/src/tests/a1_tests.rs` (+179/-): degenerate regression tests,
  4 trim unit tests, blip test, 2x-padding classification test, chirp
  WAV helpers (one fixture rewritten when trimming fixed its padded
  case by design: unpadded duration mismatch keeps the loud path tested).
- `docs/results/A1-ACOUSTIC-GATE.md`: void root-cause section (prior),
  DEV table re-recorded to trimmed grid, 17-confusion list,
  padding-fingerprint finding section.
- `docs/results/A1-dev-results.csv`: 8 DEV rows re-recorded
  (`endpoint-trimmed` notes); VOID phase-test row untouched.
- `docs/results/A1-confusion-matrix.csv`: VOID-tagged void-run rows
  (prior relabel); no DEV matrix stored — reproducible via the gate
  command cited in section 5.
- NOT committed: `sessions/phase-test-*/` (owner audio, gitignored
  policy), `trajectory-2026-10-27-214923.json` (unknown provenance,
  not session work).

## 7. Explicitly rejected / not done

Band widening (untunable honestly on uniform-duration DEV, keeps
padding-as-signal); open-end DTW (larger change, partial-path
normalization pitfalls); protocol-only re-record (leaves the lying
instrument); re-scoring retired 190133 under any circumstance;
synthetic/TTS substitution for owner audio; lowering gh1's gate
unilaterally (grading own homework — owner rescope pending, options:
A frontend-separability bead, B restated bar, C one honest fresh-take
readout before re-architecting; recommendation C-then-A).

## 8. Open questions for the architecture reviewer

1. CMN runs over trimmed speech-only frames now; `no_cmn` dropped
   equally, which argues the DEV gap is fingerprint, not CMN-shift —
   is that argument airtight or is a controlled CMN-ablation needed?
2. Deltas at trimmed edges: delta indices clamp (panic-safe), but does
   edge replication inject noise that costs accuracy vs interior deltas?
3. Support rule (±2 frames, self-must-fire): principled hysteresis or
   arbitrary window? What sensitivity analysis would satisfy you?
4. Threshold on raw-sample energies (config-independent spans) vs
   post-pre-emphasis energies (matched to features): did we pick the
   wrong domain for fricative onsets?
5. Fingerprint mechanism (word-specific within-window timing habit) is
   inferred by elimination. What direct measurement would confirm it
   (e.g. per-word offset variance vs untrimmed confusion gain)?
6. Given honest 0.8583 LOTO, is endpoint-constrained DTW+trim still the
   right classical baseline, or should option A jump to open-end DTW /
   HMM rather than polishing MFCC-DTW?
7. Hangover symmetric 5/5 (50 ms): stability shown 2-vs-5, but is there
   a case for asymmetric (longer leading for pre-voicing/frication)?
8. Retired-session rule (190133 never re-scored, even as machinery
   smoke test): correct, or is a clearly-labeled non-evidence smoke run
   acceptable?
