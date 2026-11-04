# Hearsay-IV Speech Corpus — Local Asset Manifest
### Reference inventory of pre-existing audio/transcript data in `better-voice`

Status: Requirements-gathering reference — CORPUS DATA IS NOT MOVED, only
referenced. All paths below are relative to the better-voice root
(`$BETTER_VOICE_ROOT/`).

Generated: 2026-08-15 (inventory spike). These are the pre-existing,
free, already-on-disk assets that can seed the Hearsay-IV fail-fast
experiment (§6) and downstream validation without any new downloads.

---

## 1. Phrase training set (the primary local audio+transcript source)

**Path:** `data/phrase_training/`
**Contents:**
- `audio/phrase_000000.wav .. phrase_003499.wav` — **3500 files, ~656 MB**
- `train_phrases.tsv` — 3500 lines, columns `audio_path<TAB>text`
- `phrase_stats.json` — totals
- `phrase_manifest.sha256` — integrity

**Verified format:** mono 16-bit PCM WAV, **16 kHz**, ~4.03 s per clip at
runtime, 3–7 words per phrase.

**Stats (from `phrase_stats.json`):**
- total_phrases: 3500
- total_duration: 20492 s ≈ **5.7 hours**
- avg_duration: 5.85 s
- length distribution: 3-word=714, 4-word=739, 5-word=658, 6-word=706, 7-word=683

**Use:** best-fit for the §6 fail-fast: single-speaker-style, word-length
utterances with clean transcripts, already wave/PCM — directly readable by
the Rust `hound` crate (source-pure), no decode step.

---

## 2. Bulk TTS set (word-level, clean)

**Path:** `data/bulk_tts_1k/`
- `train_1k.tsv` + `val_1k.tsv` — 1002 `audio_path<TAB>text<TAB>duration`
- `data/bulk_tts_1k/*.wav` — 1004 files, ~67 MB

**Verified format:** mono 16-bit PCM WAV. Examples: `o'flaherty(1)`, `mart's`.

**Use:** word-level closed vocabulary for calibration/enrollment; clean
read-speech.

---

## 3. Quality-checked legal vocabulary (1000, marked)

**Path:** `data/quality_checked_1k/`
- `train.tsv` (900) + `val.tsv` (100) — columns `audio_path<TAB>duration<TAB>text`
- `generation_report.json` — quality: EXCELLENT 970 / GOOD 30 (1000 generated,
  0 failed, 1.0 success rate)
- 1032 files in dir (~16 MB); the extra 32 are the TSV/json/report metadata

**IMPORTANT FORMAT CAVEAT:** the `.wav` extension is a misnomer — `file`
identifies them as **MPEG ADTS, layer III, 64 kbps, 24 kHz, monaural** (not
RIFF/WAVE). They must be decoded (MP3→PCM) before the `hound` reader can
touch them. This is a fixable pipeline detail but must be handled.

**Use:** hand-vetted single legal-vocab words; accent/legal-vocab coverage.

---

## 4. Accent / dialect variation datasets

**Paths & purities** (`data/accent_datasets/`):
- `common_voice/`, `common_voice_actual/` — Common Voice-derived, accented TSV
  (format: `audio_path text duration accent speaker_id source`)
- `librispeech_accents/` — LibriSpeech with accent labels
- `english_dialects/`, `english_dialects_actual/`, `english_dialects_fixed/`,
  `english_dialects_simulation/` — production dialect simulation (each has a
  TSV + `download_ed.py` / `download_script.py`)

**Use:** the accent/dialect variation Hearsay-IV's `Stage 10` accent-signature
feedback loop (aem.6) is designed around. Provides ACCENT-labeled speech for
the drift/calibration storyline.

---

## 5. Manifests / training corpus side

- `data/combined_training_dataset/` — 8 files, train+validation split TSV
- `data/libri_mini_spaced/` — 40 FLACs, spaced transcripts
- `artifacts/master_phonetics.tsv` + `seeds/*.tsv` — phonetics lexicon
- `data/corpus_stats/*.json` — domain frequency / exclusivity / tier stats
- `data/phonetics/*` — phonetics seed/extended

---

## 6. What is NOT here (gap)

- No full-length continuous court audio in better-voice — the existing files
  are **word/phrase-clip level** (0.8–6 s). For a true multi-utterance
  *conversational session with drift* (§6 experimental protocol) we need the
  long-form court audio from CourtListener/Oyez (see
  `court-audio-transcript-sources.md`).
- The pre-existing data was generated for a **TTS/ASR vocabulary** purpose
  (single words, phrases), not for the forensic session-drift experiment.
  It is the right *calibration + closed-vocabulary* data, but the long-session
  drift data must come from the court audio sources.

---

## 7. Verdict for the fail-fast protocol

- **Vocabulary + calibration:** use `bulk_tts_1k` / `quality_checked_1k`
  (PCM + MP3-decoded) — clean coverage.
- **Closed 30-word set:** subset of `phrase_training` transcripts (or the
  bulk_tts words) — hand-instrumentable. The Hearsay-IV fail-fast vocabulary
  (30 words) is enumerated as `fse::CLOSED_VOCABULARY`
  (`src/fse/src/lib.rs`, 39c.1): the top-30 genuine words by frequency across
  all 3500 `phrase_training` transcripts — deterministic and reproducible
  from `train_phrases.tsv`.
- **Drift / long-session:** requires CourtListener or Oyez long-form audio +
  synced transcript (see `corpus/court-audio-transport-sources.md`).

---

## Checksums / integrity

`data/phrase_training/phrase_manifest.sha256` and `data/phrase_training/
phrase_stats.json` are present in-repo for verification. The audio payloads
are **not** copied into Hearsay-IV; all pipeline readers must reference the
paths above (pass them in via a manifest/config, do not vendor).