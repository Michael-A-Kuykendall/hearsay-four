# Word-Lattice File Format (v1)

Bead: `hearsay-four-g0m--word-lattice-file-format`. The contract between
lattice producers (`hearsay-four-5n2--connected-word-decoder`,
`hearsay-four-71g--trie-decoder-lattice`) and consumers
(`hearsay-four-p3o--lattice-format-oracle` oracle work,
`hearsay-four-cjj--fabric-pruning-experiment`): lattices are generated
ONCE per acoustic configuration; conditions A/B/C read the same file.

Implements `docs/design/fabric-decision-contract.md` token/score
conventions and A3 token identity (`hearsay-four-pri`).

## Encoding (measured choice)

Text JSONL: one JSON object per line. Measured 2026-10-29
(`lattice_jsonl_size_bounded`): 1000 tokens + 2000 arcs = 142,250 bytes
(~95 B/token, ~24 B/arc). A large lattice stays ~140 KB — inspectability
wins; no binary encoding unless a measured lattice exceeds 10 MB.

## Layout

Line 1 is the header; token and arc lines follow in any order (the writer
emits tokens-then-arcs). Blank lines are ignored.

```jsonl
{"format":"hearsay-word-lattice","version_major":1,"version_minor":0,"utterance":"utt-id","frame_shift_ms":10.0,"producer":"71g/trie@<commit>"}
{"t":{"id":1,"word":"hello","start":10,"end":45,"source":"71g","acoustic_llr":3.2,"lexical_llr":0.5}}
{"t":{"id":2,"word":"hollow","start":10,"end":47,"source":"71g","acoustic_llr":1.1}}
{"a":{"from":1,"to":3}}
```

## Schema

Header: `format` (must equal `hearsay-word-lattice`), `version_major`
u32, `version_minor` u32, `utterance` string, `frame_shift_ms` f64
(frames are the time unit; 10.0 ms is typical), `producer` string
(decoder id + config/commit).

Token (`t`): `id` u64 per-occurrence identity (never reused within a
file; same word twice = two tokens, A3), `word` string (non-empty;
silence is the explicit word `"<sil>"`), `start`/`end` u32 frames
(`start < end` required), `source` string (producer component),
`acoustic_llr` f64 required, `lexical_llr`/`speaker_llr`/`verify_llr`/
`user_llr` optional (absent = absent term per contract §3 rule 2; the
writer omits `None`, the reader accepts missing and null identically).
All floats must be finite (non-finite is a loud writer/reader error).
Declared nulls are implied by this spec version (contract §3 table):
ACOUSTIC = garbage/SIL over the same frames, LEXICAL = phone-unigram,
SPEAKER = SI model, VERIFY = mismatched-span, USER = fixed large llr.

Arc (`a`): `from`/`to` u64 token ids — topology only. Endpoints must
reference known tokens (dangling arcs rejected).

## What is NOT in the format (normative)

- No LM/SEQUENCE scores anywhere: `lm_llr`, `sequence_llr`, `path_score`
  keys are rejected loudly on read. SEQUENCE terms are fabric-computed
  from the LM (contract §9); their absence here is the on-disk half of
  the byte-identity requirement (§6.4), which A5 re-checks.
- No tiling weights (`w_c`, `w_SEQ`): experiment parameters applied by
  consumers, not lattice content.
- No explicit confusion regions: p3o derives alternatives-per-region
  from span overlap. If that proves insufficient, regions arrive as a
  v1-minor additive field.

## Versioning rule

`version_major` bump = breaking (unknown majors rejected loudly with
both versions named). `version_minor` bump = additive only: the reader
accepts any minor under major 1 and ignores unknown fields (except the
forbidden LM keys, which always reject).

## Worked example

`src/lattice/tests/fixtures/d0_words.lat.jsonl`: 8 tokens over the first
8 corpus-vocabulary words (impeachment…resented, from
`CLOSED_VOCABULARY` in `src/fse/src/lib.rs`, top-30 by frequency across
3500 transcripts), chain arcs plus a two-way branch at position 0 (the
oracle-relevant shape: two alternatives, one region).
