# Hearsay-IV Research Structure

## Directory Layout

```
hearsay-four/
├── HEARSAY-IV-RESEARCH-SYNTHESIS.md    # Main thesis document (primary research)
├── agents.md                            # Agent interaction guidelines (this file)
├── research/                            # Organized research artifacts
│   ├── prior-art/                       # Cited references and bibliographic notes
│   ├── experiments/                     # Isolated experiment scripts (§6 fail-fast)
│   ├── provenance/                      # Provenance tracking implementations
│   └── calibration/                     # Stage 0 calibration scripts
├── src/                                 # Source code (prototype implementation)
│   ├── stage-0-calibration/
│   ├── stage-1-fse/
│   ├── stage-2-arena/
│   ├── stage-3-commit-window/
│   ├── stage-4-lexicon/
│   └── stage-5-sequence/
├── docs/                                # Supplementary documentation
│   ├── architecture-overview.md
│   ├── experiment-results/
│   └── test-data/
└── tests/                               # Test suites (when prototype exists)
```

## Research Phases (from the synthesis)

### Phase 1: Thesis & Prior Art (complete)
- Documented in HEARSAY-IV-RESEARCH-SYNTHESIS.md
- 13 key bibliographic references cataloged in agents.md
- Core thesis: replacement of trained models with derivation-based fact engine

### Phase 2: Isolated Prototype (§6 experiment)
- **Commit-window width sweep** against error curve
- **Accent signature carry-forward** test (one speaker, ~30 words, ~20 min session)
- Scripts go in `research/experiments/`

### Phase 3: Pipeline Integration
- Stages 1-5 composable implementation
- Multi-support justification sets (shared with dzero/MikeDB)
- Alpha-indexed dispatch integration

### Phase 4: Forensic Validation
- Error rate comparison vs. stateless model (A vs. B conditions)
- Provenance chain inspection usability study

## Key Dependencies (already implemented elsewhere in portfolio)

- **Multi-support justification sets** — required in dzero and MikeDB fact stores
- **Alpha-indexed dispatch** — replaces conventional query planner in MikeDB
- **FSE / fabric split** — FSE for stateless single-pass scoring; fabric for chaining/retraction

## Critical Unresolved Questions (from §5)

1. **Commit-window width** — empirical dial; too narrow = brittleness; too wide = combinatorial load
2. **Long-session memory growth** — unmeasured; `provenance_stats()` from fabric is the instrument
3. **Amalgamation greater than sum of parts** — untested; each component has independent precedent

## Agent Onboarding

New agents should:

1. Read `HEARSAY-IV-RESEARCH-SYNTHESIS.md` in full
2. Review `agents.md` for interaction conventions
3. Check `research/prior-art/` for bibliographic organization
4. Start with the §6 fail-fast experiment (cheapest path to validation)
5. Do not add neural/LM components — Stage 5 is deliberately flat by design