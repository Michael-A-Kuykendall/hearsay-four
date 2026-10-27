# Hearsay-Four — Session History (for external audit)

Written 2026-10-27. Purpose: let an external auditor see the same history
the local tools see, from git alone. Full verbatim transcripts of the four
OpenCode sessions are in `transcripts/` in this directory (JSON exports
from OpenCode's own store: session row + every message + every part, in
record order). They are unedited; expect local paths and dictation
artifacts.

## Session inventory

| # | Source | ID | Title / dates | Size |
|---|---|---|---|---|
| 1 | OpenCode | ses_ffd6c5422ffep2RdSLp7h2C0jW | "New session", 2026-08-14 | 713 messages, 2,750 parts, 5.4 MB |
| 2 | OpenCode | ses_ff9afbbb7ffeV5MX7uQ39F5XB8 | "New session", 2026-08-15 | 295 messages, 1,057 parts, 4.8 MB |
| 3 | OpenCode | ses_ff966ff16ffeICRqgbyddkBUVf | "Enable Serena startup skill" | 664 messages, 2,844 parts, 6.2 MB |
| 4 | OpenCode | ses_ff82d2902ffeB47AD2RfaYPqOz | "Investigating wiped directory session state" | 515 messages, 1,886 parts, 8.3 MB |
| 5 | Muse (current) | 01a178d3-4e05-7fa1-8ac5-630f30ac8f6c | Hackathon session, 2026-10-26/27 | Local store only; excluded from OpenState replay by design (live session) |

OpenState replay of sessions 1–4 (2026-10-27, procedure B: reset → assess
in order → capsule → checkpoint): 1,169 + 392 + 1,162 + 741 = 3,464
records absorbed, 0 skipped; 1,357 live facts. The same 3,464 figure
appears in the Oct 26 hackathon work log, from the earlier assessment.

## What the history shows (evidence, not narration)

- Sessions 1–2: research setup (agents.md, HEARSAY-IV-RESEARCH-SYNTHESIS),
  beads initialization, and the first plan cull: "59 issues, 12 epics, 7
  dependency layers" (74 issues then; 144 now). The D-1 fabric fix and
  retraction-cascade debugging are recorded here, with failing-then-passing
  test output in the transcripts.
- Session 3: the Serena symbolic-tool era. The failed tool calls in the
  record (`serena_find_referencing_symbols`, `serena_create_text_file`,
  etc.) are that toolset's failures, later replaced by the OpenState
  symbolic surface. Tooling fault vs agent misuse is separable per record
  in the transcripts.
- Session 4: recovery from wiped OpenCode session state — the event that
  motivated OpenState's assess/replay design. The replay you are reading
  is the direct product of that incident.
- Recorded owner directives across sessions (verbatim in OpenState user
  priorities): "do not import the D0 engine let's use what we have";
  "LISTEN dzero does saturation fabric; I didnt think to have you start
  with my engine first"; "write beads for the plan, raise your hand if
  something exceeds the original plan"; "beads heard as beats" (dictation
  artifact frozen as a task string).

## Current session (Muse, 2026-10-26/27) — condensed record

Full log lives in the Muse session store (not committed; 56k+ event
records). Load-bearing events, in order, all verifiable against git:

1. Stabilization and the 15-bead transcription-service plan (initiatives
   A–E) implemented in concurrent lane worktrees, merged, pushed.
2. WER-1: span-sliced absolute WER = 1.0 on 9 Oyez spans. PERF-1 fabric
   scaling fix (~50x, no retained wall-clock artifact).
3. Single-speaker wave: RTF-1 (0.047), TXT-1, MP3-1, MIC-1, PART-1,
   PKG-1 closed green with gates re-run on main.
4. Errors caught by independent re-verification: nested-workspace lane
   failure; a pipe-masked nonzero merge exit that closed a bead early
   (corrected); a serde_json 1-ulp parse defect (float_roundtrip fix,
   proptest regression kept); a C2/C3 merge that dropped the verification
   hook (ported, re-verified); PKG-1/SEG-1 phantom lane greens (zero
   unique commits; rebuilt).
5. SEG-1 landed (fse boundary detector). Its original Oyez-recall premise
   was measured false first (all transcript gaps 0.0s) and the bead was
   amended before implementation.
6. Audit workflow (9 agents, 2026-10-27): session audits, beads-plan
   audit, code↔Hearsay map, literature check, metrics-validity check,
   adversarial skeptic. Findings are consolidated in AUDIT-PACKAGE.md.
   One gap: its "prior session" auditor landed on this session's own log;
   the OpenCode replay above closes that gap.
7. This package committed at the owner's instruction so a cloud auditor
   with git access sees the same evidence.

## How to audit with this history

- Read AUDIT-PACKAGE.md Sections 2–5, then sample the transcripts at the
  cited events (search by session ID + quoted strings above).
- Cross-check every number against git: `git log --oneline`, bead export
  in BEADS-EXPORT.md, and `.beads/issues.jsonl` (passive export; live DB
  is Dolt under `.beads/dolt/`).
- The transcripts contain the agents' own claims. Treat agent claims in
  transcripts as claims; the git tree and test suites are the evidence.
