# Phoneme / Feature Alphabet — Design Specification

Author: Michael A. Kuykendall
Status: Resolved (bead hearsay-four-aem.10)
Date: 2026-08-15
Relates to: Stage 1 FSE (d5n), Stage 2 arena (a6d), Stage 3 phonological
rules (d73), Stage 4 lexicon (mwa)

---

## 1. Purpose

Pins the concrete phoneme set and acoustic feature vector for Stage 1
acoustic scoring and Stage 2 arena ingestion. The synthesis (§4.1–4.2)
references "formant/LPC/cepstral" features and a phoneme set without
enumerating them. This is required before Stage 1 scoring, Stage 2
hash-consing, Stage 3 phonological rules, and Stage 4 lexicon are
implementation-ready.

---

## 2. Phoneme alphabet

Use **ARPABET** (the phoneme set underlying CMUdict), which is the natural
choice because:
- Stage 4's lexicon is CMUdict-shaped (aem.14), and CMUdict is keyed on
  ARPABET.
- ARPABET is ASCII, which simplifies hash-consing and the lexical/sequence
  tables.

The alphabet is the standard 39-phoneme ARPABET set (39 distinct phones,
reduced from 40 by folding the 2–3 allophones of "ax" and using the common
39-phone convention). This is the same set used by the Kaldi/CMU ecosystem.

```
Vowels:     AA  AE  AH  AO  AW  AY  EH  ER  EY  IH  IY  OW  OY  UH  UW
Consonants: B  CH  D  DH  F  G  HH  JH  K  L  M  N  NG  P  R  S  SH  T  TH  V  W  Y  Z  ZH
```

(24 consonants + 15 vowels = 39 phones.)

---

## 3. Acoustic feature vector (per frame)

Stage 1 scores a windowed frame. The feature vector is **12 MFCC + energy +
Δ + ΔΔ** — the standard front-end representation, classical and
well-supported:

```
frame_features = [ c1..c12,        // 12 MFCC coefficients
                   e,              // log energy
                   Δ(c1..c12),     // 12 delta
                   Δ(e),           // delta energy
                   ΔΔ(c1..c12),    // 12 delta-delta
                   ΔΔ(e) ]         // delta-delta energy
                   = 39 features per frame
```

Rationale:
- **MFCC** (Mel-frequency cepstral coefficients) compress the spectrum into a
  compact, near-orthogonal vector — better matched to the classical GMM
  baseline (Reynolds & Rose) than raw formants/LPC for scoring.
- The synthesis mentions formant/LPC; those are *complementary* to MFCC and
  used in the accent-signature derivation (Stage 10), not the frame scorer.
  Keeping the frame scorer on MFCC separates concerns.

Frame rate: **10 ms frame shift, 25 ms window** (standard). Window: Hamming.

---

## 4. Feature → phoneme candidate mapping (Stage 1 output)

Stage 1 emits, per frame, N-best *phoneme candidates* (per aem.4). Each
candidate is `(phoneme: ARPABET, score: f32)` scored against the Stage 0
baseline. The candidates become the discrete dispatch keys (aem.17) and the
hash-consed arena facts (aem.5).

---

## 5. Invariants (for the PPT/C-Test system)

1. `INV: alphabet_fixed` — the phoneme alphabet is the fixed 39-phone
   ARPABET set; no phoneme outside it is emitted by Stage 1.
2. `INV: feature_vector_length` — every frame feature vector is exactly
   39 components, finite, and normalized (no NaN/inf).
3. `INV: phoneme_in_alphabet` — every candidate emitted references a phoneme
   in the alphabet.
4. `INV: score_in_range` — candidate scores ∈ [0,1].

These become contract-test invariants for Stage 1 (task d5n.3) and Stage 2
(task a6d.3).

---

## 6. Deferred (not unresolved)

- Whether the GMM baseline (Stage 0) models 39-dim or full 39-feature vectors
  is a Stage 0 detail (6lx.2); the alphabet here defines the *feature
  space*, which both share.
- The accent-signature formant measurement (Stage 10) uses formant/LPC
  trajectories, a separate feature set defined at Stage 10 (aem.6).
