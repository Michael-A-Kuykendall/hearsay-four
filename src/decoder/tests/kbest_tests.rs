//! K-best word lattice tests (bead hearsay-four-noqu--decoder-kbest-word-lattice).
//!
//! Setup mirrors `src/decoder/examples/wer_devclean.rs`: 8-mix CMVN acoustic
//! model, interpolated R1 LM (top 20k + book words), tiered lexicon. Input
//! data resolves to `<workspace>/data`, else `$HEARSAY_FOUR_ROOT/data`;
//! tests SKIP loudly when inputs are absent (no hard-coded home paths).
//! Lattice files are always written to `<workspace>/data/lattices/dev5/`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use decoder::{BookSource, DecoderConfig, MooreDecoder, Recognizer};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn am_relpath() -> &'static str {
    "models/r3v2-full93-cmvn/ckpt-8mix-iter2.json"
}

fn better_voice_root() -> PathBuf {
    for env in ["BETTER_VOICE_ROOT", "HEARSAY_BETTER_VOICE_ROOT"] {
        if let Ok(v) = std::env::var(env) {
            return PathBuf::from(v);
        }
    }
    workspace_root().join("../better-voice")
}

/// Input data dir: workspace first, then `$HEARSAY_FOUR_ROOT`; `None`
/// when neither holds the acoustic model (callers SKIP loudly).
fn data_root() -> Option<PathBuf> {
    let ws = workspace_root().join("data");
    if ws.join(am_relpath()).exists() {
        return Some(ws);
    }
    if let Ok(root) = std::env::var("HEARSAY_FOUR_ROOT") {
        let p = PathBuf::from(root).join("data");
        if p.join(am_relpath()).exists() {
            return Some(p);
        }
    }
    None
}

struct KbestInputs {
    am: PathBuf,
    lm: PathBuf,
    entries: Vec<decoder::PronEntry>,
}

fn kbest_inputs() -> Option<KbestInputs> {
    let Some(data) = data_root() else {
        eprintln!(
            "SKIP kbest_dev5_lattices: acoustic model absent (workspace data/ or $HEARSAY_FOUR_ROOT/data)"
        );
        return None;
    };
    let am = data.join(am_relpath());
    let lm = data.join("lm-text/r1v2-full/lm-r1v2-interpolated.json");
    let tier1 =
        better_voice_root().join("data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt");
    let tier2 = data.join("lexicon/librispeech-lexicon.txt");
    for p in [&am, &lm, &tier1, &tier2] {
        if !p.exists() {
            eprintln!(
                "SKIP kbest_dev5_lattices: kbest input missing: {} (workspace data/, $HEARSAY_FOUR_ROOT/data, or $BETTER_VOICE_ROOT sibling)",
                p.display()
            );
            return None;
        }
    }
    // Premise pin: the ACOUSTIC null needs the SIL phone in the AM.
    let am_set = hmm::HmmSet::load_text(&am).expect("load am for SIL check");
    assert!(am_set.phones.contains_key("SIL"), "AM lacks SIL phone");
    let lm_bytes = std::fs::read(&lm).expect("read lm");
    let ix = lm::persist::interpolated_from_bytes(&lm_bytes).expect("interpolated lm");
    let top = decoder::top_lm_words(ix.general(), Some(ix.personal()), 20_000);
    let book = decoder::book_types_from_personal(ix.personal());
    let lex = lexicon::load_tiered(&tier1, &tier2).expect("tiered lexicon");
    let (entries, report) =
        decoder::build_entries(&top, &book, 20_000, BookSource::PersonalLm, &lex);
    eprintln!(
        "kbest vocab kept_words={} kept_prons={} tier1={} tier2={} oov_lm={} oov_book_only={}",
        report.kept_words,
        report.kept_prons,
        report.tier1_words,
        report.tier2_words,
        report.oov_lm,
        report.oov_book_only
    );
    Some(KbestInputs { am, lm, entries })
}

/// First 5 rows, in order, of the strat200 manifest: (utt id, wav relpath).
fn dev5_rows() -> Option<Vec<(String, PathBuf)>> {
    let data = data_root()?;
    let text = std::fs::read_to_string(data.join("wer71g/tune200s/strat200.manifest.tsv")).ok()?;
    let rows: Vec<(String, PathBuf)> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .enumerate()
        .map(|(i, line)| {
            let mut parts = line.splitn(4, '\t');
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(id), Some(rel), Some(_), Some(_)) => (id.to_string(), PathBuf::from(rel)),
                _ => panic!("strat200 line {} malformed", i + 1),
            }
        })
        .collect();
    Some(rows)
}

fn load_pcm(wav_path: &Path) -> Vec<i16> {
    let audio = corpus::audio::decode_file(wav_path).expect("decode wav");
    let mut pcm = corpus::audio::to_mono(&audio);
    if audio.sample_rate != 16_000 {
        pcm = corpus::audio::resample_linear(&pcm, audio.sample_rate, 16_000);
    }
    assert!(!pcm.is_empty(), "empty pcm: {}", wav_path.display());
    pcm
}

/// Contract §7 conflict: spans overlap by > 50% of the shorter span.
fn conflicts(a: &lattice::Token, b: &lattice::Token) -> bool {
    let overlap = a.end.min(b.end).saturating_sub(a.start.max(b.start)) as f64;
    let shorter = (a.end - a.start).min(b.end - b.start) as f64;
    shorter > 0.0 && overlap / shorter > 0.5
}

/// A connected arc-path whose words equal `reference`: starts at frame 0
/// and ends at the lattice max end (same words can align several ways;
/// only the full-coverage alignment reproduces the 1-best transcript).
fn find_word_path(lat: &lattice::Lattice, reference: &[String]) -> Option<Vec<u64>> {
    if reference.is_empty() {
        return None;
    }
    let max_end = lat.tokens.iter().map(|t| t.end).max().unwrap_or(0);
    let mut succ: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut incoming: HashSet<u64> = HashSet::new();
    for a in &lat.arcs {
        succ.entry(a.from).or_default().push(a.to);
        incoming.insert(a.to);
    }
    let word_of: HashMap<u64, &str> = lat.tokens.iter().map(|t| (t.id, t.word.as_str())).collect();
    let end_of: HashMap<u64, u32> = lat.tokens.iter().map(|t| (t.id, t.end)).collect();
    #[allow(clippy::too_many_arguments)]
    fn dfs(
        cur: u64,
        idx: usize,
        reference: &[String],
        word_of: &HashMap<u64, &str>,
        end_of: &HashMap<u64, u32>,
        succ: &HashMap<u64, Vec<u64>>,
        max_end: u32,
        path: &mut Vec<u64>,
    ) -> bool {
        if idx == reference.len() {
            return end_of.get(&cur) == Some(&max_end);
        }
        if let Some(nexts) = succ.get(&cur) {
            for &n in nexts {
                if word_of.get(&n).is_some_and(|w| *w == reference[idx]) {
                    path.push(n);
                    if dfs(n, idx + 1, reference, word_of, end_of, succ, max_end, path) {
                        return true;
                    }
                    path.pop();
                }
            }
        }
        false
    }
    for t in &lat.tokens {
        if t.word == reference[0] && t.start == 0 && !incoming.contains(&t.id) {
            let mut path = vec![t.id];
            if dfs(
                t.id, 1, reference, &word_of, &end_of, &succ, max_end, &mut path,
            ) {
                return Some(path);
            }
        }
    }
    None
}

/// Shared lattice assertions: round-trip validity, 1-best containment with
/// a contiguous full-coverage path, and >1 alternative per region.
fn check_kbest_lattice(lat: &lattice::Lattice, ref_words: &[String], tag: &str) {
    assert!(!ref_words.is_empty(), "{tag}: empty 1-best reference");
    let text = lattice::write_lattice(lat).expect("write lattice");
    assert_eq!(lattice::read_lattice(&text).expect("read lattice"), *lat);
    let path = find_word_path(lat, ref_words).expect("1-best path in lattice");
    assert_eq!(path.len(), ref_words.len(), "{tag}: path length");
    let by_id: HashMap<u64, &lattice::Token> = lat.tokens.iter().map(|t| (t.id, t)).collect();
    assert_eq!(
        by_id[&path[0]].start, 0,
        "{tag}: path must start at frame 0"
    );
    for w in path.windows(2) {
        assert_eq!(by_id[&w[0]].end, by_id[&w[1]].start, "{tag}: contiguous");
    }
    let max_end = lat.tokens.iter().map(|t| t.end).max().unwrap_or(0);
    assert_eq!(
        by_id[path.last().unwrap()].end,
        max_end,
        "{tag}: full coverage"
    );
    let word_toks: Vec<&lattice::Token> = lat.tokens.iter().filter(|t| t.word != "SIL").collect();
    assert!(
        word_toks.len() > ref_words.len(),
        "{tag}: lattice must be richer than 1-best"
    );
    for t in &word_toks {
        let alts = lat.tokens.iter().filter(|u| conflicts(t, u)).count();
        assert!(alts > 1, "{tag}: token {} has no alternative", t.id);
        assert!(
            lat.tokens.iter().any(|u| u.word == "SIL"
                && u.start == t.start
                && u.end == t.end
                && u.acoustic_llr == 0.0),
            "{tag}: token {} lacks a zero-llr SIL twin",
            t.id
        );
    }
}

#[test]
fn kbest_demo_wav_contains_one_best_and_alternatives() {
    // Lane-local smoke: committed si_base AM + closed vocab + demo wav.
    let root = workspace_root();
    let am = root.join("models/si_base/model.json");
    let wav = root.join("data/demo/sample.wav");
    assert!(am.exists() && wav.exists(), "committed fixtures missing");
    let decoder = MooreDecoder::load(
        &am,
        None,
        lexicon::CLOSED_VOCABULARY_CMUDICT,
        DecoderConfig::default(),
    )
    .expect("load demo decoder");
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let mut pcm = corpus::audio::to_mono(&audio);
    if audio.sample_rate != 16_000 {
        pcm = corpus::audio::resample_linear(&pcm, audio.sample_rate, 16_000);
    }
    let one_best = decoder.decode(&pcm);
    let ref_words: Vec<String> = one_best.tokens.iter().map(|t| t.word.clone()).collect();
    let lat = decoder.decode_kbest(&pcm);
    check_kbest_lattice(&lat, &ref_words, "demo");
    // Determinism: separate decode, separate hash seeds, identical bytes.
    let again = decoder.decode_kbest(&pcm);
    assert_eq!(
        lattice::write_lattice(&again).expect("write"),
        lattice::write_lattice(&lat).expect("write")
    );
}

#[test]
fn kbest_dev5_lattices() {
    let Some(inputs) = kbest_inputs() else {
        return;
    };
    let decoder = MooreDecoder::load_with_vocab(
        &inputs.am,
        Some(&inputs.lm),
        &inputs.entries,
        DecoderConfig::default(),
    )
    .expect("load kbest decoder");
    eprintln!(
        "kbest decoder vocab={} skipped={}",
        decoder.vocab_size(),
        decoder.skipped_words().len()
    );
    let Some(data) = data_root() else {
        return;
    };
    let out_dir = workspace_root().join("data/lattices/dev5");
    std::fs::create_dir_all(&out_dir).expect("create dev5 dir");
    let Some(rows) = dev5_rows() else {
        eprintln!("SKIP kbest_dev5_lattices: strat200 manifest absent under data/");
        return;
    };
    assert_eq!(rows.len(), 5, "strat200 must yield 5 rows");
    for (utt, rel) in &rows {
        let started = std::time::Instant::now();
        let pcm = load_pcm(&data.join("librispeech/wav").join(rel));
        let one_best = decoder.decode(&pcm);
        let ref_words: Vec<String> = one_best.tokens.iter().map(|t| t.word.clone()).collect();
        let mut lat = decoder.decode_kbest(&pcm);
        lat.utterance = utt.clone();
        check_kbest_lattice(&lat, &ref_words, utt);
        let text = lattice::write_lattice(&lat).expect("write lattice");
        std::fs::write(out_dir.join(format!("{utt}.lat.jsonl")), &text).expect("write file");
        eprintln!(
            "{utt}: 1best={} tokens={} arcs={} decode={:.1}s",
            ref_words.len(),
            lat.tokens.len(),
            lat.arcs.len(),
            started.elapsed().as_secs_f64()
        );
    }
}
