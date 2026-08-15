# Court Audio + Transcript Sources — Research Spike
### Free, long-form, transcript-aligned corpora for court-reporting-style speech

Status: Requirements-gathering research (2026-08-15). These are the *new*
sources to acquire; contrast with the local pre-existing clips in
`local-bettervoice-inventory.md`.

---

## 1. CourtListener — Free Law Project (primary source)

**What:** the largest collection of oral-argument audio on the Internet.
SCOTUS + all federal circuit courts that provide it; state courts growing.
Collection begins ~2013 for SCOTUS (Oyez covers pre-2013).

**URLs**
- Archive search: https://www.courtlistener.com/audio/
- API: https://www.courtlistener.com/api/rest/v4/audio/
- Coverage: https://www.courtlistener.com/help/coverage/oral-arguments/
- Wiki: https://wiki.free.law/c/courtlistener/help/api/rest/v4/oral-arguments

**API fields (v4, JSON, token-authenticated)**
- `local_path_mp3` — enhanced MP3 (22050 Hz, 48 kbit/s, VBR, ID3 tags set,
  court-seal cover art) — preferred download target.
- `download_url` — original file from the court (if still up).
- `duration` — estimated seconds (sampled, not exact for VBR).
- Linked to `dockets/` (case name, docket number, court, argued date, panel
  judges) and the `people/` (judge) endpoints.

**API auth:** requires a personal token (`Authorization: Token <token>`).
Free account on courtlistener.com → API key. Rate limited (see wiki rate-
limiting page; ~5,000 req/day standard, higher with `in_row=1`). Use the
bulk/replication data path for full-table pulls instead of hammering REST.

> **VERIFIED 2026-08-15 (token working):**
> - `GET /api/rest/v4/audio/?format=json` → HTTP 200; audio objects include
>   `stt_status`, `stt_source`, `stt_transcript` — **CourtListener now ships
>   built-in speech-to-text transcripts** (STT) alongside the audio. One API
>   call yields audio URL + full transcript. This is a major simplification
>   over pairing audio with an external transcript source.
> - `GET /api/rest/v4/audio/?docket__court=scotus&format=json` → 20 results,
>   cursor-paginated (`next` field), SCOTUS case `Hikma Pharmaceuticals USA
>   Inc. v. Amarin Pharma, Inc.` returned `duration: 3109`, `stt_status: 1`,
>   `stt_transcript` ~55 KB, `local_path_mp3` (enhanced) + `download_url`
>   (original from supremecourt.gov).
> - The earlier HTTP 400 on `?limit=1` was a bogus filter, not auth failure —
>   this endpoint uses cursor pagination, not `limit`.

**Enhanced-audio details:** Free Law Project post-processes every file with
their `Doctor` tool into uniform 22.05 kHz / 48 kbp MP3 + metadata. Both the
enhanced (`local_path_mp3`) and original (`download_url`) forms are exposed.

**Credential handling (this repo):** token lives in `.env` (gitignored; repo
will go public). `.env.example` is the committed template. Do NOT commit the
real token. See `README.md` in this directory.

---

## 2. Oyez Project — SCOTUS audio + TIMESTAMP-ALIGNED TRANSCRIPTS (gold)

**What:** volunteer multimedia archive of US Supreme Court oral arguments,
"complete and authoritative for all audio since **October 1955**". Features
the crucial asset for alignment: **word/turn-level timestamps synchronized to
the audio** — i.e. ready-made `${audio} ↔ ${timed transcript}` pairs suitable
for frame-level phonetic experiments.
Sponsors: Chicago-Kent (IIT), Cornell LII, Justia. Noncommercial.
Authored/endorsed by supremecourt.gov as an official-adjacent store.

**Access**
- Website: https://www.oyez.org/ (JS-rendered)
- Backend API: https://api.oyez.org/ (user account/login gated for full
  dump; public endpoints exist for cases/transcripts)
- Coverage note: SCOTUS pre-2013 → use Oyez; post-2013 → CourtListener also.

**Transcript+audio alignment:** Oyez exposes per-case argument media with
`transcript` segments carrying start/stop offsets tied to the audio URL. This
is exactly the alignment (audio + timed transcript) that our provenance chain
(aem.16) and the fail-fast experiment need for real courtroom multiuser
acoustics (multiple speakers, judge interruptions).

---

## 3. SCOTUS.gov (primary, free)

- Official oral-argument audio + written transcripts from the Supreme Court:
  https://www.supremecourt.gov/oral_arguments/audio
- Official transcripts: https://www.supremecourt.gov/oral_arguments
- Free to download; covers SCOTUS since ~2010s onward; aligns with
  CourtListener's SCOTUS collection (which may duplicate it).

---

## 4. Federal circuit courts (individual)

Each circuit hosts its own oral-arg audio/transcripts; coverage & retention
varies. FLP began archiving because courts "deleted them, claiming too
expensive to keep." So the reliable capture is CourtListener (which stores
them), not the court sites themselves.

---

## 5. Free Law Project Data & bulk/replication

- API key endpoint: https://courtlistener.com/api/rest/v4/audio/ (needs auth)
- Bulk data (bulk dumps): https://wiki.free.law/c/courtlistener/help/api/bulk-data
- DB replication: https://wiki.free.law/c/courtlistener/help/api/replication
- Bulk-data + RECAP dumps on archive.org / S3 (e.g.,
  `https://storage.courtlistener.com/bulk-data/media/...`) — the cheap path
  for large acquisition rather than per-file REST.

---

## 6. Licensing & terms (capture constraints)

- **CourtListener Content:** hosted & enhanced by Free Law Project. Official
  federal-record audio + their enhancements; content generally within public
  domain for federal records, but enhanced audio embeds FLP ID3/artwork — 
  verify attribution; their contentType for details: 
  https://wiki.free.law/c/terms/courtlistener/courtlistener-com.txt
  (retain attribution, noncommercial constraints may apply to enhanced files)
  *Officially: federal court audio is public-domain; FLP's THANK-YOU*
  enhancement is their contribution — check THIRD-PARTY terms. See and
  honor `REMOVAL` policies — don't redistribute enhanced art.
- **Oyez**: noncommercial archive; schools-hosted data; slightly unclear
  commercial licensing (SPL + archive-funded). For research/prototype OK;
  for commercial court-reporter product → verify with LII/Justia.
- **SCOTUS**: official audio is public-domain (as work of US govt).
- **Court reporter training** — the target use case described by the user —
  aligns with these data being legal-argument/speech; not privacy restricted
  (oral arguments are public proceedings).

**Bottom line for licensing:** safe to use for a research/prototype on this
repo; if the deliverable becomes a commercial training product later, verify
CourtListener Content-Type + Oyez noncommercial terms, and do NOT redistribute
enhanced-cover-art files. Always prefer the *original* (`download_url`) over
FLP's enhanced MP3 for redistribution-free corpus building.

---

## 7. Recommended acquisition path (for THIS project)

Restate for reproducibility:

1. **Auth-free bulk/prod path (recommended):**
   - Register a (free) token; use CourtListener REST with `?format=json`.
   - For large pulls, use the S3/archive bulk-dump download rather than REST
     pagination.
2. **Local target (new downloads land here):**
   `$HEARSAY_FOUR/data/corpus/court/{court}/{docket}/{name}.mp3` +
   `transcript.{txt,json}` + `manifest.json` (audio→transcript,duration,court)
   — manifests live in `docs/research/corpus/`.
3. **SCOTUS timed-alignment is best via Oyez** (audio+timed segments); if Oyez
   phrase-account unobtainable, fall back to SCOTUS.gov transcripts (no timing)
   or CourtListener audio + manual align.
4. **Do not vendor** the audio into git; keep it under `data/` and version the
   manifests/URLs.

---

## 8. Estimated scale (for capacity/+provenance §5)

- A single oral arg is ~ 30–80 min of multi-speaker continuous audio + one
  transcript. That is exactly the "open continuous stream, closer to MikeDB's
  fact volume" regime Hearsay-IV's §5 (provenance_stats) was built to
  measure — good stress-test material.
- A small evaluation holdout (e.g., 10 arguments) plus a fail-fast
  training/experiment slice from better-voice clips is a sufficient start;
  we do NOT need the entire million-audio archive for the prototype.

---

## Summary decision

- **Local-first:** the better-voice clips satisfy calibration + closed-vocab
  needs (zero downloads).
- **CourtListener/Oyez:** for the long-form, real-courtroom, multi-speaker
  drift session the §6 experiment (condition A vs B) actually needs.
- Aligned transcripts (Oyez/SupremeCourt) give us wav+text pairs for the
  provenance chain to real acoustic evidence (aem).
- Next implementation step: build the `corpus` RFC crate/ingest tools
  (CourtListener REST client + Oyez parser + hound reader) gated by length,
  court, decodability (PCM-compatible).