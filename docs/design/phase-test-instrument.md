# Phase-Test Measurement Instrument

Bead: `hearsay-four-kyz--sealed-phase-test-instrument`. The sealed-session
bead (`hearsay-four-sq8--sealed-phase-test-session`) cites this protocol.

The recorder (`scripts/record_phase_test.sh`) + scorer (`a1_gate`, validation
in `src/fse/src/instrument.rs`) form a MEASUREMENT INSTRUMENT, not a script
pair. Threat model is self-deception: every ceremony below exists so the
experimenter cannot fool themselves, not to resist an adversary.

## Session layout

A formal phase-test session directory contains exactly:

- `utt000.wav` ... `utt029.wav` — 30 takes, PCM16 mono 16000 Hz, 2.5-3.5 s
- `manifest.tsv` — 4-col `file word rate_hz frames` + header line 1
  (reader-compatible with the DEV loader; row i is prompt position i)
- `order.txt` — line 1 `seed <N>`, then the 30 prompt words in prompt order
- `session.meta` — KEY=VALUE: device, rate_hz, date_utc, seed, order_sha256,
  manifest_sha256, recorder_commit, kept
- `SHA256SUMS` — sha256sum-compatible sidecar over the 30 WAVs + manifest +
  order.txt

## Vocabulary source

`sessions/script-wave3.txt` (30 lines) is the SINGLE source of truth. The
recorder reads it (fails unless exactly 30 lines); the validator bakes it
in via `include_str!` (`instrument_vocab_has_thirty_words` pins the shape).
No second list exists anywhere.

## Precommit ceremony

1. Recorder shuffles the vocab with `seed = date +%s`, writes `order.txt`
   and the `session.meta` stub (manifest hash pending) BEFORE the first clip.
2. Recorder prints `PRECOMMIT seed=... order_sha256=...`.
3. The hash is recorded in the session bead notes BEFORE capturing.
4. The scorer re-verifies the hash; any post-hoc order edit is a REJECT.

## Duration range

Takes are nominally 3.0 s (byte-exact capture). The recorder verifies each
take is within 2.5-3.5 s at capture (reject + re-prompt); the scorer
rejects the session unless every header duration is within 2.5-3.5 s.
Capture is all-or-nothing per word (no skip/quit); Ctrl-C abandons the
session as INCOMPLETE, which the scorer rejects.

## Hash layout

- `order_sha256`, `manifest_sha256` in `session.meta` (meta written
  pre-capture with the order hash; manifest hash + kept finalized post-capture).
- `SHA256SUMS` sidecar covers every WAV + manifest + order.txt.
- The scorer recomputes all hashes; first mismatch rejects the session.

## Scorer checks (first-violation order)

1. `session.meta` exists, parses, has all 8 keys; `rate_hz == 16000`.
2. `order.txt` hash matches meta; `seed` line matches meta; 30 words.
3. `manifest.tsv` hash matches meta; rows parse; `kept` agrees with row count.
4. Vocab multiset: unknown label, then duplicate label, then missing label
   (each names the culprit).
5. Positional: manifest row i word == `order.txt` position i.
6. Sidecar: every expected file present with a matching hash (extra sidecar
   entries ignored: forward-compatible with recorder additions).
7. Headers: every WAV PCM16 mono 16000 Hz; manifest rate/frames agree.
8. Durations within 2.5-3.5 s.

`--validate-only <dir>` runs checks only (exit 0 + ACCEPT / exit 1 +
violation), scoring nothing.

## Frozen config

Phase-test scoring requires `--frozen-config <string>` (refused without
it; DEV runs unaffected). Format: `<base>+tokens...`, e.g. the t5i freeze:

`base_cmn_d_dd_lifter22_pre097+cmvn_replaces_cmn+dtw_baseline+band0.2`

Tokens: `cmvn_replaces_cmn`, `dtw_baseline|v1|v2|v4`, `band<float>`.
Parsing fails closed (unknown token / wrong base / band outside (0,1]
refuses). The scorer cross-checks the base against `tuning_grid()[0]`,
scores templates + queries with the frozen pipeline, and records the
string plus the scorer commit (`git rev-parse HEAD`, or an honest
`unknown (git unavailable)`) in the score log.

## Score-once rule

Phase-test scoring requires `--out-dir <fresh-dir>`: validate-all, then
score, then write `score.log` (commit, config, dirs, order hash, results,
confusion) into a directory that must not exist. Any existing output dir
is refused BEFORE scoring (no overwrite, no re-score). Atomicity: any
validation violation rejects the whole session with no partial output.
