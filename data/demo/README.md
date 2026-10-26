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
