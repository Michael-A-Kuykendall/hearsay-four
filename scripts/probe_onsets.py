#!/usr/bin/env python3
"""RS1-0 onset-transient probe (bead hearsay-four-mng--rs1-0-click-check).

For takes 0,12,...,108 of sessions/run1 (sorted *.wav, minus enroll.wav): frame RMS
energy at 25ms window / 10ms shift (same framing as fse frontend), dB
plot of the first 400 ms with the replicated current-trimmer span
overlaid (peak-relative threshold max(0.001, 0.01*peak), support radius
2, hangover 5 — see src/fse/src/frontend.rs speech_span_with).

Numeric CLICK rule (no eyeballing): transient_score =
max_dB(frames starting <100 ms) - median_dB(frames 100-400 ms).
CLICK-take if score > 10 dB. Verdict CLICK if >=7/10 takes,
NO-CLICK if <=3/10, else AMBIGUOUS (then an owner listening check on
3 named files settles it — never guess).

Writes docs/results/rs1-0-onsets/onset_*.png + VERDICT.txt.
"""

import sys
import wave
from pathlib import Path

import numpy as np

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

TRIM_K = 0.01
TRIM_FLOOR = 0.001
TRIM_RADIUS = 2
TRIM_HANGOVER = 5
WIN_MS = 25.0
SHIFT_MS = 10.0
IDXS = [0, 12, 24, 36, 48, 60, 72, 84, 96, 108]
CLICK_DB = 10.0


def frame_rms(x: np.ndarray, rate: int):
    wl = int(round(rate * WIN_MS / 1000.0))
    sh = int(round(rate * SHIFT_MS / 1000.0))
    xn = x.astype(np.float64) / 32768.0
    n = (len(xn) - wl) // sh + 1
    e = np.empty(n, dtype=np.float64)
    for i in range(n):
        w = xn[i * sh : i * sh + wl]
        e[i] = float(np.sqrt(np.mean(w * w)))
    return e


def speech_span(energies: np.ndarray):
    n = len(energies)
    if n == 0:
        return None
    peak = float(np.max(energies))
    thr = max(TRIM_FLOOR, TRIM_K * peak)
    idx = []
    for i in range(n):
        if energies[i] <= thr:
            continue
        lo = max(0, i - TRIM_RADIUS)
        hi = min(n - 1, i + TRIM_RADIUS)
        if any(j != i and energies[j] > thr for j in range(lo, hi + 1)):
            idx.append(i)
    if not idx:
        return None
    return (max(0, idx[0] - TRIM_HANGOVER), min(n, idx[-1] + 1 + TRIM_HANGOVER))


def load_mono_i16(path: Path):
    with wave.open(str(path), "rb") as w:
        rate = w.getframerate()
        ch = w.getnchannels()
        sw = w.getsampwidth()
        n = w.getnframes()
        raw = w.readframes(n)
    assert sw == 2, f"{path}: sample width {sw}"
    x = np.frombuffer(raw, dtype=np.int16).reshape(-1, ch).mean(axis=1)
    return x.astype(np.int16), rate


def main() -> int:
    repo = Path(__file__).resolve().parent.parent
    wavs = sorted(
        p
        for p in (repo / "sessions" / "run1").glob("*.wav")
        if p.name != "enroll.wav"
    )
    if len(wavs) <= max(IDXS):
        print(f"FAIL: only {len(wavs)} takes in sessions/run1/audio")
        return 1
    outdir = repo / "docs" / "results" / "rs1-0-onsets"
    outdir.mkdir(parents=True, exist_ok=True)
    rows = []
    for k in IDXS:
        p = wavs[k]
        x, rate = load_mono_i16(p)
        e = frame_rms(x, rate)
        db = 20.0 * np.log10(np.maximum(e, 1e-6))
        t = np.arange(len(e)) * SHIFT_MS
        span = speech_span(e)
        early = t < 100.0
        late = (t >= 100.0) & (t < 400.0)
        score = float(np.max(db[early])) - float(np.median(db[late]))
        span_start_ms = span[0] * SHIFT_MS if span else float("nan")
        rows.append((p.name, score, span_start_ms))
        m400 = t < 400.0
        fig, ax = plt.subplots(figsize=(8, 3))
        ax.plot(t[m400], db[m400], lw=1)
        if span:
            ax.axvspan(
                span[0] * SHIFT_MS,
                (span[1] - 1) * SHIFT_MS + WIN_MS,
                color="g",
                alpha=0.25,
                label="trimmer span",
            )
        ax.axvline(100, color="r", ls="--", lw=1, label="100 ms")
        ax.set_title(f"{p.name} idx={k} score={score:.1f}dB span_start={span_start_ms:.0f}ms")
        ax.set_xlabel("ms")
        ax.set_ylabel("dB")
        ax.legend(fontsize=8)
        fig.tight_layout()
        fig.savefig(outdir / f"onset_{k:03d}_{p.stem}.png", dpi=90)
        plt.close(fig)
    clicks = sum(1 for r in rows if r[1] > CLICK_DB)
    if clicks >= 7:
        verdict = "CLICK"
    elif clicks <= 3:
        verdict = "NO-CLICK"
    else:
        verdict = "AMBIGUOUS"
    with open(outdir / "VERDICT.txt", "w") as f:
        for name, score, ss in rows:
            f.write(f"{name} score_db={score:.2f} span_start_ms={ss:.0f}\n")
        f.write(f"click_takes={clicks}/10\nRS1-0-VERDICT: {verdict}\n")
    print(f"click_takes={clicks}/10 verdict={verdict}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
