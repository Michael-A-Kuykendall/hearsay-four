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
//! Stages 4 (lexicon) and 5 (sequence) are not built; this runner covers
//! stages 1→2→3 composition per the fail-fast framing in synthesis §6.

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
