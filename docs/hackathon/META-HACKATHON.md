# Meta Global AI Developer Hackathon 2026 — Hearsay-IV Entry

Owner: Michael A. Kuykendall. Event runs the build Oct 26 – Nov 4, 2026.

## The event (verified Oct 26, 2026 via web search)

- **Name:** Meta Global AI Developer Hackathon 2026 (run by Meta with
  Cerebral Valley). 10-day virtual build, worldwide, selection-capped at
  20,000 participants.
- **Build window:** Oct 26 → Nov 4, 2026.
- **Submission deadline: Nov 4, 9:00 PM ET (Nov 5, 02:00 UTC).**
  Judging Nov 5–9, winners announced Nov 10.
- **Prizes:** $1M cash pool. **Credits:** $150 Meta Model API credits per
  participant (owner plans to extend via the contributor model).
- **Build with:** Meta's newest models, Meta open-weights models, Muse Code.
- Links: [Cerebral Valley event page](https://cerebralvalley.ai/e/meta-ai-developer-hackathon)
  (Meta dev events page: `dev.meta.ai/events/`).

## Where the project stood at hackathon start (Oct 26, read this first)

This was a 10-week-old project, not a blank slate — the hackathon built
the top of the stack, not the whole building:

- **History:** initial commit Aug 15, 2026; ~22 commits before today
  through Sep/Oct (accent fail-fast driver, runner pipeline wiring,
  Dependabot hardening). Four opencode sessions (Aug 14–15) did the
  research setup, beads planning (five audit passes), and the first
  implementation waves.
- **Pre-existing and green:** 11 crates (fabric, fse, arena, stage3,
  accent, calibration, capture, corpus, groebner, invariant_ppt, runner),
  204 tests passing, the fail-fast experiment run on real audio (39c),
  the amalgamation pipeline wired fse→arena→stage3 (aem.3), an independent
  71-bead audit with shadows resolved. ~78 beads closed before today.
- **Today added (Oct 26):** 3 new crates (`lexicon`, `sequence`,
  `output`), 2 new bins (`corpus-fetch`, `hearsay-demo`), the no-model
  guard, the demo workspace root, 39 new tests (204 → 243), 15 task beads
  + 5 epics closed, +7200 insertions across 29 files. The lexer→ngram→
  output chain, the stage3↔accent feedback loop, the conclusion demo,
  and all orchestration machinery are today's work.

## Our entry

**Hearsay-IV** — Hearsay-III (1983) re-implemented with the blackboard
swapped for the patent-pending saturation fabric: a fully mechanistic,
model-free, offline speech transcription device. No neural nets, no
embeddings, no learned weights anywhere (enforced by bead
`hearsay-four-ivs`, `scripts/no_model_guard.sh`). Accent tracking +
classical techniques throughout. Full thesis in [the whitepaper](OPENSTATE-WHITE-PAPER.md).

## Internal vs official dates

- **Internal target: Nov 1** (bead `hearsay-four-say`). Finish the work here.
- **Submission: Nov 4, 9:00 PM ET.** Buffer for packaging, demo, and writeup.
- Running through the tape either way: remainder becomes beads.

## Work log (hackathon period)

| Date | Done |
|---|---|
| Oct 26 | Assessed all 5 in-repo sessions (reset + verified replay, 3464 records). Mandated OpenState tools in AGENTS.md for Muse. Stabilization baseline 204 tests green. |
| Oct 26 | Closed `ivs` (no-model guard, proven red+green), `o21` (lexicon crate scaffold, 4 tests), `79z` (corpus-fetch bin, 5 SCOTUS objects, idempotent). Suite now 208 tests, 0 failed. |
| Oct 26 | Failure-mode audit: method-call refs invisible to `find_referencing_symbols` (real gap, `Fabric`→65 refs works); diagnostics unsupported on 2/2 files (real limitation); replay-order **cleared** (1355 = 1355 both orders — earlier gap was my comparison error). Filed conclusion bead `5qc` (missing verifiable endpoint) + prep bead `zm9` (`src/output` scaffold). Concurrency plan in whitepaper §8. |
| Oct 26 | Beads-native concurrency stood up: `swarm validate` waves (mwa, nak linear, parallel across tracks); labels `hackathon`+`track-a/b/c`; human gate `3ww` holding `say` for owner demo review; worktrees `track-a`/`track-b` (branches, shared DB, auto-gitignored); `merge-slot` for landing; `epic close-eligible` retired aem+cau; `audit record` timestamping for the time-saved ledger. Whitepaper §9. Ready: mwa.1 (A), zm9 (B). |
| Oct 26 | Parallel push executed: Track A closed `mwa.1` (9 ingest tests, arena NodeIds, verified + merged); Track B closed `zm9` (`src/output` scaffold, verified + merged). Suite now 214 tests, 0 failed. Next: mwa.2 (A), s3t wiring (B). |
| Oct 26 | Autonomous run complete: A walked mwa.2 → mwa.3 → idw.1 → idw.2; B walked s3t → nak.1 → nak.2 → nak.3; finale 5qc demo green. Guard caught+fixed one true positive (tch:: false-positive on `word_match::`, fix verified both directions). 5qc workspace root broke 3 crates — fixed with explicit `exclude`, all 14 crates green (243 tests, 0 failed). Demo re-verified on main with SCOTUS audio: exit 0, 6 words, all chains terminate. Only `say` remains, held by human gate `3ww` for owner review. |
| Oct 26 | Velocity ledger: ~4.4 h wall, 11 subagents, 15 task beads + 5 epics closed, suite 204 → 243 green. Meta API credits spent: **$0 of $150** (all local cargo + Muse subscription; portal holds the real balance). Allowance observed: window 6%, weekly 2%. Session store ~39.7 MB (upper-bound token proxy only). Details: whitepaper §10. |
| Oct 26 | Human-experiment tooling: `MicCapture::record_raw()` + `record-session` bin (enrollment + per-utterance WAVs + `manifest.tsv` with rate/samples). Bead `zg8` closed green (build, 8 mic tests, clippy `-D warnings`, fmt; headless `--list-devices` exits 0). Filed `o20` (WAVs → `session.tsv` scorer adapter + enrollment-as-baseline wiring; scorer `failfast_run` 39c.5 already in tree, currently generic baseline). Closed vocab enumerated for directions: 30 words in `src/fse` `CLOSED_VOCABULARY`. |
| Oct 26 | Mic saga + platform decision: WSL2 has zero sound cards (`/proc/asound/cards` absent) — VM cannot see the Windows mic. Wrote `attach-usb-mic.ps1` (Desktop; usbipd bind+attach, fixed missing-`bind` bug post-hoc). Mic recovered Windows-side via replug; usbipd never held it. Decision: record natively on Windows (portability audit clean — no unix-only code, cpal 0.15 WASAPI zero-setup; full Windows link not verifiable from WSL, no sudo for cross-linker). Uncommitted: capture crate changes (mic refactor, bin, hound dep). |
| Oct 26 | Remainder queued as beads (Track A/B shape): `9hh` sample clip (track-c, ready), `9gc` demo `--json` (track-c, waits on `9hh`), `2sh` WASM interactive demo (track-a, waits on `9hh`+`9gc`), `dyy` CLI release matrix (track-b, waits on `9hh`+`9gc`); experiment lane stays `o20` adapter + owner Windows recording, `say` holds submission review behind gate `3ww`. `bd lint` clean. |

## Rules of engagement for this repo during the event

- Beads are the work queue; every bead carries a green/red gate.
- No-model constraint is load-bearing: run the guard before any submission.
- Patent internals stay out of git and beads (repo will go public).
