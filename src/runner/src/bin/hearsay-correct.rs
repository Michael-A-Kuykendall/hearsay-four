//! hearsay-correct: user-correction demo without owner audio (lane G).
//!
//! Bead `hearsay-four-j1i0--correction-demo`: pick dev-clean utterances with
//! a clear single-word substitution, decode -> why-panel -> apply correction
//! (fabric USER fact + personal-LM count increment) -> re-decode shows fixed
//! -> undo restores byte-identical original. Classical pipeline only.
//!
//! Subcommands:
//! - `decode --utt <id>`: single-utterance decode with gate config, prints
//!   the hyp text plus the transcript JSON with why panels.
//! - `probe`: run the correction loop over every single-substitution
//!   candidate and report FIXED / NOFLIP per utterance (utterance picker).
//! - `demo`: the one-command gate: fixed demo utterances end to end (fix +
//!   undo asserts) plus the 20-utterance collateral report.
//!
//! Paths resolve flag > `HEARSAY_*` env > compiled default. Gate config:
//! CMVN 8-mix AM + interpolated R1-v2 LM, top_n 20000, lmw 6, wip 0.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

use fabric::correction::{CorrectionInput, CorrectionLedger};
use fabric::Fabric;
use runner::demo_transcribe::{demo_wav_root, DemoConfig, load_transcriber, Transcriber};

/// Every cell.tsv row with exactly one error and `ref_len == hyp_len`
/// (single-word substitution shape; refs verified against the
/// `LibriSpeech/dev-clean` `.trans.txt` files before the probe trusts them).
const PROBE_UTTS: [&str; 16] = [
    "1462-170145-0012",
    "1462-170145-0020",
    "2035-147961-0033",
    "2428-83699-0004",
    "2428-83699-0015",
    "251-118436-0001",
    "3170-137482-0033",
    "3752-4944-0041",
    "3752-4944-0045",
    "422-122949-0017",
    "5536-43358-0000",
    "6319-64726-0006",
    "6345-93306-0014",
    "7850-111771-0005",
    "7976-105575-0017",
    "8297-275154-0016",
];

/// Demo utterances (filled after the probe: R=1 FIXED, no other diffs).
const DEMO_UTTS: [&str; 4] = [
    "2428-83699-0004",
    "251-118436-0001",
    "3170-137482-0033",
    "3752-4944-0041",
];

/// Collateral utterances: short non-demo dev-clean utterances, decoded
/// before/after each demo correction.
const COLLATERAL_UTTS: [&str; 20] = [
    "1462-170142-0004",
    "1462-170142-0005",
    "1462-170142-0010",
    "1462-170142-0016",
    "1462-170145-0004",
    "1462-170145-0009",
    "1462-170145-0014",
    "1462-170145-0021",
    "2035-147960-0001",
    "2035-147960-0004",
    "2035-147960-0015",
    "2035-147960-0000",
    "2035-147961-0000",
    "2035-147961-0007",
    "2035-147961-0016",
    "2035-147961-0022",
    "2428-83699-0000",
    "2428-83699-0008",
    "2428-83699-0012",
    "2428-83699-0020",
];

struct Paths {
    cfg: DemoConfig,
    wav_root: PathBuf,
    trans_root: PathBuf,
    workdir: PathBuf,
    repeat: u64,
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Some(v.to_string());
        }
    }
    None
}

fn usage() -> ! {
    eprintln!(
        "usage: hearsay-correct [--am P] [--lm P] [--tier1 P] [--tier2 P] [--wav-root D] [--trans-root D] [--workdir D] [--repeat N] <decode --utt ID | probe | demo>"
    );
    std::process::exit(2);
}

fn resolve_paths(args: &[String]) -> Paths {
    let mut cfg = DemoConfig::default();
    if let Some(v) = flag(args, "--am") {
        cfg.am = PathBuf::from(v);
    }
    if let Some(v) = flag(args, "--lm") {
        cfg.lm = PathBuf::from(v);
    }
    if let Some(v) = flag(args, "--tier1") {
        cfg.tier1 = PathBuf::from(v);
    }
    if let Some(v) = flag(args, "--tier2") {
        cfg.tier2 = PathBuf::from(v);
    }
    cfg.top_n = 20_000;
    cfg.lm_weight = Some(6.0);
    cfg.word_penalty = Some(0.0);
    let wav_root = flag(args, "--wav-root")
        .map(PathBuf::from)
        .or_else(|| std::env::var("HEARSAY_WAV_ROOT").ok().map(PathBuf::from))
        .unwrap_or_else(demo_wav_root);
    let trans_root = flag(args, "--trans-root")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HEARSAY_TRANS_ROOT")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| {
            wav_root
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| wav_root.clone())
        });
    let workdir = flag(args, "--workdir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/correction-demo"));
    let repeat = flag(args, "--repeat")
        .map(|v| v.parse::<u64>().unwrap_or_else(|_| usage()))
        .unwrap_or(1)
        .max(1);
    Paths {
        cfg,
        wav_root,
        trans_root,
        workdir,
        repeat,
    }
}

fn wav_path(paths: &Paths, utt: &str) -> Result<PathBuf, String> {
    let mut parts = utt.split('-');
    let (spk, ch) = match (parts.next(), parts.next(), parts.next()) {
        (Some(s), Some(c), Some(_)) => (s, c),
        _ => return Err(format!("bad utt id {utt:?}")),
    };
    Ok(paths
        .wav_root
        .join("dev-clean")
        .join(spk)
        .join(ch)
        .join(format!("{utt}.wav")))
}

/// Reference words (UPPER-CASE) from the shipped `.trans.txt`.
fn ref_words(paths: &Paths, utt: &str) -> Result<Vec<String>, String> {
    let mut parts = utt.split('-');
    let (spk, ch) = match (parts.next(), parts.next(), parts.next()) {
        (Some(s), Some(c), Some(_)) => (s, c),
        _ => return Err(format!("bad utt id {utt:?}")),
    };
    let trans = paths
        .trans_root
        .join("LibriSpeech")
        .join("dev-clean")
        .join(spk)
        .join(ch)
        .join(format!("{spk}-{ch}.trans.txt"));
    let text =
        std::fs::read_to_string(&trans).map_err(|e| format!("read {}: {e}", trans.display()))?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(utt).filter(|r| r.starts_with(' ')) {
            return Ok(rest.split_whitespace().map(str::to_string).collect());
        }
    }
    Err(format!("utt {utt} not in {}", trans.display()))
}

fn hyp_words(lat: &lattice::Lattice) -> Vec<String> {
    lat.tokens.iter().map(|t| t.word.clone()).collect()
}

/// Index of the single substitution, iff `hyp`/`reference` differ in exactly
/// one position at equal length.
fn single_sub(hyp: &[String], reference: &[String]) -> Option<usize> {
    if hyp.len() != reference.len() {
        return None;
    }
    let mut diff = None;
    for (i, (h, r)) in hyp.iter().zip(reference.iter()).enumerate() {
        if h != r {
            if diff.is_some() {
                return None;
            }
            diff = Some(i);
        }
    }
    diff
}

fn decode_timed(tx: &Transcriber, pcm: &[i16]) -> (lattice::Lattice, f64) {
    let t0 = Instant::now();
    let lat = tx.decode(pcm);
    (lat, t0.elapsed().as_secs_f64())
}

/// Personal-LM count increment: fold `delta` x `repeat` into the personal
/// component's inspectable counts (exact `lm::InspectableModel` keying) and
/// rebuild via `KnTrigram::from_inspectable` (discounts/continuations
/// re-derived by `finalize`, deterministically).
fn corrected_personal(
    personal: &lm::KnTrigram,
    sentence: &[String],
    repeat: u64,
) -> Result<lm::KnTrigram, String> {
    let mut snap = personal.inspectable();
    let mut padded: Vec<&str> = vec![lm::BOS_TOKEN, lm::BOS_TOKEN];
    padded.extend(sentence.iter().map(String::as_str));
    padded.push(lm::EOS_TOKEN);
    let mut vocab: BTreeSet<String> = snap.vocabulary.into_iter().collect();
    let mut uni_add = 0u64;
    for w in &padded {
        if *w != lm::BOS_TOKEN {
            vocab.insert((*w).to_string());
            *snap.unigram_counts.entry((*w).to_string()).or_insert(0) += repeat;
            uni_add += repeat;
        }
    }
    for pair in padded.windows(2) {
        *snap
            .bigram_counts
            .entry(format!("{} {}", pair[0], pair[1]))
            .or_insert(0) += repeat;
    }
    for triple in padded.windows(3) {
        *snap
            .trigram_counts
            .entry(format!("{} {} {}", triple[0], triple[1], triple[2]))
            .or_insert(0) += repeat;
    }
    snap.vocabulary = vocab.into_iter().collect();
    snap.total_tokens += uni_add;
    lm::KnTrigram::from_inspectable(&snap)
}

struct Correction {
    event: u64,
    token: u64,
    variant: Option<u64>,
    lm_path: PathBuf,
    tri_key: String,
    tri_before: u64,
    tri_after: u64,
    prob_before: f64,
    prob_after: f64,
    state_before: Vec<u8>,
    ledger: CorrectionLedger,
    fabric: Fabric,
}

/// Apply one correction: USER fact set in the fabric/ledger plus a rebuilt
/// interpolated LM (personal + corrected-sentence counts) saved under
/// `workdir`. Returns the facts and the corrected-LM path, not a decoder:
/// the caller reloads (proving the artifact is self-sufficient) and undo
/// drops the ledger record (proving exact retraction).
#[allow(clippy::too_many_arguments)]
fn apply_correction(
    lexicon: &lexicon::TieredLexicon,
    base_lm: &lm::InterpolatedLm,
    lat: &lattice::Lattice,
    sub_idx: usize,
    reference: &[String],
    workdir: &std::path::Path,
    utt: &str,
    repeat: u64,
) -> Result<Correction, String> {
    let hyp = hyp_words(lat);
    let token = &lat.tokens[sub_idx];
    let sentence: Vec<String> = lm::normalize::normalize_sentence(&reference.join(" "));
    if sentence.len() != reference.len() {
        return Err(format!(
            "utt {utt}: ref normalization changed length ({} -> {})",
            reference.len(),
            sentence.len()
        ));
    }
    let corrected_word = sentence[sub_idx].clone();
    let observed_phones = lexicon
        .table
        .get(&hyp[sub_idx])
        .and_then(|v| v.first())
        .cloned()
        .ok_or_else(|| format!("utt {utt}: hyp word {} has no lexicon entry", hyp[sub_idx]))?;
    let lexicon_variants = lexicon.table.get(&reference[sub_idx]).cloned().unwrap_or_default();
    // Span in frames, half-open (lattice token frames are u32).
    let (start, end) = (u64::from(token.start), u64::from(token.end));
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(fse::FEATURE_DIM);
    let state_before = ledger.state_bytes();
    let applied = ledger
        .apply(
            &mut fabric,
            &CorrectionInput {
                corrected_word: corrected_word.clone(),
                corrected_sentence: sentence.clone(),
                start_frame: start,
                end_frame: end,
                observed_phones,
                lexicon_variants,
                map: Vec::new(),
            },
        )
        .map_err(|e| format!("utt {utt}: ledger apply: {e}"))?;
    // Trigram evidence for the corrected position (decoder history encoding:
    // last two lowercased predecessors, BOS-padded).
    let (h1, h2) = runner::demo_transcribe::lm_history_pair(&sentence[..sub_idx].to_vec());
    let tri_key = format!("{h1} {h2} {corrected_word}");
    let tri_before = base_lm.personal().count_trigram(&h1, &h2, &corrected_word);
    let personal2 = corrected_personal(base_lm.personal(), &sentence, repeat)?;
    let tri_after = personal2.count_trigram(&h1, &h2, &corrected_word);
    let prob_before = base_lm.prob(&h1, &h2, &corrected_word);
    let corrected =
        lm::InterpolatedLm::new(base_lm.general().clone(), personal2, base_lm.personal_weight())
            .map_err(|e| format!("utt {utt}: interpolate: {e}"))?;
    let prob_after = corrected.prob(&h1, &h2, &corrected_word);
    std::fs::create_dir_all(workdir).map_err(|e| format!("mkdir {}: {e}", workdir.display()))?;
    let lm_path = workdir.join(format!("lm-corrected-{utt}.json"));
    lm::persist::save_interpolated(&corrected, &lm_path)
        .map_err(|e| format!("utt {utt}: save corrected LM: {e}"))?;
    Ok(Correction {
        event: applied.event.0,
        token: applied.token.0,
        variant: applied.variant.map(|f| f.0),
        lm_path,
        tri_key,
        tri_before,
        tri_after,
        prob_before,
        prob_after,
        state_before,
        ledger,
        fabric,
    })
}

fn load_tx(paths: &Paths, lm: &std::path::Path) -> Result<Transcriber, String> {
    let mut cfg = paths.cfg.clone();
    cfg.lm = lm.to_path_buf();
    load_transcriber(&cfg)
}

fn fail(msg: String) -> ! {
    eprintln!("FAIL: {msg}");
    std::process::exit(1);
}

fn cmd_decode(paths: &Paths, args: &[String]) {
    let utt = flag(args, "--utt").unwrap_or_else(|| usage());
    let tx = load_tx(paths, &paths.cfg.lm.clone()).unwrap_or_else(|e| fail(e));
    let wav = wav_path(paths, &utt).unwrap_or_else(|e| fail(e));
    let pcm = corpus::audio::decode_to_pcm16k(&wav)
        .unwrap_or_else(|e| fail(format!("{}: {e}", wav.display())));
    let audio_secs = pcm.frames.len() as f64 / 16_000.0;
    let (lat, secs) = decode_timed(&tx, &pcm.frames);
    let hyp = hyp_words(&lat).join(" ");
    println!("utt {utt}\nhyp {hyp}");
    let value = tx.transcript_json_with_why(
        &utt,
        &wav.display().to_string(),
        audio_secs,
        secs,
        &lat,
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|e| fail(e.to_string()))
    );
}

/// One probe row per candidate: base hyp, single-sub slot, corrected hyp,
/// verdict. Shares the demo's apply path (not a parallel implementation).
fn cmd_probe(paths: &Paths) {
    let base_tx = load_tx(paths, &paths.cfg.clone().lm).unwrap_or_else(|e| fail(e));
    println!("base vocab {} flavor {}", base_tx.vocab_size(), base_tx.flavor);
    let lm_bytes =
        std::fs::read(&paths.cfg.lm).unwrap_or_else(|e| fail(format!("read base LM: {e}")));
    let base_lm = lm::persist::interpolated_from_bytes(&lm_bytes)
        .unwrap_or_else(|e| fail(format!("base LM not interpolated: {e}")));
    let lexicon = lexicon::load_tiered(&paths.cfg.tier1, &paths.cfg.tier2)
        .unwrap_or_else(|e| fail(format!("load tiered lexicon: {e}")));
    for utt in PROBE_UTTS {
        let wav = wav_path(paths, utt).unwrap_or_else(|e| fail(e));
        let reference = ref_words(paths, utt).unwrap_or_else(|e| fail(e));
        let pcm = corpus::audio::decode_to_pcm16k(&wav)
            .unwrap_or_else(|e| fail(format!("{utt}: {e} {}", wav.display())));
        let (lat0, _) = decode_timed(&base_tx, &pcm.frames);
        let hyp0 = hyp_words(&lat0);
        let Some(idx) = single_sub(&hyp0, &reference) else {
            println!("{utt} SKIP base hyp is not a single sub of ref\n  hyp0 {}\n  ref  {}", hyp0.join(" "), reference.join(" "));
            continue;
        };
        let corr = match apply_correction(
            &lexicon,
            &base_lm,
            &lat0,
            idx,
            &reference,
            &paths.workdir,
            utt,
            paths.repeat,
        ) {
            Ok(c) => c,
            Err(e) => {
                println!("{utt} APPLY-ERR {e}");
                continue;
            }
        };
        let fixed_tx = load_tx(paths, &corr.lm_path).unwrap_or_else(|e| fail(e));
        let (lat1, _) = decode_timed(&fixed_tx, &pcm.frames);
        let hyp1 = hyp_words(&lat1);
        let verdict = if hyp1 == reference {
            "FIXED"
        } else if hyp1.get(idx) == reference.get(idx) {
            "PARTIAL"
        } else {
            "NOFLIP"
        };
        println!(
            "{utt} {verdict} sub[{idx}] {}->{} tri[{}] {}->{} p {:.3e}->{:.3e} vocab {}->{}",
            hyp0[idx],
            reference[idx],
            corr.tri_key,
            corr.tri_before,
            corr.tri_after,
            corr.prob_before,
            corr.prob_after,
            base_tx.vocab_size(),
            fixed_tx.vocab_size(),
        );
        println!("  hyp1 {}", hyp1.join(" "));
    }
}

fn cmd_demo(paths: &Paths) {
    let base_tx = load_tx(paths, &paths.cfg.clone().lm).unwrap_or_else(|e| fail(e));
    println!("base vocab {} flavor {}", base_tx.vocab_size(), base_tx.flavor);
    let lm_bytes =
        std::fs::read(&paths.cfg.lm).unwrap_or_else(|e| fail(format!("read base LM: {e}")));
    let base_lm = lm::persist::interpolated_from_bytes(&lm_bytes)
        .unwrap_or_else(|e| fail(format!("base LM not interpolated: {e}")));
    let lexicon = lexicon::load_tiered(&paths.cfg.tier1, &paths.cfg.tier2)
        .unwrap_or_else(|e| fail(format!("load tiered lexicon: {e}")));
    let mut failures = 0;
    for utt in DEMO_UTTS {
        println!("=== {utt} ===");
        let wav = wav_path(paths, utt).unwrap_or_else(|e| fail(e));
        let reference = ref_words(paths, utt).unwrap_or_else(|e| fail(e));
        let pcm = corpus::audio::decode_to_pcm16k(&wav)
            .unwrap_or_else(|e| fail(format!("{utt}: {e} {}", wav.display())));
        let audio_secs = pcm.frames.len() as f64 / 16_000.0;
        let (lat0, secs0) = decode_timed(&base_tx, &pcm.frames);
        let hyp0 = hyp_words(&lat0);
        let bytes0 = lattice::write_lattice(&lat0).unwrap_or_else(|e| fail(e));
        println!("before: {}", hyp0.join(" "));
        // Why panel for the (single) wrong word.
        let Some(idx) = single_sub(&hyp0, &reference) else {
            println!("FAIL {utt}: base hyp is not a single sub of ref");
            failures += 1;
            continue;
        };
        let why = base_tx.transcript_json_with_why(
            utt,
            &wav.display().to_string(),
            audio_secs,
            secs0,
            &lat0,
        );
        println!(
            "why[{}] {}",
            hyp0[idx],
            serde_json::to_string(&why["words"][idx]).unwrap_or_default()
        );
        // Apply: USER fact + personal-LM count increment, then re-decode.
        let mut corr = match apply_correction(
            &lexicon,
            &base_lm,
            &lat0,
            idx,
            &reference,
            &paths.workdir,
            utt,
            paths.repeat,
        ) {
            Ok(c) => c,
            Err(e) => {
                println!("FAIL {utt}: apply: {e}");
                failures += 1;
                continue;
            }
        };
        println!(
            "applied event={} token={} variant={:?} tri[{}] {}->{} p {:.3e}->{:.3e}",
            corr.event,
            corr.token,
            corr.variant,
            corr.tri_key,
            corr.tri_before,
            corr.tri_after,
            corr.prob_before,
            corr.prob_after
        );
        let fixed_tx = load_tx(paths, &corr.lm_path).unwrap_or_else(|e| fail(e));
        let (lat1, _) = decode_timed(&fixed_tx, &pcm.frames);
        let hyp1 = hyp_words(&lat1);
        println!("after:  {}", hyp1.join(" "));
        if hyp1 != reference {
            println!("FAIL {utt}: corrected hyp != ref\n  ref {}", reference.join(" "));
            failures += 1;
            continue;
        }
        println!("fix verified: corrected hyp == ref");
        // Undo: retract the event, reload the base artifact, re-decode.
        let event = fabric::FactId(corr.event);
        if let Err(e) = corr.ledger.undo(&mut corr.fabric, event) {
            println!("FAIL {utt}: undo: {e}");
            failures += 1;
            continue;
        }
        if corr.ledger.state_bytes() != corr.state_before {
            println!("FAIL {utt}: ledger state not bit-identical after undo");
            failures += 1;
            continue;
        }
        let totals = corr.ledger.lm_totals();
        if !totals.unigram.is_empty() || !totals.bigram.is_empty() || !totals.trigram.is_empty() {
            println!("FAIL {utt}: lm totals nonempty after undo");
            failures += 1;
            continue;
        }
        let (lat2, _) = decode_timed(&base_tx, &pcm.frames);
        let bytes2 = lattice::write_lattice(&lat2).unwrap_or_else(|e| fail(e));
        if bytes2 != bytes0 {
            println!("FAIL {utt}: post-undo lattice differs from original");
            failures += 1;
            continue;
        }
        println!("undo verified: post-undo decode byte-identical to original");
    }
    // Collateral: the same base decoder before/after each demo correction.
    println!("=== collateral (20 utts, base vs corrected) ===");
    for utt in DEMO_UTTS {
        let lm_path = paths.workdir.join(format!("lm-corrected-{utt}.json"));
        if !lm_path.exists() {
            println!("collateral {utt}: no corrected LM (demo failed above), skipped");
            continue;
        }
        let fixed_tx = load_tx(paths, &lm_path).unwrap_or_else(|e| fail(e));
        let mut changed = 0;
        for c in COLLATERAL_UTTS {
            let wav = wav_path(paths, c).unwrap_or_else(|e| fail(e));
            let pcm = corpus::audio::decode_to_pcm16k(&wav)
                .unwrap_or_else(|e| fail(format!("{c}: {e} {}", wav.display())));
            let (l0, _) = decode_timed(&base_tx, &pcm.frames);
            let (l1, _) = decode_timed(&fixed_tx, &pcm.frames);
            let (h0, h1) = (hyp_words(&l0), hyp_words(&l1));
            if h0 != h1 {
                changed += 1;
                let words: Vec<String> = h0
                    .iter()
                    .zip(h1.iter())
                    .filter(|(a, b)| a != b)
                    .map(|(a, b)| format!("{a}->{b}"))
                    .collect();
                println!("  {c}: CHANGED [{}]", words.join(" "));
            }
        }
        println!("collateral {utt}: {changed}/20 utterances changed");
    }
    if failures > 0 {
        fail(format!("{failures} demo utterance(s) failed"));
    }
    println!("DEMO OK: all fixes + undo verified, collateral report printed");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    let sub = args
        .iter()
        .find(|a| *a == "decode" || *a == "probe" || *a == "demo")
        .cloned()
        .unwrap_or_else(|| usage());
    let paths = resolve_paths(&args);
    println!(
        "hearsay-correct {} repeat={} workdir={}",
        sub,
        paths.repeat,
        paths.workdir.display()
    );
    match sub.as_str() {
        "decode" => cmd_decode(&paths, &args),
        "probe" => cmd_probe(&paths),
        "demo" => cmd_demo(&paths),
        _ => usage(),
    }
}
