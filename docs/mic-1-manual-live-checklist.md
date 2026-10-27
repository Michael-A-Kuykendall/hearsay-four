# MIC-1 manual live checklist (bead hearsay-four-4zx)

Owner-run-once, NOT gate-blocking. The hermetic gate
(`cargo test -p runner`, stub-fed fixture stream + device-error cases)
is the green/red bar; this checklist proves the same incremental path
against a real microphone and real speakers.

## 0. Hermetic gate (reference)

```bash
cd /home/michael/repos/hearsay-four-lane-mic1 && cargo test -p runner
```

Expected: `22 passed` (lib, incl. 4 `live::live_tests`) and `11 passed`
(`hearsay-serve`, incl. 3 MIC-1 gates), 0 failed. No physical mic, no
network, fixture audio + synthetic only.

## 1. Live binary on a fixture (no mic)

```bash
cd src/runner && cargo run -q -p runner --bin hearsay-serve -- \
  --live --input ../../data/demo/sample.wav --max-secs 4 --chunk-secs 2
```

Expected: exit 0; stderr shows one `live: provisional …` line per
committing chunk plus a `live: final (N words): …` line; stdout is the
same transcript JSON as the batch run (byte-identical for the same
flags), every word validating. Reference transcript for the first 4 s:
`PANHANDLING HORRIBLY`.

Negative probes (both exit 2, empty stdout, message on stderr):

```bash
./target/debug/hearsay-serve --live </dev/null            # no input
./target/debug/hearsay-serve --live --live-frame-ms 0 \
  --input ../../data/demo/sample.wav                      # bad frame size
```

## 2. Real microphone, end to end (owner runs once)

Prereqs: system audio dev libraries for cpal (ALSA on Linux), a working
default input device, a quiet room. All commands run from the lane root
(`/home/michael/repos/hearsay-four-lane-mic1`).

```bash
# 2a. Capture one enrollment + two scripted utterances with the real mic.
cargo run -q -p capture --features mic --bin record-session -- \
  --script script.txt --out sessions/live-mic1 --utterances 2
```

Speak each prompted line after pressing Enter. Expect one WAV per
utterance plus `enroll.wav` and a `manifest.tsv` whose rate/samples
columns match the saved files.

```bash
# 2b. Stream a recorded utterance through the live ingest path.
cargo run -q -p runner --bin hearsay-serve -- \
  --live --input sessions/live-mic1/utt000.wav --max-secs 12 --chunk-secs 2
```

Expected: exit 0; `live: provisional …` lines appear incrementally as
each 2 s chunk commits; `live: final …` matches the batch
(`--live` removed) transcript word for word. Any device failure (no
input device, stream setup failure) must exit 2 with a naming message
on stderr — never a silent stall, never partial JSON on stdout.

## 3. Production-seam note

`serve --live` replays decoded file/stdin input in mic-sized frames
through the same `LiveSource → run_live_session` path a mic frame source
drives; it does not open the microphone itself (the workspace E2
contract keeps cpal/`capture` out of the runner). A true mic-driven
`LiveSource` (cpal callback chunks behind the `mic` feature) plugs into
the same seam; until then `record-session` remains the capture tool and
step 2b is the live-path proof. Wiring a native mic source is follow-up
work, not part of this bead.
