//! File transcription on the current best acoustic model (lane D demo).
//!
//! Bead `hearsay-four-xels--demo-file-transcription`: decode WAV/MP3 files
//! with the 8-mix AM + tiered lexicon + R1-v2 general LM, and emit transcript
//! JSON. Classical pipeline only (HMM-GMM AM, Kneser-Ney LM, Viterbi search):
//! no neural net, transformer, embedding model, or learned weights.
//!
//! The model path is a SINGLE config value ([`DEFAULT_AM`]) so a later model
//! swap touches one line. The LM/lexicon defaults are likewise single values.
//! Lane worktrees hold code only; these absolute paths read model/audio data
//! read-only from the main checkout.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use decoder::{BookSource, DecoderConfig, MooreDecoder, Recognizer};

/// Current best acoustic model (non-CMVN 8-mix). THE single model config
/// value: swapping models later means changing this one line.
pub const DEFAULT_AM: &str =
    "/home/michael/repos/hearsay-four/data/models/r3v2-full93/ckpt-8mix-iter4.json";
/// General (non-personal) full-size R1-v2 LM. Single value; `--lm` overrides.
pub const DEFAULT_LM: &str =
    "/home/michael/repos/hearsay-four/data/lm-text/r1v2-full/lm-r1v2-general.json";
/// Tier-1 lexicon: better-voice master phonetics (CMUdict content).
pub const DEFAULT_TIER1: &str =
    "/home/michael/repos/better-voice/data/phonetics/Master_English_Phonetics_FINAL_COMPLETE.txt";
/// Tier-2 lexicon: Librispeech G2P gap fill.
pub const DEFAULT_TIER2: &str =
    "/home/michael/repos/hearsay-four/data/lexicon/librispeech-lexicon.txt";
/// LM top-N vocab size (same default as the `wer_devclean` harness).
pub const DEFAULT_TOP_N: usize = 20_000;
/// Default LM weight: the `tune200s-fallback` best cell (`lmw=6 wip=0`,
/// WER 0.7233 on its 200-utt subset). Decoder default 1.0 under-constrains
/// the search (insertion salad on short utts); 6-10 are tied on the demo
/// files, so the demo takes the measured 200-utt optimum.
pub const DEFAULT_LM_WEIGHT: f64 = 6.0;
/// Default word penalty: 0.0 (decoder default; the tune-best region).
pub const DEFAULT_WORD_PENALTY: f64 = 0.0;

/// Fixed demo file list: `(utterance id, wav-root-relative path)`.
/// Three short dev-clean utterances (all under 2 s) for a fast demo.
pub const DEMO_FILES: [(&str, &str); 3] = [
    (
        "777-126732-0081",
        "dev-clean/777/126732/777-126732-0081.wav",
    ),
    (
        "2035-147960-0015",
        "dev-clean/2035/147960/2035-147960-0015.wav",
    ),
    (
        "1919-142785-0021",
        "dev-clean/1919/142785/1919-142785-0021.wav",
    ),
];
/// Wav root the [`DEMO_FILES`] paths are relative to.
pub const DEMO_WAV_ROOT: &str = "/home/michael/repos/hearsay-four/data/librispeech/wav";

/// Transcriber configuration. Every path overrides one `DEFAULT_*` single.
#[derive(Debug, Clone)]
pub struct DemoConfig {
    pub am: PathBuf,
    pub lm: PathBuf,
    pub tier1: PathBuf,
    pub tier2: PathBuf,
    /// Optional book text for the plain-LM vocab union (mirrors `wer_devclean`
    /// `--book`); `None` decodes with the LM top-N alone.
    pub book: Option<PathBuf>,
    pub top_n: usize,
    /// Search overrides (mirror `wer_devclean` flags); `None` keeps the
    /// [`DecoderConfig`] default.
    pub beam: Option<f64>,
    pub max_tokens: Option<usize>,
    pub lookahead_weight: Option<f64>,
    pub lm_weight: Option<f64>,
    pub word_penalty: Option<f64>,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            am: PathBuf::from(DEFAULT_AM),
            lm: PathBuf::from(DEFAULT_LM),
            tier1: PathBuf::from(DEFAULT_TIER1),
            tier2: PathBuf::from(DEFAULT_TIER2),
            book: None,
            top_n: DEFAULT_TOP_N,
            beam: None,
            max_tokens: None,
            lookahead_weight: None,
            lm_weight: Some(DEFAULT_LM_WEIGHT),
            word_penalty: Some(DEFAULT_WORD_PENALTY),
        }
    }
}

/// Loaded LM in either shipped flavor (mirrors `decoder::moore::LmKind`,
/// which is private to the decoder crate).
enum DemoLm {
    Kn(Box<lm::KnTrigram>),
    Ix(Box<lm::InterpolatedLm>),
}

impl DemoLm {
    fn prob(&self, w1: &str, w2: &str, w: &str) -> f64 {
        match self {
            DemoLm::Kn(m) => m.prob(w1, w2, w),
            DemoLm::Ix(m) => m.prob(w1, w2, w),
        }
    }
}

/// A loaded demo transcriber: decoder plus its provenance report.
pub struct Transcriber {
    decoder: MooreDecoder,
    lm: DemoLm,
    lexicon: lexicon::TieredLexicon,
    /// `general` or `interpolated`, detected from the LM bytes.
    pub flavor: String,
    pub vocab_report: decoder::VocabReport,
    pub skipped_words: Vec<String>,
}

impl Transcriber {
    /// Decode canonical 16 kHz mono PCM to a word lattice (1-best chain).
    pub fn decode(&self, pcm16k: &[i16]) -> lattice::Lattice {
        self.decoder.decode(pcm16k)
    }

    pub fn vocab_size(&self) -> usize {
        self.decoder.vocab_size()
    }

    pub fn has_lm(&self) -> bool {
        self.decoder.has_lm()
    }

    /// `P(word | history)` under the loaded LM, mirroring the decoder's own
    /// history encoding (lowercased words, [`lm::BOS_TOKEN`] for missing
    /// predecessors). `history` holds lowercased words before `word`.
    pub fn lm_probability(&self, history: &[String], word: &str) -> f64 {
        let (h1, h2) = lm_history_pair(history);
        self.lm.prob(&h1, &h2, &word.to_lowercase())
    }

    /// Lexicon provenance tag for an UPPER-CASE word, if it has one.
    pub fn lexicon_source(&self, word_upper: &str) -> Option<&'static str> {
        self.lexicon.sources.get(word_upper).map(|s| s.as_str())
    }

    /// [`transcript_json`] plus a per-word `why` panel (bead wgit shape).
    pub fn transcript_json_with_why(
        &self,
        utt_id: &str,
        input_display: &str,
        audio_secs: f64,
        decode_secs: f64,
        lat: &lattice::Lattice,
    ) -> serde_json::Value {
        let mut value = transcript_json(utt_id, input_display, audio_secs, decode_secs, lat);
        let lower: Vec<String> = lat.tokens.iter().map(|t| t.word.to_lowercase()).collect();
        if let Some(words) = value.get_mut("words").and_then(|w| w.as_array_mut()) {
            for (i, (token, word)) in lat.tokens.iter().zip(words.iter_mut()).enumerate() {
                let history = &lower[..i];
                let prob = self.lm_probability(history, &token.word);
                word["why"] = why_json(token, history, prob, self.lexicon_source(&token.word));
            }
        }
        value
    }
}

/// Load the AM + LM + tiered lexicon and build the decoder vocabulary.
/// Follows the `wer_devclean` harness recipe (LM top-N union book words,
/// tiered-lexicon pronunciations); fails loudly on any missing input.
pub fn load_transcriber(cfg: &DemoConfig) -> Result<Transcriber, String> {
    let lm_bytes =
        std::fs::read(&cfg.lm).map_err(|e| format!("read LM {}: {e}", cfg.lm.display()))?;
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
            Err(e) => {
                return Err(format!(
                    "LM {}: neither interpolated nor plain archive ({e})",
                    cfg.lm.display()
                ));
            }
        },
    }
    drop(lm_bytes);
    let (general, personal) = match (&interpolated, &plain) {
        (Some(ix), _) => (ix.general(), Some(ix.personal())),
        (None, Some(kn)) => (kn, None),
        (None, None) => unreachable!("LM load branches above"),
    };
    let top = decoder::top_lm_words(general, personal, cfg.top_n);
    let (book, book_source) = match personal {
        Some(book_lm) => (
            decoder::book_types_from_personal(book_lm),
            BookSource::PersonalLm,
        ),
        None => match &cfg.book {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| format!("read book {}: {e}", path.display()))?;
                (decoder::book_types_from_text(&text), BookSource::BookText)
            }
            None => (BTreeSet::new(), BookSource::BookText),
        },
    };
    let lexicon = lexicon::load_tiered(&cfg.tier1, &cfg.tier2)
        .map_err(|e| format!("load tiered lexicon: {e}"))?;
    let (entries, report) = decoder::build_entries(&top, &book, cfg.top_n, book_source, &lexicon);
    let mut decoder_cfg = DecoderConfig::default();
    if let Some(beam) = cfg.beam {
        decoder_cfg.beam = beam;
    }
    if let Some(max) = cfg.max_tokens {
        decoder_cfg.max_tokens = max;
    }
    if let Some(w) = cfg.lookahead_weight {
        decoder_cfg.lookahead_weight = w;
    }
    if let Some(w) = cfg.lm_weight {
        decoder_cfg.lm_weight = w;
    }
    if let Some(p) = cfg.word_penalty {
        decoder_cfg.word_penalty = p;
    }
    let decoder = MooreDecoder::load_with_vocab(&cfg.am, Some(&cfg.lm), &entries, decoder_cfg)
        .map_err(|e| format!("load decoder: {e}"))?;
    let skipped_words = decoder.skipped_words().to_vec();
    let lm = match (interpolated, plain) {
        (Some(ix), _) => DemoLm::Ix(Box::new(ix)),
        (None, Some(kn)) => DemoLm::Kn(Box::new(kn)),
        (None, None) => unreachable!("LM load branches above"),
    };
    Ok(Transcriber {
        decoder,
        lm,
        lexicon,
        flavor: flavor.to_string(),
        vocab_report: report,
        skipped_words,
    })
}

/// Render one decoded lattice as transcript JSON:
/// `{utt_id, input, audio_secs, decode_secs, text, words: [{text, start_ms,
/// end_ms}]}`. Timings derive from the lattice's own `frame_shift_ms`.
pub fn transcript_json(
    utt_id: &str,
    input_display: &str,
    audio_secs: f64,
    decode_secs: f64,
    lat: &lattice::Lattice,
) -> serde_json::Value {
    let words: Vec<serde_json::Value> = lat
        .tokens
        .iter()
        .map(|t| {
            serde_json::json!({
                "text": t.word,
                "start_ms": (t.start as f64 * lat.frame_shift_ms).round() as u64,
                "end_ms": (t.end as f64 * lat.frame_shift_ms).round() as u64,
            })
        })
        .collect();
    let text = lat
        .tokens
        .iter()
        .map(|t| t.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    serde_json::json!({
        "utt_id": utt_id,
        "input": input_display,
        "audio_secs": audio_secs,
        "decode_secs": decode_secs,
        "text": text,
        "words": words,
    })
}

/// Why-panel notes (bead `hearsay-four-wgit--why-panel-demo-script`).
/// The demo why panel carries five elements per word; only the fabric
/// element is TBD (the decoder-direct path has no fabric tiling).
pub const ACOUSTIC_NOTE: &str = "raw Viterbi segment log-score over the token frames (decoder v0: uncalibrated, NOT a true LLR)";
pub const ALTERNATIVES_NOTE: &str = "1-best Viterbi path: losing beam competitors are pruned during search and not recorded by the decoder";
pub const FABRIC_EVENTS_TBD: &str = "TBD";
pub const FABRIC_NOTE: &str =
    "fabric tiling / margin / revival events are not wired in the decoder-direct demo path";

/// LM history pair for `P(word | h1, h2)`, mirroring the decoder's own
/// history encoding: the last two lowercased history words, [`lm::BOS_TOKEN`]
/// for missing predecessors.
pub fn lm_history_pair(history: &[String]) -> (String, String) {
    let n = history.len();
    let h1 = if n >= 2 {
        history[n - 2].clone()
    } else {
        lm::BOS_TOKEN.to_string()
    };
    let h2 = if n >= 1 {
        history[n - 1].clone()
    } else {
        lm::BOS_TOKEN.to_string()
    };
    (h1, h2)
}

/// One word's demo why panel as JSON: acoustic score, losing alternatives +
/// why they lost, LM probability, lexicon source tag, fabric events.
/// `history` holds lowercased words before this token.
pub fn why_json(
    token: &lattice::Token,
    history: &[String],
    lm_prob: f64,
    lexicon_source: Option<&str>,
) -> serde_json::Value {
    let (h1, h2) = lm_history_pair(history);
    serde_json::json!({
        "acoustic_score": token.acoustic_llr,
        "acoustic_note": ACOUSTIC_NOTE,
        "alternatives": [],
        "alternatives_note": ALTERNATIVES_NOTE,
        "lm_probability": lm_prob,
        "lm_history": [h1, h2],
        "lexicon_source": lexicon_source,
        "fabric_events": FABRIC_EVENTS_TBD,
        "fabric_note": FABRIC_NOTE,
    })
}

/// Utterance id from an audio path: the file stem
/// (`777-126732-0081.wav` -> `777-126732-0081`).
pub fn utt_id_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_lattice() -> lattice::Lattice {
        lattice::Lattice {
            utterance: "test".to_string(),
            frame_shift_ms: 10.0,
            producer: "test".to_string(),
            tokens: vec![
                lattice::Token {
                    id: 1,
                    word: "HELLO".to_string(),
                    start: 0,
                    end: 50,
                    source: "test".to_string(),
                    acoustic_llr: -12.5,
                    lexical_llr: None,
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
                lattice::Token {
                    id: 2,
                    word: "WORLD".to_string(),
                    start: 50,
                    end: 90,
                    source: "test".to_string(),
                    acoustic_llr: -9.75,
                    lexical_llr: None,
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
            ],
            arcs: vec![lattice::Arc { from: 1, to: 2 }],
        }
    }

    #[test]
    fn default_config_points_at_current_best_model() {
        // Single-config-value contract: one constant names the model file.
        assert!(DEFAULT_AM.ends_with("ckpt-8mix-iter4.json"), "{DEFAULT_AM}");
        let cfg = DemoConfig::default();
        assert_eq!(cfg.am, PathBuf::from(DEFAULT_AM));
        assert_eq!(cfg.lm, PathBuf::from(DEFAULT_LM));
        assert_eq!(cfg.tier1, PathBuf::from(DEFAULT_TIER1));
        assert_eq!(cfg.tier2, PathBuf::from(DEFAULT_TIER2));
        assert_eq!(cfg.top_n, DEFAULT_TOP_N);
        assert_eq!(cfg.beam, None);
        assert_eq!(cfg.max_tokens, None);
        assert_eq!(cfg.lookahead_weight, None);
        assert_eq!(cfg.lm_weight, Some(DEFAULT_LM_WEIGHT));
        assert_eq!(cfg.word_penalty, Some(DEFAULT_WORD_PENALTY));
    }

    #[test]
    fn demo_file_list_is_fixed_three_utts() {
        assert_eq!(DEMO_FILES.len(), 3);
        let ids: Vec<&str> = DEMO_FILES.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            vec!["777-126732-0081", "2035-147960-0015", "1919-142785-0021"]
        );
        for (id, rel) in &DEMO_FILES {
            assert!(rel.ends_with(&format!("{id}.wav")), "{rel}");
        }
    }

    #[test]
    fn transcript_json_shape() {
        let v = transcript_json("utt1", "utt1.wav", 1.5, 0.2, &toy_lattice());
        assert_eq!(v["utt_id"], "utt1");
        assert_eq!(v["text"], "HELLO WORLD");
        assert_eq!(v["words"].as_array().unwrap().len(), 2);
        assert_eq!(v["words"][0]["text"], "HELLO");
        assert_eq!(v["words"][0]["start_ms"], 0);
        assert_eq!(v["words"][0]["end_ms"], 500);
        assert_eq!(v["words"][1]["start_ms"], 500);
        assert_eq!(v["words"][1]["end_ms"], 900);
    }

    #[test]
    fn utt_id_is_file_stem() {
        assert_eq!(
            utt_id_from_path(Path::new("dev-clean/777/126732/777-126732-0081.wav")),
            "777-126732-0081"
        );
    }

    #[test]
    fn lm_history_pair_pads_with_bos() {
        assert_eq!(lm_history_pair(&[]), ("<s>".to_string(), "<s>".to_string()));
        assert_eq!(
            lm_history_pair(&["the".to_string()]),
            ("<s>".to_string(), "the".to_string())
        );
        assert_eq!(
            lm_history_pair(&["a".to_string(), "b".to_string(), "c".to_string()]),
            ("b".to_string(), "c".to_string())
        );
    }

    #[test]
    fn why_json_carries_all_five_elements() {
        let lat = toy_lattice();
        let w = why_json(&lat.tokens[0], &[], 0.0012, Some("cmudict"));
        // 1. acoustic score
        assert_eq!(w["acoustic_score"], -12.5);
        assert!(
            w["acoustic_note"]
                .as_str()
                .unwrap()
                .contains("uncalibrated")
        );
        // 2. losing alternatives + why they lost
        assert_eq!(w["alternatives"].as_array().unwrap().len(), 0);
        assert!(w["alternatives_note"].as_str().unwrap().contains("1-best"));
        // 3. LM probability
        assert_eq!(w["lm_probability"], 0.0012);
        assert_eq!(w["lm_history"], serde_json::json!(["<s>", "<s>"]));
        // 4. lexicon source tag
        assert_eq!(w["lexicon_source"], "cmudict");
        // 5. fabric events: the only TBD element
        assert_eq!(w["fabric_events"], "TBD");
    }

    #[test]
    fn why_json_missing_lexicon_is_null() {
        let lat = toy_lattice();
        let w = why_json(&lat.tokens[1], &["hello".to_string()], 0.5, None);
        assert!(w["lexicon_source"].is_null());
        assert_eq!(w["lm_history"], serde_json::json!(["<s>", "hello"]));
    }
}
