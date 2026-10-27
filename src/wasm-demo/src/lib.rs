//! wasm-demo: thin WebAssembly binding over the classical 5-stage pipeline
//! (bead hearsay-four-2sh).
//!
//! Judge-facing demo: PCM 16 kHz mono in, JSON transcript + provenance out
//! over the hearsay-four-9gc schema. Audio decode happens OUTSIDE the
//! module (the page decodes to mono 16k PCM via WebAudio); this crate never
//! touches the network or the filesystem — pure computation over the
//! wasm-safe classical closure (calibration, fse, arena, stage3, fabric,
//! lexicon, sequence, output). Classical path only: no models, no learned
//! weights, no STT text in scoring.
//!
//! The runner/corpus closure (hound, symphonia, reqwest) does not compile
//! to wasm32-unknown-unknown, so this crate carries a minimal PCM struct at
//! the boundary and ports `runner::run_full_pipeline`'s stage logic over
//! the wasm-safe closure instead of depending on `runner`/`corpus`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use calibration::{Baseline, FEATURE_DIM, Frame};
use fabric::{Fabric, FactId};
use fse::PhoneTemplate;
use fse::nbest::Candidate as NbestCandidate;
use lexicon::{CLOSED_VOCABULARY_CMUDICT, CLOSED_VOCABULARY_WORDS, ingest_sample};
use output::{
    ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus,
    JustificationRef, Node, ProvenanceChain, SessionMeta, Transcript, WordRecord,
};
use sequence::BigramTable;
use stage3::window::CommitWindow;

/// Minimal PCM 16 kHz mono input at the WASM boundary.
///
/// Decode stays outside the module (page-side WebAudio): this struct only
/// carries already-decoded frames, so no audio-decode dependency enters
/// the wasm closure.
#[derive(Debug, Clone)]
pub struct PcmMono16k {
    pub sample_rate: u32,
    pub frames: Vec<i16>,
}

impl PcmMono16k {
    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames.len() as f64 / self.sample_rate as f64
    }
}

/// Configuration for [`run_demo_pipeline`]; mirrors
/// `runner::FullPipelineConfig` (one committed word per `chunk_secs`).
#[derive(Debug, Clone)]
pub struct DemoPipelineConfig {
    /// Audio seconds per transcript chunk (one committed word per chunk).
    pub chunk_secs: f64,
    /// Cap on total audio processed, in seconds (0.0 = no cap).
    pub max_secs: f64,
    /// Stage-3 commit-window width per chunk (0 = auto).
    pub window_width: usize,
}

impl Default for DemoPipelineConfig {
    fn default() -> Self {
        Self {
            chunk_secs: 2.0,
            max_secs: 12.0,
            window_width: 0,
        }
    }
}

/// Run stats for [`run_demo_pipeline`]; mirrors `runner::FullPipelineStats`.
#[derive(Debug, Clone)]
pub struct DemoPipelineStats {
    pub frame_count: usize,
    pub chunk_count: usize,
    pub arena_facts: usize,
    pub word_count: usize,
}

/// Validated transcript plus run stats from [`run_demo_pipeline`].
#[derive(Debug, Clone)]
pub struct DemoPipelineOutput {
    pub transcript: Transcript,
    pub stats: DemoPipelineStats,
}

/// Build a "generic reference" baseline: zero-mean, unit-variance per
/// dimension (FEATURE_DIM=39). Same Condition A baseline the synthesis §6
/// specifies ("scored fresh against a generic reference").
pub fn generic_reference_baseline() -> Baseline {
    Baseline {
        mean: vec![0.0; FEATURE_DIM],
        variance: vec![1.0; FEATURE_DIM],
        frame_count: 0,
    }
}

/// Convert one fse N-best candidate for one frame into an arena Candidate,
/// ready for ingest (same type-seam conversion as `runner`).
pub fn fse_nbest_to_arena_candidate(
    nbest: &NbestCandidate,
    frame_features: &Frame,
) -> arena::Candidate {
    let features_f32: Vec<f32> = frame_features.iter().map(|&x| x as f32).collect();
    let feature_signature = arena::quantize(&features_f32, 0.5);
    arena::Candidate::new(nbest.phoneme, Vec::new(), feature_signature)
}

/// Classical Levenshtein distance over phone strings (template matching —
/// counts only, no weights, no learning: a lookup-table alignment, NOT a
/// model).
fn edit_distance(a: &[String], b: &[String]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let sub = usize::from(x != y);
            cur[j + 1] = (prev[j] + sub).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Clamp to a finite [0,1] `f32` for the output contract (`score_in_range`).
fn clamp01(x: f64) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(0.0, 1.0) as f32
}

/// Hash-cons `content` against the shared fabric's map (stages share one
/// fabric because `Fabric` is not `Clone`).
fn intern_hashconsed(
    fabric: &mut Fabric,
    map: &mut HashMap<arena::Content, FactId>,
    content: arena::Content,
) -> FactId {
    if let Some(&existing) = map.get(&content) {
        return existing;
    }
    let id = fabric.intern();
    map.insert(content, id);
    id
}
/// Run the full demo pipeline on already-decoded mono PCM: stages 1→2→3
/// per chunk (window, features, N-best, hash-consed ingest, commit window),
/// then stage 4 (nearest-pronunciation alignment over the closed
/// vocabulary), stage 5 (bigram plausibility over the committed word
/// sequence), and stage 11 (one validated [`WordRecord`] per chunk).
///
/// Stage-for-stage port of `runner::run_full_pipeline` over the wasm-safe
/// closure; the only difference is the minimal [`PcmMono16k`] boundary
/// (decode lives page-side, outside the module).
///
/// Every returned word's provenance chain terminates at frame-level acoustic
/// evidence: frame nodes carry no support (they ARE the grounding), and
/// every higher node reaches a frame node through strictly descending
/// premises (`WordRecord::committed_from_live` enforces this before the
/// record is returned — a violation is an `Err`, never a dangling ref).
pub fn run_demo_pipeline(
    pcm: &PcmMono16k,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
    session: SessionMeta,
    config: &DemoPipelineConfig,
) -> Result<DemoPipelineOutput, String> {
    if pcm.frames.is_empty() {
        return Err("no PCM samples".to_string());
    }
    if pcm.sample_rate < 8_000 {
        return Err(format!("unsupported sample rate {}", pcm.sample_rate));
    }
    if !config.chunk_secs.is_finite() || config.chunk_secs <= 0.0 {
        return Err("chunk_secs must be positive".to_string());
    }
    if codebook.is_empty() {
        return Err("empty codebook".to_string());
    }

    // Stage 4 table: closed-vocabulary lookup (lexicon crate; phone strings
    // only — a lookup table, NOT a model).
    let dict = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
    if dict.is_empty() {
        return Err("empty pronunciation dictionary".to_string());
    }
    let mut vocab: Vec<String> = dict.keys().cloned().collect();
    vocab.sort();

    // Stage 5 table: classical counted bigram over the closed vocabulary
    // (sequence crate; counts only — flat, no LM).
    let mut table = BigramTable::new();
    table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);

    // Truncate to the processing cap (demo scope; the transcript covers the
    // processed span, which the caller names in `session.recording_ref`).
    let mut total_samples = pcm.frames.len();
    if config.max_secs > 0.0 {
        let cap = (pcm.sample_rate as f64 * config.max_secs) as usize;
        total_samples = total_samples.min(cap);
    }
    if total_samples == 0 {
        return Err("no samples within processing cap".to_string());
    }
    let samples = &pcm.frames[..total_samples];
    let chunk_samples = ((pcm.sample_rate as f64 * config.chunk_secs) as usize).max(1);
    let shift_ms = (fse::shift_len(pcm.sample_rate) as u64 * 1000) / pcm.sample_rate as u64;
    let win_ms = (fse::window_len(pcm.sample_rate) as u64 * 1000) / pcm.sample_rate as u64;

    // One fabric shared across all stages (synthesis §4).
    let mut shared_fabric = Fabric::new();
    let mut content_to_fact: HashMap<arena::Content, FactId> = HashMap::new();

    let mut records: Vec<WordRecord> = Vec::new();
    let mut committed_words: Vec<String> = Vec::new();
    let mut total_frames = 0usize;
    let mut chunk_count = 0usize;

    for (chunk_idx, chunk) in samples.chunks(chunk_samples).enumerate() {
        let chunk_start_sample = chunk_idx * chunk_samples;
        let chunk_start_ms = (chunk_start_sample as u64 * 1000) / pcm.sample_rate as u64;
        let chunk_end_ms =
            ((chunk_start_sample + chunk.len()) as u64 * 1000) / pcm.sample_rate as u64;

        // Stages 1-2: window + features, per-frame N-best, hash-consed ingest.
        let windows = fse::window_audio(chunk, pcm.sample_rate);
        if windows.is_empty() {
            continue;
        }
        let frames = fse::extract_features(&windows, pcm.sample_rate)
            .map_err(|e| format!("extract: {e}"))?;
        if frames.is_empty() {
            continue;
        }
        total_frames += frames.len();

        let mut per_frame_facts: Vec<Vec<FactId>> = Vec::with_capacity(frames.len());
        let mut top1: Vec<String> = Vec::with_capacity(frames.len());
        let mut top_scores: Vec<f64> = Vec::with_capacity(frames.len());
        for frame in &frames {
            let scores = fse::score_phones(frame, baseline, codebook);
            let nbest = fse::nbest::emit_nbest(&scores);
            let Some(best) = nbest.first() else {
                continue;
            };
            top1.push(best.phoneme.to_string());
            top_scores.push(best.score);
            let mut facts = Vec::with_capacity(nbest.len());
            for cand in &nbest {
                let arena_cand = fse_nbest_to_arena_candidate(cand, frame);
                let content = arena_cand.content;
                let is_new = !content_to_fact.contains_key(&content);
                let fid = intern_hashconsed(&mut shared_fabric, &mut content_to_fact, content);
                if is_new {
                    shared_fabric.assert_assumption(fid);
                }
                facts.push(fid);
            }
            per_frame_facts.push(facts);
        }
        if per_frame_facts.is_empty() || top1.is_empty() {
            continue;
        }

        // Stage 3: commit window over this chunk's frame facts, sharing the
        // single fabric (swap in/out; `Fabric` is not `Clone`).
        let width = if config.window_width > 0 {
            config.window_width
        } else {
            per_frame_facts.len() * fse::nbest::MAX_N + 64
        };
        let mut window = CommitWindow::new(width);
        std::mem::swap(&mut window.fabric, &mut shared_fabric);
        for (pos, facts) in per_frame_facts.iter().enumerate() {
            let weight = top_scores.get(pos).copied().unwrap_or(0.0);
            for &fid in facts {
                let mut evidence = BTreeSet::new();
                evidence.insert(fid);
                let _ = window.offer(pos, evidence, &[], weight);
            }
        }
        let close_result = window.close();
        std::mem::swap(&mut window.fabric, &mut shared_fabric);
        drop(window);
        // C1 (bead hearsay-four-shq): keep the stage-3 commit winner — its
        // 0–100 rating propagates verbatim into the committed WordRecord
        // below (a move, not a scheduling layer: alpha-indexed-dispatch).
        let (window_winner, _survivors) =
            close_result.map_err(|e| format!("chunk {chunk_idx}: window close: {e:?}"))?;
        // `rating()` asserts finiteness (INV `rating_in_range`); a
        // non-finite commit score degrades to 0.0 here instead, matching the
        // `clamp01` tolerance the confidence path below already uses.
        let source_rating = if window_winner.log_odds.is_finite() {
            window_winner.rating()
        } else {
            0.0
        };

        // Collapse consecutive duplicate top-1 phonemes into a phone sequence
        // with frame spans (run-length dedupe — counting, not learning).
        let mut phones: Vec<String> = Vec::new();
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for (i, ph) in top1.iter().enumerate() {
            if phones.last().is_some_and(|last| last == ph) {
                spans.last_mut().expect("span per phone").1 = i;
            } else {
                phones.push(ph.clone());
                spans.push((i, i));
            }
        }
        if phones.is_empty() {
            continue;
        }

        // Syllable-level facts: phone-only content (empty signature), hence
        // distinct `FactId`s from the frame facts above. Committed so the
        // word justification beneath is stable.
        let mut syl_facts: Vec<FactId> = Vec::with_capacity(phones.len());
        for ph in &phones {
            let content = arena::Content {
                phoneme: ph.clone(),
                context_window: Vec::new(),
                feature_signature: Vec::new(),
            };
            let is_new = !content_to_fact.contains_key(&content);
            let fid = intern_hashconsed(&mut shared_fabric, &mut content_to_fact, content);
            if is_new {
                shared_fabric.assert_assumption(fid);
            }
            shared_fabric.commit(fid);
            syl_facts.push(fid);
        }

        // Stage 4: nearest-pronunciation alignment — classical edit distance
        // against every closed-vocabulary pronunciation (lookup only).
        // Deterministic order: distance ascending, word ascending.
        let mut scored: Vec<(String, usize, f64)> = vocab
            .iter()
            .map(|word| {
                let (dist, sim) = dict[word]
                    .iter()
                    .map(|pron| {
                        let d = edit_distance(&phones, pron);
                        let denom = phones.len().max(pron.len()).max(1) as f64;
                        (d, 1.0 - d as f64 / denom)
                    })
                    .min_by(|a, b| {
                        a.0.cmp(&b.0).then_with(|| {
                            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                        })
                    })
                    .expect("pronunciation per word");
                (word.clone(), dist, sim)
            })
            .collect();
        scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let (winner_word, winner_dist, winner_sim) = scored[0].clone();
        let runners: Vec<(String, f64)> = scored
            .iter()
            .skip(1)
            .take(2)
            .map(|(w, _, s)| (w.clone(), *s))
            .collect();

        // Word fact justified by the syllable facts (`premises ==> word`),
        // mirroring `lexicon::word_match`'s inheritance pattern.
        let premises: BTreeSet<FactId> = syl_facts.iter().copied().collect();
        let word_fact = shared_fabric.intern();
        shared_fabric.assert(word_fact, premises, false);
        shared_fabric.commit(word_fact);

        // Stage 5: bigram plausibility over the committed word sequence;
        // the score fact is justified by the word fact.
        committed_words.push(winner_word.clone());
        let joint = table.sequence_prob(&committed_words);
        let score_fact = shared_fabric.intern();
        let mut seq_premises = BTreeSet::new();
        seq_premises.insert(word_fact);
        shared_fabric.assert(score_fact, seq_premises, false);

        // Stage 11: provenance chain grounded at frame-level acoustic
        // evidence. The frame layer carries NO support (it is the grounding);
        // every higher node reaches a frame node through strictly descending
        // premises.
        let mut seen_frames = BTreeSet::new();
        let mut frame_nodes: Vec<Node> = Vec::new();
        // C1 (bead hearsay-four-shq): frame nodes carry the acoustic
        // rating for their frame (top score mapped to the 0–100 scale).
        // Upper layers derive ratings at or below their premises, so the
        // chain validates monotone non-increasing.
        let mut frame_rating: BTreeMap<u64, f64> = BTreeMap::new();
        for (i, facts) in per_frame_facts.iter().enumerate() {
            let acoustic_rating =
                f64::from(clamp01(top_scores.get(i).copied().unwrap_or(0.0))) * 100.0;
            for fid in facts {
                if seen_frames.insert(*fid) {
                    let start_ms = chunk_start_ms + i as u64 * shift_ms;
                    frame_rating.insert(fid.0, acoustic_rating);
                    frame_nodes.push(Node {
                        id: fid.0,
                        label: format!("FRAME:{start_ms}-{}ms", start_ms + win_ms),
                        support: vec![],
                        active: shared_fabric.is_active(*fid),
                        rating: acoustic_rating,
                    });
                }
            }
        }
        // One phoneme node per distinct phone fact; repeated phones merge
        // spans into a single node so node ids stay unique within the chain.
        let mut phone_span_frames: BTreeMap<FactId, BTreeSet<u64>> = BTreeMap::new();
        let mut phone_order: Vec<(FactId, String)> = Vec::new();
        for (k, fid) in syl_facts.iter().enumerate() {
            let (span_start, span_end) = spans[k];
            if !phone_order.iter().any(|(id, _)| id == fid) {
                phone_order.push((*fid, phones[k].clone()));
            }
            let entry = phone_span_frames.entry(*fid).or_default();
            for facts in per_frame_facts.iter().take(span_end + 1).skip(span_start) {
                for f in facts {
                    entry.insert(f.0);
                }
            }
        }
        // Phoneme ratings sit at the minimum premise (frame) rating, so no
        // phoneme node exceeds the acoustic evidence it derives from.
        let mut phon_rating: BTreeMap<u64, f64> = BTreeMap::new();
        let phon_nodes: Vec<Node> = phone_order
            .iter()
            .enumerate()
            .map(|(k2, (fid, ph))| {
                let premises: Vec<u64> = phone_span_frames[fid].iter().copied().collect();
                let rating = premises
                    .iter()
                    .map(|premise| frame_rating.get(premise).copied().unwrap_or(0.0))
                    .fold(f64::INFINITY, f64::min);
                let rating = if rating.is_finite() { rating } else { 0.0 };
                phon_rating.insert(fid.0, rating);
                Node {
                    id: fid.0,
                    label: format!("/{ph}/"),
                    support: vec![JustificationRef {
                        justification_id: format!("j-phon-{chunk_idx}-{k2}"),
                        premises,
                        unconditional: false,
                    }],
                    active: shared_fabric.is_active(*fid),
                    rating,
                }
            })
            .collect();
        let lex_premises: Vec<u64> = phone_order.iter().map(|(id, _)| id.0).collect();
        // Lexical rating: at most the weakest phoneme premise and the
        // pronunciation-similarity rating, so the word never outranks its
        // evidence.
        let lex_rating = lex_premises
            .iter()
            .map(|premise| phon_rating.get(premise).copied().unwrap_or(0.0))
            .fold(f64::INFINITY, f64::min)
            .min((f64::from(clamp01(winner_sim)) * 100.0).max(0.0));
        let lex_rating = if lex_rating.is_finite() {
            lex_rating
        } else {
            0.0
        };
        let lex_node = Node {
            id: word_fact.0,
            label: format!("WORD:{winner_word}"),
            support: vec![JustificationRef {
                justification_id: format!("j-lex-{chunk_idx}"),
                premises: lex_premises,
                unconditional: false,
            }],
            active: shared_fabric.is_active(word_fact),
            rating: lex_rating,
        };
        // Sequence rating: at most the lexical premise and the bigram
        // plausibility rating.
        let joint_rating = if joint.is_finite() {
            (joint.clamp(0.0, 1.0) * 100.0).max(0.0)
        } else {
            0.0
        };
        let seq_rating = lex_rating.min(joint_rating);
        let seq_node = Node {
            id: score_fact.0,
            label: format!("SEQ:{} p={joint:.4}", committed_words.join(" ")),
            support: vec![JustificationRef {
                justification_id: format!("j-seq-{chunk_idx}"),
                premises: vec![word_fact.0],
                unconditional: false,
            }],
            active: shared_fabric.is_active(score_fact),
            rating: seq_rating,
        };
        let chain = ProvenanceChain::new(vec![
            ChainLayer {
                level: ChainLevel::Frame,
                nodes: frame_nodes,
            },
            ChainLayer {
                level: ChainLevel::Phoneme,
                nodes: phon_nodes,
            },
            ChainLayer {
                level: ChainLevel::Lexical,
                nodes: vec![lex_node],
            },
            ChainLayer {
                level: ChainLevel::Sequence,
                nodes: vec![seq_node],
            },
        ]);

        let acoustic = top_scores.iter().sum::<f64>() / top_scores.len().max(1) as f64;
        let confidence = clamp01(0.5 * acoustic + 0.5 * joint);
        let winner_hyp = Hypothesis::new(
            winner_word.clone(),
            clamp01(0.5 * winner_sim + 0.5 * joint),
            HypothesisStatus::Committed,
        );
        let mut losing: Vec<Hypothesis> = Vec::new();
        for (word, sim) in &runners {
            losing.push(Hypothesis::new(
                word.clone(),
                clamp01(*sim),
                HypothesisStatus::Retracted,
            ));
        }
        let runner_best = runners.first().map(|(_, s)| *s).unwrap_or(0.0);
        let decided_by = Evidence::new(
            EvidenceKind::Lexical,
            format!(
                "nearest-pronunciation alignment: {winner_word} edit distance \
                 {winner_dist} over {} phones; stage-3 commit winner backs the \
                 acoustic span; bigram p={joint:.4}",
                phones.len()
            ),
            clamp01((winner_sim - runner_best + 1.0) / 2.0),
        );
        // C1 (bead hearsay-four-shq): propagate the stage-3 commit
        // winner's rating verbatim, with a per-record evidence summary.
        let evidence_summary = format!(
            "stage-3 commit winner rating {source_rating:.2} over {} frames; \
             {} phones; nearest-pronunciation {winner_word} sim {winner_sim:.4}; \
             bigram p={joint:.4}",
            per_frame_facts.len(),
            phones.len(),
        );
        let record = WordRecord::committed_from_live(
            winner_word.clone(),
            (chunk_start_ms, chunk_end_ms),
            confidence,
            winner_hyp,
            losing,
            decided_by,
            chain,
            source_rating,
            evidence_summary,
        )
        .map_err(|e| format!("chunk {chunk_idx}: provenance contract: {e:?}"))?;
        records.push(record);
        chunk_count += 1;
    }

    if records.is_empty() {
        return Err("no chunks produced words".to_string());
    }
    let stats = DemoPipelineStats {
        frame_count: total_frames,
        chunk_count,
        arena_facts: content_to_fact.len(),
        word_count: records.len(),
    };
    Ok(DemoPipelineOutput {
        transcript: Transcript::new(records, session, Some("A".to_string())),
        stats,
    })
}
/// Serialize a pipeline output to the hearsay-four-9gc JSON schema:
/// `{"transcript": Transcript, "stats": {...}}`, mirroring
/// `hearsay-demo --json`. The contract is enforced before anything is
/// emitted: every word's chain must terminate at frame-level acoustic
/// evidence, so `--json`-equivalent output is as trustworthy as the render.
pub fn output_to_json(out: &DemoPipelineOutput) -> Result<String, String> {
    for word in &out.transcript.words {
        word.validate().map_err(|failure| {
            format!(
                "provenance contract failed for {:?}: {failure:?}",
                word.text
            )
        })?;
    }
    let value = serde_json::json!({
        "transcript": &out.transcript,
        "stats": {
            "frame_count": out.stats.frame_count,
            "chunk_count": out.stats.chunk_count,
            "arena_facts": out.stats.arena_facts,
            "word_count": out.stats.word_count,
        },
    });
    serde_json::to_string_pretty(&value).map_err(|e| format!("JSON serialization failed: {e}"))
}

/// Run the demo pipeline on raw mono samples and return the 9gc-schema JSON
/// (host-callable core; the `#[wasm_bindgen]` export wraps this).
pub fn transcribe_json(
    samples: &[i16],
    sample_rate: u32,
    chunk_secs: f64,
    max_secs: f64,
    recording_ref: &str,
) -> Result<String, String> {
    let pcm = PcmMono16k {
        sample_rate,
        frames: samples.to_vec(),
    };
    let baseline = generic_reference_baseline();
    let codebook = fse::default_codebook();
    let session = SessionMeta {
        speaker_id: "speaker-1".to_string(),
        recording_ref: recording_ref.to_string(),
        started_ms: 0,
    };
    let config = DemoPipelineConfig {
        chunk_secs,
        max_secs,
        window_width: 0,
    };
    let out = run_demo_pipeline(&pcm, &baseline, &codebook, session, &config)?;
    output_to_json(&out)
}

fn error_json(message: impl std::fmt::Display) -> String {
    serde_json::json!({ "error": message.to_string() }).to_string()
}

// ---------------------------------------------------------------------------
// WebAssembly boundary: PCM16k-in / JSON-out.
//
// Audio decode stays page-side (WebAudio `decodeAudioData` + resample to
// mono 16 kHz). The only inbound value is an `Int16Array` of samples; the
// only outbound value is a JSON string. No network, no filesystem.
// ---------------------------------------------------------------------------

use wasm_bindgen::prelude::*;

/// Transcribe mono 16 kHz PCM samples to 9gc-schema JSON.
///
/// `samples`: `Int16Array` of mono samples at `sample_rate` (16_000 for the
/// canonical path; the pipeline accepts >= 8_000). Returns pretty JSON
/// `{"transcript": ..., "stats": ...}` or `{"error": ...}` — never throws.
#[wasm_bindgen]
pub fn transcribe_pcm16k(samples: Vec<i16>, sample_rate: u32) -> String {
    let config = DemoPipelineConfig::default();
    match transcribe_json(
        &samples,
        sample_rate,
        config.chunk_secs,
        config.max_secs,
        "wasm-demo",
    ) {
        Ok(json) => json,
        Err(e) => error_json(e),
    }
}

/// Transcribe with explicit chunk/cap overrides (same JSON contract as
/// [`transcribe_pcm16k`]).
#[wasm_bindgen]
pub fn transcribe_pcm16k_with_config(
    samples: Vec<i16>,
    sample_rate: u32,
    chunk_secs: f64,
    max_secs: f64,
) -> String {
    match transcribe_json(&samples, sample_rate, chunk_secs, max_secs, "wasm-demo") {
        Ok(json) => json,
        Err(e) => error_json(e),
    }
}

/// Demo metadata for the page: closed-vocabulary size, canonical sample
/// rate, and pipeline defaults — so the page's closed-vocabulary notice is
/// data-driven, not a hard-coded claim.
#[wasm_bindgen]
pub fn demo_info() -> String {
    serde_json::json!({
        "sample_rate": 16_000,
        "closed_vocabulary_size": CLOSED_VOCABULARY_WORDS.len(),
        "closed_vocabulary": CLOSED_VOCABULARY_WORDS,
        "default_chunk_secs": DemoPipelineConfig::default().chunk_secs,
        "default_max_secs": DemoPipelineConfig::default().max_secs,
        "model_free": true,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_pcm_16k(seconds: f64, hz: f64) -> PcmMono16k {
        let rate = 16_000u32;
        let n = (rate as f64 * seconds) as usize;
        PcmMono16k {
            sample_rate: rate,
            frames: (0..n)
                .map(|i| {
                    ((2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() * 8000.0)
                        as i16
                })
                .collect(),
        }
    }

    #[test]
    fn pipeline_commits_words_as_9gc_json() {
        let pcm = sine_pcm_16k(2.0, 440.0);
        let json = transcribe_json(&pcm.frames, pcm.sample_rate, 0.5, 2.0, "test")
            .expect("pipeline produces JSON");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("output parses as JSON");
        // 9gc-schema shape: transcript + stats, mirroring hearsay-demo --json.
        let transcript = value.get("transcript").expect("transcript key");
        let stats = value.get("stats").expect("stats key");
        let words = transcript
            .get("words")
            .and_then(|w| w.as_array())
            .expect("words array");
        assert!(!words.is_empty(), "transcript non-empty");
        assert_eq!(
            stats.get("word_count").and_then(|w| w.as_u64()),
            Some(words.len() as u64),
            "stats.word_count matches transcript"
        );
        for key in ["frame_count", "chunk_count", "arena_facts", "word_count"] {
            assert!(stats.get(key).is_some(), "stats carries {key}");
        }
        // Every word re-validates: chain terminates at acoustic evidence.
        for word_json in words {
            let record: WordRecord =
                serde_json::from_value(word_json.clone()).expect("word parses");
            record.validate().expect("chain terminates");
            assert!(!record.losing.is_empty(), "loser recorded");
            assert!(!record.decided_by.description.is_empty());
        }
    }

    #[test]
    fn empty_input_is_an_error_not_silence() {
        let err = transcribe_json(&[], 16_000, 2.0, 12.0, "test").expect_err("empty must fail");
        assert!(err.contains("no PCM samples"), "names the cause: {err}");
        // The WASM export surfaces the same failure as error JSON.
        let out = transcribe_pcm16k(Vec::new(), 16_000);
        let value: serde_json::Value = serde_json::from_str(&out).expect("error JSON parses");
        assert!(value.get("error").is_some(), "error key present");
    }

    #[test]
    fn rejects_low_sample_rate() {
        let pcm = PcmMono16k {
            sample_rate: 4_000,
            frames: vec![0i16; 4_000],
        };
        let err = transcribe_json(&pcm.frames, pcm.sample_rate, 2.0, 12.0, "test")
            .expect_err("low rate must fail");
        assert!(err.contains("unsupported sample rate"), "names the cause: {err}");
    }

    #[test]
    fn demo_info_names_the_closed_vocabulary() {
        let value: serde_json::Value =
            serde_json::from_str(&demo_info()).expect("info parses");
        assert_eq!(
            value
                .get("closed_vocabulary_size")
                .and_then(|v| v.as_u64()),
            Some(CLOSED_VOCABULARY_WORDS.len() as u64)
        );
        assert_eq!(
            value.get("model_free").and_then(|v| v.as_bool()),
            Some(true)
        );
    }
}
