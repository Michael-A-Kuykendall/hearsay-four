# Hearsay-IV Research Structure — Complete with Reference Access

## Directory Layout
```
hearsay-four/
├── HEARSAY-IV-RESEARCH-SYNTHESIS.md    # Main thesis document (393 lines)
├── agents.md                           # Agent interaction guidelines (83 lines)
├── RULE-SYSTEM-CONVERSION-INVENTORY.md # Rule-system conversion inventory (296 lines)
├── HEARSAY-IV-REFERENCE-ACCESS.md      # Verified retrieval links (111 lines)
├── research/                           # Organized research artifacts
│   ├── README.md                       # Structure overview
│   └── conversion-inventory/
│       ├── framework.md                # Bucket classification framework
│       ├── mappings.md                 # Mappings to Hearsay-IV stages
│       └── findings.md                 # Cross-cutting findings
└── src/                                # Source code placeholder
```

## Verified Bibliographic Access (from HEARSAY-IV-REFERENCE-ACCESS.md)

### Tier 1 — Most Critical (immediate prototyping)

| # | Citation | Status | Link |
|---|---|---|---|
| **1** | Erman et al. (1980) — Hearsay-II | **FREE** | https://mas.cs.umass.edu/Documents/Erman_Hearsay80.pdf |
| **2** | Rabiner (1989) — HMM Tutorial | **FREE** | https://www.cs.sjsu.edu/~stamp/Rua/Rabiner.pdf |
| **3** | de Kleer (1986) — Assumption-Based TMS | **FREE (scanned)** | https://dekleer.org/Publications/An%20Assumption-Based%20TMS.pdf |
| **4** | Chomsky & Halle (1968) — SPOE | **BOOK** (no free PDF) | No legitimate free copy; library/ILS required. Kaplan & Kay 1994 is the practical substitute. |

### Tier 2 — Hearsay Lineage

| # | Citation | Status | Link |
|---|---|---|---|
| **5** | Lesser et al. — Hearsay-II (1974/1975) | **PAYWALLED** | IEEE Xplore via DOI; journal version ASSP-23(1), 11–24 (1975) usually cited |
| **6** | Erman & Lesser (1975) — IJCAI | **FREE** | https://www.ijcai.org/Proceedings/75/Papers/072.pdf |
| **7** | Balzer et al. (1980) — Hearsay-III | **FREE** | https://aaai.org/papers/00108-AAAI80-032-hearsay-iii-a-domain-independent-framework-for-expert-systems/ |
| **8** | Erman, London, Fickas (1981) — Hearsay-III | **FREE** | https://www.ijcai.org/Proceedings/81-1/Papers/075.pdf |

### Tier 3 — Classical Phonetics, Statistics, Rejected

| # | Citation | Status | Link |
|---|---|---|---|
| **9** | Davis et al. (1952) — "Audrey" | **PAYWALLED** | JASA/AIP via DOI; oldest/digitized item; library request likely fastest |
| **10** | Kaplan & Kay (1994) — FST Phonological Rules | **FREE** | https://aclanthology.org/J94-3001.pdf |
| **11** | Reynolds & Rose (1995) — GMM Speaker ID | **PAYWALLED** | IEEE Xplore via DOI |
| **12** | Richardson & Domingos (2006) — MLNs | **FREE (author-hosted)** | https://homes.cs.washington.edu/~pedrod/papers/mlj05.pdf |
| **13** | De Raedt et al. (2007) — ProbLog | **FREE** | https://www.ijcai.org/Proceedings/07/Papers/396.pdf |

### Summary Statistics
- **8 of 13 confirmed free and directly downloadable**
- **4 paywalled** (Lesser/Fennell/Erman/Reddy, Davis/Biddulph/Balashek, Reynolds & Rose) — standard IEEE/JASA journal access from this era
- **1 book** (Chomsky & Halle) — not a retrieval problem; Kaplan & Kay 1994 is the practical substitute for direct implementation

## Critical References for Immediate Prototyping (the "essential four")

1. **Erman et al. (1980)** — ACM Computing Surveys: confidence-combination and evidence-rating formulas, direct port for Stage 3
2. **Rabiner (1989)** — Viterbi/N-best lattice pruning: adopted directly for Stage 3 bounded commit window
3. **de Kleer (1986)** — Justification-set structure: underlies weighted hypothesis survival; multi-support survival semantics
4. **Kaplan & Kay (1994)** — FST cascades: the computational, formalized phonological rule framework built for machine implementation (closer to usable than Chomsky & Halle)

## Agent Interaction Conventions (from agents.md)

- Read `HEARSAY-IV-RESEARCH-SYNTHESIS.md` first — it contains the complete thesis, architecture, and research questions
- All citations are hypotheses pending the experiment in §6 — treat claims outside §2 and §7 as unproven
- Stage 5 is deliberately flat — do not attempt to add neural language model disambiguation
- Commit-window width is empirical (§4.4) — parameter to be swept against prototype error curve
- Long-session provenance growth is unmeasured — use `provenance_stats()` from fabric
- The narrow buyer: forensic and legal transcript review, not consumer dictation

## Conversion Inventory Framework (from RULE-SYSTEM-CONVERSION-INVENTORY.md)

Four buckets for porting rules into Saturation Fabric:
- **A — Direct port**: Already fact-shaped; rehost with no logic change (low cost, low risk)
- **B — Port + augment**: Core logic rule-shaped, lacks retraction/provenance (medium cost, low-medium risk)
- **C — True inversion**: Imperative control flow; must re-express as assertable fact + derivation rule (medium-high cost, high risk)
- **D — Feeds facts, stays external**: Ordinary computation; should stay outside fabric (low cost, low risk)

Key finding: Category C concentrates in exactly one place: destructive overwrite. Every true inversion is a case where the original algorithm *throws something away on purpose* for efficiency. This is a small, recognizable pattern, not an open-ended unknown difficulty.

The conversion inventory across nine unrelated fields all sort cleanly into the same four buckets, providing real evidence the conversion approach generalizes — not just that it worked once on a domain already being thought about for weeks.