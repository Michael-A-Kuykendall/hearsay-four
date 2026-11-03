//! Fabric-path transcription: transcript JSON from bridge outputs.
//!
//! Bead `hearsay-four-4lsp--fabric-path-demo`: file transcription reads the
//! FABRIC path — the bridge handoff (`<utt>.transcript.txt` +
//! `<utt>.events.jsonl`) joined against the source lattice for timing — not
//! the decoder 1-best. The handoff comes from the decision-fabric decode
//! (the `hearsay-bridge` binary on real lattices, or the experiment arm-C
//! driver on fixtures); this module only reads the files. The per-word why
//! panel keeps the five-element shape ([`demo_transcribe::why_json`]) but
//! the fabric element carries the real record: revivals, dormancy outcome,
//! the defeating tiling + margin, and explicit correction none (the v1 log
//! carries no USER evidence).
//!
//! Demo utterances are the fabric-experiment fixture population (u1-u4,
//! including the u1 late rescue), disclosed as chosen in every transcript.

use std::collections::BTreeMap;
use std::path::Path;

use output::fabric_why::{FabricEvents, FabricTokenWhy, parse_events_jsonl};

use crate::demo_transcribe::lm_history_pair;

/// Why-panel notes for the fabric path (the decoder-direct notes live in
/// [`demo_transcribe`]; the fabric panel replaces all three path notes).
pub const FABRIC_ACOUSTIC_NOTE: &str =
    "lattice acoustic LLR of the committed token (bridge input; decoder LLR, uncalibrated)";
pub const FABRIC_ALTERNATIVES_NOTE: &str =
    "tokens this commit's tiling defeated, from the bridge v1 event log";
pub const FABRIC_NOTE: &str =
    "bridge v1 event log: revivals, dormancy outcome, defeating tiling + margin";
pub const FABRIC_TIMING_NOTE: &str =
    "fabric path reads stored bridge outputs; no audio decode timed";
pub const FABRIC_CHOSEN_NOTE: &str = "chosen: demo utterance from the fabric-experiment fixture population (A6), disclosed as chosen";

/// One utterance's fabric inputs: bridge transcript + event log + lattice.
pub struct FabricUtt {
    pub utt_id: String,
    pub transcript_words: Vec<String>,
    pub events: FabricEvents,
    pub lattice: lattice::Lattice,
}

/// Load one utterance's fabric inputs: `<stem>.transcript.txt` and
/// `<stem>.events.jsonl` from `bridge_dir`, plus the source lattice.
/// Fails loudly on missing files, a log/transcript mismatch, or a lattice
/// that does not parse.
pub fn load_fabric_utt(
    bridge_dir: &Path,
    lattice_path: &Path,
    utt_id: &str,
) -> Result<FabricUtt, String> {
    let transcript_path = bridge_dir.join(format!("{utt_id}.transcript.txt"));
    let events_path = bridge_dir.join(format!("{utt_id}.events.jsonl"));
    let transcript_text = std::fs::read_to_string(&transcript_path)
        .map_err(|e| format!("read {}: {e}", transcript_path.display()))?;
    let transcript_words: Vec<String> = transcript_text
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if transcript_words.is_empty() {
        return Err(format!(
            "bridge transcript {} has no words",
            transcript_path.display()
        ));
    }
    let events_text = std::fs::read_to_string(&events_path)
        .map_err(|e| format!("read {}: {e}", events_path.display()))?;
    let events = parse_events_jsonl(&events_text)?;
    if events.utterance != utt_id {
        return Err(format!(
            "events log utterance {:?} != {utt_id:?}",
            events.utterance
        ));
    }
    let lattice_text = std::fs::read_to_string(lattice_path)
        .map_err(|e| format!("read {}: {e}", lattice_path.display()))?;
    let lattice = lattice::read_lattice(&lattice_text)?;
    if lattice.utterance != utt_id {
        return Err(format!(
            "lattice utterance {:?} != {utt_id:?}",
            lattice.utterance
        ));
    }
    Ok(FabricUtt {
        utt_id: utt_id.to_string(),
        transcript_words,
        events,
        lattice,
    })
}

/// One committed word's fabric why panel as JSON: the same five elements as
/// [`demo_transcribe::why_json`] (acoustic score, losing alternatives + why
/// they lost, LM probability, lexicon source tag, fabric record) plus the
/// per-class `evidence` the bridge saw. `history` holds lowercased words
/// before this token.
pub fn why_json_fabric(
    token: &lattice::Token,
    history: &[String],
    lm_prob: f64,
    lexicon_source: Option<&str>,
    fabric: &FabricTokenWhy,
) -> serde_json::Value {
    let (h1, h2) = lm_history_pair(history);
    serde_json::json!({
        "acoustic_score": token.acoustic_llr,
        "acoustic_note": FABRIC_ACOUSTIC_NOTE,
        "alternatives": fabric.beat,
        "alternatives_note": FABRIC_ALTERNATIVES_NOTE,
        "lm_probability": lm_prob,
        "lm_history": [h1, h2],
        "lexicon_source": lexicon_source,
        "evidence": {
            "acoustic": token.acoustic_llr,
            "lexical": token.lexical_llr,
            "speaker": token.speaker_llr,
            "verify": token.verify_llr,
            "user": token.user_llr,
        },
        "fabric_events": fabric.record_json(),
        "fabric_note": FABRIC_NOTE,
    })
}

/// Render one fabric utterance as transcript JSON: the decoder-path shape
/// (`{utt_id, input, audio_secs, decode_secs, text, words}` with per-word
/// `why` + `why_text`) plus `path: "fabric"` and the `chosen` label.
/// Joins committed event winners to lattice tokens by id and cross-checks
/// the commit words against the bridge transcript — any mismatch fails
/// loudly instead of rendering a half-joined panel.
pub fn transcript_json_from_fabric(
    utt: &FabricUtt,
    input_display: &str,
    lm_probability: &dyn Fn(&[String], &str) -> f64,
    lexicon_source: &dyn Fn(&str) -> Option<&str>,
) -> Result<serde_json::Value, String> {
    let by_id: BTreeMap<u64, &lattice::Token> =
        utt.lattice.tokens.iter().map(|t| (t.id, t)).collect();
    let committed_ids = utt.events.committed_token_ids();
    if committed_ids.is_empty() {
        return Err(format!("{}: event log commits nothing", utt.utt_id));
    }
    let mut tokens: Vec<&lattice::Token> = Vec::with_capacity(committed_ids.len());
    for id in &committed_ids {
        match by_id.get(id) {
            Some(token) => tokens.push(*token),
            None => {
                return Err(format!(
                    "{}: committed token id {id} not in lattice",
                    utt.utt_id
                ));
            }
        }
    }
    let commit_words: Vec<&str> = tokens.iter().map(|t| t.word.as_str()).collect();
    let transcript_words: Vec<&str> = utt.transcript_words.iter().map(String::as_str).collect();
    if commit_words != transcript_words {
        return Err(format!(
            "{}: committed winners [{}] != bridge transcript [{}]",
            utt.utt_id,
            commit_words.join(" "),
            transcript_words.join(" ")
        ));
    }
    let lower: Vec<String> = commit_words.iter().map(|w| w.to_lowercase()).collect();
    let mut words: Vec<serde_json::Value> = Vec::with_capacity(tokens.len());
    for (i, token) in tokens.iter().enumerate() {
        let history = &lower[..i];
        let prob = lm_probability(history, &token.word);
        let upper = token.word.to_uppercase();
        let lexicon = lexicon_source(&upper);
        let fabric = utt.events.why_for(token.id);
        words.push(serde_json::json!({
            "text": token.word,
            "start_ms": (f64::from(token.start) * utt.lattice.frame_shift_ms).round() as u64,
            "end_ms": (f64::from(token.end) * utt.lattice.frame_shift_ms).round() as u64,
            "why": why_json_fabric(token, history, prob, lexicon, &fabric),
            "why_text": output::why_text::render_demo_why_text(
                &token.word,
                token.acoustic_llr,
                history,
                prob,
                lexicon
            ),
        }));
    }
    Ok(serde_json::json!({
        "utt_id": utt.utt_id,
        "input": input_display,
        "audio_secs": 0.0,
        "decode_secs": 0.0,
        "timing_note": FABRIC_TIMING_NOTE,
        "text": transcript_words.join(" "),
        "words": words,
        "path": "fabric",
        "chosen": FABRIC_CHOSEN_NOTE,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(id: u64, word: &str, start: u32, end: u32) -> lattice::Token {
        lattice::Token {
            id,
            word: word.to_string(),
            start,
            end,
            source: "fixture".to_string(),
            acoustic_llr: 1.5,
            lexical_llr: None,
            speaker_llr: None,
            verify_llr: None,
            user_llr: None,
        }
    }

    /// Miniature fabric inputs in exact bridge shape: transcript plus a v1
    /// log where CAT revives via SAT and beats BAT. All words invented.
    fn fixture_utt() -> FabricUtt {
        let events = parse_events_jsonl(
            "{\"v\":1,\"utterance\":\"u1\",\"seq\":0,\"event\":\"revived\",\"token\":{\"id\":5,\"word\":\"CAT\",\"start\":10,\"end\":20},\"via\":{\"id\":8,\"word\":\"SAT\",\"start\":20,\"end\":30},\"llr\":4.0}\n\
             {\"v\":1,\"utterance\":\"u1\",\"seq\":1,\"event\":\"defeated\",\"token\":{\"id\":6,\"word\":\"BAT\",\"start\":10,\"end\":20},\"winner\":[{\"id\":5,\"word\":\"CAT\",\"start\":10,\"end\":20}],\"margin\":1.5}\n\
             {\"v\":1,\"utterance\":\"u1\",\"seq\":2,\"event\":\"committed\",\"region\":[0,30],\"winner\":[{\"id\":1,\"word\":\"THE\",\"start\":0,\"end\":10},{\"id\":5,\"word\":\"CAT\",\"start\":10,\"end\":20},{\"id\":8,\"word\":\"SAT\",\"start\":20,\"end\":30}],\"margin\":1.5}\n",
        )
        .expect("fixture log parses");
        FabricUtt {
            utt_id: "u1".to_string(),
            transcript_words: vec!["THE", "CAT", "SAT"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            events,
            lattice: lattice::Lattice {
                utterance: "u1".to_string(),
                frame_shift_ms: 10.0,
                producer: "fixture".to_string(),
                tokens: vec![
                    tok(1, "THE", 0, 10),
                    tok(5, "CAT", 10, 20),
                    tok(6, "BAT", 10, 20),
                    tok(8, "SAT", 20, 30),
                ],
                arcs: vec![],
            },
        }
    }

    #[test]
    fn fabric_path_builds_transcript_with_fabric_record() {
        let utt = fixture_utt();
        let value =
            transcript_json_from_fabric(&utt, "fabric: fixture", &|_, _| 0.5, &|_| Some("cmudict"))
                .expect("fixture builds");
        assert_eq!(value["utt_id"], "u1");
        assert_eq!(value["text"], "THE CAT SAT");
        assert_eq!(value["path"], "fabric");
        assert_eq!(value["chosen"], FABRIC_CHOSEN_NOTE);
        let words = value["words"].as_array().expect("words array");
        assert_eq!(words.len(), 3);
        assert_eq!(words[1]["text"], "CAT");
        assert_eq!(words[1]["start_ms"], 100);
        assert_eq!(words[1]["end_ms"], 200);
        let why = &words[1]["why"];
        assert_eq!(why["acoustic_score"], 1.5);
        assert_eq!(why["alternatives"][0], "BAT");
        assert_eq!(why["lm_probability"], 0.5);
        assert_eq!(why["lexicon_source"], "cmudict");
        assert_eq!(why["evidence"]["acoustic"], 1.5);
        assert!(why["evidence"]["lexical"].is_null());
        assert_eq!(why["fabric_events"]["revivals"][0]["via"], "SAT");
        assert_eq!(why["fabric_events"]["committed"]["margin"], 1.5);
        assert_eq!(
            why["fabric_events"]["committed"]["tiling"],
            serde_json::json!(["THE", "CAT", "SAT"])
        );
        assert!(words[1].get("why_text").is_some());
    }

    #[test]
    fn fabric_path_rejects_commit_transcript_mismatch() {
        let mut utt = fixture_utt();
        utt.transcript_words = vec!["THE", "BAT", "SAT"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let err = transcript_json_from_fabric(&utt, "fabric: fixture", &|_, _| 0.5, &|_| None)
            .expect_err("mismatched transcript must fail");
        assert!(err.contains("bridge transcript"), "{err}");
    }

    #[test]
    fn fabric_path_rejects_unknown_commit_id() {
        let mut utt = fixture_utt();
        utt.lattice.tokens.retain(|t| t.id != 5);
        let err = transcript_json_from_fabric(&utt, "fabric: fixture", &|_, _| 0.5, &|_| None)
            .expect_err("unknown commit id must fail");
        assert!(err.contains("not in lattice"), "{err}");
    }

    #[test]
    fn fabric_path_uppercases_lexicon_lookup() {
        // Experiment fixtures are lowercase; the lexicon wants UPPER.
        // (Events keep UPPER words here; the join is by token id.)
        let mut utt = fixture_utt();
        for t in utt.lattice.tokens.iter_mut() {
            t.word = t.word.to_lowercase();
        }
        for w in utt.transcript_words.iter_mut() {
            *w = w.to_lowercase();
        }
        let value = transcript_json_from_fabric(&utt, "fabric: fixture", &|_, _| 0.5, &|w| {
            assert_eq!(w, w.to_uppercase(), "lexicon lookup must be UPPER");
            Some("cmudict")
        })
        .expect("lowercase fixture builds");
        assert_eq!(value["text"], "the cat sat");
        assert_eq!(value["words"][1]["why"]["lexicon_source"], "cmudict");
    }
}
