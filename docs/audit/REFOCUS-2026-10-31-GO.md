GO — approved. This message is the final instruction set. Where it differs
from REFOCUS-2026-10-31.md or recording-standard-rs1.md, THIS MESSAGE WINS.

== APPROVALS (record in D0 as use-verified, owner-approved 2026-10-31) ==
1. LibriSpeech train-clean-100, dev-clean, test-clean (openslr.org/12,
   CC BY 4.0). Roles: TRAIN / DEV / PHASE-TEST (public benchmark).
   Audio gitignored; commit manifests + sha256 only. Credit LibriSpeech
   in README and writeup. Check free disk first (need ~15 GB); if short,
   tell me before downloading.
2. LibriSpeech LM normalized text (openslr.org/11). Role: LM-TRAIN general.
   Use a 20–40M-word subset first. Text gitignored; manifest + sha committed.
3. Owner writing = my book (written by me, not AI-generated):
   Windows: C:\Users\micha\Dropbox\Destroyer of Worlds Final 12.6.13.pdf
   WSL:     "/mnt/c/Users/micha/Dropbox/Destroyer of Worlds Final 12.6.13.pdf"
   - Copy read-only to a folder OUTSIDE the repo (e.g. ~/hearsay-personal/),
     set HEARSAY_PERSONAL_TEXT to it. Never modify the Dropbox original.
   - Extract with pdftotext (poppler). No OCR. If there's no text layer,
     stop and tell me.
   - Normalize with lm::normalize. Strip front matter, page numbers,
     running headers/footers; record the stripping rules in the bead.
   - Role: LM-TRAIN personal component, interpolated with the general LM.
     Weight tuned on DEV only (dev-clean transcripts + owner DEV prompts).
   - PRIVACY (hard rule): the PDF, the extracted text, and any LM file
     built with the book never enter git or the repo tree. Add gitignore
     rules + a test that fails if a personal artifact is tracked. Commit
     only a manifest row: dataset "owner-book", PDF sha256, word count,
     sentence count, role. No text excerpts in any doc.
   - Drop any book sentence that matches an eval prompt verbatim
     (read PHASE-TEST, fly, Harvard DEV); log the count.
4. Windows-only capture (RS-1 §1). Approved.

== OWNER RECORDING BUDGET (replaces REFOCUS §7 and §8 recording rows) ==
Hard cap ~25 minutes total, recorded in ONE sitting, as separate sessions,
in this order, after the smoke test below passes QC. Do not ask for more.

| Session ID (owner-<date>-…) | Prompts | Window |
|---|---|---|
| harvard-TRAIN-01 | Harvard lists 1–5 (50 sentences) | 6 s |
| harvard-DEV-01   | Harvard lists 6–8 (30 sentences) | 6 s |
| fly-DEV-01       | 15 lines: 10 fly + 5 family, cut from script-p4-fly-dev.txt | 8 s |
| read-PHASE-TEST-01 | first 50 lines of script-c0r-read.txt | 8 s |
| fly-PHASE-TEST-01  | 15 lines: 10 fly + 5 family, cut from script-p4-fly-ptest.txt | 8 s |
| words30-PHASE-TEST-01 (optional, last) | script-wave3.txt, 30 words | 3 s |

- Commit the cut prompt files (fly 15/15, read 50) BEFORE I record.
- No phrases30 session before Nov 4. No owner LOCKBOX session. Milestone
  claims use the read PHASE-TEST, scored once.
- P4 runs at n=15/15. Report it as descriptive with a bootstrap CI; don't
  claim significance it can't have.
- Before I record anything: RS1-a ready → I do a 5-take Windows smoke test
  (one redo) → RS1-b canonicalizes and QCs it → you tell me "cleared to
  record" with the exact commands for each session above.

== RS1-0 (you do it entirely) ==
Plot frame energy (25/10 ms, dB) for the first 400 ms of 10 run1 clips
(every 12th take), mark the current trimmer span, commit PNGs + verdict
CLICK / NO-CLICK. Only if the plots are ambiguous, ask me to listen to 3
named files.

== OVERRIDES TO REFOCUS §5 ==
- R4-v2 (jem): enrollment = harvard-TRAIN-01 (50 sentences), not ≥300.
  Gate unchanged: owner DEV WER adapted < base via R5a decoder, paired
  bootstrap CI below 0. If the CI includes 0, report "no demonstrated
  difference"; that's a valid outcome, not a reason to request recording.
- R3-v2 (30y): report dev-clean PER and WER. GREEN = plausibility
  (PER < 0.60, WER < 1.0) with training hours stated. 0.45 PER is a
  target, not a gate.
- A1-r: (1) DEV LOTO on canonical run1, (2) 100240 scored once labeled
  cross-channel, (3) words30-PHASE-TEST-01 scored once if recorded.
- L5 (Track A): A2/A5 build and tune on DEV data only (canonical run1 +
  TTS phrase sets per D1). A6 runs before Nov 4 only if DEV lattices show
  an oracle gap; otherwise it's post-deadline. No owner phrase recording.
- Everything else in §5 applies as written.

== EXECUTE IN THIS ORDER ==
1. Move REFOCUS-2026-10-31.md → docs/audit/, recording-standard-rs1.md →
   docs/design/. Append this message verbatim as
   docs/audit/REFOCUS-2026-10-31-GO.md. File one audit bead citing all three.
2. Bead surgery per REFOCUS §5 + the overrides above: close 8lx, yop, 74x,
   ay4, aj7, roa ("superseded by REFOCUS-2026-10-31 §5"); reopen/rewrite
   30y, jem, b7p, 3ay, 71g; file RS1-0, RS1-a..d, A1-r, DATA-1, R2-perf,
   R1-v2, STAT-1, TEST-1, HYG-1 with gates copied verbatim (as amended
   here). Relabel the REFOCUS §3 results in their result docs. No other
   renames or slug surgery.
3. Freeze the six interfaces (REFOCUS §6) in docs/design/interfaces-v1.md:
   decoder trait and canonical session layout first. Commit.
4. Start lanes L1–L7 in parallel (REFOCUS §6). Lanes write only in their
   owned paths. L4 decoder starts now on models/si_base + the current LM
   and swaps models later by file. L2/L3 start downloads first; L3
   extracts the book while text downloads. L1 builds the Windows
   recorder in the Windows clone (C:\Users\micha\repos\hearsay-four-win)
   from the same source as WSL.
5. Freeze for submission Nov 4 at noon ET: demo video, writeup, submit
   by 9 PM ET. Demo tiers per REFOCUS §8; report the tier actually reached.

== RULES (add to AGENTS.md) ==
- Reuse first. No new recorder, loader, resampler, or session format.
- Root cause before guard: measure chain vs data vs code first.
- Plausibility gate before GREEN: PER/WER < 1.0, training size stated,
  test population matches the claim, paired bootstrap CI.
- Seals/precommit only for PHASE-TEST owner sessions.
- Architect review of every result doc before its bead closes.
- Personal data (book, any derived file) never enters git.
- Ask me ONLY for: the smoke test, the one recording sitting, listening
  checks you can't settle from plots, or decisions that change this plan.
  Everything else is decided above.

== REPORT BACK (short) ==
Beads closed/filed, interfaces committed, lanes running + first task each,
download status + disk, book extraction stats (words, sentences, novelty
drops), RS1-0 verdict + plot paths, ETA for the smoke test.
