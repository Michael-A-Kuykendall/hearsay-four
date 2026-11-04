# Hearsay-IV — Master Plan v3 Bead Decomposition (regenerated 2026-10-27)

Source plan: `HEARSAY-IV-MASTER-PLAN.md` (repo root). Codebase audit:
`MASTER-PLAN-AUDIT.md`. This file was regenerated after the auditor surgical
pass so it contains ONE authoritative graph and the FULL current body of
every open master-plan bead (generated from `bd show`, verbatim). Live DB
is beads/Dolt; this is the git-visible copy an external auditor can certify.

Frozen on plan adoption: CHUNK-1 `urd`, SEG-2 `jf6`, WER-2 `qky` (closed,
superseded). Split originals closed: A4 `enu`, R1 `9pa`, R2 `3eh`, R5 `cri`,
R8 `7uv`, P3 `xtd`. Stale-bead dispositions: APP-1 `0d5`, SUB-1 `4s6`, DIA-1
`6if`, `say` closed superseded (fixtures/smoke migrated to P5/P1 notes);
DIA-2 `9qy` closed deferred/out-of-scope; `5qw` folded into D0. Remaining
non-plan bead `hearsay-four-3ww` closed 2026-10-27 as inert (its blocked bead was already closed; owner-approval gates removed by owner directive, replaced with D0 use-verification).

## Current beads (28 open, 151 points)

| WP | Bead | Pts | Direct dependencies |
|---|---|---|---|
| D0 | hearsay-four-0f6 | 3 | — |
| D1 | hearsay-four-7ms | 3 | D0 |
| A1 | hearsay-four-b7p | 5 | D1 |
| A2 | hearsay-four-5n2 | 8 | A1 |
| A3 | hearsay-four-pri | 5 | — |
| A4a | hearsay-four-p7u | 5 | — |
| A4b | hearsay-four-dg8 | 8 | A4a |
| A5 | hearsay-four-p3o | 5 | A2, A4b |
| A6 | hearsay-four-cjj | 8 | A3, A4b, A5 |
| A7 | hearsay-four-cya | 3 | A6 |
| R1a | hearsay-four-baj | 5 | D1 |
| R1b | hearsay-four-2a0 | 5 | R1a |
| R2a | hearsay-four-tnk | 8 | D1 |
| R2b | hearsay-four-0bv | 5 | R2a |
| R3 | hearsay-four-30y | 5 | R2b, D0 |
| R4 | hearsay-four-jem | 8 | R2b |
| R5a | hearsay-four-71g | 8 | R1b, R4, A3, A4b, A5 |
| R5b | hearsay-four-fi1 | 3 | R5a, A6 |
| R6 | hearsay-four-3ay | 8 | R5b |
| R7 | hearsay-four-c0r | 5 | R6, R1b |
| R8a | hearsay-four-wya | 5 | R7 |
| R8b | hearsay-four-5sw | 5 | R8a |
| P1 | hearsay-four-6tc | 5 | R5a |
| P2 | hearsay-four-ofy | 5 | A3, A4b |
| P3a | hearsay-four-f56 | 5 | A4b, R4, R1b |
| P3b | hearsay-four-6gt | 3 | P3a, R5a |
| P4 | hearsay-four-b6y | 5 | P3b |
| P5 | hearsay-four-kt5 | 5 | R7, P1, P2, P4 |

## Authoritative dependency graph

```text
D0 -> D1 -+-> A1 -> A2 -> A5 -> A6 -> A7
          +-> R1a -> R1b -+-> R7 -> R8a -> R8b
          +-> R2a -> R2b -+-> R3
                          +-> R4 -> R5a -> R5b -> R6 -> R7
A3 -----------------------> A6, R5a, P2
A4a -> A4b ---------------> A5, A6, R5a, P2, P3a
A5 -> R5a        R5a + A6 -> R5b
R5a -> P1        A4b + R4 + R1b -> P3a -> P3b -> P4
R7 + P1 + P2 + P4 -> P5
```

Ready now: D0 `0f6`, A4a `p7u`, A3 `pri`. After D1: A1, R1a, R2a unlock.

## Five split-parent rewires — verified 2026-10-27 against the live DB

- R2b `0bv` -> R3 `30y`: present (alongside satisfied closed parent R2 `3eh`).
- R2b `0bv` -> R4 `jem`: present.
- A4b `dg8` -> P2 `ofy`: present (alongside satisfied closed parent A4 `enu`).
- R5a `71g` -> P1 `6tc`: present (alongside satisfied closed parent R5 `cri`).
- P3b `6gt` -> P4 `b6y`: present (alongside satisfied closed parent P3 `xtd`).

No descendant is blocked solely by a closed parent; each carries its live split-child edge.

## Lockbox ownership

A6 `cjj` owns M2; P5 `kt5` owns M4; R8b `5sw` owns M5. Openings logged in
bead notes, at most once each. Details in the bead bodies below.

## Full open bead bodies (verbatim `bd show`)

### D0 — hearsay-four-0f6

```text
○ hearsay-four-0f6 · D0: Data inventory — better-voice sets + accent_datasets (3pts)   [● P1 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: HEARSAY-IV-MASTER-PLAN v3 S12 (D0). For every better-voice set and  
  accent_datasets/* (sibling repo ../better-voice + repo     
  data): record exists, file count, hours, sample rate, checksum, human-or-TTS
  determination (generation scripts/reports + listen to 10 clips per set),    
  speakers, licence, 30-word coverage, proposed data role (TRAIN/DEV per plan 
  S4.1). Assume TTS until proven human. Output: docs/results/D0-DATA-INVENTORY.md; no set is used before its use-verification is recorded. Overlaps open bead     
  hearsay-four-5qw (phrase_training provenance) — this bead is the full       
  inventory; 5qw closes when D0 records phrase_training. Data rule: inventory 
  only, no scoring, no new sources.                                           



ACCEPTANCE CRITERIA

  Gate: docs/results/D0-DATA-INVENTORY.md committed AND validator script/test 
  passes: every dataset row carries path/status, file count, duration, sample 
  rate, human|TTS|unknown determination, speaker info, licence,               
  checksum/status, 30-word coverage, assigned data role, use-verification status. The 
  phrase_training provenance question (ex-hearsay-four-5qw) is answered here  
  explicitly. Use-verification recorded per row in D0 (free, no payment/account/password/API key, licence permits use). Validator fails on any 
  missing field so future changes cannot silently invalidate the inventory.   



BLOCKS
  ← ○ hearsay-four-7ms: D1: Partitions + recording protocol (3pts) ● P1
  ← ○ hearsay-four-30y: R3: Base acoustic model, speaker-independent (5pts) ● P2
```

### D1 — hearsay-four-7ms

```text
○ hearsay-four-7ms · D1: Partitions + recording protocol (3pts)   [● P1 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: HEARSAY-IV-MASTER-PLAN v3 S12 (D1) + S4.1. Committed split manifests
  (file lists + checksums) for every D0 use-verified set BEFORE any scoring uses 
  them. Splits by recording session (owner audio) and by case chronological   
  (Oyez); never by sentence within a session/case. LM training text never     
  contains DEV/PHASE-TEST/LOCKBOX transcripts. Recording protocol doc: session
  naming, prompt lists (Harvard sentences, 30-word phrases, P4 target         
  sentences), mic/room/rate standard (16kHz mono). LOCKBOX sessions recorded  
  and sealed: checksums committed, files not read by any pipeline until a     
  milestone opening (logged).                                                 



ACCEPTANCE CRITERIA

  Gate: split manifests + recording protocol committed with checksums AND     
  automated partition-invariant test green forever: TRAIN/DEV/PHASE-          
  TEST/LOCKBOX pairwise disjoint; every referenced file exists; checksums     
  match; no recording/session crosses partitions; LM text excludes DEV/PHASE- 
  TEST/LOCKBOX transcripts; lockbox files unreferenced by ordinary evaluation 
  configs. LOCKBOX seal recorded in bead notes.                               



DEPENDS ON
  → ○ hearsay-four-0f6: D0: Data inventory — better-voice sets + accent_datasets (3pts) ● P1

BLOCKS
  ← ○ hearsay-four-tnk: R2a: HMM trainer core — flat start, forced alignment, Baum-Welch, serialization (8pts) ● P2
  ← ○ hearsay-four-baj: R1a: LM core — ingestion, normalization, Kneser-Ney counts (5pts) ● P2
  ← ○ hearsay-four-b7p: A1: Acoustic gate — leave-one-take-out DTW front-end falsification (5pts) ● P2
  ← ✓ hearsay-four-3eh: R2: HMM training subsystem — flat start to Baum-Welch (8pts) ● P2
  ← ✓ hearsay-four-9pa: R1: Language model — trigram Kneser-Ney + personal LM (8pts) ● P2
```

### A1 — hearsay-four-b7p

```text
○ hearsay-four-b7p · A1: Acoustic gate — leave-one-take-out DTW front-end falsification (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S6 A1. Question: do classical MFCC features          
  discriminate the owner's 30 words at all? DEV: leave-one-take-out DTW on    
  sessions/run1 (120 clips, 4 takes x 30 words, fixed distance on MFCC frames,
  Sakoe-Chiba band, length-normalized cost); tune front end here only (CMN,   
  deltas/delta-deltas, pre-emphasis, liftering, band). PHASE-TEST: NEW owner  
  session, 5th take of all 30 words; templates = original 4 takes; scored     
  once. Artifacts: docs/results/A1-ACOUSTIC-GATE.md + CSV + confusion matrix. 
  Fail path: fix front end on DEV; rerun needs a NEW phase-test session. Split
  before claim if over 8pts.                                                  



ACCEPTANCE CRITERIA

  Gate: PHASE-TEST top-1 >= 90% on the fresh 5th-take session; artifacts      
  committed; result recorded in bead notes.                                   



DEPENDS ON
  → ○ hearsay-four-7ms: D1: Partitions + recording protocol (3pts) ● P1

BLOCKS
  ← ○ hearsay-four-5n2: A2: Connected-word DTW decoder, token-passing (8pts) ● P2
```

### A2 — hearsay-four-5n2

```text
○ hearsay-four-5n2 · A2: Connected-word DTW decoder, token-passing (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S6 A2. One-stage connected-word DTW / token-passing  
  decoder (Ney family) over the A1 front end. DTW cells are NOT fabric facts. 
  Token state: (unit in {word,SIL,SP,GARBAGE}, template_pos, start_frame,     
  path_cost, backpointer, lm_history). Required: SIL/SP templates from        
  recording lead/trail, optional garbage state, word-insertion penalty,       
  min/max word duration, path-length normalization, SIL endpoints, beam width,
  max-live tokens, max template expansion. Data: connected 3-7-word phrases of
  30-word vocab; DEV may use D0 use-verified data (TTS allowed for DEV only);     
  PHASE-TEST is owner human speech >=100 phrases, own session. Split before   
  claim if over 8pts.                                                         



ACCEPTANCE CRITERIA

  Gate: PHASE-TEST WER <= 25%; boundary error <= 100ms median on 20 hand-     
  checked phrases; RTF < 1; peak live tokens <= configured bound; results in  
  docs/results/A2-CONNECTED-DTW.md.                                           



DEPENDS ON
  → ○ hearsay-four-b7p: A1: Acoustic gate — leave-one-take-out DTW front-end falsification (5pts) ● P2

BLOCKS
  ← ○ hearsay-four-p3o: A5: Lattice format + oracle WER + K sweep (5pts) ● P2
```

### A3 — hearsay-four-pri

```text
○ hearsay-four-pri · A3: Token identity — WORD_TOKEN/PHONE_TOKEN facts (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S6 A3. Fabric gains token facts distinct from type   
  facts. Type facts (hash-consed globally): WORD_TYPE, PRONUNCIATION, n-gram  
  entries, template ids, model parameters. Token facts (never merged across   
  time): WORD_TOKEN{word,start_frame,end_frame,source},                       
  PHONE_TOKEN{phone,start_frame,end_frame}, SIL_TOKEN{...}. Provenance        
  preserves order and multiplicity (replace BTreeSet premise collapse where   
  tokens are premises). Prerequisite consumed by R5 fabric layer, P2, P3.     



ACCEPTANCE CRITERIA

  Gate: test proves same word twice in one utterance -> two token facts,      
  distinct spans, independent retraction, distinct provenance chains; existing
  fabric suite green.                                                         



BLOCKS
  ← ○ hearsay-four-cjj: A6: Fabric A/B/C experiment — lifecycle vs beam pruning (8pts) ● P2
  ← ✓ hearsay-four-cri: R5: Phone-lexicon decoder — pronunciation trie, LM inside search (8pts) ● P2
  ← ○ hearsay-four-ofy: P2: Why-this-word view — per-word evidence rendering (5pts) ● P2
  ← ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
```

### A4a — hearsay-four-p7u

```text
○ hearsay-four-p7u · A4a: Fabric decision contract — written spec (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of A4 (hearsay-four-enu) per auditor pass 2026-10-27. Written spec    
  only: docs/design/fabric-decision-contract.md covering source classes       
  {ACOUSTIC,LEXICAL,SEQUENCE,SPEAKER,VERIFY,USER} with declared nulls and llr 
  definitions; tiling score S(T) vs liveness L(h) separation; LM accounting   
  rule; conflict (>50% shorter-span overlap); dormancy/revival via dormant-   
  predecessor index (J=3); commit (frontier e+H, margin delta, H_max);        
  deterministic ties. No code in this bead.                                   



ACCEPTANCE CRITERIA

  Gate: spec committed; frozen (content hash recorded in bead notes); A4b text
  references this spec version.                                               



BLOCKS
  ← ○ hearsay-four-dg8: A4b: Fabric decision contract — reference implementation + property tests (8pts) ● P2
```

### A4b — hearsay-four-dg8

```text
○ hearsay-four-dg8 · A4b: Fabric decision contract — reference implementation + property tests (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of A4 (hearsay-four-enu). Implement A4a spec in fabric: justified llr 
  weights per source class, tiling DP, dormant set with budgets,              
  revival/cascade via reverse index, defeat with provenance. Property tests:  
  LM-accounting byte-identity (LM weight 0 vs 1 -> identical published token  
  llrs); dormancy/revival/defeat scenario; conflict tiling; deterministic tie 
  order.                                                                      



ACCEPTANCE CRITERIA

  Gate: property suite green incl. LM-accounting byte-identity and            
  dormancy/revival/defeat tests; existing fabric suite green.                 



DEPENDS ON
  → ○ hearsay-four-p7u: A4a: Fabric decision contract — written spec (5pts) ● P2

BLOCKS
  ← ○ hearsay-four-f56: P3a: Correction facts + exact retraction (5pts) ● P2
  ← ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
  ← ○ hearsay-four-p3o: A5: Lattice format + oracle WER + K sweep (5pts) ● P2
  ← ○ hearsay-four-ofy: P2: Why-this-word view — per-word evidence rendering (5pts) ● P2
  ← ○ hearsay-four-cjj: A6: Fabric A/B/C experiment — lifecycle vs beam pruning (8pts) ● P2
```

### A5 — hearsay-four-p3o

```text
○ hearsay-four-p3o · A5: Lattice format + oracle WER + K sweep (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S6 A5. Versioned on-disk lattice format: timed tokens
  + per-class llr + arcs. Generated ONCE per acoustic configuration;          
  conditions A/B/C read the same file. K sweep K in {3,5,10,20} alternatives  
  per region: report lattice oracle WER (best achievable selecting from       
  lattice) vs K vs compute on DEV, first on A2 whole-word lattice. If oracle  
  gap insufficient, bottleneck is acoustic generation: fix A2/R5 before A6.   



ACCEPTANCE CRITERIA

  Gate: lattice format + writer/reader committed; oracle WER vs K table on DEV
  in docs/results/A5-LATTICE-ORACLE.md; chosen K has oracle WER >= 5 points   
  below condition-B WER, else bead records acoustic-bottleneck verdict and    
  blocks A6.                                                                  



DEPENDS ON
  → ○ hearsay-four-5n2: A2: Connected-word DTW decoder, token-passing (8pts) ● P2
  → ○ hearsay-four-dg8: A4b: Fabric decision contract — reference implementation + property tests (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
  ← ○ hearsay-four-cjj: A6: Fabric A/B/C experiment — lifecycle vs beam pruning (8pts) ● P2
```

### A6 — hearsay-four-cjj

```text
○ hearsay-four-cjj · A6: Fabric A/B/C experiment — lifecycle vs beam pruning (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S6 A6. Same lattice, evidence, weights w_c, H, delta 
  for B (conventional: below-threshold discarded forever) and C (Hearsay-IV:  
  dormant/revivable, cascade re-score); A = acoustic 1-best control. Identical
  score function and commit DP; only lifecycle differs. Budget frontier       
  {50,100,250,500} (B: live; C: live+dormant). Per point: WER, RTF, peak      
  live/dormant, working-set RAM, late-rescue count. Statistics per plan S4.3  
  (paired bootstrap CI 10k, improved/worsened/unchanged). Pre-registered      
  readings in plan S6 A6 govern claims. Run on A2 lattice first; rerun on R5  
  lattice at R5 gate. Requires A4 frozen. Split before claim if over 8pts.    



NOTES

  LOCKBOX OWNERSHIP (auditor pass 2026-10-27): A6 owns the M2 LOCKBOX run.    
  After A/B/C parameters freeze (weights, H, delta, budget set), open the M2  
  lockbox at most once, log the opening (date, commit hash, file checksums) in
  these notes, and report the architectural claim from that run per Master    
  Plan S9. Sealed data is not touched before the freeze.                      



ACCEPTANCE CRITERIA

  Gate: docs/results/A6-FABRIC-EXPERIMENT.md with per-budget table + CIs +    
  late-rescue counts; constructed dormancy/revival unit test green; claim     
  recorded matches the pre-registered reading (architectural / efficiency / no
  demonstrated difference).                                                   



DEPENDS ON
  → ✓ hearsay-four-enu: A4: Fabric decision contract — spec + reference implementation (8pts) ● P2
  → ○ hearsay-four-pri: A3: Token identity — WORD_TOKEN/PHONE_TOKEN facts (5pts) ● P2
  → ○ hearsay-four-p3o: A5: Lattice format + oracle WER + K sweep (5pts) ● P2
  → ○ hearsay-four-dg8: A4b: Fabric decision contract — reference implementation + property tests (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-cya: A7: Bound gate — long-input working-set independence (3pts) ● P2
  ← ○ hearsay-four-fi1: R5b: Phone-lattice A/B/C regression — A6 rerun on R5a lattice (3pts) ● P2
```

### A7 — hearsay-four-cya

```text
○ hearsay-four-cya · A7: Bound gate — long-input working-set independence (3pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S6 A7 + S4.4. On 30-min and 3-h inputs through the   
  served path: live/dormant hypothesis RAM independent of input duration;     
  committed transcript + provenance archive grows linearly with committed     
  words (never claim flat memory). Uses D1-approved long audio; RTF < 1       
  required.                                                                   



ACCEPTANCE CRITERIA

  Gate: working-set RAM within +/-10% between 30-min and 3-h runs; archive    
  growth linear (bytes/committed-word reported); RTF < 1; numbers in          
  docs/results/A7-BOUND-GATE.md.                                              



DEPENDS ON
  → ○ hearsay-four-cjj: A6: Fabric A/B/C experiment — lifecycle vs beam pruning (8pts) ● P2
```

### R1a — hearsay-four-baj

```text
○ hearsay-four-baj · R1a: LM core — ingestion, normalization, Kneser-Ney counts (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R1 (hearsay-four-9pa). Text ingestion for D0 use-verified sources 
  only (use-verification recorded in D0; LM text never contains DEV/PHASE-   
  TEST/LOCKBOX transcripts); explicit normalization rules (numbers,           
  punctuation, casing) with tests; trigram counts with modified Kneser-Ney    
  discounting stored inspectably.                                             



ACCEPTANCE CRITERIA

  Gate: hand-calculable golden corpus test asserts exact/epsilon probabilities
  for seen trigram, backed-off trigram, unseen word/history, normalization    
  sums to ~1; normalization rule tests green.                                 



DEPENDS ON
  → ○ hearsay-four-7ms: D1: Partitions + recording protocol (3pts) ● P1

BLOCKS
  ← ○ hearsay-four-2a0: R1b: LM interpolation, entropy pruning, persistence, perplexity gate (5pts) ● P2
```

### R1b — hearsay-four-2a0

```text
○ hearsay-four-2a0 · R1b: LM interpolation, entropy pruning, persistence, perplexity gate (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R1 (hearsay-four-9pa). Personal-LM interpolation with general LM   
  (weight tuned on DEV); entropy pruning to size budget; deterministic        
  serialization/reload; DEV perplexity per source mix.                        



ACCEPTANCE CRITERIA

  Gate: DEV perplexity per source mix in docs/results/R1-LANGUAGE-MODEL.md; LM
  loads < 2s; rebuild byte-identical; serialize/reload round-trip test green. 



DEPENDS ON
  → ○ hearsay-four-baj: R1a: LM core — ingestion, normalization, Kneser-Ney counts (5pts) ● P2

BLOCKS
  ← ○ hearsay-four-f56: P3a: Correction facts + exact retraction (5pts) ● P2
  ← ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
  ← ○ hearsay-four-c0r: R7: LVCSR evaluation — tiered owner-read gates (5pts) ● P2
```

### R2a — hearsay-four-tnk

```text
○ hearsay-four-tnk · R2a: HMM trainer core — flat start, forced alignment, Baum-Welch, serialization (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R2 (hearsay-four-3eh). 3-state left-to-right monophone HMMs,       
  diagonal Gaussians (1 mixture), SIL 3-state + SP 1-state tied to SIL centre;
  flat start from global mean/variance; forced state alignment from transcript
  + CMUdict (optional SP between words); Viterbi then Baum-Welch embedded re- 
  estimation (means, variances, transitions); variance floors. Model file     
  text-readable (or JSON) with lossless binary twin.                          



ACCEPTANCE CRITERIA

  Gate: synthetic fixture with known generating HMMs -> recovered parameters  
  within tolerance; model file round-trips lossless; trainer tests green.     



DEPENDS ON
  → ○ hearsay-four-7ms: D1: Partitions + recording protocol (3pts) ● P1

BLOCKS
  ← ○ hearsay-four-0bv: R2b: HMM checkpoint selection, mixture splitting, parameter provenance (5pts) ● P2
```

### R2b — hearsay-four-0bv

```text
○ hearsay-four-0bv · R2b: HMM checkpoint selection, mixture splitting, parameter provenance (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R2 (hearsay-four-3eh). Checkpoint each BW iteration; select best   
  DEV PER/WER checkpoint (likelihood diagnostic only; non-monotone DEV PER    
  expected, not failure). Mixture splitting 1->2->4->8 driven by DEV. Every   
  parameter stores sufficient statistics (count, sum-x, sum-x2) + provenance  
  pointer to source frames/utterances.                                        



ACCEPTANCE CRITERIA

  Gate: on real data selected checkpoint DEV PER < flat-start (iteration-0)   
  DEV PER with paired CI below 0; provenance pointer present for every        
  parameter (validator test); splitting decision recorded from DEV.           



DEPENDS ON
  → ○ hearsay-four-tnk: R2a: HMM trainer core — flat start, forced alignment, Baum-Welch, serialization (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-30y: R3: Base acoustic model, speaker-independent (5pts) ● P2
  ← ○ hearsay-four-jem: R4: Owner enrollment + MAP adaptation as retractable statistics (8pts) ● P2
```

### R3 — hearsay-four-30y

```text
○ hearsay-four-30y · R3: Base acoustic model, speaker-independent (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S7 R3 (optional, recommended). ONLY if D0 confirms   
  use-verified HUMAN read speech (e.g. LibriSpeech subsets, CC BY 4.0):       
  train SI base model with R2. Classical guidance: ~1h single-speaker         
  command/control, ~10h dictation; SI base + adaptation beats owner-only small
  model. If no external audio passes use-verification: close as skipped, R4 goes owner-only
  with higher recording target. No source used before D0 use-verification with
  licence + human/TTS status recorded (plan process rule 5).                  



ACCEPTANCE CRITERIA

  Gate (if D0 use-verifies external human audio): all expected phones        
  represented in trained model; model loads; held-out utterances produce      
  finite likelihoods; decoder smoke succeeds; SI DEV WER reported in          
  docs/results/R3-BASE-MODEL.md. If no external audio passes use-verification: close
  explicitly as SKIPPED — NO USE-VERIFIED EXTERNAL AUDIO / owner-only path, verification evidence
  recorded in bead notes — not as a pass.                                     



DEPENDS ON
  → ○ hearsay-four-0bv: R2b: HMM checkpoint selection, mixture splitting, parameter provenance (5pts) ● P2
  → ✓ hearsay-four-3eh: R2: HMM training subsystem — flat start to Baum-Welch (8pts) ● P2
  → ○ hearsay-four-0f6: D0: Data inventory — better-voice sets + accent_datasets (3pts) ● P1
```

### R4 — hearsay-four-jem

```text
○ hearsay-four-jem · R4: Owner enrollment + MAP adaptation as retractable statistics (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S7 R4. Recording protocol via record-session (same   
  mic class, quiet room, 16kHz mono, public-domain phonetically balanced      
  prompts: Harvard sentences then further PD prompts, session-tagged).        
  Targets: initial 1h falsification corpus, grow 5-10h as gates demand; owner-
  only path (no R3): 1h minimum before first gate. MAP adaptation via         
  retractable sufficient statistics per Gaussian per dimension (n, sum-x, sum-
  x2 occupancy-weighted; tau tuned on DEV). Stage 1 gate: means only,         
  mu=(tau*mu_base+sum-x)/(tau+n), variance stays base. Stage 2 optional:      
  variance from second moment, kept only if DEV WER improves (paired CI). Each
  contribution (count, sum-x, sum-x2, source frames, supporting hypothesis id)
  is a FABRIC FACT: retracting subtracts statistics and recomputes; base model
  immutable, adaptation is overlay. Human gate: owner recording time (1h+) is 
  on the owner, schedule explicitly.                                          



ACCEPTANCE CRITERIA

  Gate: owner DEV WER (adapted) < owner DEV WER (base) with paired CI         
  excluding 0; overlay keep/reject decided on DEV; PHASE-TEST reported once;  
  retraction-restores-base property test green.                               



DEPENDS ON
  → ○ hearsay-four-0bv: R2b: HMM checkpoint selection, mixture splitting, parameter provenance (5pts) ● P2
  → ✓ hearsay-four-3eh: R2: HMM training subsystem — flat start to Baum-Welch (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
  ← ✓ hearsay-four-xtd: P3: Correction loop — corrections as retractable fact sets (8pts) ● P2
  ← ○ hearsay-four-f56: P3a: Correction facts + exact retraction (5pts) ● P2
  ← ✓ hearsay-four-cri: R5: Phone-lexicon decoder — pronunciation trie, LM inside search (8pts) ● P2
```

### R5a — hearsay-four-71g

```text
○ hearsay-four-71g · R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R5 (hearsay-four-cri). Token passing over pronunciation trie of HMM
  states; state (time, trie_node, phone, phone_state, lm_history,             
  acoustic_score, path_score, backpointer); R1 LM inside search via trie-node 
  lookahead; emits A5-format word-token lattices into fabric (A3 tokens, A4b  
  contract); decisions by system C. Decoder only — A/B/C regression is R5b.   



ACCEPTANCE CRITERIA

  Gate: on 30-word REGRESSION phrases WER <= 1.5x A2 WER; RTF < 1; lattice    
  emission conforms to A5 format (reader round-trip test); decoder tests      
  green.                                                                      



DEPENDS ON
  → ○ hearsay-four-jem: R4: Owner enrollment + MAP adaptation as retractable statistics (8pts) ● P2
  → ○ hearsay-four-p3o: A5: Lattice format + oracle WER + K sweep (5pts) ● P2
  → ○ hearsay-four-2a0: R1b: LM interpolation, entropy pruning, persistence, perplexity gate (5pts) ● P2
  → ○ hearsay-four-dg8: A4b: Fabric decision contract — reference implementation + property tests (8pts) ● P2
  → ○ hearsay-four-pri: A3: Token identity — WORD_TOKEN/PHONE_TOKEN facts (5pts) ● P2

BLOCKS
  ← ○ hearsay-four-6gt: P3b: Correction changes decoding — integration proof (3pts) ● P2
  ← ○ hearsay-four-fi1: R5b: Phone-lattice A/B/C regression — A6 rerun on R5a lattice (3pts) ● P2
  ← ○ hearsay-four-6tc: P1: Streaming + VAD endpointing, one decoder for mic and file (5pts) ● P2
```

### R5b — hearsay-four-fi1

```text
○ hearsay-four-fi1 · R5b: Phone-lattice A/B/C regression — A6 rerun on R5a lattice (3pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R5 (hearsay-four-cri). Rerun the A6 protocol (identical            
  lattice/evidence/weights; B prune vs C dormant lifecycle) on the R5a phone  
  lattice per Master Plan S6 A6 (run first on A2 lattice, rerun on R5         
  lattice).                                                                   



ACCEPTANCE CRITERIA

  Gate: docs/results/R5-PHONE-DECODER.md contains A6-format per-budget table +
  CIs + late rescues on the phone lattice; claim matches pre-registered       
  reading.                                                                    



DEPENDS ON
  → ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
  → ○ hearsay-four-cjj: A6: Fabric A/B/C experiment — lifecycle vs beam pruning (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-3ay: R6: Triphones + decision-tree state tying (8pts) ● P2
```

### R6 — hearsay-four-3ay

```text
○ hearsay-four-3ay · R6: Triphones + decision-tree state tying (8pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S7 R6. Word-internal then cross-word triphones;      
  decision-tree state tying with HAND-WRITTEN phonetic questions (nasal? stop?
  front vowel? ...), split by likelihood gain with minimum occupancy; each    
  tied state traces to the questions that formed it. Mixture splitting on tied
  states. Split before claim if over 8pts.                                    



ACCEPTANCE CRITERIA

  Gate: owner DEV WER improves over best monophone system with paired CI      
  excluding 0; tied-state question traces inspectable; results in             
  docs/results/R6-TRIPHONES.md.                                               



DEPENDS ON
  → ○ hearsay-four-fi1: R5b: Phone-lattice A/B/C regression — A6 rerun on R5a lattice (3pts) ● P2
  → ✓ hearsay-four-cri: R5: Phone-lexicon decoder — pronunciation trie, LM inside search (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-c0r: R7: LVCSR evaluation — tiered owner-read gates (5pts) ● P2
```

### R7 — hearsay-four-c0r

```text
○ hearsay-four-c0r · R7: LVCSR evaluation — tiered owner-read gates (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S7 R7. Full CMUdict trie + R1 LM + best acoustic     
  system through system C. Comparisons on owner-read PHASE-TEST (new          
  sentences, never aligned): chunk baseline / phone decoder no LM / phone     
  decoder + n-gram (B) / phone decoder + fabric (C). Pre-registered tiers: T1 
  <= 50% WER genuinely transcribing; T2 <= 25% usable with corrections; T3 <= 
  12% daily-driver competitive. All tiers RTF < 1 live + A7 bounded working   
  set. Reported not gating: owner spontaneous 5-min hand-transcribed; Oyez    
  locked-eval cases.                                                          



ACCEPTANCE CRITERIA

  Gate: R7 closes as SUCCESSFUL only when at least T1 (<=50% owner-read PHASE-
  TEST WER) is achieved with RTF < 1 and A7 bounded working set; four-system  
  comparison (chunk / no-LM / +n-gram B / +fabric C) in docs/results/R7-LVCSR-
  EVALUATION.md with plan S4.3 statistics. If no tier is reached, results are 
  documented but the bead does NOT close as a transcriber pass — it records   
  the tier gap and files the blocking follow-up.                              



DEPENDS ON
  → ○ hearsay-four-2a0: R1b: LM interpolation, entropy pruning, persistence, perplexity gate (5pts) ● P2
  → ✓ hearsay-four-9pa: R1: Language model — trigram Kneser-Ney + personal LM (8pts) ● P2
  → ○ hearsay-four-3ay: R6: Triphones + decision-tree state tying (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-kt5: P5: Packaging + release — app, guard rename, reproducible model build (5pts) ● P2
  ← ✓ hearsay-four-7uv: R8: Open vocabulary — dynamic lexicon + OOV tokens (8pts) ● P2
  ← ○ hearsay-four-wya: R8a: Dynamic lexicon extension — spelling to inspectable G2P pronunciations (5pts) ● P2
```

### R8a — hearsay-four-wya

```text
○ hearsay-four-wya · R8a: Dynamic lexicon extension — spelling to inspectable G2P pronunciations (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R8 (hearsay-four-7uv), capability 1 of Master Plan S7 R8. User     
  supplies a new spelling -> rule-based inspectable G2P produces              
  pronunciation(s) -> word becomes recognizable. Each generated pronunciation 
  is a retractable fabric fact with provenance to the spelling submission.    



ACCEPTANCE CRITERIA

  Gate: deterministic regression — words deliberately removed from the lexicon
  are re-added by spelling and become valid decoder candidates via generated  
  pronunciations; retraction of a generated pronunciation removes candidacy;  
  tests green.                                                                



DEPENDS ON
  → ○ hearsay-four-c0r: R7: LVCSR evaluation — tiered owner-read gates (5pts) ● P2

BLOCKS
  ← ○ hearsay-four-5sw: R8b: Unknown-word OOV tokens + P2G spelling candidates (5pts) ● P2
```

### R8b — hearsay-four-5sw

```text
○ hearsay-four-5sw · R8b: Unknown-word OOV tokens + P2G spelling candidates (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of R8 (hearsay-four-7uv), capability 2 of Master Plan S7 R8. OOV token
  (phone-loop path with OOV cost) competes in the fabric; output [OOV: phones]
  + ranked candidate spellings from inspectable P2G; correct spelling NOT     
  promised (P2G ambiguous: night/knight/nite). Owns the M5 LOCKBOX opening    
  (Master Plan S9): OOV claims run on the sealed set at most once, opening    
  logged in bead notes. Threshold discipline: choose OOV precision/recall     
  threshold on DEV and pre-register it in bead notes BEFORE opening PHASE-    
  TEST/LOCKBOX; no number invented at filing time.                            



NOTES

  LOCKBOX OWNERSHIP (auditor pass 2026-10-27): R8b owns the M5 LOCKBOX opening
  for open-vocabulary/OOV claims (Master Plan S9), at most once, logged here  
  with checksums. Threshold chosen on DEV and pre-registered in these notes   
  before opening.                                                             



ACCEPTANCE CRITERIA

  Gate: OOV detection precision/recall on locked held-out-word set meets the  
  DEV-preregistered threshold recorded in bead notes before the run; M5       
  lockbox opening logged; spelling candidates reported (not gated).           



DEPENDS ON
  → ○ hearsay-four-wya: R8a: Dynamic lexicon extension — spelling to inspectable G2P pronunciations (5pts) ● P2
```

### P1 — hearsay-four-6tc

```text
○ hearsay-four-6tc · P1: Streaming + VAD endpointing, one decoder for mic and file (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S8 P1. Energy/spectral VAD with hangover for live    
  mic; endpoint-driven region closure; provisional (live/dormant) vs stable   
  (committed) text. File and mic paths share ONE decoder (existing live/batch 
  parity test extended). SEG-1 boundary detector (fse/src/boundaries.rs) is   
  available input; do not rebuild segmentation here. Consumes R5 decoder.     



NOTES

  MIGRATED from hearsay-four-6if (DIA-1, closed superseded by this bead):     
  boundary/silence fixture tests — speech-silence-speech fixture yields       
  segment boundaries within tolerance; all-silence and all-speech edge cases —
  belong to this bead's VAD/endpointing gate evidence.                        



ACCEPTANCE CRITERIA

  Gate: live/batch word parity on REGRESSION files; mic latency end-of-word to
  provisional text <= 500ms; RTF < 1; streaming suite green.                  



DEPENDS ON
  → ✓ hearsay-four-cri: R5: Phone-lexicon decoder — pronunciation trie, LM inside search (8pts) ● P2
  → ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-kt5: P5: Packaging + release — app, guard rename, reproducible model build (5pts) ● P2
```

### P2 — hearsay-four-ofy

```text
○ hearsay-four-ofy · P2: Why-this-word view — per-word evidence rendering (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S8 P2. Per committed word: acoustic llr and frames,  
  LM probability and history, speaker/adaptation contribution, alternatives   
  with scores, defeating tiling and margin, revival events, user facts        
  involved. Exportable as JSON with the transcript. Consumes A3 token facts + 
  A4 contract terms (scores shown are tiling-score components, never liveness 
  L). Existing WordRecord provenance is the substrate; extend, do not replace.



ACCEPTANCE CRITERIA

  Gate: every committed word in a REGRESSION run renders a complete chain;    
  provenance validator passes 100%; JSON export shape tested.                 



DEPENDS ON
  → ✓ hearsay-four-enu: A4: Fabric decision contract — spec + reference implementation (8pts) ● P2
  → ○ hearsay-four-pri: A3: Token identity — WORD_TOKEN/PHONE_TOKEN facts (5pts) ● P2
  → ○ hearsay-four-dg8: A4b: Fabric decision contract — reference implementation + property tests (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-kt5: P5: Packaging + release — app, guard rename, reproducible model build (5pts) ● P2
```

### P3a — hearsay-four-f56

```text
○ hearsay-four-f56 · P3a: Correction facts + exact retraction (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of P3 (hearsay-four-xtd). A user correction creates USER fact set with
  provenance to the correction: (1) USER justification on corrected token; (2)
  personal-LM count increments (R1b LM); (3) pronunciation-variant evidence if
  acoustics disagree with all lexicon pronunciations; (4) MAP sufficient-     
  statistic contributions from corrected frames (R4). Undo retracts the entire
  set exactly. Decode-effect proof is P3b, not here.                          



ACCEPTANCE CRITERIA

  Gate: property test — apply then undo N random corrections -> model state   
  byte-identical to before; each correction item carries provenance to its    
  correction event; fabric suite green.                                       



DEPENDS ON
  → ○ hearsay-four-jem: R4: Owner enrollment + MAP adaptation as retractable statistics (8pts) ● P2
  → ○ hearsay-four-2a0: R1b: LM interpolation, entropy pruning, persistence, perplexity gate (5pts) ● P2
  → ○ hearsay-four-dg8: A4b: Fabric decision contract — reference implementation + property tests (8pts) ● P2

BLOCKS
  ← ○ hearsay-four-6gt: P3b: Correction changes decoding — integration proof (3pts) ● P2
```

### P3b — hearsay-four-6gt

```text
○ hearsay-four-6gt · P3b: Correction changes decoding — integration proof (3pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Split of P3 (hearsay-four-xtd). End-to-end: audio decoded wrong (competitor 
  word wins), correction applied via P3a facts, same audio re-decoded and the 
  corrected word wins through the R5a decoder + A4b tiling, with no other     
  committed word changed on the regression fixture.                           



ACCEPTANCE CRITERIA

  Gate: integration test green — pre-correction decode != post-correction     
  decode exactly at the corrected span; REGRESSION fixture words outside the  
  span unchanged; result recorded in docs/results/P3-CORRECTION-LOOP.md.      



DEPENDS ON
  → ○ hearsay-four-71g: R5a: Phone-lexicon trie decoder — LM inside search, A5 lattice emission (8pts) ● P2
  → ○ hearsay-four-f56: P3a: Correction facts + exact retraction (5pts) ● P2

BLOCKS
  ← ○ hearsay-four-b6y: P4: Targeted-confusion benchmark — the fly proof (5pts) ● P2
```

### P4 — hearsay-four-b6y

```text
○ hearsay-four-b6y · P4: Targeted-confusion benchmark — the fly proof (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S8 P4. Owner lists real recurring errors (fly->family
  class). Per pair: >=20 DEV + >=20 PHASE-TEST owner sentences containing the 
  target in natural contexts, plus contexts where the competitor is correct.  
  Protocol: measure -> apply corrections from DEV only -> re-measure PHASE-   
  TEST target accuracy AND REGRESSION WER (collateral damage). Statistics per 
  plan S4.3 non-inferiority (CI upper bound <= +epsilon, epsilon=1.0 WER point
  default). Human gate: owner records the sentence sets.                      



ACCEPTANCE CRITERIA

  Gate: target-word accuracy improves (paired CI excludes 0); competitor-     
  correct sentences not worse AND REGRESSION WER not worse by S4.3 rule;      
  docs/results/P4-CONFUSION-BENCHMARK.md committed.                           



DEPENDS ON
  → ✓ hearsay-four-xtd: P3: Correction loop — corrections as retractable fact sets (8pts) ● P2
  → ○ hearsay-four-6gt: P3b: Correction changes decoding — integration proof (3pts) ● P2

BLOCKS
  ← ○ hearsay-four-kt5: P5: Packaging + release — app, guard rename, reproducible model build (5pts) ● P2
```

### P5 — hearsay-four-kt5

```text
○ hearsay-four-kt5 · P5: Packaging + release — app, guard rename, reproducible model build (5pts)   [● P2 · OPEN]
Owner: Michael A. Kuykendall · Type: task
Created: 2026-10-27 · Updated: 2026-10-27

DESCRIPTION

  Source: Master Plan v3 S8 P5 + S2 terminology. CLI + hearsay-serve +        
  desktop-runnable app (mic, file, export, why-view, corrections, enrollment  
  wizard running the R4 protocol); release matrix; reproducible model build   
  from recorded data with checksums. Terminology per plan S2: rename          
  scripts/no_model_guard.sh -> no_neural_guard.sh (keep checks); say non-     
  neural fully inspectable classical models, never model-free / no training   
  corpus, in served-path docs. Placeholder rule: no PLACEHOLDER_-prefixed     
  component on a served path at a milestone (milestone blocked).              



NOTES

  LOCKBOX OWNERSHIP (auditor pass 2026-10-27): P5 owns the M4 LOCKBOX run     
  after R7 + P1-P4 are all green; log the opening here. MIGRATED from hearsay-
  four-0d5 (APP-1, closed superseded): scripted assembly regression — fixture 
  file in -> transcript + TXT + SRT + VTT exports + valid provenance, exit 0 —
  is part of this bead's gate evidence. MIGRATED from hearsay-four-say (closed
  superseded): release/cross-platform smoke evidence in scripts/release.sh +  
  docs/RELEASE.md must be re-verified at release time, not inherited.         



ACCEPTANCE CRITERIA

  Gate: release build green on matrix; no_neural_guard.sh green; app scripted 
  fixture run exit 0 (mic-or-file -> exports + why-view JSON); model build    
  reproducible (identical checksum on rebuild).                               



DEPENDS ON
  → ○ hearsay-four-c0r: R7: LVCSR evaluation — tiered owner-read gates (5pts) ● P2
  → ○ hearsay-four-b6y: P4: Targeted-confusion benchmark — the fly proof (5pts) ● P2
  → ○ hearsay-four-ofy: P2: Why-this-word view — per-word evidence rendering (5pts) ● P2
  → ○ hearsay-four-6tc: P1: Streaming + VAD endpointing, one decoder for mic and file (5pts) ● P2
```

