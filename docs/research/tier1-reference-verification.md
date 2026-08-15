# Tier-1 Reference Access Verification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-cau.1)
Date: 2026-08-15

All four Tier-1 references were verified retrievable this session (HTTP 200,
`application/pdf`):

| # | Reference | URL | Verified |
|---|---|---|---|
| 1 | Erman, Hayes-Roth, Lesser, Reddy (1980) — Hearsay-II | https://mas.cs.umass.edu/Documents/Erman_Hearsay80.pdf | ✓ 200 PDF |
| 2 | Rabiner (1989) — HMM Tutorial | https://www.cs.sjsu.edu/~stamp/RUA/Rabiner.pdf | ✓ 200 PDF |
| 3 | de Kleer (1986) — Assumption-Based TMS | https://dekleer.org/Publications/An%20Assumption-Based%20TMS.pdf | ✓ 200 PDF |
| 4 | Kaplan & Kay (1994) — FST Phonological Rules | https://aclanthology.org/J94-3001.pdf | ✓ 200 PDF |

## Notes

- These are **remote-verified retrievable**; the PDFs are not vendored into the
  repo (large binaries, kept on their host sites). Download on demand via the
  URLs above.
- **Chomsky & Halle (1968)** is a book (no legitimate free PDF); Kaplan & Kay
  (1994) is the practical substitute for the phonological-rule formalism, as
  documented in `HEARSAY-IV-REFERENCE-ACCESS.md`.
- These four gate the design beads that extract from them: Hearsay-II
  confidence formulas (aem.7), phonological formalism (aem.8), phoneme
  alphabet (aem.10), calibration utterance (aem.11).
