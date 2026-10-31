//! train-r1v2-full: full-size R1-v2 LM under the 30s load gate.
//!
//! Owner override 2026-10-31: the old <2s load gate was arbitrary and gutted
//! accuracy (general ppl 247->1084). NEW gate: LM loads once at app/server
//! start in <=30s. This binary is train-r1v2 with the cascade selection bar
//! moved from 1.75s (2s gate) to 27s (30s gate, same ~10% shared-box margin),
//! plus a saved book-only model for OOV analysis. All training, tuning (DEV =
//! dev-clean + owner prompts), and reporting logic is otherwise identical.
//!
//! usage: train-r1v2-full <general_txt> <personal_txt> <dev_clean_dir> <owner_dev_prompts> <out_dir> <min_bi> <min_tri> <budgets_csv>

use std::path::{Path, PathBuf};
use std::time::Instant;

use lm::persist;

fn collect_trans_txt(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut subs: Vec<PathBuf> = Vec::new();
    for e in entries {
        let p = e.unwrap_or_else(|e| panic!("dir entry: {e}")).path();
        if p.is_dir() {
            subs.push(p);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".trans.txt"))
        {
            out.push(p);
        }
    }
    subs.sort();
    for s in subs {
        collect_trans_txt(&s, out);
    }
}

fn count_tokens(sents: &[Vec<String>]) -> u64 {
    sents.iter().map(|s| s.len() as u64).sum()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 9 {
        eprintln!(
            "usage: train-r1v2-full <general_txt> <personal_txt> <dev_clean_dir> <owner_dev_prompts> <out_dir> <min_bi> <min_tri> <budgets_csv>"
        );
        std::process::exit(2);
    }
    let (general_txt, personal_txt) = (Path::new(&args[1]), Path::new(&args[2]));
    let (dev_dir, owner_prompts, out_dir) =
        (Path::new(&args[3]), Path::new(&args[4]), Path::new(&args[5]));
    let min_big: u64 = args[6]
        .parse()
        .unwrap_or_else(|_| panic!("bad min_bi {:?}", args[6]));
    let min_tri: u64 = args[7]
        .parse()
        .unwrap_or_else(|_| panic!("bad min_tri {:?}", args[7]));
    let mut budgets: Vec<usize> = args[8]
        .split(',')
        .map(|b| {
            b.trim()
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("bad budget {b:?}"))
        })
        .collect();
    budgets.sort_unstable_by(|a, b| b.cmp(a));
    assert!(!budgets.is_empty(), "need at least one budget");
    std::fs::create_dir_all(out_dir).expect("create out_dir");

    // ---- DEV sets (tune = dev-clean + owner prompts; report = dev-clean) ----
    let mut trans_paths = Vec::new();
    collect_trans_txt(dev_dir, &mut trans_paths);
    trans_paths.sort();
    assert!(
        !trans_paths.is_empty(),
        "no *.trans.txt under {}",
        dev_dir.display()
    );
    let mut devclean_sents = Vec::new();
    let mut devclean_lines = 0u64;
    for p in &trans_paths {
        let text = std::fs::read_to_string(p)
            .unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        for line in text.lines() {
            let Some((_, transcript)) = line.split_once([' ', '\t']) else {
                continue;
            };
            devclean_lines += 1;
            devclean_sents.extend(lm::normalize_text(transcript));
        }
    }
    let praw = std::fs::read_to_string(owner_prompts)
        .unwrap_or_else(|e| panic!("read {}: {e}", owner_prompts.display()));
    let mut owner_sents = Vec::new();
    for line in praw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        owner_sents.extend(lm::normalize_text(t));
    }
    let mut tune_sents = devclean_sents.clone();
    tune_sents.extend(owner_sents.iter().cloned());
    println!(
        "dev\ttrans_files={} trans_lines={} devclean_sents={} devclean_tokens={} owner_sents={} owner_tokens={} tune_sents={} tune_tokens={}",
        trans_paths.len(),
        devclean_lines,
        devclean_sents.len(),
        count_tokens(&devclean_sents),
        owner_sents.len(),
        count_tokens(&owner_sents),
        tune_sents.len(),
        count_tokens(&tune_sents),
    );

    // ---- general model ----
    let t0 = Instant::now();
    let graw = std::fs::read_to_string(general_txt)
        .unwrap_or_else(|e| panic!("read {}: {e}", general_txt.display()));
    let mut general_sents = Vec::new();
    let mut general_lines = 0u64;
    for line in graw.lines() {
        general_lines += 1;
        general_sents.extend(lm::normalize_text(line));
    }
    drop(graw);
    let general_tokens = count_tokens(&general_sents);
    println!(
        "input\tgeneral\tlines={general_lines} sentences={} tokens={general_tokens} ingest_secs={:.1}",
        general_sents.len(),
        t0.elapsed().as_secs_f64()
    );
    let t0 = Instant::now();
    let full = lm::KnTrigram::train(&general_sents);
    drop(general_sents);
    println!(
        "trained\tgeneral\tbigrams={} trigrams={} train_secs={:.1}",
        full.bigram_entry_count(),
        full.trigram_entry_count(),
        t0.elapsed().as_secs_f64()
    );
    let ppl_full = full.perplexity(&devclean_sents);
    println!(
        "ppl\tgeneral-full\t{:.4}\tsentences={} tokens={}",
        ppl_full.perplexity, ppl_full.sentences, ppl_full.predicted_tokens
    );

    // ---- personal (owner book) model ----
    let t0 = Instant::now();
    let braw = std::fs::read_to_string(personal_txt)
        .unwrap_or_else(|e| panic!("read {}: {e}", personal_txt.display()));
    let mut personal_sents = Vec::new();
    let mut personal_lines = 0u64;
    for line in braw.lines() {
        personal_lines += 1;
        personal_sents.extend(lm::normalize_text(line));
    }
    drop(braw);
    println!(
        "input\tbook\tlines={personal_lines} sentences={} tokens={} ingest_secs={:.1}",
        personal_sents.len(),
        count_tokens(&personal_sents),
        t0.elapsed().as_secs_f64()
    );
    let personal = lm::KnTrigram::train(&personal_sents);
    drop(personal_sents);
    {
        let snap = personal.inspectable();
        println!(
            "trained\tbook\tvocab={} bigrams={} trigrams={} total_tokens={}",
            snap.vocabulary.len(),
            snap.bigram_counts.len(),
            snap.trigram_counts.len(),
            snap.total_tokens
        );
    }
    let ppl_pers = personal.perplexity(&devclean_sents);
    println!(
        "ppl\tbook-only\t{:.4}\tsentences={} tokens={}",
        ppl_pers.perplexity, ppl_pers.sentences, ppl_pers.predicted_tokens
    );

    // ---- count cutoff on the general model (before entropy pruning) ----
    let t0 = Instant::now();
    let cut_general = full.count_cutoff_pruned(min_big, min_tri);
    let cut_secs = t0.elapsed().as_secs_f64();
    drop(full);
    println!(
        "cutoff\tgeneral\tmin_bi={min_big} min_tri={min_tri} bigrams={} trigrams={} cut_secs={cut_secs:.1}",
        cut_general.bigram_entry_count(),
        cut_general.trigram_entry_count(),
    );
    let ppl_cut = cut_general.perplexity(&devclean_sents);
    println!(
        "ppl\tgeneral-cut\t{:.4}\tsentences={} tokens={}",
        ppl_cut.perplexity, ppl_cut.sentences, ppl_cut.predicted_tokens
    );

    // ---- cascade prune: budgets descending, first under-27s deser wins ----
    // 30s gate (owner override 2026-10-31): selection bar sits below it with
    // the same ~10% shared-box margin as the old 1.75s/2s bar.
    let mut cur = cut_general;
    let mut shipped_budget = 0usize;
    let mut shipped_entries = 0usize;
    let mut shipped_bytes = 0usize;
    let mut shipped_deser_secs = 0.0;
    for b in &budgets {
        let t0 = Instant::now();
        cur = cur.entropy_pruned(*b);
        let prune_secs = t0.elapsed().as_secs_f64();
        let entries = cur.trigram_entry_count();
        let t0 = Instant::now();
        let bytes = persist::kn_to_bytes(&cur);
        let ser_secs = t0.elapsed().as_secs_f64();
        let t0 = Instant::now();
        let _rt = persist::kn_from_bytes(&bytes).expect("kn round trip");
        let deser_secs = t0.elapsed().as_secs_f64();
        // Selection bar sits below the 30 s gate: shared-box deser timing
        // jitters ~10%, so only ship candidates with clear margin.
        let under = deser_secs < 27.0;
        println!(
            "prune-candidate\tbudget={b}\tentries={entries}\tarchive_bytes={}\tprune_secs={prune_secs:.1}\tser_secs={ser_secs:.1}\tdeser_secs={deser_secs:.3}\tunder_27s={under}",
            bytes.len()
        );
        if under {
            shipped_budget = *b;
            shipped_entries = entries;
            shipped_bytes = bytes.len();
            shipped_deser_secs = deser_secs;
            break;
        }
    }
    assert!(
        shipped_budget > 0,
        "no budget deserializes under 30 s; smallest tried: {}",
        budgets.last().unwrap_or(&0)
    );
    println!(
        "shipped-prune\tbudget={shipped_budget} entries={shipped_entries} archive_bytes={shipped_bytes} deser_secs={shipped_deser_secs:.3}"
    );

    // ---- tune weight on DEV ONLY, with shipped components ----
    let tune = lm::tune_personal_weight(&cur, &personal, &tune_sents).expect("tune weight");
    for (w, p) in &tune.evaluations {
        println!("tune-grid\t{w:.2}\t{p:.4}");
    }
    println!(
        "tuned\tbook_weight={:.2} dev_ppl={:.4}",
        tune.best_weight, tune.best_perplexity
    );

    // ---- report perplexities on dev-clean (same set, with/without book) ----
    let ppl_without = cur.perplexity(&devclean_sents);
    println!(
        "ppl\tgeneral-pruned-without-book\t{:.4}\tsentences={} tokens={}",
        ppl_without.perplexity, ppl_without.sentences, ppl_without.predicted_tokens
    );
    let tuned =
        lm::InterpolatedLm::new(cur.clone(), personal.clone(), tune.best_weight).expect("tuned lm");
    let ppl_with = tuned.perplexity(&devclean_sents);
    println!(
        "ppl\tinterpolated-with-book\t{:.4}\tsentences={} tokens={}",
        ppl_with.perplexity, ppl_with.sentences, ppl_with.predicted_tokens
    );

    // ---- persist shipped artifacts + file load timing ----
    let gen_path = out_dir.join("lm-r1v2-general.json");
    persist::save_kn(&cur, &gen_path).expect("save general");
    let t0 = Instant::now();
    let loaded = persist::load_kn(&gen_path).expect("load general");
    let gen_load_secs = t0.elapsed().as_secs_f64();
    assert_eq!(loaded.inspectable(), cur.inspectable());
    {
        let snap = cur.inspectable();
        println!(
            "saved-general\tpath={} archive_bytes={} vocab={} bigrams={} trigrams={} total_tokens={} load_secs={gen_load_secs:.3} load_under_30s={}",
            gen_path.display(),
            shipped_bytes,
            snap.vocabulary.len(),
            snap.bigram_counts.len(),
            snap.trigram_counts.len(),
            snap.total_tokens,
            gen_load_secs < 30.0
        );
    }
    let book_path = out_dir.join("lm-r1v2-book.json");
    persist::save_kn(&personal, &book_path).expect("save book");
    let book_bytes = persist::kn_to_bytes(&personal).len();
    let t0 = Instant::now();
    let loaded_book = persist::load_kn(&book_path).expect("load book");
    let book_load_secs = t0.elapsed().as_secs_f64();
    assert_eq!(loaded_book.inspectable(), personal.inspectable());
    {
        let snap = personal.inspectable();
        println!(
            "saved-book\tpath={} archive_bytes={book_bytes} vocab={} bigrams={} trigrams={} total_tokens={} load_secs={book_load_secs:.3}",
            book_path.display(),
            snap.vocabulary.len(),
            snap.bigram_counts.len(),
            snap.trigram_counts.len(),
            snap.total_tokens,
        );
    }
    let interp_path = out_dir.join("lm-r1v2-interpolated.json");
    persist::save_interpolated(&tuned, &interp_path).expect("save interpolated");
    let t0 = Instant::now();
    let loaded_interp = persist::load_interpolated(&interp_path).expect("load interpolated");
    let interp_load_secs = t0.elapsed().as_secs_f64();
    let interp_bytes = persist::interpolated_to_bytes(&tuned).len();
    let rt_ppl = loaded_interp.perplexity(&devclean_sents).perplexity;
    println!(
        "saved-interpolated\tpath={} archive_bytes={interp_bytes} load_secs={interp_load_secs:.3} load_under_30s={} round_trip_ppl_equal={}",
        interp_path.display(),
        interp_load_secs < 30.0,
        (rt_ppl - ppl_with.perplexity).abs() < 1e-9
    );
    println!(
        "wrote {} and {} and {}",
        gen_path.display(),
        book_path.display(),
        interp_path.display()
    );
}
