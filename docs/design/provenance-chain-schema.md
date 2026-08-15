# Provenance-Chain Output Schema — Design Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.16)
Date: 2026-08-15
Relates to: Output epic (nak), provenance chain task (nak.1), human
reviewer format (nak.2)

---

## 1. Purpose

The synthesis says the output is "transcript + full provenance chain to raw
acoustic evidence," but leaves the exact per-word provenance record
unspecified. This document pins the schema so Stage 11's output is
implementation-ready and a human reviewer can ask "why did the machine write
this word?" and get a complete, terminating answer.

---

## 2. Design principles

- **Every word maps to exactly one root provenance record** describing the
  word itself and the competition it won.
- **The chain must terminate at raw acoustic evidence** — no infinite or
  dangling references (invariant `provenance_terminates`).
- **Every winning word records the losing candidate + the deciding
  evidence** — the forensic property that is the entire point of the system
  (invariant `records_loser_and_evidence`).
- **The schema is a plain data structure** (serializable to JSON for the
  human reviewer), not a function.

---

## 3. The schema

### 3.1 `Transcript`
```
Transcript {
  words: [WordRecord],
  session: SessionMeta,      // speaker id, recording ref, timestamps
  conditions: String,        // "A" | "B" (fail-fast), else null
}
```

### 3.2 `WordRecord` — the per-word root
```
WordRecord {
  text: String,                    // the transcribed word
  start_ms: u64,                   // start of acoustic span
  end_ms: u64,                     // end of acoustic span
  confidence: f32,                 // [0,1] committed confidence
  winner: Hypothesis,              // the chosen word hypothesis
  losing: [Hypothesis],            // competing candidates that lost
  decided_by: Evidence,            // the deciding evidence
  provenance: ProvenanceChain,     // full derivation to acoustic evidence
}
```

### 3.3 `Hypothesis`
```
Hypothesis {
  text: String,                    // candidate word or phoneme string
  score: f32,                      // its confidence/competition score
  status: "committed" | "retracted" | "survived",
}
```

### 3.4 `Evidence`
```
Evidence {
  // the specific evidence that decided winner over loser
  kind: "lexical" | "phonological" | "sequence" | "acoustic" | "calibration",
  description: String,             // human-readable why
  strength: f32,                   // magnitude of the deciding evidence
}
```

### 3.5 `ProvenanceChain` — the derivation, layer by layer
```
ProvenanceChain {
  layers: [ChainLayer],            // acoustic -> phone -> syllable -> lexical -> sequence
}
ChainLayer {
  level: "frame" | "phoneme" | "syllable" | "lexical" | "sequence",
  nodes: [Node],
}
Node {
  id: NodeId,                      // hash-consed arena NodeId
  label: String,                   // e.g. "/t/", "SYL:ta", "WORD:table"
  support: [JustificationRef],     // justification sets that support it
  active: bool,                    // current survival status
}
JustificationRef {
  // reference to the arena justification set (survival semantics, aem.13)
  justification_id: String,
  premises: [NodeId],              // premise node ids
  unconditional: bool,             // unconditional vs defeasible
}
```

---

## 4. Termination guarantee

`provenance_terminates`: every `ChainLayer` at a lower level is grounded in
the layer below it, and the **frame** layer bottoms out at concrete acoustic
evidence (the windowed audio + extracted formant/LPC features for that
frame's time span). There is no cycle: frame → phoneme → syllable → lexical
→ sequence is strictly ascending, and each node references only strictly
lower levels in its support. This mirrors the synthesis §4.4 Risch-style
compositional argument: once a lower layer is committed, higher layers treat
it as given.

---

## 5. Human-reviewer rendering (nak.2)

The schema serializes to JSON for the reviewer. The rendering must let a
human ask, for any word: *"why this word and not that one?"* — which means
showing `winner`, `losing`, and `decided_by` on one screen, with the
`provenance` chain expandable down to the acoustic layer.

---

## 6. Invariants (for the PPT/C-Test system, epic vuo)

1. `INV: provenance_terminates` — every provenance chain reaches a frame
   node grounded in acoustic evidence; no cycles, no dangling refs.
2. `INV: records_loser_and_evidence` — every `WordRecord` has ≥1 losing
   hypothesis and a non-empty `decided_by`.
3. `INV: score_in_range` — all `score`/`confidence`/`strength` values ∈ [0,1]
   and finite (no NaN/inf).
4. `INV: chain_acyclic` — provenance layers strictly ascend; no node
   references itself or a higher layer.

These become the contract-test invariants for the provenance-chain output
(task nak.3).

---

## 7. Deferred (not unresolved)

- Exact JSON field ordering and casing for the reviewer UI is a rendering
  detail (nak.2), not a schema decision.
- Whether `survived` (retracted-but-alive) hypotheses render differently is a
  UI concern; the schema already distinguishes the status.
