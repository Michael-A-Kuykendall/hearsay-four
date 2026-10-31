//! R1b gate tests (bead hearsay-four-2a0): interpolation, DEV weight
//! tuning, entropy pruning, deterministic persistence, perplexity.

use lm::persist;
use lm::{InterpolatedLm, KnTrigram, normalize_sentence, tune_personal_weight};

fn sents(texts: &[&str]) -> Vec<Vec<String>> {
    texts.iter().map(|s| normalize_sentence(s)).collect()
}

fn general() -> KnTrigram {
    KnTrigram::train(&sents(&[
        "the cat sat on the mat",
        "the dog sat on the rug",
        "a cat ran to the mat",
        "the dog ran to the rug",
        "cats and dogs are animals",
        "the bird flew over the mat",
    ]))
}

fn personal() -> KnTrigram {
    KnTrigram::train(&sents(&[
        "the court held that the statute applies",
        "the court affirmed the judgment below",
        "counsel argued that the statute applies",
        "the judgment was affirmed by the court",
    ]))
}

#[test]
fn interpolation_is_convex_mix() {
    let (g, p) = (general(), personal());
    let at0 = InterpolatedLm::new(g.clone(), p.clone(), 0.0).unwrap();
    let at1 = InterpolatedLm::new(g.clone(), p.clone(), 1.0).unwrap();
    let mid = InterpolatedLm::new(g.clone(), p.clone(), 0.3).unwrap();
    for (w1, w2, w) in [
        ("the", "cat", "sat"),
        ("the", "court", "held"),
        ("zebra", "cat", "sat"),
    ] {
        // Endpoints are exactly the component models (own vocabularies,
        // <unk> mapping), and interior weights are the convex mix.
        assert!((at0.prob(w1, w2, w) - g.prob(w1, w2, w)).abs() < 1e-12);
        assert!((at1.prob(w1, w2, w) - p.prob(w1, w2, w)).abs() < 1e-12);
        let want = 0.7 * g.prob(w1, w2, w) + 0.3 * p.prob(w1, w2, w);
        assert!(
            (mid.prob(w1, w2, w) - want).abs() < 1e-12,
            "mix for ({w1},{w2},{w})"
        );
        assert!((0.0..=1.0).contains(&mid.prob(w1, w2, w)));
    }
}

#[test]
fn interpolation_rejects_bad_weight() {
    assert!(InterpolatedLm::new(general(), personal(), -0.1).is_err());
    assert!(InterpolatedLm::new(general(), personal(), 1.1).is_err());
    assert!(InterpolatedLm::new(general(), personal(), f64::NAN).is_err());
}

#[test]
fn tuning_prefers_matching_component() {
    let (g, p) = (general(), personal());
    // DEV drawn from the personal (court) distribution.
    let dev = sents(&[
        "the court held that the statute applies",
        "the judgment was affirmed by the court",
    ]);
    let tuned = tune_personal_weight(&g, &p, &dev).unwrap();
    assert_eq!(tuned.evaluations.len(), 21);
    assert!(
        tuned.best_weight >= 0.8,
        "best weight {} for court DEV",
        tuned.best_weight
    );
    assert!(tuned.best_perplexity <= tuned.evaluations[0].1);
    assert!(tuned.best_perplexity <= tuned.evaluations[20].1 + 1e-9);
    // And the reverse: animal DEV tunes toward the general component.
    let dev_g = sents(&["the cat sat on the mat", "the dog ran to the rug"]);
    let tuned_g = tune_personal_weight(&g, &p, &dev_g).unwrap();
    assert!(
        tuned_g.best_weight <= 0.2,
        "best weight {} for animal DEV",
        tuned_g.best_weight
    );
    assert!(tune_personal_weight(&g, &p, &[]).is_err());
}

#[test]
fn perplexity_matches_definition_and_favors_matching_text() {
    let g = general();
    let matched = sents(&["the cat sat on the mat"]);
    let report = g.perplexity(&matched);
    assert_eq!(report.sentences, 1);
    assert_eq!(report.predicted_tokens, 7); // 6 tokens + </s>
    let want = (-report.log_prob / report.predicted_tokens as f64).exp();
    assert!((report.perplexity - want).abs() < 1e-12);
    assert!(report.perplexity >= 1.0);
    let mismatched = sents(&["the court held that the statute applies today"]);
    assert!(report.perplexity < g.perplexity(&mismatched).perplexity);
    assert!(g.perplexity(&[]).perplexity.is_infinite());
}

#[test]
fn entropy_pruning_respects_budget_and_stays_valid() {
    // Alphabetic-only tokens (digits would normalize to <num> and collapse).
    let names = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    let verbs = ["calls", "finds", "greets", "hears", "joins"];
    let things = ["harbor", "island", "jungle", "kitten", "ladder", "meadow"];
    let mut texts: Vec<String> = Vec::new();
    for (i, a) in names.iter().enumerate() {
        for (j, b) in verbs.iter().enumerate() {
            texts.push(format!(
                "{a} {b} {} and {} again",
                things[(i + j) % things.len()],
                things[(i + 2 * j + 1) % things.len()]
            ));
        }
    }
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let m = KnTrigram::train(&sents(&refs));
    let full = m.trigram_entry_count();
    assert!(full > 20, "synthetic trigram entries {full}");
    let budget = full / 2;
    let pruned = m.entropy_pruned(budget);
    assert_eq!(pruned.trigram_entry_count(), budget);
    // Deterministic: pruning twice gives the identical snapshot.
    assert_eq!(pruned.inspectable(), m.entropy_pruned(budget).inspectable());
    // At/above budget is a no-op.
    assert_eq!(m.entropy_pruned(full).inspectable(), m.inspectable());
    assert_eq!(m.entropy_pruned(full + 10).inspectable(), m.inspectable());
    // Distributions still sum to 1 and perplexity stays finite.
    let vocab = pruned.prediction_vocabulary();
    for (w1, w2) in [("alpha", "calls"), ("calls", "harbor"), ("<s>", "<s>")] {
        let sum: f64 = vocab.iter().map(|w| pruned.prob(w1, w2, w)).sum();
        assert!((sum - 1.0).abs() < 1e-9, "pruned sum ({w1},{w2}) = {sum}");
    }
    assert!(
        pruned
            .perplexity(&sents(&["alpha calls harbor and island again"]))
            .perplexity
            .is_finite()
    );
    // Serialized pruned archive is smaller than the full archive.
    assert!(persist::kn_to_bytes(&pruned).len() < persist::kn_to_bytes(&m).len());
}

#[test]
fn persistence_round_trip_and_byte_identical_rebuild() {
    let (g, p) = (general(), personal());
    // Byte-identical rebuild from the same sentences.
    let bytes_a = persist::kn_to_bytes(&g);
    let bytes_b = persist::kn_to_bytes(&general());
    assert_eq!(bytes_a, bytes_b);
    // KN round trip: identical snapshot and probabilities.
    let back = persist::kn_from_bytes(&bytes_a).unwrap();
    assert_eq!(back.inspectable(), g.inspectable());
    for (w1, w2, w) in [("the", "cat", "sat"), ("the", "court", "held")] {
        assert!((back.prob(w1, w2, w) - g.prob(w1, w2, w)).abs() < 1e-12);
    }
    // Interpolated round trip via file save/load.
    let lm = InterpolatedLm::new(g.clone(), p.clone(), 0.35).unwrap();
    let dir = std::env::temp_dir().join(format!("lm-r1b-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("interp.json");
    persist::save_interpolated(&lm, &path).unwrap();
    let t0 = std::time::Instant::now();
    let loaded = persist::load_interpolated(&path).unwrap();
    assert!(t0.elapsed().as_secs_f64() < 2.0, "load must be < 2s");
    assert_eq!(loaded.personal_weight(), 0.35);
    let dev = sents(&["the court held that the statute applies"]);
    assert!((loaded.perplexity(&dev).perplexity - lm.perplexity(&dev).perplexity).abs() < 1e-12);
    std::fs::remove_dir_all(&dir).ok();
    // Bad archives are rejected, not misread.
    assert!(persist::kn_from_bytes(b"{}").is_err());
    assert!(persist::kn_from_bytes(b"not json").is_err());
    let wrong_kind = persist::interpolated_to_bytes(&lm);
    assert!(persist::kn_from_bytes(&wrong_kind).is_err());
}

/// Better-voice checkout: `$BETTER_VOICE_ROOT` (or `$HEARSAY_BETTER_VOICE_ROOT`),
/// else the `better-voice` sibling of this checkout.
fn better_voice_root() -> std::path::PathBuf {
    for env in ["BETTER_VOICE_ROOT", "HEARSAY_BETTER_VOICE_ROOT"] {
        if let Ok(v) = std::env::var(env) {
            return std::path::PathBuf::from(v);
        }
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../better-voice")
}

#[test]
fn dev_evaluation_lines_are_disjoint_from_training() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bw = better_voice_root();
    if !lm::lm_text_files_present(&root, &bw).expect("read LM manifests") {
        eprintln!("SKIP: LM text files absent (gitignored transcripts or better-voice sibling)");
        return;
    }
    let dev = lm::load_dev_evaluation_lines(&root, &bw).expect("DEV evaluation lines");
    assert!(dev.iter().any(|s| s.path.contains("24-889")));
    assert!(dev.iter().all(|s| !s.lines.is_empty()));
    let dev_sents = lm::sentences_from_sources(&dev);
    assert!(dev_sents.len() > 100, "DEV sentences {}", dev_sents.len());
    // No DEV sentence source may be a training source (path-level check
    // is inside load_dev_evaluation_lines; sentence count stays sane).
}
