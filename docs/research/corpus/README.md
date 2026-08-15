# Hearsay-IV Speech Corpus — README

## Purpose

Central requirements-gathering record for speech audio + transcript corpora.
Two complementary data pools:

1. **Local pre-existing clips** (already on disk) — `better-voice`
   (/home/michael/repos/better-voice). For calibration + closed-vocab +
   fail-fast vocabulary. **Referenced, not moved.**
   → see `local-bettervoice-inventory.md`

2. **New long-form court audio** (to acquire) — CourtListener (Free Law
   Project) + Oyez + SCOTUS.gov. For the multi-utterance, multi-speaker
   courtroom session **with drift** that the §6 fail-fast experiment and the
   provenance-to-acoustic-evidence chain (aem.16) need.
   → see `court-audio-transcript-sources.md`

---

## Why both pools

The Hearsay-IV fail-fast (§6) needs:
- a **closed ~30-word vocabulary** cleanly transcribed → pool 1 (better-voice)
- **a continuous session with real drift** (fatigue, mic distance, noise) →
  pool 2 (real court recordings from CourtListener) OR a synthetic drift
  session built by conditioning pool-1 clips.

The forensic/legal target use case (synthesis §3) is court-reporting
transcription; real oral-argument audio is the most on-target public data
available, and it is free.

---

## Decision framework (when pulling new data)

| Need | Source | Notes |
|---|---|---|
| Closed vocabulary, clean word clips | better-voice `bulk_tts_1k`/`quality_checked_1k` | PCM (16k) + MP3-formatted (24k) |
| Word/phrase transcripts 3–7 wds | better-voice `phrase_training` | 3500×, 16k PCM, ~5.7h total |
| SCOTUS audio + timed transcript (1955→2013) | **Oyez** | timestamp-aligned segments — gold |
| SCOTUS + federal circuit audio (2013→now) | **CourtListener** | enhanced 22.05k MP3s |
| Official SCOTUS audio + text | **SCOTUS.gov** | public domain |
| Accent/dialect variation | better-voice `accent_datasets/*` | Common Voice, LibriSpeech-labeled |

---

## Acquisition steps (CourtListener, needs a token)

Prerequisites: a free CourtListener account → API token
(https://www.courtlistener.com/api/rest/v4/audio/ requires
`Authorization: Token <token>`).

1. Fetch a cursor of audio objects:
   `GET https://www.courtlistener.com/api/rest/v4/audio/?limit=50&court__id=scotus`
   → capture `local_path_mp3` (enhanced) and `download_url` (original), plus
   `duration`, and join with `dockets/` for case name/court/date.
2. Download MP3s to `data/corpus/raw/<court>/<docket>/`.
3. Pair transcripts:
   - Oyez timed transcripts via api.oyez.org for SCOTUS;
   - SCOTUS.gov official transcripts for SCOTUS 2012+;
   - Court/arguments text where the court publishes it.
4. Write a per-case manifest (JSON) → `data/corpus/manifests/`:
   `{audio_path, transcript_path, duration_sec, court, docket, case_name,
     speakers,n}`.
5. Validate (decode OK; non-empty transcript; duration>0).

**Do not commit audio into git.** Commit manifests + URL/checksum fields.

---

## Repro steps for the spikes

```bash
# local pool inventory (better-voice)
bash docs/research/corpus/scripts/inventory_local.sh   # (TODO: add)

# court data acquisition (requires token; not run in this spike)
docs/research/corpus/scripts/fetch_courtlistener.py    # (TODO: implement)
```

---

## Status

- [x] Local pool inventoried (formats, counts, caveats) → `local-bettervoice-inventory.md`
- [x] Court sources researched (URLs, API fields, licensing) → `court-audio-transcript-sources.md`
- [x] CourtListener API token obtained + verified working (2026-08-15) — in gitignored `.env`
- [x] Audio API verified: STT transcripts ship inline (`stt_transcript`), SCOTUS filter + cursor pagination work
- [ ] Sample audio + transcript acquisition test (token ready; run the ingestor)
- [ ] `scripts/` ingestor written
- [ ] Fail-fast protocol mapping (conditions A/B) to chosen corpus (beads 39c.*)

## Credential handling (IMPORTANT)

This repo is currently private but **will go public**. The CourtListener API
token is stored in `.env` which is **gitignored** — it must never be
committed. `.env.example` is the committed template. Any scripts should read
`COURTLISTENER_API_TOKEN` from the environment (or `.env` via a loader), never
hardcode or commit the token.

---

## Related

- `docs/research/long-session-growth.md` — why long-form audio matters (§5)
- `docs/design/*` — architecture for the FSE + fabric consuming this data