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

pub mod decode_stream;
pub mod demo_transcribe;
pub mod fabric_path;
pub mod live;
pub mod rtf;
pub mod vad;

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
// ---------------------------------------------------------------------------
// Shared per-chunk machinery: batch (`run_full_pipeline`) and streaming
// (`run_streaming_pipeline`, bead hearsay-four-7e4) run IDENTICAL per-chunk
// math through [`process_chunk`]. The streaming loop adds only incremental
// commit bookkeeping, bounded-memory eviction, and the [`verify_or_add`]
// repair hook — word selection never diverges by path.
// ---------------------------------------------------------------------------
/// Read-only context for one chunk: the tables every chunk shares.
struct ChunkCtx<'a> {
    baseline: &'a Baseline,
    codebook: &'a [PhoneTemplate],
    dict: &'a HashMap<String, Vec<Vec<String>>>,
    vocab: &'a [String],
    window_width: usize,
    sample_rate: u32,
    shift_ms: u64,
    win_ms: u64,
}

/// Mutable state shared across chunks: ONE fabric (synthesis §4), the
/// hash-cons map, the stage-5 bigram table, and the committed word prefix
/// (bigram context for the next chunk).
struct StreamState {
    fabric: Fabric,
    content_to_fact: HashMap<arena::Content, FactId>,
    table: BigramTable,
    committed_words: Vec<String>,
    committed_word_facts: Vec<FactId>,
    verify_table: TrigramTable,
}

impl StreamState {
    fn new() -> Self {
        Self {
            fabric: Fabric::new(),
            content_to_fact: HashMap::new(),
            table: BigramTable::new(),
            committed_words: Vec::new(),
            committed_word_facts: Vec::new(),
            verify_table: TrigramTable::new(),
        }
    }

    fn arena_facts(&self) -> usize {
        self.content_to_fact.len()
    }
}

/// Output of one processed chunk: the committed record plus the fabric facts
/// the streaming loop may evict (frame-level arena facts) or must keep (the
/// committed word fact, the target of [`StreamingPipeline::verify_or_add`]).
struct ChunkOutput {
    record: WordRecord,
    frame_facts: Vec<FactId>,
    word_fact: FactId,
    frames: usize,
}

/// Run stages 1→2→3→4→5→11 for ONE chunk (moved verbatim out of
/// [`run_full_pipeline`]; bead hearsay-four-7e4 shares it with streaming).
/// Returns `Ok(None)` for chunks that yield no windows/frames (the batch
/// loop skips those identically); `Ok(Some(_))` for a committed word.
fn process_chunk(
    state: &mut StreamState,
    ctx: &ChunkCtx,
    chunk: &[i16],
    chunk_idx: usize,
    chunk_start_ms: u64,
    chunk_end_ms: u64,
) -> Result<Option<ChunkOutput>, String> {
    // Stages 1-2: window + features, per-frame N-best, hash-consed ingest.
    let windows = fse::window_audio(chunk, ctx.sample_rate);
    if windows.is_empty() {
        return Ok(None);
    }
    let frames =
        fse::extract_features(&windows, ctx.sample_rate).map_err(|e| format!("extract: {e}"))?;
    if frames.is_empty() {
        return Ok(None);
    }

    let mut per_frame_facts: Vec<Vec<FactId>> = Vec::with_capacity(frames.len());
    let mut top1: Vec<String> = Vec::with_capacity(frames.len());
    let mut top_scores: Vec<f64> = Vec::with_capacity(frames.len());
    for frame in &frames {
        let scores = fse::score_phones(frame, ctx.baseline, ctx.codebook);
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
            let is_new = !state.content_to_fact.contains_key(&content);
            let fid = intern_hashconsed(&mut state.fabric, &mut state.content_to_fact, content);
            // Streaming eviction (C3, bead hearsay-four-7e4): a fact interned
            // for an earlier chunk may have been retracted after that chunk
            // closed (bounded memory — prior audio dropped, only
            // justifications retained). Re-arm it so this chunk's candidates
            // stay live: `assert_assumption` clears the deliberate-retraction
            // mark and the assert-time propagation cascade revives dependents
            // — that cascade inside `Fabric::assert` IS the existing
            // reactivation hook (stage3 `window.rs` offers no propagation
            // entry point of its own: only offer/accumulate/close). In batch
            // nothing is retracted mid-run, so old facts are already active
            // and this is a no-op there.
            if is_new || !state.fabric.is_active(fid) {
                state.fabric.assert_assumption(fid);
            }
            facts.push(fid);
        }
        per_frame_facts.push(facts);
    }
    if per_frame_facts.is_empty() || top1.is_empty() {
        return Ok(None);
    }

    // Stage 3: commit window over this chunk's frame facts, sharing the
    // single fabric (swap in/out; `Fabric` is not `Clone`).
    let width = if ctx.window_width > 0 {
        ctx.window_width
    } else {
        per_frame_facts.len() * fse::nbest::MAX_N + 64
    };
    let mut window = CommitWindow::new(width);
    std::mem::swap(&mut window.fabric, &mut state.fabric);
    for (pos, facts) in per_frame_facts.iter().enumerate() {
        let weight = top_scores.get(pos).copied().unwrap_or(0.0);
        for &fid in facts {
            let mut evidence = BTreeSet::new();
            evidence.insert(fid);
            let _ = window.offer(pos, evidence, &[], weight);
        }
    }
    let close_result = window.close();
    std::mem::swap(&mut window.fabric, &mut state.fabric);
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
        return Ok(None);
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
        let is_new = !state.content_to_fact.contains_key(&content);
        let fid = intern_hashconsed(&mut state.fabric, &mut state.content_to_fact, content);
        if is_new || !state.fabric.is_active(fid) {
            state.fabric.assert_assumption(fid);
        }
        state.fabric.commit(fid);
        syl_facts.push(fid);
    }

    // Stage 4: nearest-pronunciation alignment — classical edit distance
    // against every closed-vocabulary pronunciation (lookup only).
    // Deterministic order: distance ascending, word ascending.
    let mut scored: Vec<(String, usize, f64)> = ctx
        .vocab
        .iter()
        .map(|word| {
            let (dist, sim) = ctx.dict[word]
                .iter()
                .map(|pron| {
                    let d = edit_distance(&phones, pron);
                    let denom = phones.len().max(pron.len()).max(1) as f64;
                    (d, 1.0 - d as f64 / denom)
                })
                .min_by(|a, b| {
                    a.0.cmp(&b.0)
                        .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
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
    let word_fact = state.fabric.intern();
    state.fabric.assert(word_fact, premises, false);
    state.fabric.commit(word_fact);

    // Stage 5: bigram plausibility over the committed word sequence;
    // the score fact is justified by the word fact.
    state.committed_words.push(winner_word.clone());
    // C2 (bead hearsay-four-ea3): verification hook — after the lexicon
    // align above, before sequence scoring. The committed word sequence
    // is checked against trigram plausibility with the veto/flag
    // threshold. Flagged sequences are marked, never dropped: the record
    // below keeps full provenance.
    state.committed_word_facts.push(word_fact);
    let verify_note = verify_committed_word_sequence(
        &mut state.fabric,
        &state.committed_word_facts,
        &state.committed_words,
        &state.verify_table,
        DEFAULT_VERIFY_THRESHOLD,
    )
    .map(|verdict| verdict.note())
    .unwrap_or_else(|| "VERIFY-SKIP no committed word facts".to_string());
    let joint = state.table.sequence_prob(&state.committed_words);
    let score_fact = state.fabric.intern();
    let mut seq_premises = BTreeSet::new();
    seq_premises.insert(word_fact);
    state.fabric.assert(score_fact, seq_premises, false);

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
        let acoustic_rating = f64::from(clamp01(top_scores.get(i).copied().unwrap_or(0.0))) * 100.0;
        for fid in facts {
            if seen_frames.insert(*fid) {
                let start_ms = chunk_start_ms + i as u64 * ctx.shift_ms;
                frame_rating.insert(fid.0, acoustic_rating);
                frame_nodes.push(Node {
                    id: fid.0,
                    label: format!("FRAME:{start_ms}-{}ms", start_ms + ctx.win_ms),
                    support: vec![],
                    active: state.fabric.is_active(*fid),
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
                active: state.fabric.is_active(*fid),
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
        active: state.fabric.is_active(word_fact),
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
        label: format!("SEQ:{} p={joint:.4}", state.committed_words.join(" ")),
        support: vec![JustificationRef {
            justification_id: format!("j-seq-{chunk_idx}"),
            premises: vec![word_fact.0],
            unconditional: false,
        }],
        active: state.fabric.is_active(score_fact),
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

    Ok(Some(ChunkOutput {
        record,
        frame_facts: per_frame_facts.into_iter().flatten().collect(),
        word_fact,
        frames: frames.len(),
    }))
}
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

    // Shared per-chunk state (bead hearsay-four-7e4): one fabric across all
    // stages (synthesis §4), the hash-cons map, the stage-5 bigram table
    // seeded over the closed vocabulary (sequence crate; counts only — flat,
    // no LM), and the committed word prefix (bigram context per chunk).
    let mut state = StreamState::new();
    state.table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);
    // C2 (bead hearsay-four-ea3): upper-level verification KS — a counted
    // trigram table over the same closed vocabulary (uniform fallback, no
    // evidence against any sequence until trained, mirroring the bigram
    // table above).
    state.verify_table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);

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
    let ctx = ChunkCtx {
        baseline,
        codebook,
        dict: &dict,
        vocab: &vocab,
        window_width: config.window_width,
        sample_rate: pcm.sample_rate,
        shift_ms,
        win_ms,
    };

    let mut records: Vec<WordRecord> = Vec::new();
    let mut total_frames = 0usize;
    let mut chunk_count = 0usize;

    for (chunk_idx, chunk) in samples.chunks(chunk_samples).enumerate() {
        let chunk_start_sample = chunk_idx * chunk_samples;
        let chunk_start_ms = (chunk_start_sample as u64 * 1000) / pcm.sample_rate as u64;
        let chunk_end_ms =
            ((chunk_start_sample + chunk.len()) as u64 * 1000) / pcm.sample_rate as u64;

        // Shared per-chunk math (bead hearsay-four-7e4): batch and streaming
        // run the identical stages 1→2→3→4→5→11 through `process_chunk`;
        // `None` skips silent/short chunks exactly as the old `continue`s.
        let out = match process_chunk(
            &mut state,
            &ctx,
            chunk,
            chunk_idx,
            chunk_start_ms,
            chunk_end_ms,
        )? {
            Some(out) => out,
            None => continue,
        };
        total_frames += out.frames;
        records.push(out.record);
        chunk_count += 1;
    }

    if records.is_empty() {
        return Err("no chunks produced words".to_string());
    }
    let stats = FullPipelineStats {
        frame_count: total_frames,
        chunk_count,
        arena_facts: state.arena_facts(),
        word_count: records.len(),
    };
    Ok(FullPipelineOutput {
        transcript: Transcript::new(records, session, Some("A".to_string())),
        stats,
    })
}

// ---------------------------------------------------------------------------
// True online streaming loop (bead hearsay-four-7e4, C3).
//
// `run_full_pipeline` buffers the whole (capped) utterance and commits one
// word per chunk against the shared fabric; this section extends that
// `chunk_secs` path into an incremental loop: each chunk commits immediately
// on the shared fabric, then its prior-chunk AUDIO facts are retracted
// (dropped from the live set — bounded memory), while their justification
// sets stay in the fabric (provenance preserved — `retract` never deletes
// justifications). A later chunk can still revise an earlier commit through
// [`StreamingPipeline::verify_or_add`].
//
// Model-free, Oyez sessions + synthetic only, offline: counting and table
// lookup only — no network, no STT text, no embeddings, no learned weights.
// ---------------------------------------------------------------------------

/// Configuration for [`run_streaming_pipeline`] (mirrors the `chunk_secs`
/// path of [`FullPipelineConfig`]).
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Audio seconds per transcript chunk (one committed word per chunk).
    pub chunk_secs: f64,
    /// Cap on total audio processed, in seconds (0.0 = no cap).
    pub max_secs: f64,
    /// Stage-3 commit-window width per chunk (0 = auto: 5 N-best × frames + 64).
    pub window_width: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_secs: 2.0,
            max_secs: 12.0,
            window_width: 0,
        }
    }
}

/// Run stats for [`run_streaming_pipeline`].
#[derive(Debug, Clone)]
pub struct StreamingStats {
    pub frame_count: usize,
    pub chunk_count: usize,
    pub arena_facts: usize,
    pub word_count: usize,
    /// Frame-level facts retracted after their chunk closed (prior audio
    /// dropped from the live set; justifications retained for provenance).
    pub evicted_facts: usize,
    /// Live facts at finish (`provenance_stats().active_facts`).
    pub active_facts: usize,
    /// Total interned facts at finish (provenance retained, live or not).
    pub total_facts: usize,
    /// Cross-chunk repairs applied via [`StreamingPipeline::verify_or_add`].
    pub repair_count: usize,
}

/// Validated transcript plus run stats, repairs, and audit trail from
/// [`run_streaming_pipeline`].
#[derive(Debug, Clone)]
pub struct StreamingOutput {
    pub transcript: Transcript,
    pub stats: StreamingStats,
    pub repairs: Vec<RepairEvent>,
    /// Human-readable per-chunk commit + verify/repair log (audit trail).
    pub audit: Vec<String>,
    /// Provisional-hypothesis display events (bead hearsay-four-2bs,
    /// PART-1): one `provisional` event per committed chunk in chunk order,
    /// then one `revised` event per cross-chunk repair in repair order.
    pub events: Vec<ProvisionalEvent>,
}

/// Kind of a [`ProvisionalEvent`]: a per-chunk provisional hypothesis, a
/// cross-chunk repair revision, or a final (settled) word for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionalKind {
    Provisional,
    Revised,
    Final,
}

impl ProvisionalKind {
    /// Stable NDJSON `kind` string.
    pub fn as_str(self) -> &'static str {
        match self {
            ProvisionalKind::Provisional => "provisional",
            ProvisionalKind::Revised => "revised",
            ProvisionalKind::Final => "final",
        }
    }

    /// Parse an NDJSON `kind` string back (unknown strings are `None` —
    /// never a guess).
    pub fn parse_kind(s: &str) -> Option<Self> {
        match s {
            "provisional" => Some(ProvisionalKind::Provisional),
            "revised" => Some(ProvisionalKind::Revised),
            "final" => Some(ProvisionalKind::Final),
            _ => None,
        }
    }
}

/// One provisional-hypothesis display event (bead hearsay-four-2bs, PART-1):
/// a per-chunk provisional word (committed-so-far with the provisional
/// flag) or a revision event when cross-chunk repair revises an early
/// commit. Rendered as one NDJSON object per line by `serve --provisional`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionalEvent {
    pub kind: ProvisionalKind,
    /// Index of the committed chunk this event describes.
    pub chunk_idx: usize,
    /// The hypothesized word (`new_word` for a revision).
    pub word: String,
    /// True while the hypothesis may still be revised by later evidence.
    pub provisional: bool,
    /// The superseded word (revision events only).
    pub old_word: Option<String>,
    /// Why the revision was accepted (revision events only).
    pub reason: Option<String>,
    /// The audit-trail line that recorded this commit/repair.
    pub audit: String,
}

impl ProvisionalEvent {
    /// Serialize one NDJSON event object (plain `serde_json::Value` — no
    /// new dependencies beyond the existing `serde_json`).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind.as_str(),
            "chunk_idx": self.chunk_idx,
            "word": self.word,
            "provisional": self.provisional,
            "old_word": self.old_word,
            "reason": self.reason,
            "audit": self.audit,
        })
    }
}

/// One cross-chunk repair: later evidence revised an earlier commit.
#[derive(Debug, Clone)]
pub struct RepairEvent {
    /// Index of the revised chunk.
    pub chunk_idx: usize,
    pub old_word: String,
    pub new_word: String,
    /// Why the revision was accepted (caller reason + strength).
    pub reason: String,
    /// Deliberately retracted old word fact (inactive; provenance retained).
    pub old_fact: FactId,
    /// Replacement word fact backing the revised commit (live).
    pub new_fact: FactId,
}

/// Outcome of [`StreamingPipeline::verify_or_add`].
#[derive(Debug, Clone)]
pub enum RepairOutcome {
    /// Evidence below threshold, or candidate identical: commit stands.
    Kept { chunk_idx: usize },
    /// Later evidence revised the commit (audit trail recorded).
    Repaired(RepairEvent),
}

/// Minimum combined acoustic+sequence support for [`verify_or_add`] to
/// revise a commit. Repairs need strong later evidence, so ordinary
/// inter-chunk jitter never flips a committed word.
pub const REPAIR_EVIDENCE_THRESHOLD: f32 = 0.75;

/// Incremental streaming pipeline: feed arbitrary PCM slices, commit one
/// word per chunk on the shared fabric, evict prior audio from the live
/// set. Chunk boundaries stay position-aligned from stream start, so feeding
/// whole utterances or uneven slices commits identical words.
pub struct StreamingPipeline {
    state: StreamState,
    dict: HashMap<String, Vec<Vec<String>>>,
    vocab: Vec<String>,
    baseline: Baseline,
    codebook: Vec<PhoneTemplate>,
    session: SessionMeta,
    sample_rate: u32,
    chunk_samples: usize,
    cap_samples: usize,
    window_width: usize,
    shift_ms: u64,
    win_ms: u64,
    consumed_total: usize,
    next_chunk_start_sample: usize,
    chunk_idx: usize,
    pending: Vec<i16>,
    records: Vec<WordRecord>,
    word_facts: Vec<FactId>,
    total_frames: usize,
    evicted_facts: usize,
    repairs: Vec<RepairEvent>,
    audit: Vec<String>,
    events: Vec<ProvisionalEvent>,
}

impl StreamingPipeline {
    /// Build a pipeline for `sample_rate` audio (validates like the batch
    /// path: supported rate, positive `chunk_secs`, non-empty codebook).
    pub fn new(
        sample_rate: u32,
        baseline: &Baseline,
        codebook: &[PhoneTemplate],
        session: SessionMeta,
        config: StreamingConfig,
    ) -> Result<Self, String> {
        if sample_rate < 8_000 {
            return Err(format!("unsupported sample rate {sample_rate}"));
        }
        if !config.chunk_secs.is_finite() || config.chunk_secs <= 0.0 {
            return Err("chunk_secs must be positive".to_string());
        }
        if codebook.is_empty() {
            return Err("empty codebook".to_string());
        }
        let dict = ingest_sample(CLOSED_VOCABULARY_CMUDICT);
        if dict.is_empty() {
            return Err("empty pronunciation dictionary".to_string());
        }
        let mut vocab: Vec<String> = dict.keys().cloned().collect();
        vocab.sort();
        let mut state = StreamState::new();
        state.table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);
        // C2 verification KS seed (same closed vocabulary, uniform fallback).
        state.verify_table.seed_vocabulary(&CLOSED_VOCABULARY_WORDS);
        let chunk_samples = ((sample_rate as f64 * config.chunk_secs) as usize).max(1);
        let cap_samples = if config.max_secs > 0.0 {
            (sample_rate as f64 * config.max_secs) as usize
        } else {
            usize::MAX
        };
        let shift_ms = (fse::shift_len(sample_rate) as u64 * 1000) / sample_rate as u64;
        let win_ms = (fse::window_len(sample_rate) as u64 * 1000) / sample_rate as u64;
        Ok(Self {
            state,
            dict,
            vocab,
            baseline: baseline.clone(),
            codebook: codebook.to_vec(),
            session,
            sample_rate,
            chunk_samples,
            cap_samples,
            window_width: config.window_width,
            shift_ms,
            win_ms,
            consumed_total: 0,
            next_chunk_start_sample: 0,
            chunk_idx: 0,
            pending: Vec::new(),
            records: Vec::new(),
            word_facts: Vec::new(),
            total_frames: 0,
            evicted_facts: 0,
            repairs: Vec::new(),
            audit: Vec::new(),
            events: Vec::new(),
        })
    }

    /// Feed raw PCM samples; every full chunk available commits immediately.
    /// Returns the number of chunks committed by this call. Input past the
    /// `max_secs` cap is dropped.
    pub fn feed(&mut self, samples: &[i16]) -> Result<usize, String> {
        let remaining = self.cap_samples.saturating_sub(self.consumed_total);
        let take = remaining.min(samples.len());
        self.pending.extend_from_slice(&samples[..take]);
        self.consumed_total += take;
        let mut committed = 0usize;
        while self.pending.len() >= self.chunk_samples {
            let chunk: Vec<i16> = self.pending.drain(..self.chunk_samples).collect();
            self.commit_chunk(&chunk)?;
            committed += 1;
        }
        Ok(committed)
    }

    /// Commit one full chunk positionally: identical stages to the batch
    /// path via [`process_chunk`], then evict the chunk's frame-level facts
    /// from the live set (bounded memory — only justifications retained).
    fn commit_chunk(&mut self, chunk: &[i16]) -> Result<(), String> {
        let idx = self.chunk_idx;
        let start_ms = (self.next_chunk_start_sample as u64 * 1000) / self.sample_rate as u64;
        let end_ms =
            ((self.next_chunk_start_sample + chunk.len()) as u64 * 1000) / self.sample_rate as u64;
        let ctx = ChunkCtx {
            baseline: &self.baseline,
            codebook: &self.codebook,
            dict: &self.dict,
            vocab: &self.vocab,
            window_width: self.window_width,
            sample_rate: self.sample_rate,
            shift_ms: self.shift_ms,
            win_ms: self.win_ms,
        };
        let out = process_chunk(&mut self.state, &ctx, chunk, idx, start_ms, end_ms)?;
        // Advance the positional clock even for silent chunks, exactly as the
        // batch `enumerate` does (keeps justification ids and ms spans
        // aligned with the batch path).
        self.next_chunk_start_sample += chunk.len();
        self.chunk_idx += 1;
        let Some(out) = out else { return Ok(()) };
        self.total_frames += out.frames;
        // Bounded memory: the word is committed, so this chunk's frame-level
        // arena facts leave the live set. Committed winners survive via
        // unconditional justifications; losers are already retracted by
        // `close`; word/syllable facts never depended on frame facts — so the
        // cascade touches only already-dead candidates. Justifications stay
        // in the fabric (provenance preserved).
        let mut seen = BTreeSet::new();
        let mut evictees = Vec::new();
        for fid in out.frame_facts {
            if seen.insert(fid) {
                evictees.push(fid);
            }
        }
        // Bounded memory: one scoped cascade for the whole chunk (PERF-1,
        // bead hearsay-four-gmp) instead of a per-fact retract cascade —
        // same deliberate-retraction marks, same final active set, one
        // subgraph-scoped invariant check.
        let evicted = evictees
            .iter()
            .filter(|fid| self.state.fabric.is_active(**fid))
            .count();
        self.state.fabric.retract_all(evictees);
        self.evicted_facts += evicted;
        let audit_line = format!(
            "chunk {idx}: committed {:?} (frames {}, evicted {evicted} audio facts)",
            out.record.text, out.frames,
        );
        self.audit.push(audit_line.clone());
        // PART-1 (bead hearsay-four-2bs): every commit is also a
        // provisional-hypothesis display event (committed-so-far, still
        // revisable by later-chunk evidence).
        self.events.push(ProvisionalEvent {
            kind: ProvisionalKind::Provisional,
            chunk_idx: idx,
            word: out.record.text.clone(),
            provisional: true,
            old_word: None,
            reason: None,
            audit: audit_line,
        });
        self.word_facts.push(out.word_fact);
        self.records.push(out.record);
        Ok(())
    }

    /// Cross-chunk repair hook (verify-or-add).
    ///
    /// Re-examine the committed word at `chunk_idx` in light of LATER-chunk
    /// evidence: `candidate` is the word the later context supports and
    /// `evidence_strength` (in [0,1]) is its combined acoustic+sequence
    /// support. An identical candidate, or strength below
    /// [`REPAIR_EVIDENCE_THRESHOLD`], keeps the commit (verified, audit
    /// records the check). Otherwise the old word fact is deliberately
    /// retracted (explicit local decision — only a fresh `assert` may revive
    /// it), the repair is asserted on live neighboring commits, the record
    /// is re-pointed (acoustic/phoneme layers unchanged — same span, same
    /// evidence), and the audit trail records the revision.
    pub fn verify_or_add(
        &mut self,
        chunk_idx: usize,
        candidate: &str,
        evidence_strength: f32,
        reason: &str,
    ) -> Result<RepairOutcome, String> {
        if chunk_idx >= self.records.len() {
            return Err(format!("no committed word at chunk {chunk_idx}"));
        }
        let candidate = candidate.trim().to_string();
        if candidate.is_empty() {
            return Err("repair candidate must be non-empty".to_string());
        }
        if !evidence_strength.is_finite() || !(0.0..=1.0).contains(&evidence_strength) {
            return Err("evidence_strength must be in [0,1]".to_string());
        }
        let committed = self.state.committed_words[chunk_idx].clone();
        if candidate == committed {
            self.audit.push(format!(
                "verify chunk {chunk_idx}: kept {committed:?} (candidate identical; {reason})"
            ));
            return Ok(RepairOutcome::Kept { chunk_idx });
        }
        if evidence_strength < REPAIR_EVIDENCE_THRESHOLD {
            self.audit.push(format!(
                "verify chunk {chunk_idx}: kept {committed:?} over {candidate:?} \
                 (strength {evidence_strength:.2} < threshold \
                 {REPAIR_EVIDENCE_THRESHOLD:.2}; {reason})"
            ));
            return Ok(RepairOutcome::Kept { chunk_idx });
        }
        // Strong later evidence for a different word: retract the old commit
        // and assert the repair on live neighboring commits (cross-chunk
        // linkage — the repair stays live exactly while its context does).
        let old_fact = self.word_facts[chunk_idx];
        self.state.fabric.retract(old_fact);
        let mut premises = BTreeSet::new();
        if let Some(&later) = self.word_facts.get(chunk_idx + 1)
            && self.state.fabric.is_active(later)
        {
            premises.insert(later);
        }
        if premises.is_empty()
            && let Some(earlier) = chunk_idx
                .checked_sub(1)
                .and_then(|i| self.word_facts.get(i).copied())
            && self.state.fabric.is_active(earlier)
        {
            premises.insert(earlier);
        }
        let new_fact = self.state.fabric.intern();
        if premises.is_empty() {
            self.state.fabric.assert_assumption(new_fact);
        } else {
            self.state.fabric.assert(new_fact, premises, false);
        }
        self.state.fabric.commit(new_fact);

        self.state.committed_words[chunk_idx] = candidate.clone();
        let joint = self.state.table.sequence_prob(&self.state.committed_words);
        let rec = &mut self.records[chunk_idx];
        rec.text = candidate.clone();
        rec.winner.text = candidate.clone();
        // Re-point the lexical layer at the repair fact (same span, same
        // acoustic/phoneme evidence) and refresh live flags from the fabric.
        let mut found_lex = false;
        for layer in rec.provenance.layers.iter_mut() {
            for node in layer.nodes.iter_mut() {
                if node.id == old_fact.0 {
                    node.id = new_fact.0;
                    node.label = format!("WORD:{candidate}");
                    found_lex = true;
                }
                for support in node.support.iter_mut() {
                    for premise in support.premises.iter_mut() {
                        if *premise == old_fact.0 {
                            *premise = new_fact.0;
                        }
                    }
                }
                node.active = self.state.fabric.is_active(FactId(node.id));
            }
        }
        if !found_lex {
            return Err(format!(
                "chunk {chunk_idx}: lexical node missing for repair"
            ));
        }
        if let Some(seq_layer) = rec.provenance.layers.last_mut() {
            for node in seq_layer.nodes.iter_mut() {
                node.label = format!("SEQ:{} p={joint:.4}", self.state.committed_words.join(" "));
            }
        }
        let note = format!("cross-chunk repair: {committed:?} -> {candidate:?} ({reason})");
        rec.decided_by.description = format!("{}; {note}", rec.decided_by.description);
        rec.evidence_summary = format!("{}; {note}", rec.evidence_summary);
        rec.validate()
            .map_err(|e| format!("chunk {chunk_idx}: repaired record invalid: {e:?}"))?;

        let event = RepairEvent {
            chunk_idx,
            old_word: committed,
            new_word: candidate,
            reason: format!("{reason} (strength {evidence_strength:.2})"),
            old_fact,
            new_fact,
        };
        let audit_line = format!(
            "repair chunk {chunk_idx}: {:?} -> {:?} ({})",
            event.old_word, event.new_word, event.reason
        );
        self.audit.push(audit_line.clone());
        self.repairs.push(event.clone());
        // PART-1 (bead hearsay-four-2bs): the revision is also a display
        // event, carrying the superseded word and the audit line so the
        // trail stays intact in the event stream.
        self.events.push(ProvisionalEvent {
            kind: ProvisionalKind::Revised,
            chunk_idx,
            word: event.new_word.clone(),
            provisional: false,
            old_word: Some(event.old_word.clone()),
            reason: Some(event.reason.clone()),
            audit: audit_line,
        });
        Ok(RepairOutcome::Repaired(event))
    }

    /// Drain any trailing partial chunk and return the validated transcript
    /// with stats, repairs, and audit trail.
    pub fn finish(&mut self) -> Result<StreamingOutput, String> {
        if !self.pending.is_empty() {
            let tail: Vec<i16> = std::mem::take(&mut self.pending);
            self.commit_chunk(&tail)?;
        }
        if self.records.is_empty() {
            return Err("no chunks produced words".to_string());
        }
        let fabric_stats = self.state.fabric.provenance_stats();
        let stats = StreamingStats {
            frame_count: self.total_frames,
            chunk_count: self.records.len(),
            arena_facts: self.state.arena_facts(),
            word_count: self.records.len(),
            evicted_facts: self.evicted_facts,
            active_facts: fabric_stats.active_facts,
            total_facts: fabric_stats.total_facts,
            repair_count: self.repairs.len(),
        };
        Ok(StreamingOutput {
            transcript: Transcript::new(
                self.records.clone(),
                self.session.clone(),
                Some("A".to_string()),
            ),
            stats,
            repairs: self.repairs.clone(),
            audit: self.audit.clone(),
            events: self.events.clone(),
        })
    }

    /// Committed records in chunk order.
    pub fn records(&self) -> &[WordRecord] {
        &self.records
    }

    /// The live word fact backing chunk `chunk_idx` (retracted after repair).
    pub fn word_fact(&self, chunk_idx: usize) -> Option<FactId> {
        self.word_facts.get(chunk_idx).copied()
    }

    /// Whether a fabric fact is currently live.
    pub fn is_fact_active(&self, fact: FactId) -> bool {
        self.state.fabric.is_active(fact)
    }

    /// Human-readable per-chunk commit + verify/repair log.
    pub fn audit(&self) -> &[String] {
        &self.audit
    }

    /// Cross-chunk repairs applied so far.
    pub fn repairs(&self) -> &[RepairEvent] {
        &self.repairs
    }

    /// Provisional-hypothesis display events so far (bead hearsay-four-2bs,
    /// PART-1): one `provisional` event per commit, then one `revised`
    /// event per repair.
    pub fn events(&self) -> &[ProvisionalEvent] {
        &self.events
    }

    /// Live provenance/memory snapshot over the shared fabric.
    pub fn fabric_stats(&self) -> fabric::stats::ProvenanceStats {
        self.state.fabric.provenance_stats()
    }
}

/// Run the true online streaming pipeline on raw PCM: incremental per-chunk
/// commit on the shared fabric with bounded memory (prior audio dropped,
/// only justifications retained), plus the [`verify_or_add`] repair hook
/// available on the returned pipeline for later-chunk revisions.
///
/// Word selection is identical to [`run_full_pipeline`] (shared
/// [`process_chunk`]) — see the `stream_tests` WER-parity gate.
pub fn run_streaming_pipeline(
    pcm: &Pcm16kAudio,
    baseline: &Baseline,
    codebook: &[PhoneTemplate],
    session: SessionMeta,
    config: &StreamingConfig,
) -> Result<StreamingOutput, String> {
    if pcm.frames.is_empty() {
        return Err("no PCM samples".to_string());
    }
    let mut pipe =
        StreamingPipeline::new(pcm.sample_rate, baseline, codebook, session, config.clone())?;
    pipe.feed(&pcm.frames)?;
    pipe.finish()
}

#[cfg(test)]
mod stream_tests {
    //! True online streaming gates (bead hearsay-four-7e4, C3).
    //!
    //! Model-free, Oyez sessions + synthetic only, offline: counting and
    //! table lookup only — no network. The Oyez parity test decodes staged
    //! bulk audio (`sessions/oyez-bulk/*/audio/run1.wav`, gitignored) and
    //! the committed `session.tsv` for that case; it skips when the audio is
    //! absent so CI without the staging still passes.
    use super::*;
    use corpus::ground_truth::{TimedSegment, extract_words};
    use std::path::{Path, PathBuf};

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
            recording_ref: "stream-test".to_string(),
            started_ms: 0,
        }
    }

    fn words_of(transcript: &Transcript) -> Vec<String> {
        transcript.words.iter().map(|w| w.text.clone()).collect()
    }

    /// Reference words from a committed Oyez `session.tsv` under `horizon`.
    fn oyez_reference_words(tsv: &Path, horizon_secs: f64) -> Vec<String> {
        let content = std::fs::read_to_string(tsv).expect("read session.tsv");
        let mut lines = content.lines();
        let header = lines.next().expect("tsv header");
        let cols: Vec<&str> = header.split('\t').collect();
        let col = |name: &str| {
            cols.iter()
                .position(|c| *c == name)
                .unwrap_or_else(|| panic!("session.tsv missing column {name}"))
        };
        let (c_start, c_stop, c_text) = (col("start_sec"), col("stop_sec"), col("text"));
        let mut rows = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let start: f64 = fields.get(c_start).unwrap_or(&"0").parse().unwrap_or(0.0);
            if start >= horizon_secs {
                continue;
            }
            let stop: f64 = fields.get(c_stop).unwrap_or(&"0").parse().unwrap_or(0.0);
            let text = fields.get(c_text).unwrap_or(&"").to_string();
            rows.push(TimedSegment::new(start, stop, text));
        }
        extract_words(&rows)
            .into_iter()
            .map(|(word, _, _)| word)
            .collect()
    }

    /// (1) Chunked output equals batch on a no-repair fixture: uneven
    /// incremental feeds commit exactly the batch words, confidences, and
    /// provenance (no repair hook involved).
    #[test]
    fn streaming_chunked_output_equals_batch_without_repair() {
        let pcm = sine_pcm_16k(4.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let batch_cfg = FullPipelineConfig {
            chunk_secs: 0.5,
            max_secs: 4.0,
            window_width: 0,
        };
        let stream_cfg = StreamingConfig {
            chunk_secs: 0.5,
            max_secs: 4.0,
            window_width: 0,
        };
        let batch = run_full_pipeline(&pcm, &baseline, &codebook, test_session(), &batch_cfg)
            .expect("batch");
        let mut pipe = StreamingPipeline::new(
            pcm.sample_rate,
            &baseline,
            &codebook,
            test_session(),
            stream_cfg,
        )
        .expect("streaming pipeline");
        // Uneven slices prove true incremental feeding: chunk boundaries stay
        // position-aligned from stream start regardless of feed sizes.
        let mut prev = 0;
        for &end in &[9_000usize, 27_000, 45_000, 64_000] {
            pipe.feed(&pcm.frames[prev..end]).expect("feed");
            prev = end;
        }
        let stream = pipe.finish().expect("finish");
        assert_eq!(
            words_of(&stream.transcript),
            words_of(&batch.transcript),
            "streaming commits exactly the batch words"
        );
        assert!(stream.repairs.is_empty(), "no repairs on no-repair path");
        assert_eq!(stream.stats.word_count, batch.stats.word_count);
        for (batch_word, stream_word) in batch
            .transcript
            .words
            .iter()
            .zip(stream.transcript.words.iter())
        {
            assert!((batch_word.confidence - stream_word.confidence).abs() < 1e-12);
            batch_word.validate().expect("batch record valid");
            stream_word.validate().expect("stream record valid");
        }
    }

    /// (2) Cross-chunk repair changes an early commit with an audit trail:
    /// later-chunk evidence revises chunk 0 through `verify_or_add`.
    #[test]
    fn streaming_cross_chunk_repair_revises_early_commit_with_audit() {
        let pcm = sine_pcm_16k(1.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let cfg = StreamingConfig {
            chunk_secs: 0.5,
            max_secs: 1.0,
            window_width: 0,
        };
        let mut pipe =
            StreamingPipeline::new(pcm.sample_rate, &baseline, &codebook, test_session(), cfg)
                .expect("streaming pipeline");
        pipe.feed(&pcm.frames).expect("feed");
        assert_eq!(
            pipe.records().len(),
            2,
            "two chunks committed incrementally"
        );
        let w0 = pipe.records()[0].text.clone();
        let w1 = pipe.records()[1].text.clone();
        // Later-chunk evidence: the chunk-1 word revises chunk 0 when it
        // differs; otherwise fall back to another closed-vocabulary word.
        let mut candidate = if w1 != w0 { w1 } else { String::new() };
        if candidate.is_empty() {
            let dict = lexicon::ingest_sample(lexicon::CLOSED_VOCABULARY_CMUDICT);
            let mut vocab: Vec<String> = dict.keys().cloned().collect();
            vocab.sort();
            candidate = vocab
                .into_iter()
                .find(|word| *word != w0)
                .expect("vocabulary alternative");
        }
        let old_fact = pipe.word_fact(0).expect("word fact 0");
        assert!(
            pipe.is_fact_active(old_fact),
            "old commit live before repair"
        );
        let outcome = pipe
            .verify_or_add(
                0,
                &candidate,
                0.90,
                "chunk-1 bigram context favors repetition",
            )
            .expect("repair");
        let event = match outcome {
            RepairOutcome::Repaired(event) => event,
            RepairOutcome::Kept { .. } => panic!("strong later evidence must repair"),
        };
        assert_eq!(pipe.records()[0].text, candidate, "early commit revised");
        assert_eq!(event.new_word, candidate);
        assert!(
            !pipe.is_fact_active(old_fact),
            "old commit deliberately retracted"
        );
        assert!(
            pipe.is_fact_active(event.new_fact),
            "repair fact live on neighboring commits"
        );
        assert!(!pipe.audit().is_empty(), "audit trail recorded");
        assert!(
            pipe.audit().iter().any(|line| line.contains("repair")),
            "audit names the repair"
        );
        pipe.records()[0]
            .validate()
            .expect("repaired record still terminates at acoustic evidence");
        let out = pipe.finish().expect("finish");
        assert_eq!(out.stats.repair_count, 1);
        assert_eq!(out.repairs.len(), 1);
        out.transcript.words[0]
            .validate()
            .expect("output transcript valid after repair");
    }

    /// (3) Memory bound: a 100-chunk run keeps the live-fact count under the
    /// `provenance_stats` ceiling while provenance (justifications) stays
    /// retained. Naive buffer-everything would hold ~100 chunks × ~230
    /// frame+candidate facts ≈ 23k live; the streaming loop evicts prior
    /// audio, so live stays near the committed words (~2 facts/chunk) plus
    /// one chunk of transient audio.
    #[test]
    fn streaming_memory_bounded_over_100_chunks() {
        let pcm = sine_pcm_16k(25.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let cfg = StreamingConfig {
            chunk_secs: 0.25,
            max_secs: 25.0,
            window_width: 0,
        };
        let out = run_streaming_pipeline(&pcm, &baseline, &codebook, test_session(), &cfg)
            .expect("100-chunk stream");
        assert_eq!(out.stats.word_count, 100, "one word per chunk");
        assert_eq!(out.stats.chunk_count, 100);
        eprintln!(
            "streaming memory: words={} active={} total={} evicted={} arena={}",
            out.stats.word_count,
            out.stats.active_facts,
            out.stats.total_facts,
            out.stats.evicted_facts,
            out.stats.arena_facts,
        );
        // Per-word live budget: 32/chunk is ~10× the observed steady state
        // (302 live over 100 words ≈ 3/chunk) and ~7× below the naive
        // buffer-everything growth (~23k live).
        assert!(
            out.stats.active_facts <= 32 * out.stats.word_count,
            "live facts bounded: {} active over {} words",
            out.stats.active_facts,
            out.stats.word_count
        );
        assert!(
            out.stats.evicted_facts > out.stats.active_facts,
            "prior audio dropped from the live set: evicted {} > active {}",
            out.stats.evicted_facts,
            out.stats.active_facts
        );
        assert!(
            out.stats.total_facts > out.stats.active_facts,
            "provenance retained past the live set: total {} > active {}",
            out.stats.total_facts,
            out.stats.active_facts
        );
        for word in &out.transcript.words {
            word.validate().expect("record valid under eviction");
        }
    }

    /// (4) WER parity vs batch on the Oyez bulk fixture, scored with the A2
    /// `output::wer` (S+D+I over normalized words / reference length):
    /// streaming must land within +2pp of batch on the same 12 s slice.
    /// Skips when the staged audio is absent (CI without the gitignored
    /// staging still passes).
    #[test]
    fn streaming_wer_parity_with_batch_on_oyez_fixture() {
        let case_dir = PathBuf::from("../../sessions/oyez-bulk/10-218_2011");
        let wav = case_dir.join("audio/run1.wav");
        let tsv = case_dir.join("session.tsv");
        if !wav.exists() || !tsv.exists() {
            eprintln!("SKIP: Oyez fixture not staged at {}", case_dir.display());
            return;
        }
        let full = corpus::audio::decode_to_pcm16k(&wav).expect("decode Oyez wav");
        let horizon_secs = 12.0;
        let take = ((full.sample_rate as f64 * horizon_secs) as usize).min(full.frames.len());
        let pcm = Pcm16kAudio {
            sample_rate: full.sample_rate,
            channels: full.channels,
            frames: full.frames[..take].to_vec(),
        };
        let reference = oyez_reference_words(&tsv, horizon_secs);
        assert!(!reference.is_empty(), "reference words present");
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let batch_cfg = FullPipelineConfig {
            chunk_secs: 2.0,
            max_secs: 12.0,
            window_width: 0,
        };
        let stream_cfg = StreamingConfig {
            chunk_secs: 2.0,
            max_secs: 12.0,
            window_width: 0,
        };
        let batch = run_full_pipeline(&pcm, &baseline, &codebook, test_session(), &batch_cfg)
            .expect("batch on Oyez slice");
        let stream =
            run_streaming_pipeline(&pcm, &baseline, &codebook, test_session(), &stream_cfg)
                .expect("stream on Oyez slice");
        let hyp_batch: Vec<String> = words_of(&batch.transcript);
        let hyp_stream: Vec<String> = words_of(&stream.transcript);
        let score_batch = output::wer(&reference, &hyp_batch);
        let score_stream = output::wer(&reference, &hyp_stream);
        eprintln!(
            "Oyez parity: reference={} batch WER={:.4} (S={} D={} I={}) \
             stream WER={:.4} (S={} D={} I={}) delta={:.4}",
            score_batch.reference_len,
            score_batch.wer,
            score_batch.substitutions,
            score_batch.deletions,
            score_batch.insertions,
            score_stream.wer,
            score_stream.substitutions,
            score_stream.deletions,
            score_stream.insertions,
            score_stream.wer - score_batch.wer,
        );
        assert!(
            score_stream.wer - score_batch.wer <= 0.02 + 1e-12,
            "streaming within +2pp of batch: batch {:.4} stream {:.4}",
            score_batch.wer,
            score_stream.wer
        );
    }

    /// (5) Error paths: empty input, non-positive chunking, and repair of a
    /// chunk that was never committed are all `Err` (never silent output).
    #[test]
    fn streaming_rejects_empty_input() {
        let empty = Pcm16kAudio {
            sample_rate: 16_000,
            channels: 1,
            frames: Vec::new(),
        };
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        assert!(
            run_streaming_pipeline(
                &empty,
                &baseline,
                &codebook,
                test_session(),
                &StreamingConfig::default()
            )
            .is_err(),
            "empty PCM is an error"
        );
        assert!(
            StreamingPipeline::new(
                16_000,
                &baseline,
                &codebook,
                test_session(),
                StreamingConfig {
                    chunk_secs: 0.0,
                    max_secs: 0.0,
                    window_width: 0,
                },
            )
            .is_err(),
            "non-positive chunk_secs is an error"
        );
        let mut pipe = StreamingPipeline::new(
            16_000,
            &baseline,
            &codebook,
            test_session(),
            StreamingConfig::default(),
        )
        .expect("valid pipeline");
        assert!(
            pipe.verify_or_add(0, "WORD", 0.90, "nothing committed yet")
                .is_err(),
            "repair without a commit is an error"
        );
    }

    /// (6) The verify half of verify-or-add: weak evidence, or a candidate
    /// identical to the commit, keeps the word and logs the check.
    #[test]
    fn streaming_verify_or_add_keeps_commit_on_weak_evidence() {
        let pcm = sine_pcm_16k(1.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let cfg = StreamingConfig {
            chunk_secs: 0.5,
            max_secs: 1.0,
            window_width: 0,
        };
        let mut pipe =
            StreamingPipeline::new(pcm.sample_rate, &baseline, &codebook, test_session(), cfg)
                .expect("streaming pipeline");
        pipe.feed(&pcm.frames).expect("feed");
        assert_eq!(pipe.records().len(), 2);
        let w0 = pipe.records()[0].text.clone();
        let other = if w0 == "CONGEALED" {
            "SURVIVAL"
        } else {
            "CONGEALED"
        };
        match pipe
            .verify_or_add(0, other, 0.50, "weak later hint")
            .expect("weak verify")
        {
            RepairOutcome::Kept { chunk_idx } => assert_eq!(chunk_idx, 0),
            RepairOutcome::Repaired(_) => panic!("weak evidence must not repair"),
        }
        match pipe
            .verify_or_add(0, &w0, 0.90, "identical candidate")
            .expect("identical verify")
        {
            RepairOutcome::Kept { chunk_idx } => assert_eq!(chunk_idx, 0),
            RepairOutcome::Repaired(_) => panic!("identical candidate must not repair"),
        }
        assert_eq!(pipe.records()[0].text, w0, "commit unchanged");
        assert_eq!(pipe.audit().len(), 4, "two commits + two verify lines");
        let out = pipe.finish().expect("finish");
        assert!(out.repairs.is_empty(), "no repairs recorded");
    }
}

#[cfg(test)]
mod provisional_tests {
    //! Provisional-hypothesis display gates (bead hearsay-four-2bs, PART-1).
    //!
    //! Model-free, synthetic sine PCM only, offline: counting and table
    //! lookup only — no network, no STT text, no learned weights.
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
            recording_ref: "provisional-test".to_string(),
            started_ms: 0,
        }
    }

    fn provisional_words(events: &[ProvisionalEvent]) -> Vec<String> {
        events
            .iter()
            .filter(|e| e.kind == ProvisionalKind::Provisional)
            .map(|e| e.word.clone())
            .collect()
    }

    /// (1) Repair fixture: per-chunk provisional events fire at commit, then
    /// a cross-chunk repair emits a revision event — with the audit trail
    /// intact (audit names the repair, transcript re-validates).
    #[test]
    fn provisional_then_revised_event_with_audit_intact_on_repair() {
        let pcm = sine_pcm_16k(1.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let cfg = StreamingConfig {
            chunk_secs: 0.5,
            max_secs: 1.0,
            window_width: 0,
        };
        let mut pipe =
            StreamingPipeline::new(pcm.sample_rate, &baseline, &codebook, test_session(), cfg)
                .expect("streaming pipeline");
        pipe.feed(&pcm.frames).expect("feed");
        assert_eq!(pipe.records().len(), 2, "two chunks committed");
        // Provisional events: one per commit, in chunk order, flagged
        // provisional, matching the committed words.
        let committed: Vec<String> = pipe.records().iter().map(|r| r.text.clone()).collect();
        assert_eq!(pipe.events().len(), 2, "one event per commit");
        for (i, event) in pipe.events().iter().enumerate() {
            assert_eq!(event.kind, ProvisionalKind::Provisional);
            assert_eq!(event.chunk_idx, i);
            assert_eq!(event.word, committed[i]);
            assert!(event.provisional, "commit still revisable");
            assert!(event.old_word.is_none(), "no superseded word yet");
            assert!(!event.audit.is_empty(), "event carries its audit line");
        }
        // Later-chunk evidence revises chunk 0 (same repair hook as the C3
        // cross-chunk test: strong evidence for a different word).
        let w0 = committed[0].clone();
        let candidate = {
            let dict = lexicon::ingest_sample(lexicon::CLOSED_VOCABULARY_CMUDICT);
            let mut vocab: Vec<String> = dict.keys().cloned().collect();
            vocab.sort();
            vocab
                .into_iter()
                .find(|word| *word != w0)
                .expect("vocabulary alternative")
        };
        let outcome = pipe
            .verify_or_add(0, &candidate, 0.90, "provisional-display repair check")
            .expect("repair");
        match outcome {
            RepairOutcome::Repaired(_) => {}
            RepairOutcome::Kept { .. } => panic!("strong later evidence must repair"),
        }
        // Revision event: kind + superseded word + reason + audit line.
        assert_eq!(pipe.events().len(), 3, "repair appends one event");
        let revised = pipe.events()[2].clone();
        assert_eq!(revised.kind, ProvisionalKind::Revised);
        assert_eq!(revised.chunk_idx, 0);
        assert_eq!(revised.word, candidate);
        assert!(!revised.provisional, "revision settles the display");
        assert_eq!(revised.old_word.as_deref(), Some(w0.as_str()));
        assert!(
            revised.reason.as_deref().is_some_and(|r| !r.is_empty()),
            "revision carries its reason"
        );
        assert!(
            revised.audit.contains("repair"),
            "revision carries its audit line"
        );
        // Audit trail intact: the repair line is in the pipeline audit, the
        // event audit matches it, and the repaired transcript validates.
        assert!(
            pipe.audit().iter().any(|line| line.contains("repair")),
            "audit names the repair"
        );
        assert!(
            pipe.audit().contains(&revised.audit),
            "event audit line is the trail line"
        );
        pipe.records()[0]
            .validate()
            .expect("repaired record still terminates at acoustic evidence");
        let out = pipe.finish().expect("finish");
        assert_eq!(out.stats.repair_count, 1);
        assert_eq!(out.events.len(), 3, "output carries the event stream");
        assert_eq!(out.transcript.words[0].text, candidate);
        // Every event round-trips through its NDJSON shape.
        for event in &out.events {
            let v = event.to_json();
            assert_eq!(
                ProvisionalKind::parse_kind(v["kind"].as_str().expect("kind string")),
                Some(event.kind),
                "kind survives NDJSON"
            );
            assert_eq!(
                v["chunk_idx"].as_u64().unwrap_or(u64::MAX) as usize,
                event.chunk_idx
            );
            assert_eq!(v["word"].as_str().unwrap_or(""), event.word);
            assert_eq!(
                v["provisional"].as_bool().unwrap_or(!event.provisional),
                event.provisional
            );
            assert_eq!(v["audit"].as_str().unwrap_or(""), event.audit);
        }
    }

    /// (2) No-repair fixture: provisional words equal the final words (and
    /// no revision event fires).
    #[test]
    fn provisional_equals_final_without_repair() {
        let pcm = sine_pcm_16k(2.0, 440.0);
        let baseline = generic_reference_baseline();
        let codebook = fse::default_codebook();
        let cfg = StreamingConfig {
            chunk_secs: 0.5,
            max_secs: 2.0,
            window_width: 0,
        };
        let out = run_streaming_pipeline(&pcm, &baseline, &codebook, test_session(), &cfg)
            .expect("clean stream");
        assert!(out.repairs.is_empty(), "no repairs on the clean path");
        let finals: Vec<String> = out
            .transcript
            .words
            .iter()
            .map(|w| w.text.clone())
            .collect();
        assert_eq!(out.events.len(), finals.len(), "one event per chunk");
        assert_eq!(
            provisional_words(&out.events),
            finals,
            "provisional == final with no repair"
        );
        assert!(
            out.events
                .iter()
                .all(|e| e.kind == ProvisionalKind::Provisional),
            "no revision event without a repair"
        );
        assert!(
            out.audit.iter().all(|line| !line.contains("repair")),
            "audit has no repair line"
        );
    }
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
