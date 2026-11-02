//! End-to-end decoder tests: 30-word pin gate, trie load, WAV decode,
//! lattice round-trip, streaming partials, trait conformance.

use std::path::PathBuf;
use std::time::Instant;

use decoder::{DecoderConfig, MooreDecoder, Recognizer};

fn workspace_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn am_path() -> PathBuf {
    workspace_file("../../models/si_base/model.json")
}

fn sample_wav() -> PathBuf {
    workspace_file("../../data/demo/sample.wav")
}

fn load_decoder() -> MooreDecoder {
    MooreDecoder::load(
        &am_path(),
        None,
        lexicon::CLOSED_VOCABULARY_CMUDICT,
        DecoderConfig::default(),
    )
    .expect("decoder load on si_base + closed vocab")
}

/// Bead pin gate: all 30 closed-vocabulary words resolve to >=1 in-alphabet
/// phone string, and every phone exists in the si_base inventory.
#[test]
fn pin_gate_all_30_words_resolve() {
    let table = lexicon::ingest_sample(lexicon::CLOSED_VOCABULARY_CMUDICT);
    assert_eq!(table.len(), 30, "closed vocab word count");
    let am = hmm::HmmSet::load_text(&am_path()).expect("load si_base");
    for (word, prons) in &table {
        assert!(!prons.is_empty(), "{word}: no pronunciation");
        for pron in prons {
            assert!(
                lexicon::phones_in_alphabet(pron),
                "{word}: non-alphabet phone in {pron:?}"
            );
            for phone in pron {
                assert!(
                    am.phones.contains_key(phone),
                    "{word}: phone {phone} missing from si_base"
                );
            }
        }
    }
}

#[test]
fn decoder_loads_30_word_trie() {
    let decoder = load_decoder();
    assert_eq!(decoder.vocab_size(), 30);
    assert!(
        decoder.skipped_words().is_empty(),
        "skipped: {:?}",
        decoder.skipped_words()
    );
    assert!(!decoder.has_lm());
}

/// Linear resample to 16 kHz (test-only helper; decoder takes 16k PCM).
fn resample_to_16k(mono: &[i16], from_rate: u32) -> Vec<i16> {
    if from_rate == 16_000 {
        return mono.to_vec();
    }
    assert!(from_rate > 0);
    let ratio = 16_000.0 / from_rate as f64;
    let out_len = ((mono.len() as f64 * ratio).round() as usize).max(1);
    (0..out_len)
        .map(|i| {
            let pos = i as f64 / ratio;
            let lo = pos.floor() as usize;
            let hi = (lo + 1).min(mono.len() - 1);
            let frac = pos - lo as f64;
            (mono[lo] as f64 * (1.0 - frac) + mono[hi] as f64 * frac).round() as i16
        })
        .collect()
}

/// Oct 31 checkpoint: a WAV decodes end to end into a valid A5 lattice.
/// Word output is unchecked today (plumbing proven, not accuracy).
#[test]
fn decode_wav_end_to_end() {
    let wav = sample_wav();
    assert!(wav.exists(), "demo wav missing: {}", wav.display());
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let mono = corpus::audio::to_mono(&audio);
    let pcm16k = resample_to_16k(&mono, audio.sample_rate);
    assert!(!pcm16k.is_empty());

    let decoder = load_decoder();
    let started = Instant::now();
    let lattice = decoder.decode(&pcm16k);
    let elapsed = started.elapsed();
    let audio_sec = pcm16k.len() as f64 / 16_000.0;
    let rtf = elapsed.as_secs_f64() / audio_sec;
    let words: Vec<&str> = lattice.tokens.iter().map(|t| t.word.as_str()).collect();
    eprintln!(
        "decoded {} tokens: {}",
        lattice.tokens.len(),
        words.join(" ")
    );
    eprintln!(
        "audio {audio_sec:.2}s, decode {:.2}s, RTF {rtf:.3}",
        elapsed.as_secs_f64()
    );

    assert!(
        !lattice.tokens.is_empty(),
        "expected >=1 token from demo wav"
    );
    assert_eq!(lattice.frame_shift_ms, 10.0);
    // Linear 1-best chain: ids sequential, arcs link neighbors.
    for (i, token) in lattice.tokens.iter().enumerate() {
        assert_eq!(token.id, (i + 1) as u64);
        assert!(token.start < token.end, "token {} span", token.id);
        assert!(token.acoustic_llr.is_finite());
    }
    assert_eq!(lattice.arcs.len(), lattice.tokens.len().saturating_sub(1));
    // A5 round-trip.
    let text = lattice::write_lattice(&lattice).expect("write lattice");
    let back = lattice::read_lattice(&text).expect("read lattice");
    assert_eq!(back, lattice);
    // Skeleton perf bound (generous; acceptance tightens to RTF < 1).
    assert!(rtf < 5.0, "RTF {rtf:.3} exceeds skeleton bound");
}

#[test]
fn decode_empty_and_short_is_empty() {
    let decoder = load_decoder();
    let empty = decoder.decode(&[]);
    assert!(empty.tokens.is_empty());
    assert!(empty.arcs.is_empty());
    let short = decoder.decode(&[0i16; 100]);
    assert!(short.tokens.is_empty());
    // Empty lattice still serializes.
    assert!(lattice::write_lattice(&empty).is_ok());
}

#[test]
fn stream_returns_growing_partial() {
    let wav = sample_wav();
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let pcm16k = resample_to_16k(&corpus::audio::to_mono(&audio), audio.sample_rate);
    let mut decoder = load_decoder();
    let mut ends: Vec<u32> = Vec::new();
    for chunk in pcm16k.chunks(8000) {
        let partials = decoder.stream(chunk);
        assert_eq!(partials.len(), 1);
        ends.push(partials[0].frame_end);
    }
    assert!(ends.len() >= 2);
    for pair in ends.windows(2) {
        assert!(pair[1] >= pair[0], "frame_end regressed: {ends:?}");
    }
    decoder.reset_stream();
    let fresh = decoder.stream(&pcm16k[..8000.min(pcm16k.len())]);
    assert_eq!(fresh.len(), 1);
}

#[test]
fn recognizer_is_object_safe() {
    let decoder = load_decoder();
    let boxed: Box<dyn Recognizer> = Box::new(decoder);
    let lattice = boxed.decode(&[0i16; 16_000]);
    let _ = lattice;
}

/// LM-inside-search path: a persist-format LM file loads and scores word
/// ends (trained here on toy sentences; effect direction unchecked).
#[test]
fn decode_with_lm_file_inside_search() {
    let sentences: Vec<Vec<String>> = vec![
        "the intense reflection"
            .split(' ')
            .map(str::to_string)
            .collect(),
        "hotbed ripple survival"
            .split(' ')
            .map(str::to_string)
            .collect(),
        "intense survival".split(' ').map(str::to_string).collect(),
    ];
    let model = lm::KnTrigram::train(&sentences);
    let lm_path = std::env::temp_dir().join("decoder-skel-test-lm.kn");
    lm::persist::save_kn(&model, &lm_path).expect("save test lm");
    let decoder = MooreDecoder::load(
        &am_path(),
        Some(&lm_path),
        lexicon::CLOSED_VOCABULARY_CMUDICT,
        DecoderConfig::default(),
    )
    .expect("decoder load with lm");
    assert!(decoder.has_lm());

    let wav = sample_wav();
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let pcm16k = resample_to_16k(&corpus::audio::to_mono(&audio), audio.sample_rate);
    let clip = &pcm16k[..32_000.min(pcm16k.len())];
    let lattice = decoder.decode(clip);
    let words: Vec<&str> = lattice.tokens.iter().map(|t| t.word.as_str()).collect();
    eprintln!(
        "with-LM decoded {} tokens: {}",
        words.len(),
        words.join(" ")
    );
    assert!(!lattice.tokens.is_empty());
    let text = lattice::write_lattice(&lattice).expect("write lattice");
    assert_eq!(lattice::read_lattice(&text).expect("read lattice"), lattice);
    std::fs::remove_file(&lm_path).ok();
}
/// Lookahead exactness: with pruning disabled (astronomical beam + token
/// cap), weight 0 and weight 1 decode identically — lookahead only ranks.
#[test]
fn lookahead_weight_zero_matches_weight_one_without_pruning() {
    let dict = "HOTBED HH AA T B EH D\nRIPPLE R IH P AH L\n";
    let sentences: Vec<Vec<String>> = vec![
        "a hotbed".split(' ').map(str::to_string).collect(),
        "b hotbed".split(' ').map(str::to_string).collect(),
        "c ripple".split(' ').map(str::to_string).collect(),
    ];
    let model = lm::KnTrigram::train(&sentences);
    let lm_path = std::env::temp_dir().join("decoder-unpruned-test-lm.kn");
    lm::persist::save_kn(&model, &lm_path).expect("save test lm");
    let cfg_off = DecoderConfig {
        beam: 1e9,
        max_tokens: 100_000,
        lookahead_weight: 0.0,
        ..DecoderConfig::default()
    };
    let cfg_on = DecoderConfig {
        lookahead_weight: 1.0,
        ..cfg_off.clone()
    };
    let dec_off = MooreDecoder::load(&am_path(), Some(&lm_path), dict, cfg_off).expect("load off");
    let dec_on = MooreDecoder::load(&am_path(), Some(&lm_path), dict, cfg_on).expect("load on");
    std::fs::remove_file(&lm_path).ok();

    let wav = sample_wav();
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let pcm16k = resample_to_16k(&corpus::audio::to_mono(&audio), audio.sample_rate);
    let clip = &pcm16k[..8_000.min(pcm16k.len())];
    let lat_off = dec_off.decode(clip);
    let lat_on = dec_on.decode(clip);
    assert_eq!(
        lat_off, lat_on,
        "unpruned search must not depend on lookahead"
    );
}
/// Large-vocab path on synthetic inputs: tiered lexicon + LM top-N +
/// `load_with_vocab` decodes end to end.
#[test]
fn large_vocab_load_from_tiered_lexicon() {
    let sentences: Vec<Vec<String>> = vec![
        "hotbed hotbed ripple"
            .split(' ')
            .map(str::to_string)
            .collect(),
        "hotbed ripple".split(' ').map(str::to_string).collect(),
    ];
    let model = lm::KnTrigram::train(&sentences);
    let top = decoder::top_lm_words(&model, None, 10);
    assert!(top.contains(&"hotbed".to_string()));
    assert!(top.contains(&"ripple".to_string()));
    let tier1 = "HOTBED HH AA T B EH D\n";
    let tier2 = "RIPPLE R IH P AH L\n";
    let lex = lexicon::ingest_tiered(tier1, tier2);
    let book: std::collections::BTreeSet<String> = ["ripple".to_string()].into_iter().collect();
    let (entries, report) =
        decoder::build_entries(&top, &book, 10, decoder::BookSource::BookText, &lex);
    assert_eq!(report.kept_words, 2);
    assert_eq!(report.tier1_words, 1);
    assert_eq!(report.tier2_words, 1);
    assert_eq!(report.oov_lm, 0);
    let lm_path = std::env::temp_dir().join("decoder-vocab-test-lm.kn");
    lm::persist::save_kn(&model, &lm_path).expect("save test lm");
    let decoder = MooreDecoder::load_with_vocab(
        &am_path(),
        Some(&lm_path),
        &entries,
        DecoderConfig::default(),
    )
    .expect("load_with_vocab");
    std::fs::remove_file(&lm_path).ok();
    assert_eq!(decoder.vocab_size(), 2);
    assert!(decoder.skipped_words().is_empty());
    assert!(decoder.has_lm());

    let wav = sample_wav();
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let pcm16k = resample_to_16k(&corpus::audio::to_mono(&audio), audio.sample_rate);
    let clip = &pcm16k[..8_000.min(pcm16k.len())];
    let lattice = decoder.decode(clip);
    assert!(!lattice.tokens.is_empty());
}

// ---- Frontend guard (decoder side): model stamp vs selected frontend. ----

/// Two-word dict over si_base phones (same as the lookahead tests).
fn tiny_dict() -> &'static str {
    "HOTBED HH AA T B EH D\nRIPPLE R IH P AH L\n"
}

/// Copy si_base stamped with `frontend_id` into a temp file.
fn stamped_am_path(tag: &str, frontend_id: Option<&str>) -> PathBuf {
    let mut model = hmm::HmmSet::load_text(&am_path()).expect("load si_base");
    model.frontend_id = frontend_id.map(str::to_string);
    let path =
        std::env::temp_dir().join(format!("decoder-guard-{tag}-{}.json", std::process::id()));
    model.save_text(&path).expect("save stamped am");
    path
}

#[test]
fn guard_match_cmvn_passes() {
    let stamped = stamped_am_path("cmvn", Some(fse::HMM_FRONTEND_ID));
    let decoder = MooreDecoder::load(&stamped, None, tiny_dict(), DecoderConfig::default())
        .expect("cmvn-stamped AM with cmvn cfg must load");
    std::fs::remove_file(&stamped).ok();
    assert_eq!(decoder.vocab_size(), 2);
}

#[test]
fn guard_match_raw_passes() {
    let stamped = stamped_am_path("raw", Some(fse::HMM_FRONTEND_ID_RAW));
    let cfg = DecoderConfig {
        cmvn: false,
        ..DecoderConfig::default()
    };
    let decoder = MooreDecoder::load(&stamped, None, tiny_dict(), cfg)
        .expect("raw-stamped AM with raw cfg must load");
    std::fs::remove_file(&stamped).ok();
    assert_eq!(decoder.vocab_size(), 2);
}

#[test]
fn guard_mismatch_refuses_and_names_both() {
    // Raw-stamped AM under the default (CMVN) config.
    let raw = stamped_am_path("mismatch-raw", Some(fse::HMM_FRONTEND_ID_RAW));
    let err = match MooreDecoder::load(&raw, None, tiny_dict(), DecoderConfig::default()) {
        Ok(_) => panic!("raw AM + cmvn cfg must refuse"),
        Err(e) => e,
    };
    std::fs::remove_file(&raw).ok();
    assert!(
        err.contains(fse::HMM_FRONTEND_ID_RAW) && err.contains(fse::HMM_FRONTEND_ID),
        "error must name both sides: {err}"
    );
    // CMVN-stamped AM under a raw config.
    let cmvn = stamped_am_path("mismatch-cmvn", Some(fse::HMM_FRONTEND_ID));
    let cfg = DecoderConfig {
        cmvn: false,
        ..DecoderConfig::default()
    };
    let err = match MooreDecoder::load(&cmvn, None, tiny_dict(), cfg) {
        Ok(_) => panic!("cmvn AM + raw cfg must refuse"),
        Err(e) => e,
    };
    std::fs::remove_file(&cmvn).ok();
    assert!(
        err.contains(fse::HMM_FRONTEND_ID_RAW) && err.contains(fse::HMM_FRONTEND_ID),
        "error must name both sides: {err}"
    );
}

#[test]
fn guard_mismatch_refuses_in_load_with_vocab() {
    let raw = stamped_am_path("vocab-raw", Some(fse::HMM_FRONTEND_ID_RAW));
    let entries = vec![decoder::PronEntry {
        word: "HOTBED".to_string(),
        phones: ["HH", "AA", "T", "B", "EH", "D"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }];
    let err = match MooreDecoder::load_with_vocab(&raw, None, &entries, DecoderConfig::default()) {
        Ok(_) => panic!("load_with_vocab must enforce the guard"),
        Err(e) => e,
    };
    std::fs::remove_file(&raw).ok();
    assert!(
        err.contains(fse::HMM_FRONTEND_ID_RAW) && err.contains(fse::HMM_FRONTEND_ID),
        "error must name both sides: {err}"
    );
}

#[test]
fn guard_legacy_warns_and_passes() {
    // Premise pin: si_base ships unstamped.
    let model = hmm::HmmSet::load_text(&am_path()).expect("load si_base");
    assert_eq!(model.frontend_id, None);
    // Legacy proceeds (loud stderr warning comes from hmm::check_frontend).
    let decoder = MooreDecoder::load(&am_path(), None, tiny_dict(), DecoderConfig::default())
        .expect("legacy AM must load");
    assert_eq!(decoder.vocab_size(), 2);
}

/// stream() determinism: the final streamed partial equals batch decode on
/// the same audio. Raw mode is exact by construction (same frontend path);
/// CMVN mode matches because the cumulative live estimate covers the same
/// frames as batch CMVN (1.5 s clip clears the 100-frame live gate).
#[test]
fn stream_final_matches_batch() {
    let wav = sample_wav();
    let audio = corpus::audio::decode_file(&wav).expect("decode demo wav");
    let pcm16k = resample_to_16k(&corpus::audio::to_mono(&audio), audio.sample_rate);
    let clip = pcm16k[..24_000.min(pcm16k.len())].to_vec();
    assert!(clip.len() >= 16_000, "clip must clear the 100-frame gate");

    for cmvn in [false, true] {
        let cfg = DecoderConfig {
            cmvn,
            ..DecoderConfig::default()
        };
        let batch = MooreDecoder::load(&am_path(), None, tiny_dict(), cfg.clone())
            .expect("load batch decoder");
        let started = Instant::now();
        let lattice = batch.decode(&clip);
        let batch_sec = started.elapsed().as_secs_f64();
        let batch_text: String = lattice
            .tokens
            .iter()
            .map(|t| t.word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!batch_text.is_empty(), "cmvn={cmvn}: batch decode empty");

        let mut live =
            MooreDecoder::load(&am_path(), None, tiny_dict(), cfg).expect("load stream decoder");
        let mut final_text = String::new();
        let mut chunk_ms: Vec<f64> = Vec::new();
        for chunk in clip.chunks(6000) {
            let started = Instant::now();
            let partials = live.stream(chunk);
            chunk_ms.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(partials.len(), 1);
            final_text = partials[0].text.clone();
        }
        let chunk_ms_str: Vec<String> = chunk_ms.iter().map(|v| format!("{v:.0}")).collect();
        eprintln!(
            "cmvn={cmvn} batch {batch_sec:.2}s chunk_latency_ms [{}] text: {batch_text}",
            chunk_ms_str.join(" "),
        );
        assert_eq!(
            final_text, batch_text,
            "cmvn={cmvn}: streamed final must equal batch"
        );
    }
}
