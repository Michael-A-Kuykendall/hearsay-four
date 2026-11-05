# Dead-crate audit (before app build)

Bead: hearsay-four-ejxe--audit-dead-crates. Base: 934f251073fd7d43d1a51df31eb5e2f92180a8b2.
Scope: read-only audit; NO deletions in this bead. Deletions become follow-ups.
Method per crate: (1) `cargo tree` / Cargo.toml manifest evidence (who depends on it),
(2) call-site check against the current decode path (runner -> decoder -> hearsay -> fabric),
(3) verdict + reason (+ follow-up title if DELETE).

Decode-path reference (measured `cargo tree`, normal edges):
- `runner` direct deps (measured `cargo tree -p runner --edges normal --depth 1`):
  arena, calibration, corpus, decoder, fabric, fse, invariant_ppt, lattice,
  lexicon, lm, output, sequence, stage3.
- `decoder` deps include fse, hmm, lattice, lexicon, lm.
- `hearsay` deps: fabric, lattice, lm. `fabric` deps: invariant_ppt only.

Summary: 7 KEEP, 2 DELETE.

| Crate | Verdict | One-line reason |
|---|---|---|
| groebner | DELETE | Zero external references; 333-line research spike, own bin only |
| invariant_ppt | KEEP | 11 manifest dependents incl. fabric; on decode path |
| accent | DELETE | Sole consumer is calibration dev-deps (cfg(test) only); ships in no binary |
| fse | KEEP | Decoder frontend (28 call sites in moore.rs); carries a1_gate |
| stage3 | KEEP | runner lib uses CommitWindow in production code |
| a2 | KEEP | Live consumer: experiment lib (oracle_sweep, arms); off decode path, see caveat |
| calibration | KEEP | runner lib + fse production use |
| output | KEEP | Runner demo/serve/scoring bins; carries why-panels |
| wasm-demo | KEEP | Demo surface referenced by web/ pages; workspace-excluded so zero host cost |

## 1. groebner — Verdict: DELETE

- Manifest evidence: no `path = "../groebner"` in any Cargo.toml under src/
  (grep over src/*/Cargo.toml returns only src/groebner/Cargo.toml's own name line).
  `cargo tree -p groebner --edges all --invert` shows no dependents.
- Call-site evidence: text search for `groebner` over src/ returns exactly 3 hits,
  all self-internal: src/groebner/Cargo.toml, src/groebner/Cargo.lock,
  src/groebner/src/bin/measure.rs (`use groebner::{...}` in its own bin).
  Zero `groebner::` uses outside the crate. Not on the decode path.
- Size/purpose: 231 lib + 102 bin = 333 lines; Gröbner pair-classification
  measurement spike with its own `measure` bin.
- Reason: unreferenced by every crate, bin, example, script, and doc; dead weight
  in the workspace build.
- Follow-up bead (coordinator files): "Delete dead groebner research-spike crate
  (dead-crate audit follow-up)" — remove src/groebner + workspace member line;
  confirm `cargo test --workspace` still green.

## 2. invariant_ppt — Verdict: KEEP

- Manifest evidence: 11 dependents declare `invariant_ppt = { path = ... }`:
  arena, calibration, corpus, fabric, fse, lexicon, output, sequence, stage3,
  accent, runner (grep over src/*/Cargo.toml).
- Call-site evidence: fabric/src/lib.rs:13 `use invariant_ppt::assert_invariant;`
  puts it directly on the decode path (runner -> ... -> hearsay -> fabric).
- Prior verified: core is load-bearing for production contracts (fabric, decoder
  via fse, runner, output, sequence, lexicon, corpus, arena, calibration).
- Reason: foundational invariant-harness crate; deleting it breaks the workspace.

## 3. accent — Verdict: DELETE

- Manifest evidence: zero normal dependents. Sole declared consumer is
  calibration's [dev-dependencies] (`accent = { path = "../accent" }` in
  src/calibration/Cargo.toml with a "dev-only cycle" comment). Ships in no binary.
- Call-site evidence: only external `accent::` use is
  src/calibration/src/arena.rs:218-219
  (`accent::derive::derive_and_assert`, `accent::derive::PHONEME_COUNT`) inside the
  private test fn `stub_enrollment_enters_valid_payload_consumable_by_derive`
  (lines 177-228, under `#[cfg(test)]` at line 170). Internal uses are confined to
  accent's own bins (failfast_run.rs, session_ingest.rs) and its own
  tests/separation_gate_tests.rs. The only other mention is a doc comment in
  src/runner/src/bin/hearsay-serve.rs:242-243 noting the service-side twin —
  runner does NOT depend on accent. No scripts/ invoke the accent crate or its bins;
  docs/ never mentions failfast_run/session_ingest. Not on the decode path.
- Reason: measurement crate (ConditionA/ConditionB, baseline_from_frames) with no
  production consumer; test-only coupling via a dev-dep.
- Follow-up bead (coordinator files): "Delete dead accent measurement crate and
  drop calibration test-only dev-dep (dead-crate audit follow-up)" — decide
  archive-vs-delete for the measure bins/tests, remove the calibration dev-dep,
  remove the workspace member; confirm `cargo test --workspace` green.

## 4. fse — Verdict: KEEP

- Manifest evidence: dependents a2, accent, calibration (dev), capture, decoder,
  hmm, runner, stage3.
- Call-site evidence: 28 `fse::` sites in src/decoder/src/moore.rs production code
  (CmvnLive, window_audio, extract_features*, FEATURE_DIM, SHIFT_MS, Frame) —
  the decoder frontend, directly on the decode path.
- Prior verified: carries the a1_gate instrument (src/fse/src/bin/a1_gate.rs)
  plus frontend validators consumed by decoder/hmm/runner.
- Reason: the feature-extraction core of the current pipeline.

## 5. stage3 — Verdict: KEEP

- Manifest evidence: dependents accent (tests), runner, wasm-demo.
- Call-site evidence: src/runner/src/lib.rs:32
  `use stage3::window::CommitWindow;` in production runner lib code — on the
  decode path. Also used by wasm-demo lib and accent tests.
- Reason: live production dependency of the runner; not a leftover.

## 6. a2 — Verdict: KEEP (narrow; see caveat)

- Manifest evidence: single dependent: experiment (`a2 = { path = "../a2" }`).
- Call-site evidence: live non-test uses in experiment lib code:
  src/experiment/src/oracle_sweep.rs:8 (`use a2::oracle::{OracleResult, sweep_k};`),
  src/experiment/src/arms.rs:209 (`a2::oracle::acoustic_best_path_ids`),
  plus experiment examples (s_cmp.rs). Own bin a2_dev.rs also consumes it.
  Crate is ~4757 lines (oracle, decoder, templates, concat, wer).
- Caveat: experiment itself is a leaf (no external crate consumers; all
  `experiment::` uses are inside experiment), so the a2+experiment subgraph sits
  OFF the decode path — it is the earlier multi-stage pipeline plus its research
  harness. a2 is not dead (it has a live consumer), so no DELETE verdict here;
  recommend a separate audit of the experiment subgraph if the app build wants to
  shed research-only code. No follow-up filed by this bead.
- Reason: deleting a2 breaks experiment lib compilation today.

## 7. calibration — Verdict: KEEP

- Manifest evidence: dependents accent, capture, fse, runner, wasm-demo.
- Call-site evidence: src/runner/src/lib.rs:27
  `use calibration::{Baseline, FEATURE_DIM, Frame};` in production runner lib;
  fse depends on calibration normally (decoder -> fse -> calibration chain).
- Reason: on the decode path via runner and fse.

## 8. output — Verdict: KEEP

- Manifest evidence: dependents decoder, experiment, hmm, runner, sequence,
  stage3 (test-only).
- Call-site evidence: 34 `output::` sites under src/runner/src (production bins
  hearsay-demo, hearsay-serve, hearsay-rtf, wer-bulk: SessionMeta, Transcript,
  wer); decoder example wer_devclean.rs uses `output::wer`.
- Prior verified: carries the why-panels (src/output/src/fabric_why.rs,
  src/output/src/why_text.rs feeding the runner demo panel `why_json`).
- Reason: product-value output/why/scoring surface used by shipped runner bins.

## 9. wasm-demo — Verdict: KEEP

- Manifest evidence: excluded from the host workspace (root Cargo.toml `exclude`;
  own `[workspace]` + Cargo.lock, pinned wasm-bindgen); no crate depends on it,
  and it costs the host build nothing.
- Call-site evidence: it is a leaf demo app (single src/lib.rs) consuming
  calibration/fse/stage3/output. Referenced by web/demo.html:9,12 (wasm-pack /
  cargo build instructions for src/wasm-demo) and web/tcv2-demo.html:24
  (capability note: "wasm-demo exports transcribe_pcm16k batch only").
- Reason: live demo surface with web entry points; exclusion already isolates it
  from the host test/release matrix. Deleting it would break the web demos.

## Gate check

- `test -s docs/audit/DEAD-CRATE-AUDIT.md` — this file, non-empty.
- All nine crates (groebner, invariant_ppt, accent, fse, stage3, a2, calibration,
  output, wasm-demo) carry an explicit KEEP or DELETE verdict above.
- Every DELETE verdict (groebner, accent) cites cargo-tree/manifest + call-site
  evidence; no behavior changes, no deletions in this bead.
