# Interfaces v1 (frozen 2026-10-31, REFOCUS §6 + GO step 3)

Architect freeze. Lanes L1–L7 build against this file. A lane that needs
an interface change STOPS and asks the architect; it never edits another
lane's files and never forks the interface. Amendments bump the version
(v1 → v2) with a dated changelog entry at the bottom; v1 text is never
rewritten in place.

Authority order: this file > RS-1 (`docs/design/recording-standard-rs1.md`)
for capture/canon details > bead text. Where the GO message
(`docs/audit/REFOCUS-2026-10-31-GO.md`) differs from anything else, GO wins.

## I1. Canonical session layout + `CanonSession`

Canonical session directory (written by `canon-session`, RS-1 §4):

```
<canon-dir>/
  take0000.wav  take0001.wav  ...   # 16 kHz mono i16, exact frames
  manifest.tsv                      # take, prompt, frames, span_ms, qc_flags
  provenance.tsv                    # raw sha → canon sha, canon version
  SHA256SUMS                        # shas of every file above
```

Loader (the ONLY function experiments use for owner audio, RS-1 §5):

```rust
// crate corpus, module corpus::canonical
pub struct CanonTake { pub id: String, pub prompt: String, pub pcm: Vec<i16>, pub span_ms: (u32, u32) }
pub struct CanonSession { pub dir: PathBuf, pub takes: Vec<CanonTake> }
pub fn load_session(dir: &Path) -> Result<CanonSession, CanonError>;
```

- Sample rate is always 16000, mono, i16. No format detection, no fallback.
- `span_ms` is the RS-1 endpoint span; padding is exactly 250 ms each side.
- CI grep test fails on `hound::WavReader` outside `corpus` and `canon-session`.

## I2. Acoustic model file schema

Existing `models/si_base/model.json` is the schema. Triphone extension
fields are reserved (names TBD by L2, additive only — old readers ignore
unknown fields). L4 starts on `models/si_base` and swaps model files by
path; no code dependency from L4 to L2.

## I3. Language model file

Existing `lm::persist` format (see `src/lm`). Personal-LM interpolation
weights live beside the model file as `<model>.interp.json`
(`{general, personal, tuned_on}`); tuning happens on DEV only.

## I4. Decoder trait + lattice types

```rust
pub trait Recognizer {
    fn decode(&self, pcm16k: &[i16]) -> Lattice;
    fn stream(&mut self, chunk: &[i16]) -> Vec<Partial>;
}
```

`Lattice`/`Token` are the types already in `src/lattice` (L4 may extend
with additive fields only). `decode` takes canonical 16 kHz mono PCM and
returns a lattice; `stream` drives live/VAD use in L6. R5a is the first
implementor (`moore`/trie decoder); file-level model swap, no retraining
plumbing inside the trait.

## I5. Lattice → fabric publication

Per-class `llr` fields exactly as in `fabric-decision-contract.md`:
ACOUSTIC llr only on tokens; SEQUENCE llr pairwise. No new justification
classes without an architect amendment.

## I6. Result-doc template

Every result doc (under `docs/results/`) carries, in this order:

1. Command (exact, re-runnable).
2. Data partition + shas (catalog session ids, canon version, corpus manifest rows).
3. Training-data size (hours/words, stated numerically).
4. Plausibility check (PER/WER < 1.0, size meets bead minimum, test population matches claim).
5. Bootstrap CI (paired, 10k resamples, seed pinned; STAT-1 helper).
6. Verdict line (`<BEAD>-VERDICT: ...`, exactly one).

No bead closes on a result doc missing any of 1–6. Architect reviews every
result doc before its bead closes (GO rule).

## Changelog

- v1, 2026-10-31: initial freeze (REFOCUS §6 six interfaces).
