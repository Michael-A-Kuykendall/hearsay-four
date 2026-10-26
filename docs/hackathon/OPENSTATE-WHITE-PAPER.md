# OpenState as a Development Accelerator — Working Whitepaper

Status: living document. Updated Oct 26, 2026 (hackathon day 1).
Companion: [META-HACKATHON.md](META-HACKATHON.md).

## 1. What this project is

Hearsay-IV re-implements Hearsay-III (Erman, London, Fickas, circa 1983)
with the failing blackboard control structure replaced by a saturation
fabric: a patent-pending (filed, details withheld) justification engine
with multi-support survival semantics. The device is fully mechanistic and
model-free — no neural nets, no embeddings, no learned weights — built
from classical techniques (hand-rolled MFCC, counted n-grams, dictionary
lookup, per-speaker classical baseline). Thesis: AI models fail
unpredictably and catastrophically even at 99% uptime, so a transcription
device for forensic/legal review must be predictable end to end.

## 2. The problem OpenState solves

Three failure modes this repo had already suffered before the hackathon:

1. **Session amnesia.** Four opencode sessions held the entire plan, but no
   new session could see it. Context compaction had wiped live state
   (the prior timeline records `root` reset to `/` and 24 messages
   stripped by compaction).
2. **Multi-tool fragmentation.** Work lived across opencode and Muse
   sessions with no shared memory between them.
3. **Unverifiable "done".** Closed work with no runnable proof rots; an
   earlier audit found shadow clauses (e.g. an unimplemented
   "subsumption-based minimality" acceptance criterion) hiding inside
   green-looking beads.

## 3. What OpenState is, functionally

- A **decision log, not an edit log**: what is true, done, failed, blocked,
  and why. State compiles deterministically from recorded facts.
- **Idempotent session replay**: any recorded session can be re-absorbed
  (`assess`) without double-counting; the store can be wiped and rebuilt
  (`reset`) from verified transcripts.
- **Symbolic code tools with a mandatory reference check before every
  edit**, and a preview→apply chain so no edit lands blind.
- **Beads as the executable work queue**: every bead self-contained with a
  binary green/red gate. Done means the gate ran green, not that someone
  said so.

## 4. Evidence from this repo (Oct 26, 2026, all observed)

Scope accounting, from git log and bead counts: this repo was 10 weeks old when the hackathon
started (initial commit Aug 15; ~22 pre-hackathon commits; 11 crates,
204 green tests, the fail-fast experiment, the wired amalgamation
pipeline, ~78 closed beads from Aug–Oct sessions). Today's autonomous
run added the top of the stack — 3 crates, 2 bins, the guard, the demo,
39 tests, 15 task beads + 5 epics — on top of that foundation. Nothing
here claims the whole system was built in a day; the claim is about
throughput on scoped, gated work with full provenance.

- 5 in-repo sessions identified; each one's directory verified against its
  own store (`opencode.db`, session `cwd`) before import — no foreign
  sessions entered the state.
- Reset + replay absorbed 3464 records; the compiled timeline then showed
  all 4 prior sessions with full event detail.
- A prior audit's findings were closed in-tree (deliberate-retraction
  semantics, subsumption bead `bud`).
- 3 hackathon beads closed the same day with gate evidence: no-model
  guard (proven red on three planted violation classes, green on clean),
  lexicon scaffold (4/4 tests), corpus-fetch (5 SCOTUS objects, idempotent
  re-run). Suite: 208 tests, 0 failed.

## 5. Why this is a competitive advantage

- **Model-switching without amnesia.** Any agent, any model, any IDE can
  pick up the exact frontier: capsule in, work, checkpoint out. The
  $150 credit budget is spent on work, not re-orientation.
- **Judge-verifiable claims.** Every "done" names its gate command and
  expected result. A judge (or competitor) can re-run the proof.
- **The no-model guard is itself a differentiator**: a one-command proof
  that the entry contains zero AI — the thesis, executable.
- **Speed with receipts.** The full session archaeology above took one
  sitting and left the store checkpointed for the next session.

## 6. Agent assessment (requested; weaknesses included with evidence)

Where it earned its keep today: verification-before-claim (the reset
refusal surfaced a real journal/id-mapping quirk instead of silently
wiping); procedure-B discipline (reset → assess → report) made the
"which sessions belong here" question answerable with evidence; beads
gates stopped the classic failure of declaring victory early.

Known weaknesses, stated plainly so they can be fixed, not hidden:
(1) `find_referencing_symbols("retract")` returned 0 references while two
real cross-crate call sites exist (`accent/drift.rs:110`,
`stage3/window.rs:267`) — a symbol-recall gap; text search covered for it.
(2) `diagnostics` reports `unsupported_project` for these standalone
crates, so `cargo clippy` via shell remains the gate. (3) Replay order was INVESTIGATED Oct 26 and CLEARED: the same four
transcripts assessed oldest-first and newest-first both compile to exactly
1355 live facts. The earlier 3179-vs-1301 gap came from comparing a store
that already held 1830 older facts against a clean replay — my comparison
error, not engine behavior. Replay is order-independent on this store.
(1) and (2) stand as stated; neither blocked hackathon work.

## 7. Roadmap for the paper

- Resolve the recall gap with the OpenState owner (same person as the repo
  owner — short loop). Replay-order question: answered (order-independent).
- Per-crate diagnostics support so the shell fallback can retire.
- Append hackathon results (Nov 1 internal target, Nov 4 submission).

## 8. Concurrent execution protocol (hackathon)

Three tracks, each in its own git worktree, each owning its beads (atomic
`bd claim` — no two agents touch the same bead or crate):

- **Track A (spine):** mwa.1 → mwa.2 → mwa.3 → idw.1 → idw.2. Sequential;
  one agent walks the chain.
- **Track B (output):** scaffold `src/output` (prep bead), then nak.1 →
  nak.2 → nak.3 once unblocked. Starts in parallel with A — no shared
  files (new crate vs lexicon).
- **Track C (guarantees):** guard upkeep, whitepaper/hackathon-doc
  contributions, epic-closure assessment. Always safe to run alongside.

Every agent works the same way: claim one bead, implement, run its gate,
close with evidence, checkpoint. Human gates (owner stops the line):
(1) model/credit spend — Dash default, paid models only for hard items;
(2) transcript/eval-data spot-checks before trusting numbers;
(3) conclusion-demo review before submission packaging.
Contributor convention: when OpenState visibly helps (or a novel,
report-worthy observation appears), append a dated line to this paper's
§4/§6 and to META-HACKATHON.md's work log via the OpenState text
preview→apply chain — never rewrite another agent's lines.

## 10. Cost ledger — Oct 26 autonomous run (measured, not modeled)

Wall time ~4.4 h (15:32 → 19:57 UTC). 11 subagents, all on the
subscription route (Muse Spark Dash 1.3, contributor model). Closed 15
task beads + 5 epics (mwa.1/2/3, s3t, idw.1/2, nak.1/2/3, 5qc, zm9, o21,
79z, ivs, 3na; epics aem, cau, mwa, idw, nak). Test suite 204 → 243,
0 failed. New: `sequence` crate, `output` crate, `corpus-fetch` bin,
`hearsay-demo`, no-model guard, demo workspace.

- **Meta API credits spent: $0 of $150.** Everything ran on local cargo
  plus the Muse subscription. Verify the $150 balance in the hackathon
  portal — this repo has no access to Meta billing.
- **Muse allowance observed:** `muse usage` showed window 6% / weekly 2%
  (subscription allowance, not dollars) at end of run.
- **Session footprint on disk:** ~39.7 MB session store (~23.7 MB
  subagent transcripts). As a token proxy that is an UPPER bound only
  (logs include snapshots/duplicates, not pure model I/O) — treat any
  per-token math from it as rough, not billed.
- **Velocity:** ~3.4 gated beads/hour including independent verification,
  merging, and two self-caught red flags (guard false-positive, workspace
  breakage) — both auto-resolved, zero human interruptions needed.
- Only `say` remains, held by human gate `3ww` for owner demo review.

## 11. Evening session — human-experiment tooling + mic saga (Oct 26, observed)

- Built the missing recorder: `MicCapture::record_raw()` (mono i16 + rate;
  `record()` delegates) and `record-session` bin behind `--features mic`
  (`--list-devices`, enrollment capture, utterance loop from `--script`,
  `manifest.tsv`). Bead `zg8` closed green. OpenState chain used
  end-to-end: symbol_overview → read_symbol/read_text_region → 8×
  preview→apply (including 2 compiler-driven corrections: tuple return,
  `new(duration_ms)` ctor, cpal-direct device listing) → cargo
  build/test/clippy/fmt. No file was written by hand; every byte went
  through preview→apply or `cargo fmt`.
- Failure modes, both exercised: (a) anchor-digest staleness on
  same-file double previews — second apply rebased cleanly, no lost edit;
  (b) hallucinated APIs (`list_devices`, `default()`) — compiler rejected,
  `symbol_overview` supplied the real surface. Local-tool discipline held:
  no defect filed against the OpenState app from this session.
- WSL2 audio finding: `/proc/asound/cards` absent — no passthrough without
  usbipd; wrote and fixed `attach-usb-mic.ps1` (missing `bind` step was the
  attach failure). Decision: Windows-native recording session; audit found
  zero platform-specific code (cpal 0.15 WASAPI needs no setup). Filed
  `o20` (scorer adapter). Open beads after tonight: `say` (gate `3ww`),
  `o20` (unclaimed). Uncommitted work: capture crate (mic refactor, bin,
  hound dep) — commit pending owner review.

## 9. Beads-native concurrency (set up Oct 26)

The beads app turned out to already contain the orchestration layer, so
no custom harness was built. Native primitives in use:

- **Ready fronts + atomic claim.** `bd ready` shows only unblocked work;
  `bd ready --claim --json` lets agents self-assign race-free.
  `bd swarm validate <epic>` reports waves, worker-sessions, and max
  parallelism (mwa and nak: 3 waves each, max parallelism 1 — linear
  chains, parallel across tracks).
- **Track labels.** `hackathon` + `track-a` (spine: mwa→idw), `track-b`
  (output: zm9→nak), `track-c` (guarantees: guard, docs, milestone).
- **Human gates.** `bd gate create --type=human` blocks an issue from
  `ready` until `bd gate resolve` (gate `3ww` currently holds `say` for
  the owner demo + submission review). This is the stop-and-ask
  mechanism — enforced by the queue, not by agent discipline.
- **Worktrees.** `bd worktree create track-a|track-b` (own branches,
  shared beads DB, auto-gitignored). Main tree hosts Track C.
- **Merge discipline.** `bd merge-slot` serializes merges when tracks
  land; epic auto-close via `bd epic close-eligible` (retired aem, cau).
- **Formulas/molecules** (`formula`, `cook`, `mol`) are the template
  layer for repeating this shape on future epics — adopted as-is, not
  reinvented.

Time attribution (the "where we saved time, precisely" ledger): every
setup step and every bead close lands in `bd audit record`
(timestamped, issue-linked, in `.beads/interactions.jsonl`) and beads
carry created/updated/closed timestamps. End-of-project accounting =
`bd history` per bead + audit log + gate evidence in close reasons.
Novelty claim for submission: the whole orchestration is beads-native —
the issue tracker IS the scheduler, the merger, and the time clock.

## 12. Track A/B rerun — packaging lane (Oct 26 evening, in progress)

Same shape as the morning run, second lap: substrate beads sequential on
main, surface beads parallel on refreshed tracks.

- `9hh` (track-c, main): sample clip cut with the repo's own decoder
  (throwaway trimmer in /tmp against `corpus::audio::decode_to_pcm16k`,
  loudest-8s window) → `data/demo/sample.wav` (252K) + README provenance
  + narrow `.gitignore` exception. Gate: demo exit 0, 4-word transcript,
  all chains terminate. Closed, committed as `6626f00`.
- `9gc` (track-c, main): `--json` on `hearsay-demo` reusing
  `output::Transcript` serialization + `WordRecord::validate`-first
  contract. Test-first: focused `json_output` test written before
  `emit_json` existed (observed `E0425` failure), then green; CLI gate on
  the `9hh` fixture: exit 0, 4 words with text+provenance; runner suite
  7+1, clippy `-D warnings`, fmt. Closed, committed as `cd892f3`.
- Tracks `track-a`/`track-b` fast-forwarded to `cd892f3` (both clean,
  both ancestors — zero-divergence refresh).
- `2sh` (track-a) and `dyy` (track-b) launched as parallel subagents on
  their worktrees after the substrate landed.
- Track B landed first: `dyy` closed green (release script + README +
  smoke JSON + guard evidence + checksums; commit `eb2a81b`), merged to
  main as `937dacf`, release gate re-verified on the merged tree (binary
  OK, `sha256sum -c` OK).
- Track A hit a real plan mismatch and the discipline worked: the child
  proved the wasm32 closure fails in `wait-timeout` (proptest declared
  as a hard dep in `invariant_ppt` but used only in `cfg(test)` fuzz
  code), stopped before writing repo code, released `2sh` to OPEN with
  the evidence in bead notes. Fix filed as `amc` (one-line manifest
  move, verified: 20 tests green, wasm32 links), closed and committed as
  `a4bd294`; `2sh` now depends on `amc` and is retrying on a refreshed
  track-a. Total cost of the mismatch: one prerequisite bead, zero
  wasted implementation, zero broken trees.
- Retry landed: `2sh` closed green on track-a (4 wasm-demo tests, wasm32
  links, page with both callouts, guard green; commit `3db7990`), merged
  to main, gate re-verified on the merged tree. Track B's release gate
  likewise re-verified post-merge. Runner suite re-run after the `amc`
  manifest move: 7+1 passed.
- Lane totals: 5 beads closed (`9hh`, `9gc`, `amc`, `dyy`, `2sh`),
  2 track merges, 0 pushes. Remaining: `o20` (needs owner recording),
  `say` (submission review, gate `3ww`). The final argument for the
  submission: every claim in this paper traces to a bead close reason
  with a command and an observed result — the speed came from parallel
  tracks with zero-divergence refreshes plus a queue that made the one
  real blocker cheap (a child proved it, a one-line bead fixed it, the
  retry landed).

## 13. Run1 verdict (Oct 26 evening — the experiment scores)

- Windows lane merged (`872e54e`, add/add resolved for origin's
  `--resume-at` version), 121 WAVs verified in `sessions/run1/`
  (120 utterances, 30 words × 4, all 48 kHz mono; 23 rows 143520
  samples — the adapter uses actual lengths, as designed).
- `o20` built and closed: `session-ingest` (validation, enrollment
  skip, pre-registered K=3×40, cumulative timestamps) plus
  `failfast_run --root`. Bead gate ran on a synthetic 2-bucket
  real-speech fixture (exit 0, stats + verdict line) before the real
  session was touched.
- Run1 result: exit 0, **PASS** — check1 true, check2 margin 107.53
  against a 0.02 floor, check3 true; per-bucket mean scores
  A=(171.50, 177.47, 200.42), B=(85.21, 77.95, 84.87).
- Limits, stated plainly: the baseline is generic (enrollment wiring
  is follow-up bead `fo7`), the compared quantities are mean-NLL
  magnitudes per the implemented 39c.5 rule rather than hand-counted
  word errors, and B leads from bucket 0 (no first-half
  comparability baseline). The verdict is recorded; the interpretation
  belongs to the submission writeup under `say`, not to this ledger.

OpenState usage this lap (all observed): every source edit went through
`ast_search` → `read_text_region`/`read_symbol` → `preview` → `apply`
(`symbol_overview` supplied the real `MicCapture`/`FullPipeline`
surfaces after two hallucinated APIs were compiler-rejected on the
previous lap). Edit batches on one file serialize on digest (second
preview rebases); batches across files apply in parallel. No shell
redirects into the repo; no hand-written files except /tmp probes.
