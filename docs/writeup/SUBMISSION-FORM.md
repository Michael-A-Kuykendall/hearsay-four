# Submission form fields — drafts (lane L)

One section per field. Bracketed items are owner fills (see
`OWNER-QUESTIONS.md`). Numbers match `WRITEUP.md` §4 exactly.

## Title

Hearsay-IV: Transcription You Can Argue With

## Tagline

A fully mechanistic, model-free, offline speech transcriber — every
word carries its provenance, so every error has an address.

## Problem fit (Build for Your World track)

Dictation errors are silent and sourceless: a wrong word appears and
neither the user nor the developer can ask why. For people who
dictate text they cannot re-read, and for records that must be quoted
exactly — court proceedings, medical notes — that is the product
failing at its one job. Hearsay-IV is built for that world: a
transcription device where every word ships with its reason (acoustic
score, losing alternatives, language-model probability, lexicon
source), so an error points at the component that caused it and the
fix has an aim. It runs fully offline with zero learned weights, so
it works where the cloud doesn't reach and keeps speech on-device.
[Confirm track name "Build for Your World" against the submission
portal — owner.]

## Description

Hearsay-IV re-implements Hearsay-III (1983) with the blackboard
swapped for our patent-pending saturation fabric: a fully
mechanistic, model-free, offline speech transcription device. No
neural nets, no embeddings, no learned weights — enforced by a guard
in the repo (`scripts/no_model_guard.sh`) that fails the build if
any appear. Classical speech science throughout: HMM-GMM acoustic
models, Kneser-Ney n-gram language model, pronunciation lexicons,
DTW baselines.

What we built during the event (Oct 26 – Nov 2): the top of the
stack on a pre-existing fabric core — 9 new crates (20 workspace
members total), the lexer→LM→output chain, the dev-clean WER
harness, the full93 acoustic model (26,727 utterances / 93.9 h /
251 speakers), a one-command demo (`bash web/demo.sh`) with a
click-a-word why panel for every decoded word.

Measured results (LibriSpeech, CC BY 4.0; every value traces to a
file in the repo): controlled CMVN shootout won with phone error
rate 0.6137 vs 0.6416 baseline (paired-bootstrap 95% CI excludes
zero — first CI in the repo); LM tuning best word error rate 0.6624
on a 200-utterance dev-clean subset, beam-adequacy proven at 2x
beam; 30-word cross-channel check 29/29, zero confusions.
[Headline TBD: full dev-clean WER from the Lane A gate run;
fills `data/wer71g/tunefull/fullgate/cell.tsv`.]
[16-mix TBD: overnight swap verdict, night of Nov 2.]

## Meta tooling usage

Built with Muse Code as a multi-lane agent build. All work was
planned as small verifiable tasks (beads) and executed concurrently
by parallel agents: Oct 26 lap closed 15 task beads + 5 epics in
~4.4 h wall time (test suite 204 → 243 green); the Nov 1–2 wave
ran 8 lanes at once — acoustic-model tuning, triphone support,
decoder hardening, file-transcription demo + why panel, adaptation
plumbing, and the number ledger this entry quotes — with 6 of 8
beads merged to main same-day. Verification gates ran on every
lane: the agents discarded bad training runs instead of patching
them, fixed a leaky evaluation instead of quoting its flattering
numbers, and double-checked every word-error count with an
independent second scorer. Meta API credits spent: $0 of $150 as
of Oct 26 (all local compute + Muse subscription; [owner to
confirm current balance]). Humans owned the microphone, the model
verdicts, and the submission; the machine owned the parallelism.

## Links

- Repository: [TBD — public repo URL at submission time; repo goes
  public per plan, patent internals excluded per
  `docs/hackathon/META-HACKATHON.md`]
- Demo video (3 min): [TBD — hosted video URL; script in
  `docs/video/SCRIPT.md`]
- Live demo: [TBD — owner decides: none / WASM page / hosted clip.
  Note: `2sh` WASM demo exists in-repo; live-mic RTF is unmeasured]
- Writeup: [repo path `docs/writeup/WRITEUP.md` at the submission
  tag — fill tag on freeze]
- Event page: https://cerebralvalley.ai/e/meta-ai-developer-hackathon
- Data: LibriSpeech SLR12 via openslr.org (CC BY 4.0)
