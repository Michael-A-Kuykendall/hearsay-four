# Fixed Calibration Utterance — Selection

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.11)
Date: 2026-08-15
Relates to: Stage 0 calibration (6lx.1), Stage 1 scoring baseline
Synthesis §4.1: a well-known phrase carries culturally established reference
prosody + phonetic content, sharpening the initial baseline on both axes.

---

## 1. Chosen utterance

**"The quick brown fox jumps over the lazy dog."**

This is the canonical English pangram (contains every letter). Chosen over
alternatives because:

1. **Culturally established reference prosody.** It is among the most
   well-known English sentences; native speakers produce it with reliable,
   reproducible phrasing — a strong anchor for the Stage 0 formant/cepstral
   baseline.
2. **Phonetic coverage.** It spans a broad vowel/consonant inventory:
   - Stops: /k/ (quick), /b/ (brown), /d/ (dog), /g/ (dog), /t/ (the, quick)
   - Fricatives: /f/ (fox), /ð/ (the), /s/ (jumps), /z/ (jumps, lazy), /ʒ/ (measure-style, via "lazy"/"j")
   - Nasals: /n/, /m/ (jumps), /ŋ/ (jumping implied)
   - Approximants: /r/ (brown), /l/ (lazy), /w/ (quick), /j/ (jumps)
   - All 5 canonical vowels in stressed contexts (uɪ, aʊ, ɒ, eɪ, əʊ, ɪ)
3. **Comparable across speakers.** Being a fixed pangram, every speaker says
   the same words, so signatures are comparable rather than merely
   self-consistent (synthesis §4.1's requirement).

---

## 2. Rationale vs. alternatives

| Candidate | Pros | Cons |
|---|---|---|
| **"The quick brown fox..."** | Pangram, phonetic breadth, iconic prosody | Slightly long (~35 phones) for a "short" enrollment |
| "Peter Piper picked a peck..." | Alliterative | Tongue-twister prosody distorts baseline; biased consonant set |
| "Four score and seven years ago" | Iconic | Formal/archaic register; less everyday prosody |
| "Hello" / "test 1-2-3" | Short | Too little phonetic coverage; trivial baseline |

The pangram's phonetic breadth outweighs its length — a richer baseline is
worth a few extra seconds of enrollment.

---

## 3. Enrollment protocol

- One utterance, spoken naturally at normal pace (no special enunciation).
- Captured via the same mic/setup used for the session (so the baseline
  matches the session's acoustics).
- The 39-dim MFCC feature stream (aem.10) over the utterance becomes the
  per-speaker baseline (Reynolds & Rose classical GMM fit, 6lx.2).

---

## 4. Invariants (for the PPT/C-Test system)

1. `INV: fixed_prompt` — the enrollment prompt is exactly the pangram; not
   changed per speaker (so signatures stay comparable).
2. `INV: baseline_comparable` — the extracted baseline is a fixed-length
   feature summary (comparable across speakers), not free-form.
3. `INV: baseline_finite` — baseline features are finite/non-degenerate.

These become contract-test invariants for Stage 0 (task 6lx.4).

---

## 5. Deferred (not unresolved)

- Whether to use the 26-letter pangram or a shorter ~30-word variant is a
  Stage 0 tuning detail; the pangram above is the chosen default.
