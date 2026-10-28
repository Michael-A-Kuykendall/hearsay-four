//! R1b measured run (bead hearsay-four-2a0): source mixes, DEV-tuned
//! interpolation, entropy pruning to a trigram-entry budget, deterministic
//! persistence, load timing, and DEV perplexity per mix.
//! Usage: lm-report <repo_root> <better_voice_root> <out_dir>
//!
//! Training text comes only from `load_lm_train_sources` (D1 LM-train
//! partition). DEV text from `load_dev_evaluation_lines` is scored only.
//! Source mix: paths containing `oyez` form the personal/adapted
//! component; every other LM-train source forms the general component.

use std::path::Path;
use std::time::Instant;

use lm::persist;
use lm::{InterpolatedLm, KnTrigram};

/// Trigram-entry size budget for the entropy-pruning row.
const PRUNE_BUDGET: usize = 10_000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: lm-report <repo_root> <better_voice_root> <out_dir>");
        std::process::exit(2);
    }
    let (repo, bw, out_dir) = (
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    );
    std::fs::create_dir_all(out_dir).expect("create out_dir");

    let sources = lm::load_lm_train_sources(repo, bw).expect("LM-train ingestion");
    let (mut general_sents, mut personal_sents) = (Vec::new(), Vec::new());
    for s in &sources {
        let sents = lm::sentences_from_sources(std::slice::from_ref(s));
        println!(
            "train-source\t{}\t{}\tsentences={}",
            if s.path.contains("oyez") {
                "personal"
            } else {
                "general"
            },
            s.path,
            sents.len()
        );
        if s.path.contains("oyez") {
            personal_sents.extend(sents);
        } else {
            general_sents.extend(sents);
        }
    }
    let all_sents: Vec<Vec<String>> = general_sents
        .iter()
        .chain(personal_sents.iter())
        .cloned()
        .collect();

    let dev_sources = lm::load_dev_evaluation_lines(repo, bw).expect("DEV evaluation text");
    let mut dev_sents = Vec::new();
    for s in &dev_sources {
        let sents = lm::sentences_from_sources(std::slice::from_ref(s));
        println!(
            "dev-source\t{}\t{}\tsentences={}",
            s.dataset,
            s.path,
            sents.len()
        );
        dev_sents.extend(sents);
    }
    let dev_tokens: u64 = dev_sents.iter().map(|s| s.len() as u64 + 1).sum();
    println!(
        "dev-basis\tsentences={} predicted_tokens={}",
        dev_sents.len(),
        dev_tokens
    );

    let general = KnTrigram::train(&general_sents);
    let personal = KnTrigram::train(&personal_sents);
    let pooled = KnTrigram::train(&all_sents);
    println!(
        "trained\tgeneral_sents={} personal_sents={} pooled_trigrams={} pooled_vocab={}",
        general_sents.len(),
        personal_sents.len(),
        pooled.trigram_entry_count(),
        pooled.inspectable().vocabulary.len()
    );

    let show = |name: &str, ppl: lm::PerplexityReport| {
        println!(
            "ppl\t{name}\t{:.4}\tsentences={} tokens={}",
            ppl.perplexity, ppl.sentences, ppl.predicted_tokens
        );
    };
    show("general-only", general.perplexity(&dev_sents));
    show("personal-only", personal.perplexity(&dev_sents));
    show("pooled", pooled.perplexity(&dev_sents));

    let tune = lm::tune_personal_weight(&general, &personal, &dev_sents).expect("tune weight");
    for (w, p) in &tune.evaluations {
        println!("tune-grid\t{w:.2}\t{p:.4}");
    }
    println!(
        "tuned\tpersonal_weight={:.2} dev_ppl={:.4}",
        tune.best_weight, tune.best_perplexity
    );
    let tuned =
        InterpolatedLm::new(general.clone(), personal.clone(), tune.best_weight).expect("tuned lm");
    show("interpolated-tuned", tuned.perplexity(&dev_sents));

    // Entropy pruning of the pooled model to the entry budget.
    let pooled_bytes_before = persist::kn_to_bytes(&pooled).len();
    let pruned = pooled.entropy_pruned(PRUNE_BUDGET);
    let pooled_bytes_after = persist::kn_to_bytes(&pruned).len();
    println!(
        "prune\tbudget={} trigram_entries {} -> {} archive_bytes {} -> {}",
        PRUNE_BUDGET,
        pooled.trigram_entry_count(),
        pruned.trigram_entry_count(),
        pooled_bytes_before,
        pooled_bytes_after
    );
    show("pooled-pruned", pruned.perplexity(&dev_sents));

    // Deterministic persistence: byte-identical rebuild + reload timing.
    let rebuild = KnTrigram::train(&all_sents);
    let bytes_a = persist::kn_to_bytes(&pooled);
    let bytes_b = persist::kn_to_bytes(&rebuild);
    println!(
        "rebuild\tbyte_identical={} archive_bytes={}",
        bytes_a == bytes_b,
        bytes_a.len()
    );
    assert!(bytes_a == bytes_b, "rebuild must be byte-identical");

    let kn_path = out_dir.join("lm-pooled.json");
    persist::save_kn(&pooled, &kn_path).expect("save pooled");
    let t0 = Instant::now();
    let loaded = persist::load_kn(&kn_path).expect("load pooled");
    let load_secs = t0.elapsed().as_secs_f64();
    println!(
        "load\tkn archive_bytes={} load_seconds={load_secs:.4} load_under_2s={}",
        bytes_a.len(),
        load_secs < 2.0
    );
    assert_eq!(loaded.inspectable(), pooled.inspectable());

    let interp_path = out_dir.join("lm-interpolated.json");
    persist::save_interpolated(&tuned, &interp_path).expect("save interpolated");
    let t0 = Instant::now();
    let loaded_interp = persist::load_interpolated(&interp_path).expect("load interpolated");
    let interp_load_secs = t0.elapsed().as_secs_f64();
    println!(
        "load\tinterpolated archive_bytes={} load_seconds={interp_load_secs:.4} load_under_2s={} round_trip_ppl_equal={}",
        persist::interpolated_to_bytes(&tuned).len(),
        interp_load_secs < 2.0,
        (loaded_interp.perplexity(&dev_sents).perplexity - tuned.perplexity(&dev_sents).perplexity)
            .abs()
            < 1e-9
    );
    println!("wrote {} and {}", kn_path.display(), interp_path.display());
}
