# Oyez API ingest evaluation (research spike, Oct 26 2026)

Question: is there a free, no-strings source of court audio with
**timed** transcripts that beats anything already on disk? Answer: yes —
the Oyez API. Verified live tonight; nothing was taken on trust.

## 1. License and cost (the strings-attached check)

- **Price:** $0. No key, no account, no membership, no purchase.
  Every endpoint hit tonight returned `200` anonymously.
- **License:** Creative Commons non-commercial + attribution on Oyez's
  enhanced MP3s and transcripts (corroborated by Network World, D-Lib,
  and Creative Commons' own site). The underlying National Archives
  recordings are public domain, but Oyez's dubbings carry the CC-NC
  terms — use Oyez's terms, not the Archives'.
- **Strings attached (stated plainly):**
  1. **Non-commercial use only.** Covers hackathon research. If
     Hearsay-IV ever goes commercial, Oyez audio needs relicensing
     consideration — flag, not blocker.
  2. **Attribution required.** One line in release notes / submission.
- Compare: DCMP requires qualifying membership and has no bulk path;
  NCRA (courtreporting.org) is a trade association, not a data source.

## 2. API mechanics (all probed, not documented-from-memory)

- Index: `GET https://api.oyez.org/cases?page=N` → 30 cases/page,
  each with `href: /cases/{term}/{docket}`. Pagination confirmed
  (`?page=2` returns different cases; `?limit=` is ignored).
- Case: `GET /cases/{term}/{docket}` → `oral_argument_audio[]` with
  per-segment `href: /case_media/oral_argument_audio/{id}`.
- Media: that endpoint returns `media_file[]` (MP3 + OGG + HLS direct
  S3 links, with byte sizes) and `transcript`.
- Transcript shape (`21627.json`, 240 KB, saved 2026-10-26):
  `transcript.duration` (3661.222 s),
  `sections[{start,stop,byte_start,byte_stop,
  turns[{start,stop,byte_start,byte_stop,
  speaker:{name,ID,roles}, text_blocks[{start,stop,byte_start,byte_stop,text}]}]}]`.
  Sample case: 4 sections, 216 speaker turns, 643 text blocks.
- Bulk strategy: crawl `/cases?page=N`, keep cases whose media has a
  transcript object, download MP3 + JSON. No auth headers anywhere.

## 3. Ingest mapping to our scorer layout

Each `text_block` (or coarser speaker `turn`) maps 1:1 to a
`session.tsv` row: `bucket, idx, start_sec, stop_sec, speaker.name,
drift_tags, text`. The byte offsets additionally allow HTTP-Range
slicing of the MP3 per block without downloading the full file —
verified tonight: bytes 135544–145575 decoded to 40124 samples
(2.507 s) against text timings 7.245–9.761 s (2.516 s). Match.
Our `corpus::audio::decode_to_pcm16k` handles the files (symphonia
MP3 path), and the full `hearsay-demo` pipeline runs them exit 0
with terminating provenance chains (15 words on a 30 s probe of
`PPL Montana v. Montana`; repetitive output is the 30-word closed
vocab on open speech, not a data defect).

## 4. File-size budget (measured, not estimated)

| Case | Duration | MP3 | Transcript JSON | Turns |
|---|---|---|---|---|
| 10-218 (2011) | 61 min | 14.7 MB | 240 KB | 216 |
| 70-112 (1971) | 60 min | 14.4 MB | ~240 KB | 231 |
| 70-314 (1971) | 40 min | 9.7 MB | ~200 KB | 110 |

Model: **~15 MB/hour audio, ~250 KB transcript JSON per case.**
10-case pilot ≈ 150 MB (fits beside the 154 MB corpus dir);
100 cases ≈ 1.5 GB (belongs in gitignored bulk storage with a
manifest, same discipline as `data/corpus`).

## 5. Gaps and risks (for the beads)

- Transcript presence varies by case/era — the crawler must filter
  on the transcript object, not assume it.
- Turn/text-block boundaries are Oyez's professional alignment, not
  ours — good (independent ground truth), but spot-check drift on old
  tapes before trusting sub-second edges.
- Speaker `roles` distinguish justices from advocates — free
  multi-speaker A/B structure, but advocate names repeat across cases
  while justices persist for decades (carry-forward across sessions
  is a separate design question, out of the pilot).
- Page count of `/cases` is unknown — the pilot finds out by walking
  it; cap the pilot at 1 case + index walk.

## 6. Bead plan (filed from this doc)

- `oyez-pilot`: one case end to end — fetch MP3 + JSON, transform
  blocks to `session.tsv`-style manifest, score A-vs-B through the
  existing scorer, record verdict.
- `oyez-bulk`: N cases across terms, pre-registered buckets, per-case
  verdicts; multi-speaker carry-forward evidence.
- `cv-eval`: Common Voice bulk clips through phone-level scoring
  (zero alignment work, accent-diverse; no vocab change needed for a
  front-end health metric).
- Local `phrase_training` clips: front-end use only until provenance
  is established (0/3500 fully in-vocab, origin undocumented) —
  separate provenance bead, not this lane.

## 7. Outcomes (2026-10-27 wave)

- `oyez-bulk` (`hearsay-four-c1g`, CLOSED): N=3 cases across
  1971/2011/2025 terms, 3/3 `failfast_run --root` exit 0 PASS,
  margins 94.72 / 38.72 / 40.86, monotone-B true. Carry-forward:
  Roberts/Sotomayor/Alito/Kagan persist 2011->2025, advocates
  rotate fully. Manifests+transcripts+MANIFEST.json committed under
  `data/oyez-bulk/`; audio gitignored.
- `cv-eval` (`hearsay-four-jhn`, IN PROGRESS, blocked): Common
  Voice HF copies are README-only (removed Oct 2025 to Mozilla
  Data Collective, gated on signup + API key); S3 sample URL 403;
  mirrors lack accent columns; v1 archive.org lacks accent
  metadata. Eval pipeline (`cv_eval.rs` lib + bin, fetch script
  with `--mdc`/`--from-dir`, 3 unit tests) built and
  synthetic-green on `hackathon/track-b` (uncommitted). Resume:
  human completes MDC signup, exports MDC_API_KEY, fetch `--mdc`,
  run eval, commit on green.
