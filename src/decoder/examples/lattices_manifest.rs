//! lattices_manifest: K-best word lattices for every row of a manifest.
//!
//! Bead `hearsay-four-ic1w--fabric-experiment-arms` (wave 2): generates the
//! DEV-200 and TEST-200 lattice sets the A/B/C experiment scores exactly
//! once. Decoder setup (vocab build, lexicon tiers, LM handling, config
//! flags) mirrors `wer_devclean` exactly so arm A (1-best) and arms B/C
//! (lattice) share one search; only the output differs (`decode_kbest`
//! plus `<utt>.lat.jsonl` per row).
//!
//! usage: lattices_manifest --am <model.json> --lm <lm-file> --tier1 <master.tsv>
//!   --tier2 <lex.txt> --wav-root <dir> --manifest <tsv> --out-dir <dir>
//!   [--top-n N] [--book <txt>] [--skip N] [--limit N] [--beam F]
//!   [--max-tokens N] [--lookahead-weight F] [--lm-weight F]
//!   [--word-penalty F] [--no-cmvn]

use decoder::{BookSource, DecoderConfig, MooreDecoder};
use std::path::PathBuf;
use std::time::Instant;

struct Args {
    am: PathBuf,
    lm: PathBuf,
    tier1: PathBuf,
    tier2: PathBuf,
    wav_root: PathBuf,
    manifest: PathBuf,
    out_dir: PathBuf,
    top_n: usize,
    book: Option<PathBuf>,
    skip: usize,
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
        "usage: lattices_manifest --am <model.json> --lm <lm-file> --tier1 <master.tsv> --tier2 <lex.txt> --wav-root <dir> --manifest <tsv> --out-dir <dir> [--top-n N] [--book <txt>] [--skip N] [--limit N] [--beam F] [--max-tokens N] [--lookahead-weight F] [--lm-weight F] [--word-penalty F] [--no-cmvn]"
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
        out_dir: PathBuf::from(req("--out-dir")),
        top_n: parse_usize("--top-n").unwrap_or(20_000),
        book: flag(&args, "--book").map(PathBuf::from),
        skip: parse_usize("--skip").unwrap_or(0),
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
    let decoder = MooreDecoder::load_with_vocab(&args.am, Some(&args.lm), &entries, cfg)
        .unwrap_or_else(|e| panic!("load decoder: {e}"));
    drop(entries);
    eprintln!(
        "decoder vocab {} skipped {} lm {} load {:.1}s",
        decoder.vocab_size(),
        decoder.skipped_words().len(),
        decoder.has_lm(),
        t0.elapsed().as_secs_f64()
    );

    // ---- manifest rows: utt-id, relpath, dur, transcript (tab-separated).
    let manifest_text = std::fs::read_to_string(&args.manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", args.manifest.display()));
    let mut rows: Vec<(String, PathBuf)> = Vec::new();
    for (lineno, line) in manifest_text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let (id, rel) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(id), Some(rel), Some(_), Some(_)) => (id.to_string(), rel.to_string()),
            _ => panic!(
                "manifest line {} malformed (want id, path, dur, transcript)",
                lineno + 1
            ),
        };
        rows.push((id, PathBuf::from(rel)));
    }
    let total_rows = rows.len();
    let rows: Vec<(String, PathBuf)> = rows
        .into_iter()
        .skip(args.skip)
        .take(args.limit.unwrap_or(usize::MAX))
        .collect();
    eprintln!(
        "utterances {}/{} (skip {} limit {:?})",
        rows.len(),
        total_rows,
        args.skip,
        args.limit
    );
    std::fs::create_dir_all(&args.out_dir)
        .unwrap_or_else(|e| panic!("mkdir {}: {e}", args.out_dir.display()));

    // ---- decode + write lattices. Fail fast: the experiment needs every file.
    let mut total_audio = 0.0f64;
    let mut total_decode = 0.0f64;
    let mut total_tokens = 0usize;
    let mut total_arcs = 0usize;
    for (i, (id, rel)) in rows.iter().enumerate() {
        let audio = corpus::audio::decode_file(&args.wav_root.join(rel))
            .unwrap_or_else(|e| panic!("decode {id} {}: {e}", rel.display()));
        let mut pcm = corpus::audio::to_mono(&audio);
        if audio.sample_rate != 16_000 {
            pcm = corpus::audio::resample_linear(&pcm, audio.sample_rate, 16_000);
        }
        let audio_sec = pcm.len() as f64 / 16_000.0;
        let started = Instant::now();
        let mut lattice = decoder.decode_kbest(&pcm);
        let decode_sec = started.elapsed().as_secs_f64();
        lattice.utterance = id.clone();
        let text = lattice::write_lattice(&lattice).unwrap_or_else(|e| panic!("write {id}: {e}"));
        let out_path = args.out_dir.join(format!("{id}.lat.jsonl"));
        std::fs::write(&out_path, &text)
            .unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
        total_audio += audio_sec;
        total_decode += decode_sec;
        total_tokens += lattice.tokens.len();
        total_arcs += lattice.arcs.len();
        if (i + 1) % 10 == 0 {
            eprintln!(
                "progress {}/{} utt RTF {:.3} tokens {} arcs {}",
                i + 1,
                rows.len(),
                total_decode / total_audio.max(1e-9),
                total_tokens,
                total_arcs
            );
        }
    }
    println!("lattices_manifest summary");
    println!("am {} sha256 {am_sha}", args.am.display());
    println!("lm {flavor} {} sha256 {lm_sha}", args.lm.display());
    println!(
        "utterances {} tokens {total_tokens} arcs {total_arcs} out {}",
        rows.len(),
        args.out_dir.display()
    );
    println!(
        "audio_hours {:.2} decode_hours {:.2} RTF {:.3}",
        total_audio / 3600.0,
        total_decode / 3600.0,
        total_decode / total_audio.max(1e-9)
    );
}
