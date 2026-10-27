# Master Plan v3 — Codebase Audit (2026-10-27)

Audited `HEARSAY-IV-MASTER-PLAN (1).md` (Claude + GPT converged) against
code at HEAD 7213203 + this session's reads. Method: OpenState symbolic
search/read of every cited symbol; text scans of src/ (148 files).

## Ground truth §3 — verified

| Plan claim | Verdict | Evidence |
|---|---|---|
| Phone templates are arithmetic | **CONFIRMED, byte-exact** | fse/src/lib.rs:428 `default_codebook`: `mean = ((i*7+j*3)%11)/5.0 − 1.0`. No fitted statistics anywhere. Acoustic phone labels are placeholder-grade |
| Fabric observes, doesn't decide | CONFIRMED | runner process_chunk: window winner feeds only a rating; word from top-1 phone alignment |
| One word per chunk (`scored[0]`) | CONFIRMED | runner/src/lib.rs process_chunk; 81% floor arithmetic checks (576/711) |
| Token identity destroyed | CONFIRMED | Phone-only syllable facts; word premises are a `BTreeSet` (order/multiplicity lost) |
| N-grams untrained, post-commit | CONFIRMED | runner constructs `BigramTable::new()` (lib.rs:410); zero `.train(` calls in runner. `train()` exists in sequence crate, never invoked on the served path |
| Accent PASS circular | PARTIALLY SUPPORTED | accent/src/lib.rs:96 ConditionB: EMA signature (α=0.2, β=1.0) adapted from the stream it scores. Online adaptation is legitimate if adaptation lags scoring; the PASS wording overstates it as recognition improvement. Plan's narrowed claim is the correct one |
| Vocab coverage 35/39, missing DH G UH ZH | NOT RE-VERIFIED | Plausible against the 30-word closed vocab; not recomputed here |
| Keep/Demote/Freeze lists | CONFIRMED as accurate | All "keep" items exist (fabric, provenance, decode, mic, streaming, PART-1, exports, guard, MFCC). All "demote" items verified above. WER-2/SEG-2/CHUNK-1 freeze is consistent with AUDIT-PACKAGE §8 |

## Work-package existence map

| WP | Status in code |
|---|---|
| D0/D1 data inventory + partitions | Absent (better-voice sibling repo exists with assessment docs; no inventory artifact, no split manifests) |
| A1 DTW acoustic gate | **Absent** — zero `dtw` hits in src/ |
| A2 connected DTW decoder | Absent |
| A3 token identity | Absent (type facts only; see above) |
| A4 fabric decision contract | Absent — no docs/design/fabric-decision-contract.md; no source classes or llr weights in fabric |
| A5 lattice + oracle | Absent — no lattice format (only a provenance doc comment) |
| A6 A/B/C experiment | Absent |
| A7 bound gate | Partial — RTF harness exists (fe3); no RAM/archive bound test |
| R1 LM (Kneser-Ney, personal LM) | Partial substrate — bigram/trigram counting exists; no KN discounting, no text pipeline, never trained in served path |
| R2 HMM trainer | **Absent entirely** — no HMM, no Baum-Welch (zero hits) |
| R3 SI base model | Absent |
| R4 enrollment + MAP | Partial — enrollment baseline fit exists (calibration); no sufficient-statistic facts, no MAP overlay |
| R5 trie phone decoder | Absent — lexicon is table + edit distance (the `trie` text hits are inside `entries`); B2 fuzzy matcher is the stand-in |
| R6 triphones/state tying | Absent |
| R7 LVCSR evaluation | Partial — WER harness exists (output::wer lib.rs:739, wer-bulk); no LVCSR path to evaluate |
| R8 open vocabulary | Absent |
| P1 streaming + VAD | Partial — live/batch streaming + SEG-1 boundaries exist; no VAD/endpointing module |
| P2 why-this-word view | Partial — WordRecord provenance + runners-up recorded as retracted hypotheses; no per-word llr/alternatives view |
| P3 correction loop | Absent — no USER source class, no retractable correction facts |
| P4 fly benchmark | Absent |
| P5 packaging | Partial — release.sh, serve binary, guard exist |

## Where the plan overreaches (auditor notes)

1. **Scale vs deadline.** R2 (HTK-style trainer) + R5 (trie decoder) +
   R6 are each multi-week classical builds done carefully. The plan has
   no dates by design, but as a Nov 1–4 hackathon path only M1 (A1) and
   possibly M2 (A2+A5+A6 on the 30-word vocab) are credible. M4 app
   claims are not reachable in the window; the plan's own milestone
   table supports submitting M1/M2 evidence instead.
2. **Owner-hours dependency.** R4 wants 1h minimum, 5–10h for dictation.
   That is owner recording time, not agent time; it gates Track R
   regardless of code speed.
3. **A2 gate optimism.** WER ≤ 25% on owner connected phrases is a real
   bar for template DTW on 30 words; plausible, unproven. A1's 90%
   held-out gate is the correct first falsifier and is cheap.
4. **Terminology conflict.** Plan §2 bans "model-free" / "no training
   corpus" and renames the guard. Committed docs, beads, and the
   hackathon papers use "model-free" throughout. Adopting the plan
   requires a terminology sweep + guard rename (small, mechanical).
5. **TTS contamination is the plan's own tripwire and it is armed:**
   better-voice sets are unprovenanced (bead hearsay-four-5qw already
   open on exactly this). D0 must precede any DEV use, as the plan says.

## Bottom line

The plan's §3 is more accurate about this codebase than the bead queue
it supersedes — it caught the arithmetic codebook, which the local audit
(AUDIT-PACKAGE.md) missed and which is more fundamental than the chunking
defect: with formula-generated templates, even perfect segmentation
decodes against meaningless phone prototypes. The plan is adoptable as
the bead source document. Recommended adoption order: D0 → D1 → A1
(falsify the front end first, cheapest gate), with WER-2/SEG-2/CHUNK-1
frozen as the plan directs. Owner decisions required: terminology sweep
(item 4), external-audio approval for R3, recording-time commitment
(item 2).
