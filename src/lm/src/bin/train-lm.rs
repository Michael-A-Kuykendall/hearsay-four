//! Train the R1a KN trigram on D1 LM-train text and write inspectable counts.
//! Usage: train-lm <repo_root> <better_voice_root> <out.json>

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: train-lm <repo_root> <better_voice_root> <out.json>");
        std::process::exit(2);
    }
    let sources = lm::load_lm_train_sources(Path::new(&args[1]), Path::new(&args[2]))
        .unwrap_or_else(|e| panic!("ingest failed: {e}"));
    for s in &sources {
        println!("source\t{}\t{}\tlines={}", s.dataset, s.path, s.lines.len());
    }
    let sentences = lm::sentences_from_sources(&sources);
    let model = lm::KnTrigram::train(&sentences);
    let snap = model.inspectable();
    println!(
        "sentences={} vocab={} tokens={} tri_discounts={:?} bi_discounts={:?}",
        sentences.len(),
        snap.vocabulary.len(),
        snap.total_tokens,
        snap.trigram_discounts,
        snap.bigram_discounts
    );
    let json = serde_json::to_string_pretty(&snap).expect("serialize model");
    std::fs::write(&args[3], json).expect("write model");
    println!("wrote {}", args[3]);
}
