# Windows mic evidence — hearsay-four (2026-10-26)

Goal: prove the WSL session's Windows path (WASAPI/fifine) before the human
fail-fast recording run. WSL checkout audited at
`~/repos/hearsay-four`; uncommitted capture work ported byte-identical
(md5-verified) to this Windows clone: `src/capture/Cargo.toml` (+hound, +[[bin]]),
`src/capture/Cargo.lock`, `src/capture/src/mic.rs` (`record_raw()`),
`src/capture/src/bin/record-session.rs` (untracked on both sides).

Environment: `rustc/cargo 1.99.1` (rustup, stable MSVC), VS 2022 BuildTools,
cpal 0.15.3 / WASAPI. No system audio prerequisites needed.

| Check | Command | Result |
|---|---|---|
| Build | `cargo build --features mic --bin record-session` (src/capture) | OK |
| Tests | `cargo test --features mic` | 8 passed, incl. live-capture test |
| Devices | `record-session.exe --list-devices` | `Line In (Realtek)`, **`Microphone (fifine Microphone)`**, `Microphone (Steam Streaming Microphone)`; exit 0 |
| Default input | cpal probe (scratch crate, %TEMP%\mic-probe) | Host `Wasapi`, default = **fifine**, 2ch 48000 Hz F32 |
| Smoke record | `record-session.exe --out sessions\smoke-win --script script.txt --utterances 3` (Enters piped) | `enroll.wav` + `utt000..002.wav`, exit 0 |

WAV verification (Python stdlib `wave`): all files mono, 48000 Hz, 16-bit,
144000 frames = 3.00 s (utt001: 143520 / 2.99 s). Nonzero samples 2-22%,
peak 27-67, RMS 1.0-3.9 — live room tone from the real device, not zeros.
Nobody spoke on the smoke takes; levels are therefore ambient only.
`manifest.tsv` rows match the bin contract: `file \t expected \t rate \t samples`.

Script: `script.txt` at repo root was reduced from 160 to 120 lines after the
speaker completed the enrollment and 64 scripted utterances. The first 64
lines are exactly the recorded prefix. `sessions/generate_waves.py` adds:

- Wave 2 (`sessions/script-wave2.txt`, 26 lines): brings every word to 3 exposures.
- Wave 3 (`sessions/script-wave3.txt`, 30 lines): brings every word to 4 exposures.

Generated repeats are spaced 18-34 utterances apart. Each final 40-utterance
bucket contains all 30 words, with each word occurring once or twice per bucket.
The original list is preserved as `script-160.txt`.

Continue in waves (own terminal, press Enter before each 3 s take):

```powershell
cd C:\Users\<you>\repos\hearsay-four-win
.\src\capture\target\debug\record-session.exe --out sessions\run1 --script script.txt --resume-at 64 --utterances 26
# Later, Wave 3:
.\src\capture\target\debug\record-session.exe --out sessions\run1 --script script.txt --resume-at 90 --utterances 30
```

`--resume-at` validates the existing manifest against the script prefix,
skips the duplicate enrollment, appends to `manifest.tsv`, and refuses to
overwrite existing WAVs. It was smoke-tested on a disposable copy of run1
(recorded `utt064.wav`, manifest appended; real `sessions\run1` untouched);
the copy was removed after verification.

Recording completed 2026-10-26 (speaker, fifine, WASAPI): `sessions\run1`
holds 121 WAVs — `enroll.wav` plus `utt000.wav`-`utt119.wav`. Verified from
`manifest.tsv` + WAV headers: every row's file exists, all mono 48000 Hz,
143520-144000 samples per take, and each of the 30 vocabulary words occurs
exactly 4 times across the 120 scripted utterances. WAV bytes stay local
(gitignored); the committed `manifest.tsv` is the provenance index.

The WAV + TSV bundle is byte-format identical to the Linux build and can be
copied to WSL (`/mnt/c/...`) for the o20 scorer adapter.
Beads: intentionally not instantiated on this side (WSL `.beads` embeddeddolt
remains canonical); no bead state was created or modified here.
