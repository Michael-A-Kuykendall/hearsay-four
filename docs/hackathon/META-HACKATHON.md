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
| Oct 26 | Packaging lane executed Track A/B style: `9hh` closed (sample.wav 252K, 4 words, chains terminate; `6626f00`), `9gc` closed test-first (focused test failed `E0425` pre-implementation, then green; CLI `--json` 4 words; `cd892f3`). Tracks refreshed to `cd892f3` zero-divergence; `2sh`+`dyy` running as parallel subagents on track-a/track-b. Whitepaper §12 carries the OpenState effectiveness ledger. |
| Oct 26 | `dyy` landed (`eb2a81b`, merged `937dacf`, release gate re-verified on main). `2sh` hit a verified wasm32 closure mismatch (proptest as hard dep in `invariant_ppt`, test-only use); prerequisite `amc` fixed it (20 tests green, wasm32 links; `a4bd294`) and `2sh` retries on refreshed track-a with `amc` as a dependency. Stop-the-line cost: one small bead, zero broken trees. |
| Oct 26 | Lane complete: `2sh` landed (`3db7990`, merged, WASM gate re-verified on main — 4 tests, wasm32 links, page callouts present). Closed this lap: `9hh`, `9gc`, `amc`, `dyy`, `2sh`. Post-merge state verified: release smoke green, runner 7+1 green, guard green. Remaining queue: `o20` (owner recording) + `say` (submission review, gate `3ww`). |
| Oct 26 | Windows/WSL reunited: origin had 3 Windows commits (`5a410c8` recorder byte-identical to WSL `787178d`, `43eaa39` `--resume-at` + tests, `20a8dc3` 120-utterance wave plan + run1 manifest). Merged as `872e54e` (add/add conflict in `record-session.rs` resolved for origin). Capture gates green post-merge (8+2 tests, clippy, fmt). 121 WAVs copied to `sessions/run1/` (gitignored); verified: 0 missing, 120 utterances, 30 words × 4, all 48 kHz mono; 23 rows are 143520 samples vs 144000 (adapter must use actual WAV lengths — already the `o20` design). Lock normalization follow-through committed (`4c1915a`); pushed, 0/0 with origin. |
| Oct 26 | `o20` closed (`7faa438`): `session-ingest` bin (manifest validation, enrollment skip, K=3×40 pre-registered in bead notes, timestamps from actual WAV lengths) + `failfast_run --root`. Full accent suite 45+3 green. Run1 verdict: exit 0, **PASS** — tail separation true, margin 107.53 (floor 0.02), monotone-B true; bucket means A=(171.50, 177.47, 200.42), B=(85.21, 77.95, 84.87). Stated limits: generic baseline (not enrollment — follow-up `fo7` filed), scores are mean-NLL magnitudes, B already ahead in bucket 0. `say` still holds submission review (gate `3ww`). |
| Oct 27 | `fo7` closed (`3de1e46`): `failfast_run --enroll` builds the baseline from enroll.wav via `accent::baseline_from_frames` (same fse front end; test-first, 2 new tests). Second run1 verdict with speaker baseline: exit 0, **PASS** — margin 1.8942, monotone-B true. Measured delta vs generic run: enrollment baseline improves A enormously (generic A≈189 → enroll A≈-5.7 NLL), so the carry-forward effect size under a speaker baseline is ~1.9 points, superseding the ~107-point generic-baseline figure — still a PASS, and the submission quotes the measured ~1.9. Full accent suite 47+3 green. |
| Oct 27 | `zl3` Oyez pilot closed (`73c751c`): `scripts/oyez_ingest.py` + corpus `mp3-to-wav` bin; PPL Montana (21627, 61 min, 216 speaker turns) → 642 scored sentence-blocks, K=8 (7×80+82) pre-registered then amended for 1 zero-duration Breyer block dropped loudly. 3/3 API spot-checks match. Verdict: exit 0, **PASS** — margin 38.72, monotone-B true; A=(82.19..69.31), B=(38.96..37.07). Court lane proven end to end on free CC data; audio (14.7 MB) gitignored, manifest+transcript committed. `c1g` bulk lane unblocked. |
| Oct 27 | `c1g` Oyez bulk closed: `scripts/oyez_bulk_fetch.py` crawls `/cases` (no auth), first transcript-bearing segment per term; N=3 pre-registered eras {1971 Groppi (15919, 444 blocks, 7×55+59), 2011 PPL Montana (21627, 642 blocks, 7×80+82, byte-identical pilot replica), 2025 Hikma (25951, 503 blocks, 7×62+69)}; 2 stop<=start blocks dropped loudly under the zl3 rule. Verdicts: 3/3 exit 0 **PASS** — margins 94.72 / 38.72 / 40.86, monotone-B true. Carry-forward evidence: Roberts/Sotomayor/Alito/Kagan persist 2011→2025, advocates rotate fully. Audio (~44 MB) gitignored, manifests+transcripts+`sessions/oyez-bulk/MANIFEST.json` committed. |

## Rules of engagement for this repo during the event

- Beads are the work queue; every bead carries a green/red gate.
- No-model constraint is load-bearing: run the guard before any submission.
- Patent internals stay out of git and beads (repo will go public).
