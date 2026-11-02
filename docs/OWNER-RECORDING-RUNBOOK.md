# Owner recording runbook (GO sessions, ~25 min total)

Run on Windows in `C:\Users\micha\repos\hearsay-four-win` with the rebuilt
RS1-a + sealed `record-session.exe` (checklist item 1 must be GREEN).
`$R` below = `.\src\capture\target\debug\record-session.exe`.
If the sitting date slips, rename `owner-20261101-` to the real date.

Each take: read the prompt, press Enter, speak inside the window.
Legacy takes show peak dBFS then `[Enter]=next [r]=redo`.
Sealed takes: single Enter, no redo, keep lead/trail silence; Ctrl-C
abandons the session (a partial session will NOT validate).

## 0. Smoke test first (then STOP and wait)

```powershell
cd C:\Users\micha\repos\hearsay-four-win
$R = ".\src\capture\target\debug\record-session.exe"
& $R --list-devices
& $R --script sessions\prompts-harvard-train-01.txt --out sessions\smoke-win --utterances 5 --dur 5
```

Redo exactly one take with `r` when prompted. Then STOP — report the
session to the coordinator for canonicalization + QC. Record nothing
further until you hear "cleared to record".

## 1–3. Open sessions (legacy, enroll.wav first — keep it)

```powershell
& $R --script sessions\prompts-harvard-train-01.txt --out sessions\owner-20261101-harvard-TRAIN-01 --dur 6
& $R --script sessions\prompts-harvard-dev-01.txt --out sessions\owner-20261101-harvard-DEV-01 --dur 6
& $R --script sessions\prompts-fly-dev-01.txt --out sessions\owner-20261101-fly-DEV-01 --dur 8
```

## 4–6. Phase-test sessions (sealed 16 kHz — copy the PRECOMMIT hash each time, then validate)

```powershell
& $R --script sessions\prompts-read-phase-test-01.txt --out sessions\owner-20261101-read-PHASE-TEST-01 --sealed --rate 16000 --dur 8
& $R --validate sessions\owner-20261101-read-PHASE-TEST-01
& $R --script sessions\prompts-fly-phase-test-01.txt --out sessions\owner-20261101-fly-PHASE-TEST-01 --sealed --rate 16000 --dur 8
& $R --validate sessions\owner-20261101-fly-PHASE-TEST-01
& $R --script sessions\script-wave3.txt --out sessions\owner-20261101-words30-PHASE-TEST-01 --sealed --rate 16000 --dur 3
& $R --validate sessions\owner-20261101-words30-PHASE-TEST-01
```

Words30 is optional and last; skip it if time or voice runs short.

Time budget: smoke 5x5 s + TRAIN 50x6 s + DEV 30x6 s + fly-DEV 15x8 s +
read-PT 50x8 s + fly-PT 15x8 s + words30 30x3 s ≈ 21 min of takes,
≈ 25 min with pacing and validation.
