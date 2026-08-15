# Classification Framework

Your original split — straight comparison vs. inversion — is correct as a starting cut, but the actual porting work sorts into four buckets, not two. The difference matters because each bucket has a different cost and a different risk profile:

| Bucket | Definition | Cost | Risk |
|---|---|---|---|
| **A — Direct port** | Already fact-shaped: a condition, a state, a derived value read from and written to shared structure. Rehost with no logic change. | Low | Low — the hard thinking is already done by the original authors |
| **B — Port + augment** | The core logic is already rule-shaped, but lacks retraction, provenance, or justification tracking. Add the missing piece without touching the underlying rule logic. | Medium | Low-medium — augmentation is additive, unlikely to break the ported logic |
| **C — True inversion** | Currently expressed as imperative control flow — a decision embedded in a branch or loop, with a side effect that evaporates. Must be taken apart and re-expressed as an assertable fact plus a derivation rule. | Medium-high | This is where the real engineering risk concentrates |
| **D — Feeds facts, stays external** | The algorithm is legitimately ordinary computation — numerical iteration, signal processing — that should stay outside the fabric and simply hand its output in as a fact. Not every algorithm needs to *be* the fabric. | Low | Low, but easy to get wrong in the other direction — over-inverting internals that have no downstream reasoning consumer is pure overhead |

The fourth bucket matters as much as the third. The failure mode isn't only "this is harder to invert than I thought" — it's also "I inverted something that never needed inverting," which produces a slower system with no architectural benefit.