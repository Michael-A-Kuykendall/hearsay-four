# `data/demo/sample.wav` — provenance

- **What:** 8.0 s mono 16 kHz 16-bit WAV (256 044 bytes), the loudest
  consecutive 8 s window (1 s stride) of the source below.
- **Source:** `data/corpus/court/scotus/106764/audio.mp3`
  (U.S. Supreme Court oral argument, public record; fetched by the
  `corpus-fetch` bin, bead `79z`). Trim offset: start = 2871.0 s.
- **License:** U.S. federal government work, public domain. No new
  rights encumber this clip.
- **Why it exists:** zero-config fixture for `hearsay-demo`, the WASM
  demo, and CLI release smoke tests (bead `9hh`) — the full corpora are
  git-ignored and 14–22 MB each.
- **Expected result:** `hearsay-demo -- --input data/demo/sample.wav
  --max-secs 8` exits 0 with a 4-word transcript
  (`PANHANDLING HORRIBLY HORRIBLY UNCORROBORATED`) and all 4
  provenance chains terminating at frame-level acoustic evidence.
  Re-cut with `/tmp/wavtrim` (throwaway; not in this repo) if the
  expectation ever drifts — do not hand-edit the WAV.

# MP3-1 serve fixtures (bead `hearsay-four-gmv`)

- **What:** `serve-mp3.mp3` (40 097 bytes, ~10 s MPEG-2 Layer III
  22.05 kHz, Roberts intro to Case 24-889) + `serve-mp3-twin.wav`
  (canonical 16 kHz mono 16-bit WAV decoded from the MP3 by the corpus
  `mp3-to-wav` bin, 159 660 frames) + `corrupt.mp3` (1 034 bytes of
  synthetic garbage: an `ID3` preamble followed by fixed-seed PRNG bytes
  from `random.Random(0xC0DE)`, undecodable by design).
- **Source:** the Oyez delivery MP3 for "Oral Argument - April 29, 2026"
  (Hikma v. Amarin, Case 24-889; U.S. Supreme Court oral argument, public
  record; fetched anonymously 2026-10-27 from the s3 URL recorded in
  `data/session/oyez_media_25951.json`). The clip is the first ~10 s of
  audio frames after the ID3v2 tag, cut at an MPEG frame boundary with a
  throwaway frame-chaining script (not in this repo; 383 consecutive
  frames, zero gaps) — do not hand-edit either fixture.
- **License:** U.S. federal government work, public domain. No new
  rights encumber these clips.
- **Why they exist:** twin-parity gate for `hearsay-serve` MP3 input —
  the MP3 must transcribe byte-identically (modulo `recording_ref`) to
  its WAV twin both via `--input` and via piped-stdin sniffing, and the
  corrupt fixture must exit 2 with empty stdout.
- **Data rule:** synthetic fixture (`corrupt.mp3`) + one Oyez MP3 clip
  (`serve-mp3.mp3`); the WAV twin is derived from that clip by decoding,
  not new data. Gates using these fixtures make no network calls.
