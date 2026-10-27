//! Composition runner: wires the existing stage crates into one pipeline on
//! real audio. Recovers the fse→arena→stage3 plumbing that the synthesis §4
//! architected but was never built (synthesis §4.2 emits N-best, §4.3 enters
//! as hash-consed arena facts, §4.4 competes in the commit window).
//!
//! Implementation note: the synthesis §4 describes a single saturation
//! fabric engine shared across stages. The current implementation gives each
//! stage its own Fabric instance, so arena facts aren't active in the
//! stage3 CommitWindow's fabric. This runner bridges that by doing the
//! hash-consing ingest in-place (replicating arena::Arena's job) against a
//! single shared fabric, and handing those FactIds to the commit window.
//! Stages 4 (lexicon), 5 (sequence), and 11 (output) are composed in
//! [`run_full_pipeline`] (bead hearsay-four-5qc); [`run_pipeline`] covers
//! stages 1→2→3 per the fail-fast framing in synthesis §6.

use std::collections::HashMap;

use arena::Candidate as ArenaCandidate;
use arena::quantize;
use calibration::{Baseline, FEATURE_DIM, Frame};
use corpus::audio::Pcm16kAudio;
use fabric::{Fabric, FactId};
use fse::PhoneTemplate;
use fse::nbest::Candidate as NbestCandidate;
use stage3::window::CommitWindow;

/// What the pipeline produced: committed winner fact + the arena fact that
/// was its origin (so a reviewer can trace it back through the fabric).
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub winner_fact: FactId,
    pub arena_fact: FactId,
    pub frame_count: usize,
    pub arena_facts: usize,
    pub candidates_offered: usize,
    pub winner_score: f64,
    pub winner_phoneme: String,
}

/// Build a "generic reference" baseline: zero-mean, unit-variance per
/// dimension (FEATURE_DIM=39). This is the Condition A baseline the
/// synthesis §6 specifies ("scored fresh against a generic reference").
pub fn generic_reference_baseline() -> Baseline {
    Baseline {
        mean: vec![0.0; FEATURE_DIM],
        variance: vec![1.0; FEATURE_DIM],
        frame_count: 0,
    }
}

/// Convert one fse N-best candidate for one frame into an arena Candidate,
/// ready for ingest. The feature_signature is the frame's continuous
/// features quantized via arena::quantize (epsilon=0.5 — coarse bins
/// adequate for the composition test, not a linguistic tuning). The
/// context_window is left empty: this runner is a composition test (do
/// stages hand off), not a phonological-context test. See synthesis
/// §4.3 for where real context would come in.
pub fn fse_nbest_to_arena_candidate(
    nbest: &NbestCandidate,
    frame_features: &Frame,
) -> ArenaCandidate {
    let features_f32: Vec<f32> = frame_features.iter().map(|&x| x as f32).collect();
    let feature_signature = quantize(&features_f32, 0.5);
    ArenaCandidate::new(nbest.phoneme, Vec::new(), feature_signature)
}

/// Run the composition pipeline on raw PCM samples with a known baseline
/// and codebook. This is the core wiring (§4.2→§4.3→§4.4). The window
/// width is set generously to hold all N-best candidates across all frames.
pub fn run_pipeline(
    pcm: &Pcm16kAudio,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
    window_width: usize,
) -> Result<PipelineResult, String> {
    if pcm.frames.is_empty() {
        return Err("no PCM samples".to_string());
    }

    // Stage 1: window + features.
    let windows = fse::window_audio(&pcm.frames, pcm.sample_rate);
    if windows.is_empty() {
        return Err("no windows from window_audio (clip too short)".to_string());
    }
    let frames = fse::extract_features(&windows, pcm.sample_rate)
        .map_err(|e| format!("extract_features: {e}"))?;
    let frame_count = frames.len();
    if frame_count == 0 {
        return Err("zero frames after extract".to_string());
    }

    // Stage 2: hash-consed ingest against a SHARED fabric so the stage3
    // CommitWindow (which we construct with the same fabric) can use the
    // arena facts as active local_evidence. We replicate arena::Arena's
    // hash-consing map here in the runner (content -> FactId) because
    // Arena::new() creates its own Fabric and Fabric isn't Clone — we
    // need the window to share this one.
    let mut shared_fabric = Fabric::new();
    let mut content_to_fact: HashMap<arena::Content, FactId> = HashMap::new();
    let mut per_frame_arena_facts: Vec<Vec<FactId>> = Vec::with_capacity(frame_count);
    let mut per_frame_top_score: Vec<f64> = Vec::with_capacity(frame_count);
    let mut per_frame_top_phoneme: Vec<String> = Vec::with_capacity(frame_count);
    let mut arena_facts_total = 0usize;

    for frame in &frames {
        let scores = fse::score_phones(frame, baseline, codebook);
        let nbest = fse::nbest::emit_nbest(&scores);
        let top = nbest.first().copied();
        if let Some(t) = top {
            per_frame_top_score.push(t.score);
            per_frame_top_phoneme.push(t.phoneme.to_string());
        } else {
            per_frame_top_score.push(0.0);
            per_frame_top_phoneme.push(String::new());
        }
        let mut facts_this_frame = Vec::with_capacity(nbest.len());
        for cand in &nbest {
            let arena_cand = fse_nbest_to_arena_candidate(cand, frame);
            // Hash-cons: identical content resolves to the same FactId.
            let fid = if let Some(&existing) = content_to_fact.get(&arena_cand.content) {
                existing
            } else {
                let id = shared_fabric.intern();
                content_to_fact.insert(arena_cand.content.clone(), id);
                id
            };
            // Assert as assumption so the fact is active for the commit window.
            shared_fabric.assert_assumption(fid);
            facts_this_frame.push(fid);
            arena_facts_total += 1;
        }
        per_frame_arena_facts.push(facts_this_frame);
    }

    // Stage 3: commit window sharing the same fabric.
    let mut window = CommitWindow::new(window_width);
    window.fabric = shared_fabric;
    let mut candidates_offered = 0usize;
    for (pos, facts) in per_frame_arena_facts.iter().enumerate() {
        let weight = per_frame_top_score.get(pos).copied().unwrap_or(0.0);
        for &fid in facts {
            if window
                .offer(pos, std::iter::once(fid).collect(), &[], weight)
                .is_ok()
            {
                candidates_offered += 1;
            }
        }
    }

    // Close the window: commit winner, retract losers.
    let (winner, _retracted) = window.close().map_err(|e| format!("window close: {e}"))?;
    let winner_fact = winner.fact;

    let winner_pos = winner.position;
    let arena_fact = per_frame_arena_facts
        .get(winner_pos)
        .and_then(|facts| facts.first().copied())
        .ok_or_else(|| "winner position has no arena facts".to_string())?;
    let winner_phoneme = per_frame_top_phoneme
        .get(winner_pos)
        .cloned()
        .unwrap_or_default();
    let winner_score = per_frame_top_score.get(winner_pos).copied().unwrap_or(0.0);

    Ok(PipelineResult {
        winner_fact,
        arena_fact,
        frame_count,
        arena_facts: arena_facts_total,
        candidates_offered,
        winner_score,
        winner_phoneme,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_pcm_16k(seconds: f64, hz: f64) -> Pcm16kAudio {
        let rate = 16_000u32;
        let n = (rate as f64 * seconds) as usize;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                ((2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() * 8000.0) as i16
            })
            .collect();
        Pcm16kAudio {
            sample_rate: rate,
            channels: 1,
            frames: samples,
        }
    }

    /// aem.3.2 gate: the type-seam conversion produces an arena Candidate
    /// with the right phoneme and a non-empty feature_signature.
    #[test]
    fn conversion_maps_phoneme_and_quantizes_features() {
        let frame: Frame = [0.5; FEATURE_DIM];
        let nbest = NbestCandidate {
            phoneme: "AA",
            score: 0.9,
        };
        let cand = fse_nbest_to_arena_candidate(&nbest, &frame);
        assert_eq!(cand.content.phoneme, "AA");
        assert!(cand.content.context_window.is_empty());
        assert!(
            !cand.content.feature_signature.is_empty(),
            "feature_signature must be non-empty (got {})",
            cand.content.feature_signature.len()
        );
    }

    /// aem.3.3 / aem.3.4 gate: the pipeline runs end-to-end on synthetic
    /// PCM and produces a committed winner with provenance (aem.3.3 = the
    /// runner exists and runs; aem.3.4 = real-audio composition test).
    #[test]
    fn pipeline_runs_on_synthetic_pcm() {
        let pcm = sine_pcm_16k(0.5, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let result = run_pipeline(&pcm, &baseline, &codebook, 500).expect("pipeline");
        assert!(result.frame_count > 0, "frames produced");
        assert!(result.arena_facts > 0, "arena facts ingested");
        assert!(
            result.candidates_offered > 0,
            "candidates offered to window"
        );
        assert!(!result.winner_phoneme.is_empty(), "winner phoneme resolved");
    }

    /// aem.3.4 gate: the pipeline runs on REAL audio (the fail-fast session
    /// MP3) when the file is present. Skips (returns Ok early) when absent
    /// so CI without the 14MB session audio still passes.
    #[test]
    fn pipeline_runs_on_real_session_audio() {
        let real_audio =
            std::path::Path::new("../../data/session/audio/24-889_20260429-argument.mp3");
        if !real_audio.exists() {
            eprintln!("SKIP: real session audio not present at {real_audio:?}");
            return;
        }
        let full = corpus::audio::decode_to_pcm16k(real_audio).expect("decode real MP3");
        // Truncate to first 2 seconds — the full 14-min clip is too heavy
        // for a test (millions of windows). 2s is enough to prove the
        // composition plumbing on real audio.
        let limit = (full.sample_rate as usize) * 2;
        let frames: Vec<i16> = full.frames.into_iter().take(limit).collect();
        let pcm = Pcm16kAudio {
            sample_rate: full.sample_rate,
            channels: full.channels,
            frames,
        };
        assert!(!pcm.frames.is_empty(), "decoded samples");
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let result = run_pipeline(&pcm, &baseline, &codebook, 2000).expect("real-audio pipeline");
        assert!(result.frame_count > 0, "real frames");
        assert!(result.arena_facts > 0, "real arena facts");
        assert!(result.candidates_offered > 0, "real candidates offered");
    }
}

// ---------------------------------------------------------------------------
// Full 5-stage pipeline: stages 4 (lexicon) + 5 (sequence) + 11 (output)
// (bead hearsay-four-5qc).
//
// `run_pipeline` (above) covers stages 1→2→3. This section composes the
// remaining stages on real audio: per-chunk stage-3 commits feed a
// nearest-pronunciation alignment against the closed-vocabulary lookup
// table (stage 4, classical edit distance — counts only, NOT a model),
// bigram plausibility over the committed word sequence (stage 5), and
// per-word [`output::WordRecord`]s whose provenance chains bottom out at
// frame-level acoustic evidence (stage 11).
//
// One committed word per audio chunk: chunking keeps a multi-minute
// argument tractable while every word stays grounded in its own chunk's
// frames. No STT text, no embeddings, no learned weights enter scoring;
// the guard (`scripts/no_model_guard.sh`) stays green by construction.

use std::collections::{BTreeMap, BTreeSet};

use lexicon::{CLOSED_VOCABULARY_CMUDICT, CLOSED_VOCABULARY_WORDS, ingest_sample};
use output::{
    ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus, JustificationRef,
    Node, ProvenanceChain, SessionMeta, Transcript, WordRecord,
};
use sequence::BigramTable;
use sequence::trigram::TrigramTable;
use sequence::verify::{DEFAULT_VERIFY_THRESHOLD, verify_committed_word_sequence};

/// Configuration for [`run_full_pipeline`].
#[derive(Debug, Clone)]
pub struct FullPipelineConfig {
    /// Audio seconds per transcript chunk (one committed word per chunk).
    pub chunk_secs: f64,
    /// Cap on total audio processed, in seconds (0.0 = no cap).
    pub max_secs: f64,
    /// Stage-3 commit-window width per chunk (0 = auto: 5 N-best × frames + 64).
    pub window_width: usize,
}

impl Default for FullPipelineConfig {
    fn default() -> Self {
        Self {
            chunk_secs: 2.0,
            max_secs: 12.0,
            window_width: 0,
        }
    }
}

/// Run stats for [`run_full_pipeline`].
#[derive(Debug, Clone)]
pub struct FullPipelineStats {
    pub frame_count: usize,
    pub chunk_count: usize,
    pub arena_facts: usize,
    pub word_count: usize,
}

/// Validated transcript plus run stats from [`run_full_pipeline`].
#[derive(Debug, Clone)]
pub struct FullPipelineOutput {
    pub transcript: Transcript,
    pub stats: FullPipelineStats,
}

/// Thin wrapper over [`output::edit_distance`] (bead hearsay-four-chd):
/// the canonical Levenshtein core lives in `output`; this keeps
/// [`run_full_pipeline`] behavior unchanged (counts only, NOT a model).
pub fn edit_distance(a: &[String], b: &[String]) -> usize {
    output::edit_distance(a, b)
}

/// Clamp to a finite [0,1] `f32` for the output contract (`score_in_range`).
fn clamp01(x: f64) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(0.0, 1.0) as f32
}

/// Hash-cons `content` against the shared fabric's map (cf. `run_pipeline`'s
/// bridging note: stages share one fabric because `Fabric` is not `Clone`).
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

/// Run the full pipeline on raw PCM: stages 1→2→3 per chunk (window,
/// features, N-best, hash-consed ingest, commit window), then stage 4
/// (nearest-pronunciation alignment over the closed vocabulary), stage 5
/// (bigram plausibility over the committed word sequence), and stage 11
/// (one validated [`WordRecord`] per chunk).
///
/// Every returned word's provenance chain terminates at frame-level acoustic
/// evidence: frame nodes carry no support (they ARE the grounding), and
/// every higher node reaches a frame node through strictly descending
/// premises (`WordRecord::committed_from_live` enforces this before the
/// record is returned — a violation is an `Err`, never a dangling ref).
pub fn run_full_pipeline(
    pcm: &Pcm16kAudio,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
    session: SessionMeta,
    config: &FullPipelineConfig,
) -> Result<FullPipelineOutput, String> {
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

    // C2 (bead hearsay-four-ea3): upper-level verification KS — a counted
    // trigram table over the same closed vocabulary (uniform fallback, no
    // evidence against any sequence until trained, mirroring the bigram
    // table above).
    let mut verify_table = TrigramTable::new();
    verify_table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);

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
    let mut committed_word_facts: Vec<FactId> = Vec::new();
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

        // Word fact justified by the syllable facts (`premises ⟹ word`),
        // mirroring `lexicon::word_match`'s inheritance pattern.
        let premises: BTreeSet<FactId> = syl_facts.iter().copied().collect();
        let word_fact = shared_fabric.intern();
        shared_fabric.assert(word_fact, premises, false);
        shared_fabric.commit(word_fact);

        // C2 (bead hearsay-four-ea3): verification hook — after the
        // lexicon align above, before sequence scoring. The committed
        // word sequence is checked against trigram plausibility with the
        // veto/flag threshold. Flagged sequences are marked, never
        // dropped: the record below keeps full provenance.
        committed_words.push(winner_word.clone());
        committed_word_facts.push(word_fact);
        let verify_note = verify_committed_word_sequence(
            &mut shared_fabric,
            &committed_word_facts,
            &committed_words,
            &verify_table,
            DEFAULT_VERIFY_THRESHOLD,
        )
        .map(|verdict| verdict.note())
        .unwrap_or_else(|| "VERIFY-SKIP no committed word facts".to_string());

        // Stage 5: bigram plausibility over the committed word sequence;
        // the score fact is justified by the word fact.
        let joint = table.sequence_prob(&committed_words);
        let score_fact = shared_fabric.intern();
        let mut seq_premises = BTreeSet::new();
        seq_premises.insert(word_fact);
        shared_fabric.assert(score_fact, seq_premises, false);

        // Stage 11: provenance chain grounded at frame-level acoustic
        // evidence. The frame layer carries NO support (it is the grounding);
        // every higher node reaches a frame node through strictly descending
        // premises. (No separate syllable layer: the collapsed phone
        // sequence serves as the phoneme layer.)
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
        // One phoneme node per distinct phone fact; repeated phones (e.g.
        // the two /T/ in STAKED) merge spans into a single node so node ids
        // stay unique within the chain.
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
                 acoustic span; bigram p={joint:.4}; {verify_note}",
                phones.len()
            ),
            clamp01((winner_sim - runner_best + 1.0) / 2.0),
        );
        // C1 (bead hearsay-four-shq): propagate the stage-3 commit
        // winner's rating verbatim, with a per-record evidence summary.
        let evidence_summary = format!(
            "stage-3 commit winner rating {source_rating:.2} over {} frames; \
             {} phones; nearest-pronunciation {winner_word} sim {winner_sim:.4}; \
             bigram p={joint:.4}; {verify_note}",
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
    let stats = FullPipelineStats {
        frame_count: total_frames,
        chunk_count,
        arena_facts: content_to_fact.len(),
        word_count: records.len(),
    };
    Ok(FullPipelineOutput {
        transcript: Transcript::new(records, session, Some("A".to_string())),
        stats,
    })
}

#[cfg(test)]
mod full_pipeline_tests {
    use super::*;

    fn sine_pcm_16k(seconds: f64, hz: f64) -> Pcm16kAudio {
        let rate = 16_000u32;
        let n = (rate as f64 * seconds) as usize;
        Pcm16kAudio {
            sample_rate: rate,
            channels: 1,
            frames: (0..n)
                .map(|i| {
                    ((2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() * 8000.0)
                        as i16
                })
                .collect(),
        }
    }

    fn test_session() -> SessionMeta {
        SessionMeta {
            speaker_id: "test".to_string(),
            recording_ref: "synthetic".to_string(),
            started_ms: 0,
        }
    }

    #[test]
    fn wer_wrapper_matches_output_core_on_fixtures() {
        // Bead hearsay-four-chd (4): the thin wrapper agrees with the
        // canonical `output::edit_distance` core on all fixtures.
        let phone = |s: &str| s.to_string();
        let fixtures: [(Vec<String>, Vec<String>); 3] = [
            (vec![phone("S"), phone("T")], vec![phone("S"), phone("T")]),
            (
                vec![phone("S"), phone("T")],
                vec![phone("S"), phone("T"), phone("EY")],
            ),
            (vec![], vec![phone("S"), phone("T"), phone("EY")]),
        ];
        for (a, b) in &fixtures {
            assert_eq!(edit_distance(a, b), output::edit_distance(a, b));
            assert_eq!(edit_distance(a, b), output::edit_distance(b, a));
        }
        assert_eq!(
            edit_distance(&fixtures[1].0, &fixtures[1].1),
            1,
            "wrapper still counts single insertion"
        );
    }

    #[test]
    fn edit_distance_counts_edits() {
        let phone = |s: &str| s.to_string();
        let a = vec![phone("S"), phone("T")];
        let b = vec![phone("S"), phone("T"), phone("EY")];
        assert_eq!(edit_distance(&a, &b), 1);
        assert_eq!(edit_distance(&b, &a), 1);
        assert_eq!(edit_distance(&a, &a), 0);
        assert!(edit_distance(&[], &b) == 3);
        assert!(edit_distance(&a, &[]) == 2);
    }

    #[test]
    fn full_pipeline_commits_words_with_terminating_provenance() {
        let pcm = sine_pcm_16k(2.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let config = FullPipelineConfig {
            chunk_secs: 0.5,
            max_secs: 2.0,
            window_width: 0,
        };
        let out = run_full_pipeline(&pcm, &baseline, &codebook, test_session(), &config)
            .expect("full pipeline");
        assert!(!out.transcript.words.is_empty(), "transcript non-empty");
        assert_eq!(out.stats.word_count, out.transcript.words.len());
        for word in &out.transcript.words {
            word.validate()
                .expect("chain terminates at acoustic evidence");
            assert!(!word.losing.is_empty(), "loser recorded");
            assert!(!word.decided_by.description.is_empty());
        }
        let text: String = out
            .transcript
            .words
            .iter()
            .map(|w| w.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!text.is_empty(), "transcript text non-empty");
    }

    #[test]
    fn full_pipeline_rejects_empty_input() {
        let pcm = Pcm16kAudio {
            sample_rate: 16_000,
            channels: 1,
            frames: Vec::new(),
        };
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        assert!(
            run_full_pipeline(
                &pcm,
                &baseline,
                &codebook,
                test_session(),
                &FullPipelineConfig::default()
            )
            .is_err()
        );
    }

    /// 5qc gate (real-audio leg): the full pipeline runs on the fetched
    /// SCOTUS session MP3 and every word's provenance terminates. Skips when
    /// the audio is absent so CI without the 55MB clip still passes.
    #[test]
    fn full_pipeline_runs_on_real_session_audio() {
        let real_audio = std::path::Path::new("../../data/session/audio/25-5343.mp3");
        if !real_audio.exists() {
            eprintln!("SKIP: real session audio not present at {real_audio:?}");
            return;
        }
        let full = corpus::audio::decode_to_pcm16k(real_audio).expect("decode real MP3");
        assert!(!full.frames.is_empty(), "decoded samples");
        let config = FullPipelineConfig {
            chunk_secs: 2.0,
            max_secs: 4.0,
            window_width: 0,
        };
        let out = run_full_pipeline(
            &full,
            &generic_reference_baseline(),
            &fse::default_codebook(),
            test_session(),
            &config,
        )
        .expect("real-audio full pipeline");
        assert!(
            !out.transcript.words.is_empty(),
            "real transcript non-empty"
        );
        for word in &out.transcript.words {
            word.validate().expect("real chain terminates");
        }
    }
}
