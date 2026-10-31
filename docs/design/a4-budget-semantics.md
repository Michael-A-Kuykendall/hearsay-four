# A4 budget semantics freeze (spec for the dormant-tiling repair)

Sources: `docs/design/fabric-decision-contract.md` v1.0 (SHA-256
`4b7c4e7b8a603ff6ae33127298fe8687049ecf105dbb99532b6501a80b8f5521`
per `hearsay-four-dg8` notes; file verified byte-identical 2026-10-28),
`hearsay-four-cjj--fabric-pruning-experiment` (arm parameterization),
`docs/audit/CLOUD-AUDIT-2026-10-28-A1-ACOUSTIC.md` (A4 findings),
`docs/audit/CLOUD-REJOINDER-2026-10-28-A1-ACOUSTIC.md` point 2
(either/or enjoined). Code refs: `src/fabric/src/decision.rs`.

Decision: normative-live-budget

## R1 — Tiling eligibility requires liveness

`Decision::best_tilings` (`decision.rs:705-765`) changes its candidate
filter from `fabric.is_active` to `Decision::is_live` (`decision.rs:511`).
`Token::tiling_eligible` (`decision.rs:90-92`) checks kind only
(Word|Sil), so no other layer excludes dormant — the filter change is
the whole fix.

Textual basis (recorded precisely): contract §8 says "Live tokens may
be extended (streaming), join tilings, and count against the live
budget" — RULED as an exclusive reading: ONLY live tokens join tilings.
Gap admitted: v1.0's explicit exclusion wording covers extension
("excluded from extension"), not tilings; the sentence "excluded from
tilings" appears zero times in v1.0 (verified by search). The exclusive
reading is forced by: (i) A6 validity — dormant winners without revival
would manufacture the measured effect and void the late-rescue metric;
(ii) the reef contract test intent (revive-then-win,
`src/fabric/tests/decision_contract.rs` ~87-121); (iii) no other
exclusion layer exists.

Contract amendment 1.1 (applied by the implementation bead, per the
contract's own §14 rule: edit, bump header version, re-record hash):
insert immediately after the Dormant bullet's last line
(`  removed, and §9 can raise `L(h)` again.`) the line:

Amendment 1.1 (section 8): dormant tokens are excluded from tilings; only live tokens join tilings.

and add a header version line immediately after the document title line:

Spec version: 1.1 (amends 1.0: section 8 tiling exclusion; see docs/design/a4-budget-semantics.md)

(v1.0 has no header version line — it was identified by hash citation in
`hearsay-four-dg8` notes — so 1.1 introduces the declaration.) Re-record
the file SHA-256 in the implementation bead's notes as
`contract-sha256: <64 hex>`.

## R2 — The live budget is normative

Rule: while `live_tokens().len() > params.live_budget`, evict the
lowest-`L(h)` live token, ties by the §10 tie order (`token_tie_cmp` —
the same deterministic order as dormant eviction). Eviction is a
deliberate retraction (`fabric.retract`) plus a provenance event:
`DecisionEvent::Evicted` MUST record which budget triggered (Live,
Dormant, or Combined — exact Rust shape at implementation discretion,
recorded in implementation notes; the distinction must be queryable).

Enforcement order inside `enforce_budgets`: (1) live cap, then
(2) dormant cap + combined cap (existing loop, unchanged). Totality:
combined-exceeded with live within cap implies a dormant victim exists
(combined = live + dormant); the B arm (no dormant tokens, R3) with
live within cap trivially satisfies step 2.

Frozen arm parameterization (T in {50, 100, 250, 500} per the cjj
frontier): B: live_budget = T, dormant_budget = 0, combined_budget =
None. C: live_budget = T, dormant_budget = T, combined_budget = Some(T).
Arm-SYMMETRIC (identical rule and value): live-cap eviction,
tiling/commit/scoring/evidence/weights. Arm-ASYMMETRIC (the treatment,
and the ONLY difference): below-threshold handling (B discards forever
per R3; C retains dormant) plus the dormant/combined caps. B's beam
identity is threshold-discard plus live-cap eviction; C adds dormant
retention and revival within the same total T. The live-cap eviction is
background pressure, identical in both arms — it is not part of the
treatment difference.

Rejected: remove-live-budget. Without a live cap, B's working set is
unbounded (threshold-only pruning), which breaks same-total fairness
(C capped at T, B uncapped) and voids the {50..500} frontier for B. The
alternatives (adaptive threshold, measured-not-enforced budgets) are
redesigns, not freezes.

## R3 — B below-threshold tokens are never retained

Observable rule (mechanism at implementation discretion): no dormant
tokens exist in B-arm runs. Below-threshold tokens are discarded at or
before admission; `dormant_budget = 0` is a backstop, not the mechanism.

## Contract-tests:

- `dormant_token_excluded_from_tilings_before_revival` in
  `src/fabric/tests/decision_contract.rs`: mint token, keep dormant (no
  revival), call `best_tilings` over its region, assert the dormant token
  appears in NEITHER winner nor runner-up.
- `live_budget_constrains_candidate_pool` in
  `src/fabric/tests/decision_contract.rs`: over-budget live pool is
  deterministically pruned per R2 (lowest-L(h) first, §10 ties); assert
  pool size equals `live_budget`, victim identity, and `Evicted`
  budget attribution.

## Citations

Contract §8 liveness/budget bullets (~169-183); §11 arm table (~259-261:
"Budget counted | live | live | live + dormant"); §12 budget row (~281);
§14 amendment clause (~309-316); cjj description ("Budget frontier
{50,100,250,500} (B: live; C: live+dormant)"); `decision.rs`: 90-92
(`tiling_eligible`), 123/148 (`live_budget`), 511 (`is_live`), 516-518
(`is_dormant`), 520-534 (token lists), 546-571 (`enforce_budgets`),
705-765 (`best_tilings`).
