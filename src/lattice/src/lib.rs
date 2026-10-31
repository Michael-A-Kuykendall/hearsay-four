//! Versioned on-disk word-lattice format (bead g0m).
//!
//! JSONL: a header line, then token (`{"t":...}`) and arc (`{"a":...}`)
//! lines in any order. Tokens carry per-occurrence identity (A3:
//! WORD_TOKEN{word,start,end,source}) plus unary-class llrs (ACOUSTIC
//! required; LEXICAL/SPEAKER/VERIFY/USER optional-absent per contract
//! rule 2). Arcs carry topology only: the SEQUENCE/LM term is
//! fabric-computed and MUST NOT appear in any lattice field (contract
//! §6) — the reader rejects `lm_llr`/`sequence_llr`/`path_score` keys
//! loudly, which is the on-disk half of the byte-identity requirement.
//! Full spec: `docs/design/lattice-format.md` (v1).

use std::collections::BTreeSet;

pub const FORMAT_NAME: &str = "hearsay-word-lattice";
pub const VERSION_MAJOR: u32 = 1;
pub const VERSION_MINOR: u32 = 0;

/// Keys that must never appear in a token object (contract §6: LM evidence
/// never lives in lattice fields; their absence is the on-disk half of the
/// byte-identity requirement A5 re-checks).
const FORBIDDEN_TOKEN_KEYS: [&str; 3] = ["lm_llr", "sequence_llr", "path_score"];

/// Timed word token with per-occurrence identity (A3) + unary-class llrs.
/// ACOUSTIC is required; the other four are optional-absent (contract §3
/// rule 2: absent class = absent term, never a negative llr).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Token {
    pub id: u64,
    pub word: String,
    pub start: u32,
    pub end: u32,
    pub source: String,
    pub acoustic_llr: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_llr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_llr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_llr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_llr: Option<f64>,
}

/// Lattice arc: topology only (from-token → to-token). SEQUENCE llrs are
/// fabric-computed from the LM and never stored here.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Arc {
    pub from: u64,
    pub to: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lattice {
    pub utterance: String,
    pub frame_shift_ms: f64,
    pub producer: String,
    pub tokens: Vec<Token>,
    pub arcs: Vec<Arc>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Header {
    format: String,
    version_major: u32,
    version_minor: u32,
    utterance: String,
    frame_shift_ms: f64,
    producer: String,
}

fn check_finite(name: &str, id: u64, v: f64) -> Result<(), String> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(format!("lattice token {id} non-finite {name}"))
    }
}

/// Serialize a lattice to JSONL (header, tokens, arcs). Non-finite floats
/// are a loud writer error, never silent nulls.
pub fn write_lattice(lat: &Lattice) -> Result<String, String> {
    if !lat.frame_shift_ms.is_finite() {
        return Err("lattice non-finite frame_shift_ms".to_string());
    }
    for t in &lat.tokens {
        check_finite("acoustic_llr", t.id, t.acoustic_llr)?;
        for (name, v) in [
            ("lexical_llr", t.lexical_llr),
            ("speaker_llr", t.speaker_llr),
            ("verify_llr", t.verify_llr),
            ("user_llr", t.user_llr),
        ] {
            if let Some(x) = v {
                check_finite(name, t.id, x)?;
            }
        }
    }
    let mut out = String::new();
    let header = Header {
        format: FORMAT_NAME.to_string(),
        version_major: VERSION_MAJOR,
        version_minor: VERSION_MINOR,
        utterance: lat.utterance.clone(),
        frame_shift_ms: lat.frame_shift_ms,
        producer: lat.producer.clone(),
    };
    out.push_str(
        &serde_json::to_string(&header).map_err(|e| format!("lattice write header: {e}"))?,
    );
    out.push('\n');
    for t in &lat.tokens {
        let line = serde_json::json!({"t": t});
        out.push_str(
            &serde_json::to_string(&line)
                .map_err(|e| format!("lattice write token {}: {e}", t.id))?,
        );
        out.push('\n');
    }
    for a in &lat.arcs {
        let line = serde_json::json!({"a": a});
        out.push_str(&serde_json::to_string(&line).map_err(|e| format!("lattice write arc: {e}"))?);
        out.push('\n');
    }
    Ok(out)
}

/// Parse JSONL back into a lattice. Unknown MAJOR versions are rejected
/// loudly; same-major/minor differences are accepted with unknown fields
/// ignored (additive-only minor rule), except the forbidden LM keys.
pub fn read_lattice(text: &str) -> Result<Lattice, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let first = lines
        .next()
        .ok_or("lattice empty: missing header line".to_string())?;
    let hv: serde_json::Value =
        serde_json::from_str(first).map_err(|e| format!("lattice header not JSON: {e}"))?;
    let h: Header =
        serde_json::from_value(hv).map_err(|e| format!("lattice header malformed: {e}"))?;
    if h.format != FORMAT_NAME {
        return Err(format!("lattice format {:?} != {FORMAT_NAME:?}", h.format));
    }
    if h.version_major != VERSION_MAJOR {
        return Err(format!(
            "lattice major version {} unsupported (reader v{VERSION_MAJOR}); refusing loudly",
            h.version_major
        ));
    }
    let mut tokens = Vec::new();
    let mut arcs = Vec::new();
    for (n, line) in lines.enumerate() {
        let v: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| format!("lattice line {} not JSON: {e}", n + 2))?;
        let obj = v
            .as_object()
            .ok_or_else(|| format!("lattice line {} not an object", n + 2))?;
        if let Some(t) = obj.get("t") {
            let tobj = t
                .as_object()
                .ok_or_else(|| format!("lattice line {} token not an object", n + 2))?;
            for k in FORBIDDEN_TOKEN_KEYS {
                if tobj.contains_key(k) {
                    return Err(format!(
                        "lattice line {} forbidden key {k:?}: LM evidence must never live in lattice fields (contract §6)",
                        n + 2
                    ));
                }
            }
            let tok: Token = serde_json::from_value(t.clone())
                .map_err(|e| format!("lattice line {} bad token: {e}", n + 2))?;
            tokens.push(tok);
        } else if let Some(a) = obj.get("a") {
            let arc: Arc = serde_json::from_value(a.clone())
                .map_err(|e| format!("lattice line {} bad arc: {e}", n + 2))?;
            arcs.push(arc);
        } else {
            return Err(format!(
                "lattice line {} unknown kind (need t/a): {line}",
                n + 2
            ));
        }
    }
    let mut ids = BTreeSet::new();
    for t in &tokens {
        if !ids.insert(t.id) {
            return Err(format!("lattice duplicate token id {}", t.id));
        }
        if t.word.is_empty() {
            return Err(format!("lattice token {} empty word", t.id));
        }
        if t.start >= t.end {
            return Err(format!(
                "lattice token {} start {} >= end {}",
                t.id, t.start, t.end
            ));
        }
        check_finite("acoustic_llr", t.id, t.acoustic_llr)?;
        for (name, v) in [
            ("lexical_llr", t.lexical_llr),
            ("speaker_llr", t.speaker_llr),
            ("verify_llr", t.verify_llr),
            ("user_llr", t.user_llr),
        ] {
            if let Some(x) = v {
                check_finite(name, t.id, x)?;
            }
        }
    }
    for a in &arcs {
        if !ids.contains(&a.from) {
            return Err(format!(
                "lattice arc from={} dangles (no such token)",
                a.from
            ));
        }
        if !ids.contains(&a.to) {
            return Err(format!("lattice arc to={} dangles (no such token)", a.to));
        }
    }
    Ok(Lattice {
        utterance: h.utterance,
        frame_shift_ms: h.frame_shift_ms,
        producer: h.producer,
        tokens,
        arcs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_lattice() -> Lattice {
        Lattice {
            utterance: "synth-001".to_string(),
            frame_shift_ms: 10.0,
            producer: "g0m-test".to_string(),
            tokens: vec![
                Token {
                    id: 1,
                    word: "hello".into(),
                    start: 0,
                    end: 30,
                    source: "5n2".into(),
                    acoustic_llr: 3.25,
                    lexical_llr: Some(0.5),
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
                Token {
                    id: 2,
                    word: "hollow".into(),
                    start: 0,
                    end: 32,
                    source: "5n2".into(),
                    acoustic_llr: 1.1,
                    lexical_llr: None,
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: None,
                },
                Token {
                    id: 3,
                    word: "world".into(),
                    start: 30,
                    end: 60,
                    source: "5n2".into(),
                    acoustic_llr: 4.0,
                    lexical_llr: None,
                    speaker_llr: None,
                    verify_llr: None,
                    user_llr: Some(9.0),
                },
            ],
            arcs: vec![Arc { from: 1, to: 3 }, Arc { from: 2, to: 3 }],
        }
    }

    #[test]
    fn lattice_round_trip_write_read_identical() {
        let lat = synth_lattice();
        let text = write_lattice(&lat).expect("write");
        let back = read_lattice(&text).expect("read");
        assert_eq!(back, lat);
        let text2 = write_lattice(&back).expect("rewrite");
        assert_eq!(text2, text, "serialization is byte-stable");
    }

    #[test]
    fn lattice_rejects_unknown_major_version() {
        let bad = "{\"format\":\"hearsay-word-lattice\",\"version_major\":2,\"version_minor\":0,\"utterance\":\"x\",\"frame_shift_ms\":10.0,\"producer\":\"t\"}\n";
        let e = read_lattice(bad).expect_err("major 2 rejects");
        assert!(e.contains('2') && e.contains("major"), "violation: {e}");
    }

    #[test]
    fn lattice_rejects_lm_fields() {
        let lat = synth_lattice();
        let mut text = write_lattice(&lat).expect("write");
        text.push_str("{\"t\":{\"id\":9,\"word\":\"intruder\",\"start\":0,\"end\":10,\"source\":\"x\",\"acoustic_llr\":1.0,\"lm_llr\":5.0}}\n");
        let e = read_lattice(&text).expect_err("lm field rejects");
        assert!(e.contains("lm_llr"), "violation: {e}");
    }

    #[test]
    fn lattice_rejects_dangling_arc() {
        let mut lat = synth_lattice();
        lat.arcs.push(Arc { from: 1, to: 99 });
        let text = write_lattice(&lat).expect("write");
        let e = read_lattice(&text).expect_err("dangling arc rejects");
        assert!(e.contains("99"), "violation: {e}");
    }

    #[test]
    fn lattice_jsonl_size_bounded() {
        // Encoding-choice measurement: 1000 tokens + 2000 arcs must stay
        // comfortably textual (bound 1MB; actual printed for the spec).
        let mut lat = Lattice {
            utterance: "big".into(),
            frame_shift_ms: 10.0,
            producer: "g0m-test".into(),
            tokens: vec![],
            arcs: vec![],
        };
        for i in 0..1000u64 {
            lat.tokens.push(Token {
                id: i,
                word: format!("w{i}"),
                start: (i * 10) as u32,
                end: (i * 10 + 9) as u32,
                source: "s".into(),
                acoustic_llr: 1.5,
                lexical_llr: None,
                speaker_llr: None,
                verify_llr: None,
                user_llr: None,
            });
            lat.arcs.push(Arc {
                from: i,
                to: (i + 1) % 1000,
            });
            lat.arcs.push(Arc {
                from: i,
                to: (i + 2) % 1000,
            });
        }
        let text = write_lattice(&lat).expect("write");
        println!("MEASURED-BYTES-1000T-2000A: {}", text.len());
        assert!(
            text.len() < 1_000_000,
            "jsonl stays textual: {} bytes",
            text.len()
        );
        let back = read_lattice(&text).expect("read");
        assert_eq!(back.tokens.len(), 1000);
        assert_eq!(back.arcs.len(), 2000);
    }

    #[test]
    fn lattice_reads_d0_derived_fixture() {
        let text = include_str!("../tests/fixtures/d0_words.lat.jsonl");
        let lat = read_lattice(text).expect("fixture reads");
        assert_eq!(lat.tokens.len(), 8);
        assert_eq!(lat.utterance, "d0-fixture-001");
        // Oracle-relevant shape: position 0 has two alternatives.
        let alts: Vec<_> = lat.tokens.iter().filter(|t| t.start == 0).collect();
        assert_eq!(alts.len(), 2);
    }
}
