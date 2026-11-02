# Recording readiness checklist (lane E, bead hearsay-four-9ud8--adaptation-plumbing-runbook)

Each item is GREEN or a named blocker, with the exact verification command.
Recording (docs/OWNER-RECORDING-RUNBOOK.md) starts only when all four are
GREEN and the coordinator has said "cleared to record" after the smoke test.

## 1. Windows recorder built (RS1-a) — RED

Blocker B-REC-1 (unify): two divergent recorders exist, neither does the
whole job. The Windows clone
(`C:\Users\micha\repos\hearsay-four-win`, commits `be8b49f`/`76fc45b`,
unknown to main) has RS1-a UX — `[r]` redo, click guard, per-attempt log,
`#` comment skipping — in a 375-line `record-session.rs` with NO sealed
mode (`seal`: 0 matches). Main's recorder (621 lines) has sealed sessions,
PRECOMMIT, and `--validate`, but no redo and no click guard. The GO plan
needs both (redo smoke test + sealed phase-tests) in one binary.

Blocker B-REC-2 (rebuild): the current
`src\capture\target\debug\record-session.exe` is dated Oct 26 17:47 and
predates the Oct 31 RS1-a source (31 Oct 12:22) — it has neither feature
set. Rebuild on Windows after the unify lands:

```powershell
cd C:\Users\micha\repos\hearsay-four-win\src\capture
cargo build --features mic --bin record-session
.\target\debug\record-session.exe --list-devices   # expect fifine, exit 0
```

Verify (green criteria): exe mtime newer than
`src\capture\src\bin\record-session.rs`, `--list-devices` exit 0, and
`--validate` on a sealed session prints VALID (see item 4).

## 2. Canonicalizer QC ready — GREEN

`canon-session` (RS-1 §4) builds from this checkout and the canonical
run1 on main carries manifest + QC: `sessions/canon/run1`
(`manifest.tsv`, `qc.tsv`, `provenance.tsv`, `SHA256SUMS`, 120 takes),
plus canonicalized `sessions/canon/phase-test-20261031-100240`.

Observed (lane, 2026-11-01):

```bash
cargo build -p corpus --bin canon-session   # Finished dev profile, OK
ls sessions/canon/run1/qc.tsv              # present; take000 status OK
```

RS1-b action at recording time (not a tooling gap): canonicalize + QC the
smoke session, then the six GO sessions, before scoring anything.

## 3. Prompt files cut and committed — GREEN in lane (commit at merge)

| Session | File | Lines | State |
|---|---|---|---|
| harvard-TRAIN-01 | `sessions/prompts-harvard-train-01.txt` | 50 | NEW this bead, byte-exact cut of `script-harvard.txt` lines 1–50 (lists 1–5) |
| harvard-DEV-01 | `sessions/prompts-harvard-dev-01.txt` | 30 | NEW this bead, byte-exact cut of lines 51–80 (lists 6–8) |
| fly-DEV-01 | `sessions/prompts-fly-dev-01.txt` | 15 | committed (RS1-d GO cut) |
| read-PHASE-TEST-01 | `sessions/prompts-read-phase-test-01.txt` | 50 | committed (RS1-d GO cut) |
| fly-PHASE-TEST-01 | `sessions/prompts-fly-phase-test-01.txt` | 15 | committed (RS1-d GO cut) |
| words30-PHASE-TEST-01 | `sessions/script-wave3.txt` | 30 | committed |

New cuts carry `#` provenance headers (`source_sha256`,
`content_lines`) in the RS1-d format; both recorders skip `#` lines.
`source_sha256=68353761c9462cb4da4669f4b496247e74ff0fd903a2ba248e3d818f51a5b28a`
(matches the committed `script-harvard.txt`). Lane E leaves changes
uncommitted per lane rules; the coordinator commits the two new files at
merge — that commit is the remaining step, not new work.

Verify:

```bash
bash -c 'cd sessions && head -50 script-harvard.txt | cmp - <(tail -n +3 prompts-harvard-train-01.txt) && sed -n "51,80p" script-harvard.txt | cmp - <(tail -n +3 prompts-harvard-dev-01.txt)'
# TRAIN-BYTES-OK + DEV-BYTES-OK (observed 2026-11-01)
```

## 4. PRECOMMIT flow tested — GREEN

Seal builders unit-tested (`cargo test -p capture`: 9 passed, incl.
`order_txt_has_seed_line_first`, `manifest_has_header_plus_rows`,
`sums_file_format_matches_sha256sum`), and the full validate path proven
on the real sealed session from Oct 31 with a fresh mic-feature build:

```bash
cargo build -p capture --features mic --bin record-session  # Finished, OK
./target/debug/record-session --validate sessions/phase-test-20261031-100240
# VALID: sessions/phase-test-20261031-100240, exit 0 (observed 2026-11-01)
```

Note: live end-to-end sealed capture still happens first at recording
time (no mic in lanes); every sealed session in the runbook is followed
immediately by its `--validate` line, so a failure surfaces before the
next session starts.
