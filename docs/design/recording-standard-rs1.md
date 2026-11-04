# RS-1 — Hearsay-IV Recording & Canonicalization Standard

Status: PROPOSED 2026-10-31 (cloud audit). Supersedes the capture rules in
`docs/design/recording-protocol.md` §1–2 and the session layout in
`docs/design/phase-test-instrument.md`. Lists, partitions, LOCKBOX rules and
P4 design in `recording-protocol.md` §3–4 remain in force.

**Purpose:** one way to record, one way to turn a recording into
experiment input, and one loader. No experiment may read a raw session.
No lane may write a new recorder, a new session layout, or a new WAV loader.

---

## 0. Why (measured history)

| Session | Chain | Outcome | Cause |
|---|---|---|---|
| `run1` (Oct 26) | Windows WASAPI, fifine, 48 kHz, legacy `record-session.exe` | 120/120 good, exact frame counts | known-good chain |
| `phase-test-20261028-182142`, `-183205` | WSL | unclassified | not in any results doc; classify in the catalog |
| `phase-test-20261028-190133` | WSL `record_phase_test.sh` rewrite, 44.1 kHz, 3.8–5.9 s variable windows | VOID | different protocol; DTW band unreachable |
| `phase-test-20261030-103200` | WSL | unclassified | classify in the catalog |
| `phase-test-20261030-143058` | WSL Pulse, sealed 16 kHz | INVALID (quarantined) | 5/30 takes muted input (−90 dBFS) |
| `phase-test-20261031-092608` | WSL Pulse, sealed 16 kHz | INVALID (deleted) | 1/30 take muted |
| `phase-test-20261031-100240` | WSL Pulse, sealed 16 kHz | VALID capture; A1 NOT COMPARABLE | trimmed lengths 59–174 frames vs 128–298 on run1 templates |

Every capture failure happened on the WSL/Pulse (WSLg/RDP audio) chain.
That chain also delivered 5–99% more samples than wall-clock time
("burst overflow", truncated by `record_exact`) and two muted transients.
The Windows chain recorded 150+ takes with exact frame counts. Treat
the channel as a variable: **one capture chain for all owner data.**

The A1 length mismatch is very likely an **endpointing** defect, not
speaking rate. The trimmer threshold is `max(0.001, 0.01·peak)` (−40 dB
re peak). The committed endpoint audit (`A1-ENDPOINT-AUDIT.md`) shows the
run1 speech span starting 0–20 ms into the clip on **12/12** audited
clips. A person pressing Enter then speaking does not start talking 0 ms
into the window. The likely culprit is the Enter key click or stream-start
transient, which the −40 dB threshold accepts as speech. The audit
checked truncation, "not tightness", so it could not catch this.
**Check before anything else:** plot frame energy for the first 400 ms
of 10 run1 clips and listen to 3. If a click sits at 0–100 ms, run1
templates carry 0.3–1.0 s of non-speech, and the "honest" 0.8583 DEV
number still contains a padding artifact.

## 1. Capture chain (frozen)

| Item | Standard |
|---|---|
| Host | **Windows native** (WASAPI) only. No WSL/Pulse/RDP audio capture for any owner session. |
| Recorder | The existing legacy `record-session.exe` (the binary that recorded run1), plus only the two changes in §2. No new recorder, no rewrite. |
| Mic | fifine USB (the run1 mic). Fixed position: record distance in cm in `session.meta`. |
| Rate | Device native (48 kHz, 2ch F32 → mono i16 mixdown, as run1). Resampling happens once, in the canonicalizer (§4), never in the recorder. |
| Room | Same room as run1, door closed, no fan. |
| Level | Peak between −20 and −3 dBFS on a test word before the session starts. |

## 2. Take mechanics (two changes to the legacy recorder, nothing else)

1. **Key-click guard.** After Enter, wait 250 ms, then show
   `SPEAK NOW` and start the capture window. The keystroke is never
   inside the window. (Alternative if the recorder can't delay: the
   canonicalizer ignores the first 300 ms; pick one and record it in
   `session.meta`.)
2. **Redo.** After each take the recorder prints peak dBFS and
   `[Enter]=next  [r]=redo`. `r` re-records the same prompt and overwrites
   that take. The redo is logged in `session.log`. The recorder also
   auto-redoes (`SILENT TAKE`, existing 52e logic) when peak < −40 dBFS.
   **No auto-advance.** Every take waits for Enter.

Fixed window per list:

| List | File | Window | Notes |
|---|---|---|---|
| `words30` | `prompts/script-wave3.txt` (30) | 3.0 s | A1; same as run1 |
| `phrases30` | `prompts/script-a2-phrases.txt` (104) | 5.0 s | A2/A5/A6 |
| `harvard` | `prompts/script-harvard.txt` (720) | 6.0 s | enrollment/adaptation (R4). 5 s truncates long sentences |
| `read` | `prompts/script-c0r-read.txt` (104) | 8.0 s | R7 eval |
| `fly-dev`, `fly-ptest` | `prompts/script-p4-fly-*.txt` | 8.0 s | P4 |
| `spont` | none | continuous 300 s | R7 spontaneous rung |

Only one word list exists for the 30-word vocabulary: generate
`CLOSED_VOCABULARY` checks from `script-wave3.txt` (or the reverse), plus
a test that they are equal as sets.

## 3. Raw session layout (immutable after capture)

```
sessions/raw/<session-id>/
  take000.wav ... takeNNN.wav     native rate, mono i16
  prompts.txt                     prompt per take, in recorded order
  session.meta                    KEY=VALUE (below)
  session.log                     redos, silent-take retries, timestamps
  SHA256SUMS                      over every file above
```

`<session-id>` = `owner-<YYYYMMDD>-<list>-<PART>-<NN>`, PART ∈
TRAIN|DEV|PHASE-TEST|LOCKBOX, assigned **before** recording.

`session.meta` keys: `host` (windows-wasapi), `device`, `mic_distance_cm`,
`rate_hz`, `channels_in`, `list`, `prompt_file_sha256`, `order_seed`,
`order_sha256` (printed as PRECOMMIT before the first take),
`partition`, `recorder_version` (git rev), `click_guard_ms`, `date_utc`.

Raw sessions are never edited. A bad session gets status INVALID in the
catalog (§6) and stays on disk.

## 4. Canonicalizer — the only path into experiments

One tool: `canon-session <raw-dir> <out-dir>` (new bin in `src/corpus`).
Deterministic: the same raw input gives byte-identical output.

1. **Validate:** meta keys, SHA256SUMS, prompt count = take count, prompt
   file hash, order hash, format.
2. **Resample** to 16 kHz mono with one pinned resampler: a windowed-sinc
   polyphase implementation with its parameters and version string
   frozen in code. The parameters are never tuned. run1 is re-derived
   with it (replacing whatever made `run1-16k`) and the derivation is
   recorded raw sha → canon sha.
3. **Endpoint (RS-1 endpointer)** on 25 ms / 10 ms frame log-energy (dB):
   - skip the first 300 ms (click guard) when computing the span;
   - noise floor N = 10th-percentile frame energy of the take;
   - threshold T = max(N + 12 dB, peak − 30 dB);
   - speech = the region from the first to the last above-T frame,
     bridging gaps ≤ 200 ms, then ±50 ms hangover;
   - reject the take if speech < 150 ms or peak < −40 dBFS.
   The constants above are starting values. Adjust them **only** from
   the tightness audit (5) on DEV takes. Never from any accuracy number.
4. **Re-pad** to exactly 250 ms before and after the span, using the
   take's own audio when it exists. If the clip edge is closer, extend
   with the take's lowest-energy 250 ms segment (not zeros, which break
   CMN). Every canonical take then has identical padding geometry, which
   removes the timing/padding fingerprint by construction.
5. **QC + tightness audit**, written to `qc.tsv`: per take peak dBFS,
   clipping count, estimated SNR (peak − N), span start/end ms, speech
   duration, muted flag, guard-window energy. A fixed predeclared sample
   (every 10th take) gets a PNG energy plot with the span marked
   (`qc/`). Gate for a new endpointer version: on the run1 sample, every
   span starts within 100 ms of audible onset, judged by eye on the plot
   (and ear for 3).
6. **Write** `<out-dir>/take*.wav` (16 kHz mono i16), `manifest.tsv`
   (take, prompt, frames, span_ms, qc_flags), `provenance.tsv`
   (raw sha → canon sha, canon version), `SHA256SUMS`.

Canonical output is what the partition builder (`build_d1_partitions.py`)
registers and what every experiment reads.

## 5. One loader

`corpus::canonical::load_session(dir) -> CanonSession` is the only
function experiments may use to read owner audio. `a1_gate`, the `hmm`
gate tests, `phase_test_scoring_tests`, and any future eval read through
it. A grep test in CI fails on `hound::WavReader` outside `corpus` and
`canon-session`.

## 6. Session catalog

`sessions/CATALOG.tsv`, one row per session ever recorded:
`session_id | list | partition | chain | raw_sha | canon_version | canon_sha | status | consumed_by | notes`.
Status ∈ VALID, INVALID, VOID, RETIRED, QUARANTINED.
The catalog is owner-local working metadata (untracked in public git
since the release scrub); owner checkouts keep it at this path.
`consumed_by` lists result docs that scored it. A PHASE-TEST row with a
non-empty `consumed_by` is REGRESSION from then on.

Initial rows (to fill in from disk): `run1` (VALID, DEV, windows-48k,
legacy layout, grandfathered via canonicalizer), `phase-test-20261031-100240`
(VALID, PHASE-TEST, **wsl-16k**, unconsumed), `-190133` (VOID),
`-20261030-143058` (INVALID), `-182142`, `-183205`, `-20261030-103200`
(classify), `-20261031-092608` (INVALID, deleted).

## 7. Precommit (kept, simplified)

Before the first take the recorder prints
`PRECOMMIT order_sha256=<hex>` (seeded shuffle of the prompt file). The
owner pastes it into the session's bead notes before pressing Enter.
The canonicalizer verifies it. That is the whole ceremony.

## 8. Rules for lanes

- No new recorder, session layout, WAV reader, or resampler. Change the
  existing ones through a bead that cites this document.
- No experiment reads `sessions/raw/`.
- No recording on the WSL chain. If Windows capture is unavailable,
  recording stops; there is no fallback chain.
- A session is never "fixed" after capture. It is re-recorded as a new
  session ID.
