# Hearsay-IV: Revisiting the Blackboard with a Saturation Fabric

Nov 2, 2026 · @Mike

## Abstract

Hearsay-IV is a non-neural speech recognizer that revisits the Hearsay-II blackboard and replaces its hypothesis store with a Saturation Fabric: a fact store in which every hypothesis keeps all of its independent justifications, survives the loss of any one, and can go dormant and be revived instead of being pruned for good.

Hearsay-II's hardest problem was not representing competing hypotheses but deciding which to keep. Its authors found that inaccurate ratings made early, opportunistic decisions harmful at the lower levels, and its contemporary HARPY won by delaying pruning until larger partial solutions existed. We argue that a store which never truly discards a supported hypothesis attacks that problem directly, and we specify a decision contract that makes the claim testable: two systems with identical evidence, scoring, commit rule and hypothesis budget, differing only in whether weak hypotheses are discarded or kept dormant.

In ten days we built the full classical stack around the Fabric:

- CMVN-normalized MFCC front end;
- Gaussian-mixture HMM acoustic models trained by Baum-Welch on 93.9 hours of LibriSpeech;
- a Kneser-Ney trigram language model over 29.8 million words;
- a 259,107-entry two-tier pronunciation lexicon;
- a trie decoder;
- per-word provenance, and corrections that undo byte for byte.

On the full LibriSpeech dev-clean set (2,703 utterances, 54,402 words), the system reaches 67.6% word error rate (95% CI 66.9–68.3%) at 0.65× real time. On the owner's 30-word vocabulary, it recognizes 29 of 29 held-out words recorded through a different capture chain.

We then ran the experiment with the Fabric deciding every transcript. On 200 held-out LibriSpeech test utterances, at four hypothesis budgets and two liveness thresholds, dormancy and pruning produced identical transcripts in 1,599 of 1,600 paired runs (largest difference −0.02 points of word error, 95% CI −0.07 to 0.00): no demonstrated difference. No revived hypothesis ever entered a committed answer. The Fabric itself scaled: rebuilt as a true saturation loop, its cost fell from a growth exponent of about 3.8 to 1.07, and it adjudicated lattices of up to 67,000 competing hypotheses per utterance at 0.08–0.26× real time. Its absolute error (85.5%) remains well above the decoder's 64.3%, because its path scoring is not yet equivalent to the decoder's; Section 8 identifies the defect.

## 1. Introduction

Modern dictation is accurate and opaque: when it writes "family" every time its user says "fly", nothing in the system can say why, and nothing the user does fixes it for good. This paper starts from that complaint. It asks whether a recognizer built only from classical, inspectable mathematics can attach a reason to every word, and whether a better hypothesis store can recover the strength that 1970s knowledge-integrating systems gave up.

The Hearsay line is the natural starting point. Hearsay-II integrated independent knowledge sources through a shared blackboard of competing hypotheses, which is exactly the structure a provenance-first recognizer needs. Its weakness was control: which hypotheses to pursue, and which to abandon, when every rating is uncertain. Hearsay-IV keeps the multi-source structure and changes the substrate underneath it.

The contributions are:

1. A historical restatement of the blackboard problem. It is grounded in the primary sources rather than the common story that "the blackboard failed" (Sections 2–3).
2. The Saturation Fabric as a hypothesis store, with multi-support justification, survival, dormancy, revival and cascade retraction, plus a frozen decision contract that turns the core claim into a controlled experiment (Section 4).
3. A complete non-neural recognizer built around it in ten days, with every parameter traceable to the frames it was estimated from (Section 5).
4. An evaluation discipline that caught and voided several of our own results before they could mislead (Sections 6–7).
5. The experiment itself. A pre-registered comparison of dormancy against pruning on held-out speech, its null result, and the measured diagnosis (Section 8).

Throughout, "non-neural" means no neural networks, embeddings, pretrained recognizers or gradient-trained weights anywhere in the product. The system does use classical statistical models: Gaussian mixtures, hidden Markov models and smoothed n-gram counts, all stored in plain view. The software was built with Meta's Muse Code agents running in parallel lanes; nothing those agents wrote runs a neural model.

## 2. Background: Hearsay-II and Hearsay-III

Hearsay-II was a working speech-understanding system, not a failed one. Built at Carnegie Mellon for the ARPA speech program, it recognized connected speech over a 1,011-word vocabulary. On its blind test it was right in meaning on about 90% of sentences: 9% semantic error and 19% sentence error, on a small test set of 23 utterances from one speaker (Erman et al. 1980).

Its architecture had three parts:

- Knowledge sources (KSs): independent experts for segmentation (SEG), phone labeling (POM), word hypothesization (MOW), word sequences (WORD-SEQ), parsing (PARSE), prediction (PREDICT), verification (VERIFY), concatenation (CONCAT), rating policy (RPOL), stopping (STOP) and semantics (SEMANT), plus control KSs.
- The blackboard: a shared, leveled database of hypotheses (segments, syllables, words, word sequences, phrases), each with a time span and a rating. Any KS could read what any other wrote without knowing its author.
- The scheduler: a focus-of-control mechanism that decided which pending KS action to run next.

Control was measured, and it mattered. Opportunistic scheduling gave 29% word error against 48% with no KS ordering, in under half the time. Island-driven search (anchor on confident words, expand outward) gave 33% against 53% for strict left-to-right (Erman et al. 1980, §3.2). The authors' own retrospective credits the uniform, data-directed structure for making the system easy to understand and modify (Lesser & Erman 1977).

The same paper names the limit. Opportunistic processing at the lower levels "was ineffective and harmful", because hypothesis ratings were not accurate enough to focus the search. Even at the word level, MOW's hypotheses contained only about 75% of the spoken words, and a correct word ranked about 4.5th among roughly 20 competitors. HARPY, the CMU system that compiled all knowledge into one precomputed network, took the opposite bet: speed up the search so that more of the space could be examined explicitly, and delay pruning until larger partial solutions existed.

Hearsay-III is a different kind of system. It is "a domain-independent framework for knowledge-based expert systems" and, in its authors' words, "specifically not a speech-understanding system" (Erman, London & Fickas 1981). It generalized the blackboard and added two ideas that matter here. Choice sets represent competing solutions at a single locus. Contexts let an assumed choice be explored in a child context while the alternatives stay available. Scheduling became explicitly programmable by the application writer.

Hearsay-IV therefore inherits two things: Hearsay-II's speech architecture, and Hearsay-III's insight that alternatives should be retained, not discarded, while a choice is explored.

## 3. The blackboard problem, restated

The blackboard's problem is not representation; it is commitment under uncertainty. A blackboard can hold every competing hypothesis. What it cannot afford is to keep acting on all of them, so the scheduler must drop some. When ratings are unreliable, the hypothesis dropped is often the right one, and once dropped it is gone. We separate this into four failure modes, each of which Hearsay-IV addresses with a specific mechanism:

| Failure mode | What happens | Hearsay-IV mechanism |
|---|---|---|
| Premature pruning | A correct hypothesis with a temporarily low rating is discarded, and later evidence that would have supported it arrives too late | Dormancy and revival: weak hypotheses are retained, not discarded, and come back when new support raises them above threshold |
| Single-justification retraction | A hypothesis supported two independent ways is removed when one support is withdrawn | Multi-support survival: a fact is active while any one of its justification sets is active |
| Stale dependents | When an earlier hypothesis changes, conclusions built on it are not revisited | Cascade retraction through a reverse premise index: dependents are re-scored or retracted automatically |
| Lost provenance | The final answer cannot say which evidence produced it, or what it beat | Every committed word keeps its full justification chain, the alternatives it defeated, and the margin |

Two cautions keep this honest. First, Hearsay-II's later designers already moved specialized search out of the blackboard: MOW kept unhypothesized words in its own internal store, and HARPY compiled search into a network. Hearsay-IV follows that lesson. A specialized decoder does the frame-level search, and the Fabric holds only meaningful word-level hypotheses.

Second, retaining more hypotheses is not free. Any claim that dormancy beats pruning has to hold at an equal hypothesis budget. Otherwise the win comes from extra memory, not from the mechanism. Section 4 builds that constraint into the contract.

## 4. The Saturation Fabric

The Saturation Fabric changes one thing about the blackboard: a hypothesis that falls below threshold is kept, with all of its evidence, instead of being thrown away. Everything else follows from treating each hypothesis as a fact with a set of justifications.

Facts and justifications. A fact is active if and only if at least one of its justification sets is active. A justification set is a set of premise facts that jointly support a conclusion. Facts are never overwritten on a second derivation: each new derivation adds a justification. Withdrawing one support therefore leaves a fact standing as long as another independent support remains. A reverse premise index lets activation and retraction touch only the affected part of the graph.

Type facts and token facts. Global knowledge (a word, a pronunciation, an n-gram) is shared. Every acoustic hypothesis is a token tied to its own time span, so the same word said twice in one utterance is two distinct facts that retract independently.

Evidence and scores. Each justification carries a log-likelihood ratio against a declared null, tagged with a source class: ACOUSTIC, LEXICAL, SEQUENCE, SPEAKER, VERIFY or USER. Two scores are kept apart so nothing is counted twice. Commits are decided by the tiling score of a non-overlapping sequence of word tokens:

S(T) = \sum_i \sum_{c \in \text{unary}} w_c \max_{j \in \text{active}_c(t_i)} \text{llr}_j \; + \; \sum_i w_{\text{SEQ}} \, \text{llr}_{\text{SEQ}}(t_{i-1} \to t_i)

Only the strongest active justification per class counts, so many weak supports cannot stack, and language-model evidence enters once per adjacent pair. A separate liveness score decides only whether a token is live or dormant:

L(h) = \text{unary}(h) + \max_p w_{\text{SEQ}} \, \text{llr}_{\text{SEQ}}(p \to h) + \max_s w_{\text{SEQ}} \, \text{llr}_{\text{SEQ}}(h \to s)

Lifecycle.

- Dormancy: a token whose liveness falls below the threshold goes dormant. It keeps every justification but no longer joins tilings or extends.
- Revival: when the decoder later produces a word starting where the dormant token ends, the Fabric attaches that right-context evidence to it, and the token returns to the live set if its liveness recovers.
- Cascade: when a premise token changes, dependent evidence is re-scored through the reverse index.
- Commit: a region commits when the best tiling beats the runner-up by a margin. Losers are retracted with a record of the tiling that defeated them. Ties break deterministically.
- Eviction: budgets cap live and dormant tokens together; over budget, the weakest dormant token is evicted and that eviction is logged.

The accented state is the difference from a conventional decoder: there, a hypothesis that drops below threshold leaves the picture for good. The full rules are frozen in a versioned decision contract (v1.2), implemented in src/fabric and covered by contract tests for survival, token identity, dormant exclusion from tilings, budget enforcement and correction retraction.

## 5. The Hearsay-IV system

Hearsay-IV is a conventional classical recognizer underneath and a provenance system on top. Every component is a model a person can read: means and variances, transition probabilities, counted and discounted n-grams, dictionary entries.

| Component | What it is | Trained or built from | Size |
|---|---|---|---|
| Front end | 25 ms / 10 ms frames; 12 cepstra + log energy, deltas (39 dims); per-utterance mean and variance normalization (CMVN) | fixed signal processing, no fitting | — |
| Acoustic model | 3-state left-to-right monophone HMMs, 8 diagonal-Gaussian mixtures per state, plus silence and short-pause models; flat start, Viterbi alignment, Baum-Welch re-estimation | LibriSpeech train-clean-100, CC BY 4.0 | 93.9 h, 26,727 utterances, 251 speakers |
| Language model | Modified Kneser-Ney trigram, light count cutoffs, linearly interpolated with a personal component | 29.8M words of public-domain book text + the owner's novel (weight 0.25, tuned on DEV) | 892k bigrams, 2.48M trigrams |
| Lexicon | Two tiers: a curated CMUdict-derived dictionary first, LibriSpeech's lexicon filling gaps; every entry tagged with its source | 124,744 + 134,363 entries | 259,107 entries |
| Decoder | Pronunciation trie, frame-synchronous token passing with the language model applied inside the search, beam and live-token caps | — | 22,125-word decode vocabulary |
| Fabric layer | Token facts, decision contract, retractable correction ledger, provenance validation | — | contract v1.2 |
| Why-view and corrections | Per-word panel with plain-English reasons from fixed templates; a correction writes USER, personal-LM and pronunciation-variant facts; undo restores byte-identical state | — | — |

Two properties make this more than a classical recognizer with a log file.

Every parameter has provenance. Each Gaussian stores its sufficient statistics (count, sum, sum of squares) and a pointer to the utterances and frames it was estimated from. Speaker adaptation is specified the same way: contributions are facts that can be retracted, and retracting one subtracts exactly its statistics.

Corrections are facts, not overrides. Correcting "family" to "fly" asserts facts in the Fabric: a USER justification on that token, n-gram count increments in the personal model, and pronunciation-variant evidence where the sounds disagree with the dictionary. Undoing the correction retracts them, and the system returns to a byte-identical state.

For the experiment of Section 8, the Fabric decides the transcript. The decoder emits a top-10 word lattice per utterance; the Fabric ingests its tokens in frame order, applies the lifecycle and budgets, and the transcript is the tiling it commits. Two rebuilds were needed to get there. The first decision layer polled every token on every query and scaled as roughly the fourth power of lattice size; it was rebuilt as a true saturation loop (interned facts, subject indexes, a pending queue with keyed dispatch, incremental liveness) and now scales linearly. The tiling rule was then constrained to follow lattice arcs, and duplicate tokens were interned into single facts.

## 6. Methodology

The evaluation rules were written to stop the experimenter from fooling himself, and they caught several of our own results. Four rules matter most.

Partitions are fixed before scoring. Data is split by recording session for owner audio, by book and speaker for LibriSpeech, and by case for court audio. Five roles keep tuning and testing apart:

| Role | Used for | Rule |
|---|---|---|
| TRAIN | models and counts | anything |
| DEV | all tuning: front end, beam, LM weight, insertion penalty | look as often as needed |
| PHASE-TEST | a one-time gate for a frozen configuration | scored once, then becomes REGRESSION |
| REGRESSION | known-result benchmark | never reported as a test |
| LOCKBOX | final milestone claims | sealed by hash; each opening logged |

One recording standard (RS-1). Owner audio is captured on one Windows chain at the device's native rate. It is resampled once, endpointed by a single noise-floor-relative rule, and re-padded to fixed silence. Every experiment reads it through one loader. The standard exists because early sessions recorded through a second chain produced muted takes and sample overruns, and because a keyboard click at the start of each take was being counted as speech.

Plausibility before significance. A number is accepted only if its training size is stated, its test population matches the claim, and its error rates are below 100%. Comparisons are paired by utterance, with a 95% paired-bootstrap confidence interval. "No demonstrated difference" is the reading whenever the interval includes zero.

Voided results stay on the record. Results that failed these rules were labeled, not deleted:

- An isolated-word test scored 3.3%. Recording lengths had fallen outside the matcher's allowed band, and every comparison returned infinity; the run was declared void, not failed.
- A speaker-adaptation "win" from 180% to 167% phone error was relabeled as a machinery check, since error above 100% is not a recognizer.
- A context-dependent phone probe reported 23% phone error against 59%. It had been scored on its own training audio, with the reference transcript choosing the phone contexts. Both leaks were removed, and the number was voided.
- Isolated-word accuracy sat at exactly 113 of 120 across eight different front-end settings. The cap was the matcher's length band, which made seven takes impossible to score; with the band widened, accuracy was 119 of 120.

## 7. Results

The classical stack works end to end and improves at every step, but it is still far from everyday dictation: 67.6% word error on LibriSpeech dev-clean. The acoustic side is strong on a small vocabulary and is the main limit on open speech.

Each step was a known classical lever. More data alone barely helped: going from 20 to 94 hours moved phone error by half a point, which showed the model was capacity-limited. More Gaussians per state and per-utterance normalization did the work. The word-error figures below come from an earlier checkpoint of the CMVN model (iteration 2 of 4); the final checkpoint has not yet been scored on words.

| Measure | Result | Data |
|---|---|---|
| Word error rate, full dev-clean | 67.6% (95% CI 66.9–68.3%) | 2,703 utterances, 54,402 words |
| Error mix | 78.5% substitutions, 12.6% deletions, 9.0% insertions | same |
| Per-speaker word error | 52.7% to 82.6%, median 67.5% | 40 speakers |
| Speed | 0.65× real time (5.39 h of audio in 3.50 h) | same, 24-core desktop |
| Language-model perplexity | 273 with the novel interpolated, 346 general-only | dev-clean transcripts, OOV excluded |
| Lexicon coverage | training OOV tokens 1.53% → 0.23%; usable training audio 61.4 → 93.9 h | train-clean-100 |
| 30 words, within one session | 119 of 120 | owner, leave-one-take-out |
| 30 words, held out, different capture chain | 29 of 29 (exact 95% interval 88.1–100%) | owner, one sealed session |
| One-correction fixes | 7 of 16 single-word errors fixed, 3 partially, 6 not; undo byte-identical every time | dev-clean |

Where the errors are. The top 20 confusions are all function words: "the" heard as "a" (174 times), "a" as "the" (103), "and" as "then" (78). Short, unstressed words are where monophone models are weakest. Context-dependent phone models are the classical fix, and the next step in Section 10.

What a correction does. In the demo, correcting "trifle odds" to "trifle odd" raised the probability of the corrected phrase from 2.3 × 10⁻⁵ to 7.5 × 10⁻² in the personal model, and the re-decode produced the reference. In three of the sixteen cases, the same correction fixed the target word but shifted an unrelated word elsewhere in the sentence. The demo's collateral check exists to catch exactly that side effect, and it reports every one.

## 8. The experiment: does dormancy beat pruning?

Dormancy did not beat pruning. On 200 held-out test utterances, arms B and C produced identical transcripts in 1,599 of 1,600 paired runs. The experiment ran as designed, with the Fabric deciding every word; this section gives the design, the result and the measured reasons.

Design. Three systems read the same saved lattice of timed word candidates:

| | A: acoustic 1-best | B: conventional | C: Hearsay-IV |
|---|---|---|---|
| Evidence | acoustic only | all classes | all classes |
| Score function | acoustic term | identical | identical |
| Commit rule | identical | identical | identical |
| Below threshold | discarded | discarded for good (beam prune) | dormant, revivable |
| Earlier word changes | — | not revisited | cascade re-score |
| Budget counted | live | live | live + dormant |

B and C get identical evidence, scoring, commit rule and total hypothesis budget, so any difference comes from the lifecycle alone. Budgets are swept at 50, 100, 250 and 500 hypotheses. Each point reports word error, real-time factor, peak live and dormant counts, and late rescues: words C commits that B had already pruned. Readings were fixed in advance:

- C beats B at equal budget (paired bootstrap interval below zero), with late rescues present: the architectural result.
- C matches B with a smaller budget: an efficiency result.
- No demonstrated difference: the Fabric adds provenance and retractable corrections, not accuracy.

Result. TEST-200 is 200 LibriSpeech test-clean utterances, 5 per speaker from 40 speakers (4,530 words). Lattices were generated once and scored once. Parameters were frozen on a separate DEV-200 set. The liveness threshold θ was set at the 50th and 25th percentiles of DEV liveness; B and C always shared it.

| Budget | A: decoder 1-best | B: prune, θ p50 | C: dormant, θ p50 | B, θ p25 | C, θ p25 | ΔWER C−B (95% CI) |
|---|---|---|---|---|---|---|
| 50 | 64.30% | 94.55% | 94.55% | 94.55% | 94.55% | 0.00 [0, 0] |
| 100 | 64.30% | 93.71% | 93.71% | 93.71% | 93.71% | 0.00 [0, 0] |
| 250 | 64.30% | 91.02% | 91.02% | 91.02% | 91.02% | 0.00 [0, 0] |
| 500 | 64.30% | 85.50% | 85.50% | 85.50% | 85.47% | −0.02 [−0.07, 0.00] at p25 |

A third threshold (75th percentile) was stopped after 80 of 1,600 runs when its workers stalled; it is excluded, per the pre-registered rule of reporting complete rows only. The single differing transcript, at budget 500 and θ p25, removed one insertion. It came from different pressure on the live set, not from a revival.

Why B and C tie. Two diagnostics were logged in every run of arm C, and both were zero in all 24 cells:

- Revivals that entered a committed tiling: 0. Tokens did go dormant (peaks of 17–64 per utterance), but no revived token ever won a commit.
- Dormant tokens on the oracle path at commit time: 0. The correct words that the lattice contained were not the ones being held dormant, so there was nothing for revival to rescue.

The lifecycle ran; it was not the lever. Budgets did bind, with thousands of evictions per utterance and errors shifting from deletions at budget 50 toward substitutions at 500, but both arms lost the same tokens to them.

Why the Fabric trails the decoder. At unlimited budget, the Fabric's transcript should match the lattice's own best path (68.1% error on DEV) if its path score equals the decoder's. It does not: 84.7%, with 5 of 200 utterances identical. Three scoring defects were found and measured:

- an uppercase/lowercase mismatch had zeroed all language-model evidence (fixed);
- tilings were not constrained to lattice arcs (fixed in contract v1.4);
- the SEQUENCE evidence, defined against a unigram null, gives every extra word a bonus of about −log P(w). The corrective per-word term never engaged, because the lexical hook is missing from the scorer. Open.

What the result shows, and what it does not.

- It shows the Saturation Fabric can hold and adjudicate real-speech hypothesis sets at scale: lattices of 3.4 million tokens over 200 utterances, up to 67,000 per utterance, at 0.08–0.26× real time, faster than the decoder that produced them.
- It shows that, under the current scoring, keeping weak hypotheses dormant gives no measurable advantage over pruning them at equal budget.
- It does not show that dormancy cannot help. Every condition was run with path scoring known to be non-equivalent to the decoder's, and at two of the three planned thresholds.

What would change it. First, finish the scoring equivalence: wire the unigram cancel through the scorer and pass the exact sanity gate (the Fabric reproduces the lattice's best path at unlimited budget). Then rerun the same frozen comparison at the 75th-percentile threshold, where most tokens sit near the live/dormant line, with the stall fixed. If C still equals B there, dormancy is not the lever for speech at this acoustic quality. If C beats B, that is the Hearsay-IV result. Either way, the measurement will be clean.

## 9. Lessons and threats to validity

The biggest lesson is that measurement failed more often than modeling did. In ten days, the recording chain, the endpointer, a length band and two evaluation leaks each produced numbers that looked like findings and weren't. Each was caught by a rule written before the result arrived.

Lessons.

- Plumbing can pass every test while the science is absent. Before the event, more than 300 unit tests passed on a pipeline whose phone templates were arithmetic placeholders. Accuracy on held-out audio is the only gate that measures recognition.
- Treat the capture chain as a variable. Two recording paths for the same speaker and microphone produced different failures; one standard removed them.
- When more data doesn't help, suspect capacity. Going from 20 to 94 hours changed phone error by 0.5 points; adding mixtures and normalization changed it by 5.7.
- A parallel agent build needs a reviewer seat. Seven Muse Code lanes produced the stack quickly. Most defects that reached the record came from judgment, not code: a gate that measured the wrong thing, a recorder rewritten instead of reused, a number accepted without a plausibility check. A separate architect and reviewer role caught most of them.

Threats to validity.

- Single test set. Word error is reported on LibriSpeech dev-clean, the same set used to tune the language-model weight and insertion penalty on a 200-utterance subset. The full-set figure is therefore slightly optimistic. test-clean is registered as the untouched benchmark and has not been scored.
- Small owner samples. The 29/29 result is one session of 29 isolated words; its exact 95% interval reaches down to 88%.
- Synthetic connected speech. The connected-word results were built by joining isolated recordings. Joined clips lack coarticulation, which flatters any decoder.
- Personal language model. The owner's novel is in the language model. It was checked against every evaluation prompt for verbatim overlap (none found), but any owner-dictation test of prose like the novel will benefit from it, and should be reported separately.
- Monophone ceiling. Every result here uses context-independent phone models. Conclusions about the decoder or the Fabric drawn from this error level may not transfer to a stronger acoustic model.

## 10. Conclusion and future work

Hearsay-IV rebuilt Hearsay-II's architecture on a Saturation Fabric and tested its central claim on real speech. The Fabric works as a substrate: it holds tens of thousands of competing hypotheses per utterance with full provenance, decides every word, and runs faster than real time. The claim that dormancy beats pruning was not supported in this first test: no demonstrated difference, with every reason measured.

What stands today:

- A working non-neural recognizer, trained on 94 hours of public speech: 67.6% word error on LibriSpeech dev-clean and 64.3% on the held-out TEST-200 set. Every parameter traces to the frames it came from.
- A Saturation Fabric decision layer that scales linearly (growth exponent 1.07, down from about 3.8), matches the original contract decision for decision on 500 randomized sequences, and adjudicates 67,000-hypothesis lattices at 0.08–0.26× real time.
- A pre-registered dormancy-versus-pruning experiment, run once on held-out data: identical transcripts in 1,599 of 1,600 paired runs.
- Near-perfect discrimination of the owner's 30-word vocabulary, held out and across capture chains (29 of 29).
- Corrections stored as facts: one correction fixed 7 of 16 single-word errors, and every undo restored a byte-identical state.

Next steps, in order of expected payoff:

1. Scoring equivalence. Wire the unigram cancel through the scorer and pass the exact sanity gate: at unlimited budget, the Fabric reproduces the lattice's best path.
2. The rerun. The same frozen comparison at the 75th-percentile threshold, with the worker stall fixed, so tokens actually sit near the live/dormant line.
3. Context-dependent phones. Word-internal triphones with decision-tree tying. The largest classical accuracy lever left; the one-hour probe was killed before producing a leak-free number.
4. The substrate comparison. The same lifecycle on the Fabric versus a plain hypothesis store: retraction cost, memory, real-time factor, provenance overhead.
5. Owner adaptation and the "fly" benchmark, from about 25 minutes of the owner's read speech.

Hearsay-II's authors concluded that their ratings were not reliable enough to commit early. Hearsay-IV committed late, kept every reason, and measured the result. In this first test, committing late did not change the answer, and the Fabric made it possible to say exactly why: at commit time, the right words were not among those waiting in reserve.

## References

1. Erman, L. D., Hayes-Roth, F., Lesser, V. R., & Reddy, D. R. (1980). The Hearsay-II speech-understanding system: Integrating knowledge to resolve uncertainty. ACM Computing Surveys, 12(2).
2. Lesser, V. R., & Erman, L. D. (1977). A retrospective view of the Hearsay-II architecture. Proceedings of IJCAI-77, 790–799.
3. Erman, L. D., London, P. E., & Fickas, S. F. (1981). The design and an example use of Hearsay-III. Proceedings of IJCAI-81.
4. Panayotov, V., Chen, G., Povey, D., & Khudanpur, S. (2015). LibriSpeech: an ASR corpus based on public domain audio books. ICASSP 2015. Audio and transcripts CC BY 4.0; LM text and lexicon from OpenSLR 11.
5. Hearsay-IV repository: decision contract docs/design/fabric-decision-contract.md (v1.2), recording standard docs/design/recording-standard-rs1.md, number ledger docs/results/NUMBER-LEDGER.md, error analysis docs/results/analysis/HEADLINE-ERROR-ANALYSIS.md, oracle verdict docs/results/A5-LATTICE-ORACLE.md, correction demo docs/demo/correction-demo.md.

Pre-existing work: the Saturation Fabric fact-store core (multi-support justification and survival) dates from August 2026 and is the author's own (patent pending). Everything else described here was built during the hackathon, October 26 to November 4, 2026.
