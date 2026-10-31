//! Golden-corpus gate for bead hearsay-four-baj (R1a).
//! Corpus (hand-calculable): "the cat sat", "the cat ran",
//! "the dog sat", "a cat sat". Expected values computed by hand from
//! the modified Kneser-Ney recursion in kneser_ney.rs (counts-of-counts
//! discounts: trigram D=(0.6923,0.9615,1.5), bigram fallback (0.5,1.0,1.5)).

use lm::{KnTrigram, normalize_sentence, normalize_text};

fn golden() -> KnTrigram {
    let sents: Vec<Vec<String>> = ["the cat sat", "the cat ran", "the dog sat", "a cat sat"]
        .iter()
        .map(|s| normalize_sentence(s))
        .collect();
    KnTrigram::train(&sents)
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "got {actual}, want {expected}"
    );
}

#[test]
fn golden_seen_trigram() {
    close(golden().prob("the", "cat", "sat"), 0.4475524475524475);
}

#[test]
fn golden_backed_off_trigram() {
    // (a, cat, ran) unseen as trigram; (cat, ran) seen as bigram.
    close(golden().prob("a", "cat", "ran"), 0.14685314685314682);
}

#[test]
fn golden_unseen_word() {
    // zebra -> <unk>
    close(golden().prob("the", "cat", "zebra"), 0.03146853146853147);
}

#[test]
fn golden_unseen_history() {
    // (zebra, cat) unseen history -> bigram P(sat|cat)
    let m = golden();
    close(m.prob("zebra", "cat", "sat"), 0.4242424242424242);
    close(m.bigram_prob("cat", "sat"), 0.4242424242424242);
    close(m.unigram_prob("sat"), 0.18181818181818182);
}

#[test]
fn golden_distributions_sum_to_one() {
    let m = golden();
    let vocab = m.prediction_vocabulary();
    for (w1, w2) in [
        ("the", "cat"),
        ("a", "cat"),
        ("zebra", "cat"),
        ("<s>", "<s>"),
        ("cat", "sat"),
    ] {
        let sum: f64 = vocab.iter().map(|w| m.prob(w1, w2, w)).sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "history ({w1},{w2}) sums to {sum}"
        );
    }
}

#[test]
fn normalization_rules() {
    assert_eq!(
        normalize_sentence("The CAT Sat."),
        vec!["the", "cat", "sat"]
    );
    assert_eq!(
        normalize_sentence("Don't o'Flaherty 'tis dogs'"),
        vec!["don't", "o'flaherty", "tis", "dogs"]
    );
    assert_eq!(
        normalize_sentence("well-known state-of-the-art"),
        vec!["well", "known", "state", "of", "the", "art"]
    );
    assert_eq!(
        normalize_sentence("Case 10-218 cost 1,000 dollars in 2026"),
        vec![
            "case", "<num>", "<num>", "cost", "<num>", "<num>", "dollars", "in", "<num>"
        ]
    );
    assert_eq!(
        normalize_sentence("Pi is 3.14 roughly"),
        vec!["pi", "is", "<num>", "<num>", "roughly"]
    );
    assert_eq!(
        normalize_sentence("Hello, world! (Yes?)"),
        vec!["hello", "world", "yes"]
    );
    assert_eq!(
        normalize_text("One two. Three four! Five?"),
        vec![vec!["one", "two"], vec!["three", "four"], vec!["five"]]
    );
    assert!(normalize_text("... !!!").is_empty());
}

#[test]
fn inspectable_counts() {
    let m = golden();
    let s = m.inspectable();
    assert_eq!(s.trigram_counts["the cat sat"], 1);
    assert_eq!(s.bigram_counts["cat sat"], 2);
    assert_eq!(s.unigram_counts["cat"], 3);
    assert_eq!(s.bigram_continuation_counts["cat sat"], 2);
    assert!((s.trigram_discounts.d1 - 0.6923076923076923).abs() < 1e-12);
    // Deterministic serialization.
    let a = serde_json::to_string(&s).unwrap();
    let b = serde_json::to_string(&golden().inspectable()).unwrap();
    assert_eq!(a, b);
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
fn ingest_only_lm_train_partition() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bw = better_voice_root();
    if !lm::lm_text_files_present(&root, &bw).expect("read LM manifests") {
        eprintln!("SKIP: LM text files absent (gitignored transcripts or better-voice sibling)");
        return;
    }
    let sources = lm::load_lm_train_sources(&root, &bw).expect("LM-train ingestion");
    assert_eq!(sources.len(), 9);
    assert!(sources.iter().all(|s| !s.lines.is_empty()));
    assert!(sources.iter().any(|s| s.path.contains("oyez")));
    assert!(sources.iter().any(|s| s.path.contains("bulk_tts_1k")));
}
