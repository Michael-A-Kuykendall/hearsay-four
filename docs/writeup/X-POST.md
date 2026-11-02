# X post for the demo clip — draft (lane L)

Post with the demo clip (shot C: `web/demo.sh` → click-a-word why
panel). Two lengths; pick one at post time. [Video URL TBD.]

## Short (233 chars)

Hearsay-IV: transcription you can argue with. Fully mechanistic,
model-free, offline — and every word carries its why panel: acoustic
score, losing alternatives, LM probability. An error with an address.
Built with Muse Code. [video]

## With numbers (262 chars)

Hearsay-IV: transcription you can argue with. Model-free, offline,
zero learned weights — every word ships with provenance. LibriSpeech:
CMVN win 0.6137 vs 0.6416 PER (paired-bootstrap CI), tune-best 0.6624
WER, 29/29 cross-channel. Built with Muse Code. [video]

## Alt text for the clip (accessibility)

Screen recording: a terminal runs the Hearsay-IV demo, then a browser
shows transcribed sentences; clicking words opens panels with acoustic
scores, losing alternatives, and language-model probabilities.

## Reply-ready follow-up (if asked "why no neural nets?")

Inspectability. When a neural stack errs, the cause is smeared across
millions of weights. When Hearsay-IV errs, the why panel names the
component — acoustics, LM, or lexicon — so the fix has an aim.
