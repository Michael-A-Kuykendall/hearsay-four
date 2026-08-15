# Mappings to Hearsay-IV Stages

| Inventory Item | Stage | Bucket | Notes |
|---|---|---|---|
| **2.7 YFilter/XPush/Aho-Corasick** | Stage 1 (FSE) / Stage 2 (Arena) | A | Already the FSE half of architecture; forward-only stream processors. Conversion means recognizing this is already the FSE layer, not a fabric candidate. |
| **2.2 SPE phonological rules (Chomsky & Halle)** | Stage 3 (Local commit window) | B | Rules already context-sensitive rewrites `A → B / X_Y`. Porting: keep rule as assert new fact justified by original phoneme fact + context, don't overwrite. |
| **2.3 Kaplan & Kay FST cascades** | Stage 3 / Stage 4 (Lexicon) | B | Transition function `(state, symbol) → (state', output)` is already rule-shaped. Same pattern: keep transition logic, add provenance as wrapper. |
| **2.4 Viterbi / HMM decoding** | Stage 3 (Local commit window) | C | Hardest inversion. Viterbi overwrites suboptimal paths per time step — opposite of fabric's keep-competing-alternatives. Resolution: bounded local N-best window (§4.4 of Hearsay-IV), keep N-best as justification sets within window, Viterbi-style discard once window closes. |
| **2.5 GMM speaker calibration** | Stage 0 (Calibration) | D | EM iterative numerical computation. Should stay outside fabric; output (per-speaker baseline) enters as single derived fact. This is the reference "don't invert this" example. |
| **2.6 Hearsay-II/III blackboard** | N/A (already resolved in Hearsay-IV) | split B/C/discarded | Knowledge-source structure = B; hypothesis rejection = C (retraction with survival); control blackboard = structurally replaced by alpha-indexed dispatch (deletion, not conversion). |
| **2.8 JTMS / ATMS (de Kleer)** | Fabric's own justification model | Not a conversion target | This is the source the fabric's own justification model was drawn from. Nothing gets "converted." |
| **2.9 Soufflé provenance / egg** | Output format target | B | Proof lattice/equality-saturation proof by reconstruction at query time. Porting: keep their proof-tree format as target output, source from live justification sets rather than reconstructing on demand. |
| **Buchberger's algorithm** | Provenance/stability | C | `continue` leaves nothing behind. Reify elimination as `CriterionElimination(i, j, kind)` fact asserted by rule instead of control-flow branch. |