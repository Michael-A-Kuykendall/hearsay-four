# A1 endpoint audit (gh1 predeclared sample, 2026-10-28)

Operating point: raw-domain trim, K 0.01 / floor 0.001 / hangover 5 /
support radius 2.

Sample (fixed, deterministic): `ls sessions/run1/utt*.wav | sort | awk
'NR % 10 == 1'` — 12 clips (utt000, utt010, ..., utt110).

Probe (scratch example `audit_probe`, removed after use):
`cargo run --release -p fse --example audit_probe -- sessions/run1`.
Per clip: raw-sample frame energies with `fse::window_len` /
`fse::shift_len` framing, span from the production `speech_span`.

Criterion: frames within 12 dB of the clip peak (`e > peak /
10^(12/20)`) lying OUTSIDE the span. Verdict CLEAN iff that fraction is
exactly 0; any TRUNCATED would be investigated before close.

| clip | word | n | span | lead ms | trail ms | loud outside/total | frac | verdict |
|---|---|---|---|---|---|---|---|---|
| utt000.wav | congealed | 298 | 0..294 | 0 | 40 | 0/47 | 0.0000 | CLEAN |
| utt010.wav | interpersonal | 298 | 1..180 | 10 | 1180 | 0/58 | 0.0000 | CLEAN |
| utt020.wav | staked | 298 | 2..160 | 20 | 1380 | 0/26 | 0.0000 | CLEAN |
| utt030.wav | enchantment | 298 | 1..160 | 10 | 1380 | 0/55 | 0.0000 | CLEAN |
| utt040.wav | survival | 298 | 0..232 | 0 | 660 | 0/35 | 0.0000 | CLEAN |
| utt050.wav | constitution | 298 | 0..162 | 0 | 1360 | 0/30 | 0.0000 | CLEAN |
| utt060.wav | genetic | 298 | 1..141 | 10 | 1570 | 0/36 | 0.0000 | CLEAN |
| utt070.wav | kinship | 298 | 1..220 | 10 | 780 | 0/33 | 0.0000 | CLEAN |
| utt080.wav | highboy | 298 | 0..276 | 0 | 220 | 0/37 | 0.0000 | CLEAN |
| utt090.wav | survival | 297 | 0..146 | 0 | 1510 | 0/39 | 0.0000 | CLEAN |
| utt100.wav | uncorroborated | 297 | 0..182 | 0 | 1150 | 0/49 | 0.0000 | CLEAN |
| utt110.wav | highboy | 298 | 0..146 | 0 | 1520 | 0/34 | 0.0000 | CLEAN |

Observation (not a finding): utt000 spans 0..294 — speech or a leading
transient within 50 ms of clip start pins the span open. No truncation
either way (0 loud frames outside); the audit checks truncation, not
tightness. Clips with n=297 are the short files (143520 samples).

Verdict: 12/12 CLEAN, zero TRUNCATED.

---

## RS1-0 onset-transient check (2026-10-31, bead hearsay-four-mng--rs1-0-click-check)

Probe: `python3 scripts/probe_onsets.py` (committed). Takes 0,12,...,108
of sessions/run1; 25/10 ms RMS framing; replicated production trimmer
(K 0.01 / floor 0.001 / hangover 5 / radius 2). Plot: first 400 ms dB +
span overlay (docs/results/rs1-0-onsets/onset_*.png, 10 PNGs).
CLICK-take rule: max dB(0-100 ms) minus median dB(100-400 ms) > 10 dB.
Verdict CLICK iff >=7/10 takes.

| take | score dB | span start ms |
|---|---|---|
| utt000.wav | 19.75 | 0 |
| utt012.wav | 12.42 | 0 |
| utt024.wav | 9.89 | 0 |
| utt036.wav | 15.70 | 0 |
| utt048.wav | 22.37 | 0 |
| utt060.wav | 14.21 | 10 |
| utt072.wav | 4.91 | 0 |
| utt084.wav | 10.53 | 0 |
| utt096.wav | 18.97 | 0 |
| utt108.wav | 16.22 | 0 |

8/10 takes show a leading transient 10-22 dB above the following speech
floor, and all 10 spans start at 0-10 ms. The trimmer is pinning open on
an Enter-key/stream-start click, not on speech. No owner listening check
needed (not ambiguous).

RS1-0-VERDICT: CLICK
