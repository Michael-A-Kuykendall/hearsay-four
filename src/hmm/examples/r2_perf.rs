//! R2-perf cost probe (hearsay-four-1c9--mfcc-viterbi-speed).
//!
//! Measures MFCC + Viterbi/Baum-Welch per-audio-hour cost on real 16 kHz
//! LibriSpeech WAVs through the exact R3 training path:
//! `corpus::audio` decode -> `fse` window/MFCC -> `HmmSet::flat_start` ->
//! `baum_welch` iterations + `forced_align` (Viterbi) pass.
//!
//! Usage:
//! `r2_perf <manifest.tsv> <wav_root> <max_hours> <bw_repeats>`
//!
//! Manifest rows (written by the corpus conversion step):
//! `utt_id<TAB>relpath<TAB>seconds<TAB>text`
//! Selection is manifest order until cumulative audio reaches `max_hours`.
//! Prints `KEY=VALUE` lines plus a human summary on stderr.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::audio::{decode_file, to_mono};
use fse::{FEATURE_DIM, extract_features, window_audio};
use hmm::{HmmSet, TrainConfig, UttData};
use lexicon::{TIER1_IDENTITY, TieredLexicon, load_tiered};

type Lexicon = HashMap<String, Vec<Vec<String>>>;

fn lexicon_table_path() -> PathBuf {
    if let Ok(root) = std::env::var("BETTER_VOICE_ROOT") {
        let cand =
            PathBuf::from(root).join("data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt");
        if cand.is_file() {
            return cand;
        }
    }
    let lane = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    lane.join("../better-voice/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt")
}

fn librispeech_lexicon_path() -> PathBuf {
    if let Ok(path) = std::env::var("LIBRISPEECH_LEXICON") {
        let cand = PathBuf::from(path);
        if cand.is_file() {
            return cand;
        }
    }
    let lane = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    lane.join("data/lexicon/librispeech-lexicon.txt")
}

fn load_lexicon() -> TieredLexicon {
    let tier1 = lexicon_table_path();
    let tier2 = librispeech_lexicon_path();
    load_tiered(&tier1, &tier2).unwrap_or_else(|e| {
        panic!(
            "read tiered lexicon {} + {}: {e}",
            tier1.display(),
            tier2.display()
        )
    })
}

fn norm_token(tok: &str) -> String {
    let mut w = tok.trim().to_uppercase();
    if w.ends_with(')')
        && let Some(open) = w.rfind('(')
        && w[open + 1..w.len() - 1].chars().all(|c| c.is_ascii_digit())
    {
        w.truncate(open);
    }
    while w.ends_with('\'') {
        w.pop();
    }
    w
}

fn strip_edge_punct(w: &str) -> String {
    w.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '(' | ')' | '[' | ']' | '-' | '—' | '–'
        )
    })
    .to_string()
}

fn lex_lookup(lex: &Lexicon, tok: &str) -> Option<(String, Vec<String>)> {
    let w = norm_token(tok);
    if w.is_empty() {
        return None;
    }
    let mut cands = vec![w.clone()];
    let stripped = strip_edge_punct(&w);
    if stripped != w {
        cands.push(stripped);
    }
    let mut extra = Vec::new();
    for c in &cands {
        if let Some(base) = c.strip_suffix("'S")
            && base.len() >= 2
        {
            extra.push(base.to_string());
        }
    }
    cands.extend(extra);
    for c in cands {
        if c.is_empty() {
            continue;
        }
        if let Some(prons) = lex.get(&c)
            && let Some(first) = prons.first()
        {
            return Some((c, first.clone()));
        }
    }
    None
}

fn phones_for_train(lex: &Lexicon, text: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let toks: Vec<&str> = text.split_whitespace().collect();
    if toks.is_empty() {
        return None;
    }
    for t in toks {
        out.extend(lex_lookup(lex, t)?.1);
    }
    Some(out)
}

/// Count-only type/token census of free text against the two-tier lexicon.
///
/// Tokenizes with `lm::normalize_text` (the exact DATA-1 census tokenizer),
/// looks every word token up with the same [`lex_lookup`] candidate chain as
/// the utterance census, and attributes each hit to its tier via the table
/// provenance. Prints `wordscan_*` COUNTS ONLY — never the words themselves —
/// so it is safe to run over personal text (e.g. the owner-book stripped
/// text) and paste the output into beads or notes.
fn wordlist_scan(text_path: &str, tier1_path: &str, tier2_path: &str) {
    let tiered = load_tiered(tier1_path, tier2_path).expect("read tiered lexicon");
    let text = std::fs::read_to_string(text_path).expect("read text");
    let sents = lm::normalize_text(&text);
    let mut tok_total = 0usize;
    let mut num_tokens = 0usize;
    let mut tier1_tok = 0usize;
    let mut tier2_tok = 0usize;
    let mut miss_tok = 0usize;
    let mut tier_of_type: HashMap<String, u8> = HashMap::new();
    let mut types: HashSet<String> = HashSet::new();
    for sent in &sents {
        for tok in sent {
            tok_total += 1;
            if tok == lm::NUM_TOKEN {
                num_tokens += 1;
                continue;
            }
            let up = tok.to_uppercase();
            types.insert(up.clone());
            let hit = lex_lookup(&tiered.table, &up)
                .and_then(|(word, _)| tiered.sources.get(&word).copied());
            match hit {
                Some(lexicon::LexiconSource::Cmudict) => {
                    tier1_tok += 1;
                    tier_of_type.entry(up).or_insert(1);
                }
                Some(lexicon::LexiconSource::LibrispeechG2p) => {
                    tier2_tok += 1;
                    tier_of_type.entry(up).or_insert(2);
                }
                None => {
                    miss_tok += 1;
                    tier_of_type.entry(up).or_insert(0);
                }
            }
        }
    }
    // Per-type tier is first-seen attribution; token counts are exact.
    let tier1_types = tier_of_type.values().filter(|v| **v == 1).count();
    let tier2_types = tier_of_type.values().filter(|v| **v == 2).count();
    let miss_types = tier_of_type.values().filter(|v| **v == 0).count();
    println!("wordscan_sentences={}", sents.len());
    println!("wordscan_tokens_total={tok_total}");
    println!("wordscan_num_tokens={num_tokens}");
    println!("wordscan_word_tokens={}", tok_total - num_tokens);
    println!("wordscan_types_total={}", types.len());
    println!("wordscan_tier1_types={tier1_types}");
    println!("wordscan_tier1_tokens={tier1_tok}");
    println!("wordscan_tier2_types={tier2_types}");
    println!("wordscan_tier2_tokens={tier2_tok}");
    println!("wordscan_missing_types={miss_types}");
    println!("wordscan_missing_tokens={miss_tok}");
}

struct Row {
    id: String,
    rel: String,
    seconds: f64,
    text: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // ---- wordlist-scan mode: count-only free-text census, no audio ----
    if args.len() == 5 && args[1] == "wordlist-scan" {
        wordlist_scan(&args[2], &args[3], &args[4]);
        return;
    }
    assert!(
        args.len() == 5 || (args.len() == 6 && args[5] == "lexscan"),
        "usage: r2_perf <manifest.tsv> <wav_root> <max_hours> <bw_repeats> [lexscan] | r2_perf wordlist-scan <text.txt> <tier1.tsv> <tier2.txt>"
    );
    let manifest_path = PathBuf::from(&args[1]);
    let wav_root = PathBuf::from(&args[2]);
    let max_hours: f64 = args[3].parse().expect("max_hours");
    let bw_repeats: usize = args[4].parse().expect("bw_repeats");
    assert!(max_hours > 0.0 && bw_repeats >= 1);

    let tiered = load_lexicon();
    let (tier1_words, tier2_words) = (tiered.tier1_words(), tiered.tier2_words());
    eprintln!(
        "lexicon words: {} (tier1 {tier1_words} cmudict + tier2 {tier2_words} librispeech-g2p; tier1: {TIER1_IDENTITY})",
        tiered.table.len(),
    );
    let lex: Lexicon = tiered.table;

    let text = std::fs::read_to_string(&manifest_path).expect("read manifest");
    let mut rows = Vec::new();
    let mut budget = 0.0;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut cols = line.splitn(4, '\t');
        let (id, rel, sec, txt) = (
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("").to_string(),
            cols.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0),
            cols.next().unwrap_or("").to_string(),
        );
        if id.is_empty() || rel.is_empty() || sec <= 0.0 {
            continue;
        }
        rows.push(Row {
            id,
            rel,
            seconds: sec,
            text: txt,
        });
        budget += sec;
        if budget >= max_hours * 3600.0 {
            break;
        }
    }
    eprintln!(
        "manifest selected: {} utts, {:.3}h audio",
        rows.len(),
        budget / 3600.0
    );

    // ---- lexicon-scan mode: text-only OOV census, no audio, no training ----
    if args.len() > 5 && args[5] == "lexscan" {
        let mut drop = 0usize;
        let mut tok_total = 0usize;
        let mut oov_map: BTreeMap<String, usize> = BTreeMap::new();
        let mut kept_sec = 0.0;
        for r in &rows {
            let toks: Vec<&str> = r.text.split_whitespace().collect();
            tok_total += toks.len();
            if phones_for_train(&lex, &r.text).is_none() {
                drop += 1;
                for t in &toks {
                    if lex_lookup(&lex, t).is_none() {
                        *oov_map.entry(norm_token(t)).or_insert(0) += 1;
                    }
                }
            } else {
                kept_sec += r.seconds;
            }
        }
        let oov_tok: usize = oov_map.values().sum();
        println!("scan_utts={}", rows.len());
        println!("scan_audio_h={:.4}", budget / 3600.0);
        println!("scan_kept_h={:.4}", kept_sec / 3600.0);
        println!("scan_utt_dropped={drop}");
        println!("scan_oov_tokens={oov_tok}");
        println!("scan_tokens_total={tok_total}");
        println!("scan_oov_types={}", oov_map.len());
        println!("scan_tier1_words={tier1_words}");
        println!("scan_tier2_words={tier2_words}");
        let mut top: Vec<(&String, &usize)> = oov_map.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let top30: Vec<String> = top
            .iter()
            .take(30)
            .map(|(w, c)| format!("{w}x{c}"))
            .collect();
        println!("scan_oov_top30={}", top30.join(","));
        return;
    }

    // ---- MFCC stage (decode + window + MFCC+deltas), timed as one block ----
    let t0 = Instant::now();
    let mut data: Vec<UttData> = Vec::new();
    let mut skipped: BTreeMap<&str, usize> = BTreeMap::new();
    let mut oov: BTreeMap<String, usize> = BTreeMap::new();
    let mut oov_tokens_total: usize = 0;
    let mut audio_sec = 0.0;
    let mut rate_seen = BTreeSet::new();
    for r in &rows {
        let pcm = match decode_file(&wav_root.join(&r.rel)) {
            Ok(p) => p,
            Err(_) => {
                *skipped.entry("decode").or_insert(0) += 1;
                continue;
            }
        };
        rate_seen.insert(pcm.sample_rate);
        let mono = to_mono(&pcm);
        if mono.is_empty() {
            *skipped.entry("no_frames").or_insert(0) += 1;
            continue;
        }
        let windows = window_audio(&mono, pcm.sample_rate);
        if windows.is_empty() {
            *skipped.entry("no_frames").or_insert(0) += 1;
            continue;
        }
        let frames = match extract_features(&windows, pcm.sample_rate) {
            Ok(f) => f,
            Err(_) => {
                *skipped.entry("no_frames").or_insert(0) += 1;
                continue;
            }
        };
        if frames.is_empty() || frames[0].len() != FEATURE_DIM {
            *skipped.entry("no_frames").or_insert(0) += 1;
            continue;
        }
        let toks: Vec<&str> = r.text.split_whitespace().collect();
        oov_tokens_total += toks.len();
        let Some(phones) = phones_for_train(&lex, &r.text) else {
            *skipped.entry("lexicon_oov").or_insert(0) += 1;
            for t in &toks {
                if lex_lookup(&lex, t).is_none() {
                    *oov.entry(norm_token(t)).or_insert(0) += 1;
                }
            }
            continue;
        };
        audio_sec += r.seconds;
        data.push((
            r.id.clone(),
            phones,
            frames.iter().map(|f| f.to_vec()).collect(),
        ));
    }
    let t_mfcc = t0.elapsed().as_secs_f64();
    let frames_total: usize = data.iter().map(|(_, _, o)| o.len()).sum();
    let phones_total: usize = data.iter().map(|(_, s, _)| s.len()).sum();
    eprintln!(
        "MFCC kept={} skipped={:?} rates={:?} audio_h={:.4} frames={} dim={FEATURE_DIM}",
        data.len(),
        skipped,
        rate_seen,
        audio_sec / 360.0 / 10.0,
        frames_total
    );
    assert!(!data.is_empty(), "no usable utterances");

    // ---- flat start ----
    let mut inv: BTreeSet<String> = BTreeSet::new();
    for (_, seq, _) in &data {
        inv.extend(seq.iter().cloned());
    }
    let inv_ref: Vec<&str> = inv.iter().map(String::as_str).collect();
    let obs: Vec<Vec<Vec<f64>>> = data.iter().map(|(_, _, o)| o.clone()).collect();
    let t1 = Instant::now();
    let mut model = HmmSet::flat_start(&inv_ref, &obs, 0.01).expect("flat_start");
    let t_flat = t1.elapsed().as_secs_f64();

    // ---- Viterbi forced-align pass over the subset (flat model) ----
    let t2 = Instant::now();
    let mut align_ll = 0.0;
    for (_, seq, o) in &data {
        let (_, ll) = model.forced_align(seq, o).expect("forced_align");
        align_ll += ll;
    }
    let t_viterbi = t2.elapsed().as_secs_f64();

    // ---- Baum-Welch iterations (sequential, like real training) ----
    let cfg = TrainConfig {
        bw_iterations: 1,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
    };
    let mut t_bw = Vec::new();
    for _ in 0..bw_repeats {
        let tb = Instant::now();
        let ll = model.baum_welch_ids(&data, &cfg).expect("baum_welch");
        t_bw.push((tb.elapsed().as_secs_f64(), ll));
    }

    // ---- dev-eval proxy: phone_error_rate over the subset (trained model) ----
    let dev_pairs: Vec<(Vec<String>, Vec<Vec<f64>>)> = data
        .iter()
        .map(|(_, s, o)| (s.clone(), o.clone()))
        .collect();
    let t3 = Instant::now();
    let per = model
        .phone_error_rate(&dev_pairs)
        .expect("phone_error_rate")
        .per;
    let t_per = t3.elapsed().as_secs_f64();

    // ---- mixture scaling: one BW iter at 2/4/8 mixes (split trained model) ----
    let mut t_bwmix: Vec<(usize, f64, f64)> = Vec::new();
    for &mixes in &[2usize, 4, 8] {
        let mut mm = model.clone();
        mm.split_mixtures_to(mixes);
        let tm = Instant::now();
        let llm = mm.baum_welch_ids(&data, &cfg).expect("baum_welch mix");
        t_bwmix.push((mixes, tm.elapsed().as_secs_f64(), llm));
    }

    let audio_h = audio_sec / 3600.0;
    println!("utts={}", data.len());
    println!("audio_h={audio_h:.4}");
    println!("frames={frames_total}");
    println!("phones_total={phones_total}");
    println!("phone_inventory={}", inv.len());
    let oov_tok: usize = oov.values().sum();
    println!(
        "oov_utt_dropped={}",
        skipped.get("lexicon_oov").copied().unwrap_or(0)
    );
    println!("oov_tokens={oov_tok}");
    println!("oov_tokens_total={oov_tokens_total}");
    println!("oov_word_types={}", oov.len());
    let mut top: Vec<(&String, &usize)> = oov.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let top20: Vec<String> = top
        .iter()
        .take(20)
        .map(|(w, c)| format!("{w}x{c}"))
        .collect();
    println!("oov_top20={}", top20.join(","));
    println!("t_mfcc_s={t_mfcc:.1}");
    println!("t_flat_s={t_flat:.1}");
    println!("t_viterbi_s={t_viterbi:.1}");
    println!("align_ll={align_ll:.1}");
    println!("t_pereval_s={t_per:.1}");
    println!("subset_per={per:.4}");
    for (mixes, t, ll) in &t_bwmix {
        println!("t_bw_{mixes}mix_s={t:.1}");
        println!("train_ll_{mixes}mix={ll:.1}");
    }
    for (i, (t, ll)) in t_bw.iter().enumerate() {
        println!("t_bw_iter{}_s={t:.1}", i + 1);
        println!("train_ll_iter{}={ll:.1}", i + 1);
    }
    println!("mfcc_per_h_s={:.1}", t_mfcc / audio_h);
    println!("viterbi_per_h_s={:.1}", t_viterbi / audio_h);
    println!("pereval_per_h_s={:.1}", t_per / audio_h);
    for (mixes, t, _) in &t_bwmix {
        println!("bw_{mixes}mix_per_h_s={:.1}", t / audio_h);
    }
    for (i, (t, _)) in t_bw.iter().enumerate() {
        println!("bw_iter{}_per_h_s={:.1}", i + 1, t / audio_h);
    }
    eprintln!(
        "PER-HOUR mfcc={:.0}s viterbi={:.0}s bw1={:.0}s (audio_h={audio_h:.3}, utts={}, frames={})",
        t_mfcc / audio_h,
        t_viterbi / audio_h,
        t_bw[0].0 / audio_h,
        data.len(),
        frames_total
    );
}
