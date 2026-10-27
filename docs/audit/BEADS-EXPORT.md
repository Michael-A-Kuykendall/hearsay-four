# Hearsay-Four — Beads Export (git-visible copy)

Exported 2026-10-27 from the beads (Dolt) database, which is not readable
from git alone. Live DB: `.beads/dolt/`; passive export `.beads/issues.jsonl`.
Stats at export: **144 total · 133 closed · 9 open · 0 in progress · 4 blocked**.

Audit flags (from the 2026-10-27 beads-plan audit, AUDIT-PACKAGE.md §4):

- `hearsay-four-urd` (CHUNK-1): fully self-contained, but its pass bar tests
  the wrong property (acoustic word-boundary recall). Supersede or rewrite.
- `hearsay-four-jf6` (SEG-2): description embeds the word/syllable
  conflation ("SEG-1 detects word boundaries"); ordering vs CHUNK-1 is a
  note, not a dependency (`bd` forbids epic↔task deps). False-ready.
- `hearsay-four-5qw`: no method, no binary gate. Placeholder.
- `hearsay-four-6if` / `9qy` (DIA stretch): gates depend on APP-1 landing;
  9qy clustering criterion vague.
- `hearsay-four-say`: premise says "all 11 crates"; workspace now has 14
  members. Stale. Human gate `3ww` holds submission review.
- `hearsay-four-4s6` (SUB-1): sound, but blocked on WER-2 numbers that are
  currently WER = 1.0.

## Open beads (9)

### hearsay-four-urd [EPIC, P2, OPEN] — CHUNK-1: Fail-fast — can any stream be chunked into words acoustically
Tests whether the SEG-1 detector can chunk streams into words, using
concatenated isolated-word WAVs (sessions/run1, 120 clips) at gaps
{0,50,150,300}ms with construction-exact ground truth. Pre-registered bar:
batch recall ≥70% at 0ms gaps → integrate detector as word chunker; below
→ pivot to phrase groups + downstream lexicon/bigram word splitting.
Streaming-parity bar: within 10pts of batch. Gate: `cargo test -p fse
chunk_streams` exits 0; decision + per-condition precision/recall recorded
in bead notes; SEG-2 amended to match. **Audit flag: pass bar tests a
property the architecture does not require (AUDIT-PACKAGE §2).**

### hearsay-four-jf6 [P2, OPEN] — SEG-2: Variable-segment pipeline integration (5pts)
Replace fixed chunking with boundary-driven segments (one committed word
candidate per segment; multi-word segments may emit N-best sequences via
B2 fuzzy matcher). Depends on SEG-1 (closed). Ordering note: do not start
until CHUNK-1 records its go/no-go. Gate: streaming + parity suites green;
hypothesis density within 2x of reference on Oyez fixture; WER-REPORT
addendum. **Audit flag: "SEG-1 detects word boundaries" is the conflation;
density gate is not an accuracy gate.**

### hearsay-four-qky [P2, OPEN] — WER-2: Open-vocabulary path accuracy delta (5pts)
Wire full-dict fuzzy match + trigram rescore into the served path; re-run
WER-1 driver over the 3 bulk cases; append before/after WER to
sessions/oyez-bulk/WER-REPORT.md. Gate: report updated with deltas, driver
exits 0, served-path test green. Blocks SUB-1.

### hearsay-four-0d5 [P2, OPEN] — APP-1: Single-speaker app assembly (5pts)
Assemble mic-or-file → live text + provenance + SRT/VTT/TXT exports from
RTF-1/MIC-1/TXT-1/MP3-1/PART-1 (all closed). Gate: scripted fixture run
produces all three exports with valid provenance, exit 0; measured profile
in hackathon doc. Blocks DIA-1/DIA-2.

### hearsay-four-4s6 [P2, OPEN] — SUB-1: Submission freeze with measured numbers (3pts)
Final Nov 1 assembly: hackathon + OpenState papers updated with absolute
numbers, workspace suite + guard green, pushed hash recorded. Depends on
PKG-1 (closed), WER-1 (closed), WER-2 (open).

### hearsay-four-6if [P2, OPEN, STRETCH] — DIA-1: Speech segmentation for diarization (5pts)
Classical energy-based speech segmentation after APP-1. Gate: fixture
boundaries within tolerance; silence/speech edge cases green.

### hearsay-four-9qy [P2, OPEN, STRETCH] — DIA-2: Per-segment voice signatures (5pts)
Cluster per-segment voice signatures (D1 machinery) into speaker labels.
Gate: two-voice fixture → exactly 2 stable labels; single-voice → 1.

### hearsay-four-5qw [P1, OPEN] — Establish provenance of better-voice phrase_training clips
3,500 clips (633MB) with undocumented origin; 0/3500 fully in the 30-word
vocab. Determine human/synthetic/mixed with cited evidence before any
evidentiary use. **Audit flag: no method, no binary gate.**

### hearsay-four-say [P1, OPEN] — Hackathon (Nov 1): stabilization green + full project planned in beads
Umbrella: stabilization gates per crate + full stem-to-stern bead plan.
Human submission review held separately (gate hearsay-four-3ww). **Audit
flag: "all 11 crates" premise is stale (14 workspace members now).**

## Recently closed (133 total; most recent shown)

SEG-1 uth (boundary detector, gate weakened — see AUDIT-PACKAGE §4.2) ·
PART-1 2bs (provisional display) · PKG-1 nbm (packaging; gate re-run on
main) · MIC-1 4zx · RTF-1 fe3 · PERF-1 gmp · WER-1 4b7 (WER = 1.0) ·
MP3-1 gmv · TXT-1 kpz · A1 e24 / A2 chd / A3 nwu · B2 r4g / B3 7jq ·
C1 shq / C2 ea3 / C3 7e4 · D1 cz0 / D2 dib / D3 7eh · E1 j6r / E2 pbq /
E3 aff · o21 lexicon scaffold · zm9 output scaffold · 7e4 streaming ·
s3t stage3-accent wiring. Full list: `bd list --status closed` (133).
