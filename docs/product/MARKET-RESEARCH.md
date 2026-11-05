# Market research: placement for local-first inspectable transcription

Date: 2026-11-05. All factual claims carry a source link + access date. Access date for all web sources below: 2026-11-05.

## Product in one line

Hearsay-IV is a local-first, inspectable speech-to-text engine: classical HMM/mono toolchain with why-panels (inspectable decoding evidence) and undoable corrections. It competes on inspectability and privacy, not on raw accuracy against cloud neural STT.

## Segment sizing (4 tiers, 3+ sized with cited sources)

Market sizing caveat: analyst firms disagree on definitions and totals by 2-5x; figures below are presented as ranges across sources, not point estimates.

### 1. Desktop tier (privacy-sensitive professional transcription)

- The global speech-to-text apps market was valued at USD 5.52B in 2025, projected USD 6.61B in 2026 ([Value Market Research](https://www.valuemarketresearch.com/report/speech-to-text-apps-market), accessed 2026-11-05).
- The worldwide online audio/video transcription services market stood at USD 3.20B in 2025, projected USD 3.78B in 2026 ([PW Consulting](https://pmarketresearch.com/worldwide-online-audio-and-video-transcription-services-market-research/), accessed 2026-11-05).
- The worldwide AI transcription software and service market reached USD 5.47B in 2025 ([PW Consulting](https://pmarketresearch.com/worldwide-ai-transcription-software-and-service-market-research/), accessed 2026-11-05).
- Privacy demand signal: a 2025 consumer survey found 67% of respondents preferred AI assistants processing voice locally rather than transmitting to cloud servers ([MarketIntelo](https://marketintelo.com/report/small-language-model-on-device-inference-market), accessed 2026-11-05); 87% of professionals cite data security as their top concern ([LocalMeetNotes](https://localmeetnotes.com/en/blog/local-vs-cloud-transcription-on-device-ai-future.html), accessed 2026-11-05).
- Target users: journalists, researchers, lawyers, podcasters handling sensitive audio; MacWhisper's core audience (indie local transcription on Mac) demonstrates this segment pays ([MacWhisper review](https://www.getvoibe.com/resources/macwhisper-review/), accessed 2026-11-05).

### 2. CLI tier (developers, pipeline integrators, accessibility hackers)

- The global speech-to-text API market was valued at USD 4.4B in 2025, projected USD 5.28B in 2026 ([Contrive Datum Insights](https://contrivedatuminsights.com/reports/speech-to-text-api-6011), accessed 2026-11-05).
- The broader speech/voice recognition market is put at $19.09B in 2025 rising to $23.70B in 2026 (Fortune Business Insights, via [MacDaddy roundup](https://macdaddy.io/articles/voice-first-mac-workflows/), accessed 2026-11-05).
- Target users: developers embedding offline STT in tools, archival/batch transcription pipelines, self-hosters. This tier overlaps the API tier below but is distinguished by local-execution and no-per-minute-metering.

### 3. Mobile tier (on-device dictation and capture)

- No single analyst figure isolates on-device mobile dictation; proxy: the speech-to-text apps market (USD 6.61B in 2026, cited above) includes mobile apps, and the AI meeting assistants market was $2.789B in 2024 / $3.503B in 2025 ([Market Research Future](https://www.marketresearchfuture.com/reports/ai-meeting-assistants-market-12218), accessed 2026-11-05).
- Target users: field interviewers, clinicians-adjacent note-takers, accessibility users wanting offline dictation. Highest platform cost (iOS/Android ports, on-device model footprint); deferred (see Recommendation).

### 4. API tier (usage-billed cloud or self-hosted endpoint)

- Sized with the CLI tier: speech-to-text API market USD 5.28B in 2026 ([Contrive Datum Insights](https://contrivedatuminsights.com/reports/speech-to-text-api-6011), accessed 2026-11-05).
- Five speech-to-text APIs had a median batch price of $0.003/min in September 2026, range $0.0025-$0.006 ([MeetWave](https://meetwave.io/blog/transcription-pricing-2026), accessed 2026-11-05) — the commodity price floor this tier must beat on something other than price (privacy, inspectability, fixed cost).

## Willingness-to-pay proxies

| Proxy | Price | Source (accessed 2026-11-05) |
|---|---|---|
| MacWhisper Pro, one-time (local Mac transcription) | ~EUR 64-65 (~$69-72) lifetime | [review](https://www.getvoibe.com/resources/macwhisper-review/), [pricing](https://lumevoice.com/blog/macwhisper-review-2026/) |
| Superwhisper Pro (dictation + files) | $8.49/mo, $84.99/yr, $249.99 lifetime | [TranscribeX](https://www.transcribex.io/blog/ai-note-taker-pricing/) |
| Otter Pro (cloud meetings) | $16.99/mo monthly, $8.33/mo annual | [TranscribeX](https://www.transcribex.io/blog/ai-note-taker-pricing/) |
| Rev AI (cloud API) | $0.25/min ($15/hr); human $1.99/min | [iamtypist](https://iamtypist.dev/blog/transcription-service-cost) |
| Deepgram Nova-3 (cloud API) | $0.0043/min ($0.26/hr) | [iamtypist](https://iamtypist.dev/blog/transcription-service-cost) |
| AssemblyAI Universal-2 (cloud API) | $0.0025/min ($0.15/hr) | [iamtypist](https://iamtypist.dev/blog/transcription-service-cost) |

Reading: prosumers pay roughly $70 one-time or $8-17/mo for transcription tools; API usage bills at fractions of a cent per minute. A local-first product captures value via license/subscription (no marginal cloud cost), positioning between MacWhisper one-time (~$70) and Superwhisper/Otter subscriptions (~$85-170/yr). No pricing promise is made here.

## Competitor table

| Competitor | Type | Accuracy claim | Price | Offline? | Source (accessed 2026-11-05) |
|---|---|---|---|---|---|
| Deepgram Nova-3 | Cloud STT API | 5.26% median batch WER (vendor benchmark, 2,703 files); independent voice-agent bench ~8.64% WER | $0.0043/min batch | No | [Eachlabs](https://dev.eachlabs.ai/blog/deepgram-nova-3-things-to-know), [AssemblyAI bench](https://webflow.assemblyai.com/blog/speech-to-text-accuracy-ai-agent) |
| AssemblyAI Universal-3.5/3.6 | Cloud STT API | 5.19% WER on vendor voice-agent bench (12,460 scenarios) | $0.0025-$0.0035/min | No | [AssemblyAI](https://www.assemblyai.com/blog/how-accurate-speech-to-text) |
| Rev AI | Cloud STT API + human | Vendor-claimed high accuracy; human service $1.99/min | $0.25/min AI; $1.99/min human | No | [iamtypist](https://iamtypist.dev/blog/transcription-service-cost) |
| Otter.ai | Cloud meetings app | Not published as WER; meeting-notes product | Free / $8.33-$16.99/mo | No | [TranscribeX](https://www.transcribex.io/blog/ai-note-taker-pricing/) |
| MacWhisper | Local whisper wrapper (Mac) | Whisper-model accuracy, no vendor WER claim | Free / ~EUR 65 one-time | Yes, fully on-device | [GetVoibe](https://www.getvoibe.com/resources/macwhisper-review/) |
| Superwhisper | Local+cloud dictation (Mac/Win/mobile) | No vendor WER claim; local or cloud models | $8.49/mo / $84.99/yr / $249.99 lifetime | Local-model path yes | [Superwhisper](https://superwhisper.com/best-speech-to-text) |

## Differentiation: why-panels + undoable corrections at the ~8-9% classical ceiling

- Hearsay-IV's classical pipeline targets the ~8-9% WER ceiling typical of well-tuned classical/HMM systems; cloud neural APIs claim ~5% WER on vendor benchmarks (see table). We do not compete on raw WER.
- Value carriers instead: (a) why-panels — every transcript segment carries inspectable decoding evidence (lattice/alignment provenance), which no cloud API exposes; (b) undoable corrections — user edits are first-class, reversible operations, not opaque re-decoding; (c) fully local execution — audio never leaves the device, addressing the 67% local-preference and 87% security-concern signals cited above.
- No competitor in the table offers inspectable decoding evidence or correction provenance; local wrappers (MacWhisper, Superwhisper) offer privacy but wrap opaque Whisper models with no why-panel equivalent. This is a synthesis judgment from the sourced feature survey above, not a sourced claim.

## Recommendation (feeds the tier-choice bead)

**Recommended order: Desktop first, CLI second, API third, Mobile last.** Reasons:

1. Desktop first: largest reachable privacy-sensitive segment with proven willingness to pay (~$70 one-time / ~$85+/yr proxies); MacWhisper validates the local-transcription wedge; why-panels are a desktop-native UI surface.
2. CLI second: cheapest to ship from the current engine (it already is a CLI-shaped toolchain), serves developers and batch pipelines, and builds the integrator channel without per-minute cloud economics.
3. API third: the API market is large (USD 5.28B in 2026) but commoditized at ~$0.003/min median; enter only with a differentiated self-hosted/fixed-cost story, not a me-too cloud endpoint.
4. Mobile last: real demand but highest port cost and thinnest local-model story for a classical engine; revisit after desktop traction.

Explicit non-recommendation: do not lead with a cloud API tier competing on price-per-minute against Deepgram/AssemblyAI; the ~5% vendor WER claims and sub-cent pricing make that a losing head-on fight for a classical ~8-9% engine.
