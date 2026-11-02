//! Dev-clean WER: large-vocab trie decode over a LibriSpeech wav manifest.
//!
//! Builds the R5a search vocabulary (LM top-N by unigram count plus book
//! word types, tiered-lexicon pronunciations), loads
//! [`MooreDecoder`](decoder::MooreDecoder) with an acoustic model + LM file,
//! decodes every manifest row, and scores with [`output::wer`].
//!
//! Usage:
//! ```text
//! wer_devclean --am <model.json> --lm <lm-file> --tier1 <master.tsv>
//!   --tier2 <librispeech-lexicon.txt> --wav-root <dir> --manifest <tsv>
//!   --out <tsv> [--top-n N] [--book <txt>] [--limit N] [--beam F]
//!   [--max-tokens N] [--lookahead-weight F] [--lm-weight F] [--word-penalty F]
//! ```
//!
//! The LM may be a plain (general) or interpolated archive; the flavor is
//! detected from the bytes and printed. Book words come from the personal
//! component when the LM is interpolated, else `--book` text is required.
//! Per-utterance `(errors, ref_len)` rows go to `--out` TSV for paired
//! comparison across runs.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use decoder::{BookSource, DecoderConfig, MooreDecoder, Recognizer};

struct Args {
    am: PathBuf,
    lm: PathBuf,
    tier1: PathBuf,
    tier2: PathBuf,
    wav_root: PathBuf,
    manifest: PathBuf,
    out: PathBuf,
    top_n: usize,
    book: Option<PathBuf>,
    limit: Option<usize>,
    beam: Option<f64>,
    max_tokens: Option<usize>,
    lookahead_weight: Option<f64>,
    lm_weight: Option<f64>,
    word_penalty: Option<f64>,
    no_cmvn: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: wer_devclean --am <model.json> --lm <lm-file> --tier1 <master.tsv> --tier2 <lex.txt> --wav-root <dir> --manifest <tsv> --out <tsv> [--top-n N] [--book <txt>] [--limit N] [--beam F] [--max-tokens N] [--lookahead-weight F] [--lm-weight F] [--word-penalty F] [--no-cmvn]"
    );
    std::process::exit(2);
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
    }
    None
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }
    let req = |name: &str| match flag(&args, name) {
        Some(v) => v,
        None => {
            eprintln!("missing required {name}");
            usage();
        }
    };
    let parse_usize = |name: &str| -> Option<usize> {
        flag(&args, name).map(|v| {
            v.parse::<usize>()
                .unwrap_or_else(|_| panic!("bad {name} {v:?}"))
        })
    };
    let parse_f64 = |name: &str| -> Option<f64> {
        flag(&args, name).map(|v| {
            v.parse::<f64>()
                .unwrap_or_else(|_| panic!("bad {name} {v:?}"))
        })
    };
    Args {
        am: PathBuf::from(req("--am")),
        lm: PathBuf::from(req("--lm")),
        tier1: PathBuf::from(req("--tier1")),
        tier2: PathBuf::from(req("--tier2")),
        wav_root: PathBuf::from(req("--wav-root")),
        manifest: PathBuf::from(req("--manifest")),
        out: PathBuf::from(req("--out")),
        top_n: parse_usize("--top-n").unwrap_or(20_000),
        book: flag(&args, "--book").map(PathBuf::from),
        limit: parse_usize("--limit"),
        beam: parse_f64("--beam"),
        max_tokens: parse_usize("--max-tokens"),
        lookahead_weight: parse_f64("--lookahead-weight"),
        lm_weight: parse_f64("--lm-weight"),
        word_penalty: parse_f64("--word-penalty"),
        no_cmvn: args.iter().any(|a| a == "--no-cmvn"),
    }
}

fn main() {
    let args = parse_args();

    // ---- LM (flavor detected from bytes; works for plain + interpolated).
    let t0 = Instant::now();
    let lm_bytes =
        std::fs::read(&args.lm).unwrap_or_else(|e| panic!("read {}: {e}", args.lm.display()));
    let lm_sha = fse::instrument::sha256_hex(&lm_bytes);
    let am_bytes =
        std::fs::read(&args.am).unwrap_or_else(|e| panic!("read {}: {e}", args.am.display()));
    let am_sha = fse::instrument::sha256_hex(&am_bytes);
    drop(am_bytes);
    let mut interpolated: Option<lm::InterpolatedLm> = None;
    let mut plain: Option<lm::KnTrigram> = None;
    let flavor: &str;
    match lm::persist::interpolated_from_bytes(&lm_bytes) {
        Ok(model) => {
            flavor = "interpolated";
            interpolated = Some(model);
        }
        Err(_) => match lm::persist::kn_from_bytes(&lm_bytes) {
            Ok(model) => {
                flavor = "general";
                plain = Some(model);
            }
            Err(e) => panic!(
                "lm {}: neither interpolated nor plain archive ({e})",
                args.lm.display()
            ),
        },
    }
    drop(lm_bytes);
    let (general, personal) = match (&interpolated, &plain) {
        (Some(ix), _) => (ix.general(), Some(ix.personal())),
        (None, Some(kn)) => (kn, None),
        (None, None) => unreachable!("lm load branches above"),
    };
    eprintln!(
        "lm {flavor} {} load {:.1}s",
        args.lm.display(),
        t0.elapsed().as_secs_f64()
    );
    if let Some(ix) = &interpolated {
        eprintln!("lm personal_weight {}", ix.personal_weight());
    }

    // ---- vocab: LM top-N plus book words, tiered-lexicon pronunciations.
    let t0 = Instant::now();
    let top = decoder::top_lm_words(general, personal, args.top_n);
    let (book, book_source) = match personal {
        Some(book_lm) => (
            decoder::book_types_from_personal(book_lm),
            BookSource::PersonalLm,
        ),
        None => match &args.book {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                (decoder::book_types_from_text(&text), BookSource::BookText)
            }
            None => {
                eprintln!("plain LM needs --book <txt> for the book word list");
                std::process::exit(2);
            }
        },
    };
    let lexicon = lexicon::load_tiered(&args.tier1, &args.tier2)
        .unwrap_or_else(|e| panic!("load tiered lexicon: {e}"));
    eprintln!(
        "lexicon tier1 {} tier2 {} total {}",
        lexicon.tier1_words(),
        lexicon.tier2_words(),
        lexicon.table.len()
    );
    let (entries, report) = decoder::build_entries(&top, &book, args.top_n, book_source, &lexicon);
    eprintln!(
        "vocab top_n={} lm_candidates={} book_types={} ({:?}) union={} kept_words={} kept_prons={} tier1={} tier2={} oov_lm={} oov_book_only={} build {:.1}s",
        report.top_n_requested,
        report.lm_candidates,
        report.book_types,
        report.book_source,
        report.union_types,
        report.kept_words,
        report.kept_prons,
        report.tier1_words,
        report.tier2_words,
        report.oov_lm,
        report.oov_book_only,
        t0.elapsed().as_secs_f64()
    );
    let vocab_upper: HashSet<String> = entries.iter().map(|e| e.word.clone()).collect();

    // ---- decoder.
    let mut cfg = DecoderConfig::default();
    if let Some(beam) = args.beam {
        cfg.beam = beam;
    }
    if let Some(max) = args.max_tokens {
        cfg.max_tokens = max;
    }
    if let Some(w) = args.lookahead_weight {
        cfg.lookahead_weight = w;
    }
    if let Some(w) = args.lm_weight {
        cfg.lm_weight = w;
    }
    if let Some(p) = args.word_penalty {
        cfg.word_penalty = p;
    }
    if args.no_cmvn {
        cfg.cmvn = false;
    }
    let t0 = Instant::now();
    let selected_frontend = if args.no_cmvn {
        fse::HMM_FRONTEND_ID_RAW
    } else {
        fse::HMM_FRONTEND_ID
    };
    let decoder = MooreDecoder::load_with_vocab(&args.am, Some(&args.lm), &entries, cfg)
        .unwrap_or_else(|e| panic!("load decoder: {e}"));
    drop(entries);
    eprintln!(
        "decoder vocab {} skipped {} lm {} frontend {} load {:.1}s",
        decoder.vocab_size(),
        decoder.skipped_words().len(),
        decoder.has_lm(),
        selected_frontend,
        t0.elapsed().as_secs_f64()
    );
    for s in decoder.skipped_words().iter().take(10) {
        eprintln!("skipped: {s}");
    }

    // ---- manifest rows: utt-id, relpath, dur, transcript (tab-separated).
    let manifest_text = std::fs::read_to_string(&args.manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", args.manifest.display()));
    let mut rows: Vec<(String, PathBuf, String)> = Vec::new();
    for (lineno, line) in manifest_text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let (id, rel, transcript) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(id), Some(rel), Some(_), Some(t)) => {
                (id.to_string(), rel.to_string(), t.to_string())
            }
            _ => panic!(
                "manifest line {} malformed (want id, path, dur, transcript)",
                lineno + 1
            ),
        };
        rows.push((id, PathBuf::from(rel), transcript));
    }
    if let Some(limit) = args.limit {
        rows.truncate(limit);
    }
    eprintln!("utterances {}", rows.len());

    // ---- decode + score.
    let mut out = String::from("utt\terrors\tref_len\thyp_len\taudio_sec\tdecode_sec\thyp\n");
    let mut errors: Vec<u32> = Vec::with_capacity(rows.len());
    let mut denoms: Vec<u32> = Vec::with_capacity(rows.len());
    let mut oov_ref = 0u64;
    let mut total_ref = 0u64;
    let mut total_audio = 0.0f64;
    let mut total_decode = 0.0f64;
    for (i, (id, rel, transcript)) in rows.iter().enumerate() {
        let audio = corpus::audio::decode_file(&args.wav_root.join(rel))
            .unwrap_or_else(|e| panic!("decode {}: {e}", rel.display()));
        let mut pcm = corpus::audio::to_mono(&audio);
        if audio.sample_rate != 16_000 {
            pcm = corpus::audio::resample_linear(&pcm, audio.sample_rate, 16_000);
        }
        let audio_sec = pcm.len() as f64 / 16_000.0;
        let started = Instant::now();
        let lattice = decoder.decode(&pcm);
        let decode_sec = started.elapsed().as_secs_f64();
        let hyp: Vec<String> = lattice.tokens.iter().map(|t| t.word.clone()).collect();
        let reference: Vec<String> = transcript.split_whitespace().map(str::to_string).collect();
        let score = output::wer(&reference, &hyp);
        let errs = (score.substitutions + score.deletions + score.insertions) as u32;
        errors.push(errs);
        denoms.push(score.reference_len as u32);
        for sent in lm::normalize_text(transcript) {
            for tok in sent {
                total_ref += 1;
                if !vocab_upper.contains(&tok.to_uppercase()) {
                    oov_ref += 1;
                }
            }
        }
        total_audio += audio_sec;
        total_decode += decode_sec;
        out.push_str(&format!(
            "{id}\t{errs}\t{}\t{}\t{audio_sec:.3}\t{decode_sec:.3}\t{}\n",
            score.reference_len,
            hyp.len(),
            hyp.join(" ")
        ));
        if (i + 1) % 100 == 0 {
            let done_errs: u64 = errors.iter().map(|&e| e as u64).sum();
            let done_ref: u64 = denoms.iter().map(|&d| d as u64).sum();
            eprintln!(
                "progress {}/{} utt WER {:.4} RTF {:.3}",
                i + 1,
                rows.len(),
                done_errs as f64 / done_ref.max(1) as f64,
                total_decode / total_audio.max(1e-9)
            );
        }
    }
    std::fs::write(&args.out, &out).unwrap_or_else(|e| panic!("write {}: {e}", args.out.display()));

    // ---- summary: corpus WER + paired bootstrap CI + RTF.
    let zeros = vec![0u32; errors.len()];
    let ci = eval::corpus_delta_ci(&zeros, &errors, &denoms, eval::BootstrapConfig::default());
    let ref_words: u64 = denoms.iter().map(|&d| d as u64).sum();
    let err_total: u64 = errors.iter().map(|&e| e as u64).sum();
    println!("wer_devclean summary");
    println!("am {} sha256 {am_sha}", args.am.display());
    println!("lm {flavor} {} sha256 {lm_sha}", args.lm.display());
    println!(
        "vocab top_n={} kept_words={} kept_prons={} tier1={} tier2={} oov_lm={} oov_book_only={}",
        report.top_n_requested,
        report.kept_words,
        report.kept_prons,
        report.tier1_words,
        report.tier2_words,
        report.oov_lm,
        report.oov_book_only
    );
    println!(
        "utterances {} ref_words {ref_words} errors {err_total} WER {:.4}",
        errors.len(),
        ci.estimate
    );
    println!(
        "ci 95pct [{:.4}, {:.4}] resamples {} seed {}",
        ci.lo, ci.hi, ci.resamples, ci.seed
    );
    println!(
        "audio_hours {:.2} decode_hours {:.2} RTF {:.3}",
        total_audio / 3600.0,
        total_decode / 3600.0,
        total_decode / total_audio.max(1e-9)
    );
    println!(
        "ref_oov_rate {:.4} ({oov_ref}/{total_ref} normalized ref tokens outside vocab)",
        oov_ref as f64 / total_ref.max(1) as f64
    );
    println!("per_utt {}", args.out.display());
}
