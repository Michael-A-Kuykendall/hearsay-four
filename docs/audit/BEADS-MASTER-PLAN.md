# Hearsay-IV — Master Plan v3 Bead Decomposition

Created 2026-10-27 from `HEARSAY-IV-MASTER-PLAN (1).md` (repo root),
after the codebase audit in `MASTER-PLAN-AUDIT.md`. For cloud audit
against the plan: every work package (WP) below maps 1:1 to a bead;
the gate column is the bead's binary acceptance gate, verbatim in
substance. Live DB is beads/Dolt; this file is the git-visible copy.

Frozen on plan adoption (§3 Freeze/close): `hearsay-four-urd` (CHUNK-1
epic), `hearsay-four-jf6` (SEG-2), `hearsay-four-qky` (WER-2) — closed
as superseded, reasons recorded in the DB.

Pre-plan beads left open for owner decision: `hearsay-four-0d5` (APP-1),
`hearsay-four-4s6` (SUB-1), `hearsay-four-6if`/`9qy` (DIA stretch),
`hearsay-four-say` (umbrella; stale 11-crate premise),
`hearsay-four-5qw` (phrase_training provenance — subsumed by D0, closes
when D0 records that set).

## Beads by track

| WP | Bead | Pts | Depends on | Binary gate (summary) |
|---|---|---|---|---|
| D0 | hearsay-four-0f6 | 3 | — | Inventory md committed: every set hours/rate/checksum/human-TTS/licence/coverage/role + owner approval |
| D1 | hearsay-four-7ms | 3 | D0 | Split manifests + protocol committed with checksums; LOCKBOX sealed |
| A1 | hearsay-four-b7p | 5 | D1 | Fresh 5th-take PHASE-TEST top-1 ≥ 90% (run1 leave-one-take-out DTW) |
| A2 | hearsay-four-5n2 | 8 | A1 | Owner PHASE-TEST phrases: WER ≤ 25%, boundary ≤ 100ms median, RTF < 1, live tokens bounded |
| A3 | hearsay-four-pri | 5 | — | Same word twice → 2 token facts, distinct spans, independent retraction |
| A4 | hearsay-four-enu | 8 | — | Contract spec frozen (hash recorded); LM-accounting byte-identity + revival property tests green |
| A5 | hearsay-four-p3o | 5 | A2 | Oracle WER at chosen K ≥ 5pts below B's WER on DEV, else acoustic-bottleneck verdict blocks A6 |
| A6 | hearsay-four-cjj | 8 | A5, A4 | Per-budget B-vs-C table + paired CIs + late rescues; claim matches pre-registered reading |
| A7 | hearsay-four-cya | 3 | A6 | RAM ±10% between 30-min and 3-h; archive linear; RTF < 1 |
| R1 | hearsay-four-9pa | 8 | D1 | DEV perplexity per mix reported; LM loads < 2s; deterministic rebuild |
| R2 | hearsay-four-3eh | 8 | D1 | Synthetic-HMM recovery in tolerance; real DEV PER < flat-start, paired CI < 0 |
| R3 | hearsay-four-30y | 5 | R2, D0 | SI DEV WER reported, or closed owner-rejected with record |
| R4 | hearsay-four-jem | 8 | R2 | Adapted owner DEV WER < base, paired CI excludes 0; retraction-restores-base test green |
| R5 | hearsay-four-cri | 8 | A3, A4, R4 | REGRESSION WER ≤ 1.5× A2; RTF < 1; A6 rerun on phone lattice reported |
| R6 | hearsay-four-3ay | 8 | R5 | Owner DEV WER improves over monophone, paired CI excludes 0 |
| R7 | hearsay-four-c0r | 5 | R6, R1 | Four-system comparison; tier T1 ≤50% / T2 ≤25% / T3 ≤12% stated |
| R8 | hearsay-four-7uv | 8 | R7 | Re-added-word recall reported; OOV precision/recall on locked set |
| P1 | hearsay-four-6tc | 5 | R5 | Live/batch parity; provisional latency ≤ 500ms; RTF < 1 |
| P2 | hearsay-four-ofy | 5 | A3, A4 | 100% of committed words render complete evidence chain; validator passes |
| P3 | hearsay-four-xtd | 8 | A4, R4 | Apply+undo N corrections → state byte-identical; correction changes next decode |
| P4 | hearsay-four-b6y | 5 | P3 | Target accuracy improves (CI); competitor + REGRESSION not worse (§4.3 rule) |
| P5 | hearsay-four-kt5 | 5 | P4, P1, P2 | Release green; no_neural_guard green; reproducible model checksum |

Total: 22 beads, 124 points. Beads flagged in their text "split before
claim if over 8pts": A2, A4, A6, R1, R2, R5, R6, R8, P3 (the 8pt set —
splitting happens at claim time per beads skill, gate unchanged).

## Dependency shape (plan §5)

```
D0 ─► D1 ─┬─► A1 ─► A2 ─► A5 ─► A6 ─► A7
          ├─► R1 ────────────────► R7 ─► R8
          └─► R2 ─┬─► R3
                  └─► R4 ─► R5 ─► R6 ─► R7
A3 ────────────────► R5, P2
A4 ────────────────► A6, R5, P2, P3 ─► P4 ─► P5
R5 ─► P1 ─► P5        P2 ─► P5
```

Ready now (no blockers): D0 (0f6), A3 (pri), A4 (enu). After D1 lands:
A1, R1, R2 start concurrently — the plan's stated concurrency point.

## Owner decisions embedded in the beads

1. Per-source approval for R1 text and R3 external audio (licence +
   human/TTS recorded; plan process rule 5).
2. Recording time: R4 needs ≥1 owner hour before its first gate (5–10h
   for dictation targets); A1 PHASE-TEST needs one fresh owner session;
   A2 PHASE-TEST ≥100 owner phrases; P4 needs owner sentence sets.
3. Terminology (plan §2): P5 renames the guard to no_neural_guard.sh and
   retires "model-free" in served-path docs. Committed hackathon papers
   still use the old wording — sweep or exempt explicitly.
4. Pre-plan beads above (APP-1/SUB-1/DIA/say/5qw): adopt, fold into
   P-track, or close. They are not in the plan's map.
