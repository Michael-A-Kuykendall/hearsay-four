# X post for the demo clip — draft (lane L)

Post with the demo clip (shot C: `web/demo.sh` → click-a-word why
panel). Two lengths; pick one at post time. [Video URL TBD.]

Frozen paper numbers (from `docs/writeup/WRITEUP.md`) for both variants:
decoder 64.3% TEST-200 + 67.6% dev-clean
(`docs/results/A6-FABRIC-EXPERIMENT.md`,
`docs/results/analysis/HEADLINE-ERROR-ANALYSIS.md`); Fabric 85.5% with
dormancy-vs-pruning delta -0.02 pts [-0.07, 0.00]
(`docs/results/A6-FABRIC-EXPERIMENT.md`); 1599/1600 identical transcripts,
rev_winner 0 — no demonstrated difference; 29/29 cross-channel
(`docs/results/NUMBER-LEDGER.md` §5); 7/16 one-correction fixes
(`docs/demo/correction-demo.md`); CX1 1.07
(`src/fabric/tests/decision_cx1.rs`); fabric RTF 0.08-0.26
(`scripts/a6/out/sweep_insurance.tsv`).

## Short (342 chars — exceeds the 280 post limit, trim before posting)

Hearsay-IV: transcription you can argue with. Fully mechanistic,
non-neural: no neural nets, embeddings, or pretrained models; classical HMM-GMM and n-gram models trained in plain view, offline — and every word carries its why panel: acoustic
score, losing alternatives, LM probability. An error with an address.
Built with Muse Code. [video]

## With numbers

Hearsay-IV: transcription you can argue with. Non-neural (no neural nets, embeddings, or pretrained models; classical HMM-GMM and n-gram models trained in plain view), offline — every word ships with provenance. LibriSpeech:
decoder 64.3% TEST-200 / 67.6% dev-clean, Fabric 85.5%, null result
1599/1600 identical (rev_winner 0), 29/29 cross-channel. Built with
Muse Code. [video]
non-neural · inspectable · offline.

## Alt text for the clip (accessibility)

Screen recording: a terminal runs the Hearsay-IV demo, then a browser
shows transcribed sentences; clicking words opens panels with acoustic
scores, losing alternatives, and language-model probabilities.

## Reply-ready follow-up (if asked "why no neural nets?")

Inspectability. When a neural stack errs, the cause is smeared across
millions of weights. When Hearsay-IV errs, the why panel names the
component — acoustics, LM, or lexicon — so the fix has an aim.
