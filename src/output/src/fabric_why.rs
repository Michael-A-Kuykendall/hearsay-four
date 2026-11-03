//! File-path fabric why: per-token why records parsed from bridge v1 logs.
//!
//! Bead `hearsay-four-4lsp--fabric-path-demo`: the demo reads bridge outputs
//! (`<utt>.transcript.txt` + `<utt>.events.jsonl`, the handoff contract in the
//! `hearsay` crate docs under "Event-log format (v1, handoff contract)"), not
//! the decoder 1-best. This module parses the JSONL log and answers per-token
//! why queries: dormancy / revival, defeat (winner + margin), and commit
//! (region, tiling, margin).
//!
//! Correction facts: the bridge feeds no USER evidence (the `hearsay` docs
//! explain lattice LLRs carry no correction-premise provenance), so a v1 log
//! never names a correction fact. The record carries explicit none — the
//! correction ledger (`hearsay-correct`) is where facts and undo live.

use serde_json::Value;

/// The only bridge event-log version this parser accepts.
pub const SUPPORTED_EVENT_LOG_VERSION: u32 = 1;

/// One token reference as rendered in the v1 log: lattice tokens carry
/// `id`/`word`/`start`/`end`; a fact with no lattice token renders as
/// `{"fact": N}` and carries only `fact`.
#[derive(Debug, Clone, PartialEq)]
pub struct EventToken {
    pub id: Option<u64>,
    pub fact: Option<u64>,
    pub word: Option<String>,
    pub start: Option<u32>,
    pub end: Option<u32>,
}

/// One parsed v1 log line. Field shapes mirror `hearsay::logged_event`.
#[derive(Debug, Clone, PartialEq)]
pub enum FabricEvent {
    Pruned {
        seq: usize,
        token: EventToken,
        liveness: f64,
    },
    Revived {
        seq: usize,
        token: EventToken,
        via: EventToken,
        llr: f64,
    },
    Defeated {
        seq: usize,
        token: EventToken,
        winner: Vec<EventToken>,
        margin: Option<f64>,
    },
    Evicted {
        seq: usize,
        token: EventToken,
        liveness: f64,
        budget: String,
    },
    Committed {
        seq: usize,
        region: (u32, u32),
        winner: Vec<EventToken>,
        margin: Option<f64>,
    },
}

/// A whole utterance log: every event in emission order.
#[derive(Debug, Clone, PartialEq)]
pub struct FabricEvents {
    pub utterance: String,
    pub events: Vec<FabricEvent>,
}

fn req_u64(v: &Value, line: usize, field: &str) -> Result<u64, String> {
    v.get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("events line {line}: {field} must be a u64"))
}

fn req_u32(v: &Value, line: usize, field: &str) -> Result<u32, String> {
    let n = req_u64(v, line, field)?;
    u32::try_from(n).map_err(|_| format!("events line {line}: {field} {n} overflows u32"))
}

fn req_f64(v: &Value, line: usize, field: &str) -> Result<f64, String> {
    let x = v
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("events line {line}: {field} must be a number"))?;
    if x.is_finite() {
        Ok(x)
    } else {
        Err(format!("events line {line}: {field} must be finite"))
    }
}

fn opt_f64(v: &Value, line: usize, field: &str) -> Result<Option<f64>, String> {
    match v.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => req_f64(v, line, field).map(Some),
    }
}

fn req_str(v: &Value, line: usize, field: &str) -> Result<String, String> {
    v.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("events line {line}: {field} must be a string"))
}

fn parse_token(v: &Value, line: usize, field: &str) -> Result<EventToken, String> {
    let t = v
        .get(field)
        .ok_or_else(|| format!("events line {line}: missing {field}"))?;
    if let Some(fact) = t.get("fact").and_then(Value::as_u64) {
        return Ok(EventToken {
            id: None,
            fact: Some(fact),
            word: None,
            start: None,
            end: None,
        });
    }
    Ok(EventToken {
        id: Some(req_u64(t, line, "id")?),
        fact: None,
        word: Some(req_str(t, line, "word")?),
        start: Some(req_u32(t, line, "start")?),
        end: Some(req_u32(t, line, "end")?),
    })
}

fn parse_token_list(v: &Value, line: usize, field: &str) -> Result<Vec<EventToken>, String> {
    let arr = v
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("events line {line}: {field} must be an array"))?;
    arr.iter()
        .map(|t| {
            if let Some(fact) = t.get("fact").and_then(Value::as_u64) {
                return Ok(EventToken {
                    id: None,
                    fact: Some(fact),
                    word: None,
                    start: None,
                    end: None,
                });
            }
            Ok(EventToken {
                id: Some(req_u64(t, line, "id")?),
                fact: None,
                word: Some(req_str(t, line, "word")?),
                start: Some(req_u32(t, line, "start")?),
                end: Some(req_u32(t, line, "end")?),
            })
        })
        .collect()
}

fn req_region(v: &Value, line: usize) -> Result<(u32, u32), String> {
    let arr = v
        .get("region")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("events line {line}: region must be a [u32, u32] array"))?;
    if arr.len() != 2 {
        return Err(format!("events line {line}: region must have 2 elements"));
    }
    let pair: Vec<u32> = arr
        .iter()
        .map(|x| {
            x.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| format!("events line {line}: region elements must be u32"))
        })
        .collect::<Result<_, _>>()?;
    Ok((pair[0], pair[1]))
}

fn parse_event(v: &Value, line: usize, seq: usize, kind: &str) -> Result<FabricEvent, String> {
    match kind {
        "pruned" => Ok(FabricEvent::Pruned {
            seq,
            token: parse_token(v, line, "token")?,
            liveness: req_f64(v, line, "liveness")?,
        }),
        "revived" => Ok(FabricEvent::Revived {
            seq,
            token: parse_token(v, line, "token")?,
            via: parse_token(v, line, "via")?,
            llr: req_f64(v, line, "llr")?,
        }),
        "defeated" => Ok(FabricEvent::Defeated {
            seq,
            token: parse_token(v, line, "token")?,
            winner: parse_token_list(v, line, "winner")?,
            margin: opt_f64(v, line, "margin")?,
        }),
        "evicted" => Ok(FabricEvent::Evicted {
            seq,
            token: parse_token(v, line, "token")?,
            liveness: req_f64(v, line, "liveness")?,
            budget: req_str(v, line, "budget")?,
        }),
        "committed" => Ok(FabricEvent::Committed {
            seq,
            region: req_region(v, line)?,
            winner: parse_token_list(v, line, "winner")?,
            margin: opt_f64(v, line, "margin")?,
        }),
        other => Err(format!("events line {line}: unknown event {other:?}")),
    }
}

/// Parse one bridge v1 JSONL log. Fails loudly on bad JSON, a wrong `v`, a
/// mixed `utterance`, a `seq` gap, or an unknown event — never silently.
pub fn parse_events_jsonl(text: &str) -> Result<FabricEvents, String> {
    let mut events = Vec::new();
    let mut utterance: Option<String> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("events line {line}: invalid JSON: {e}"))?;
        let version = req_u64(&v, line, "v")?;
        if version != u64::from(SUPPORTED_EVENT_LOG_VERSION) {
            return Err(format!(
                "events line {line}: v {version} != supported {SUPPORTED_EVENT_LOG_VERSION}"
            ));
        }
        let utt = req_str(&v, line, "utterance")?;
        match &utterance {
            None => utterance = Some(utt),
            Some(first) if *first == utt => {}
            Some(first) => {
                return Err(format!(
                    "events line {line}: utterance {utt:?} != log utterance {first:?}"
                ));
            }
        }
        let seq = req_u64(&v, line, "seq")? as usize;
        if seq != events.len() {
            return Err(format!(
                "events line {line}: seq {seq} breaks 0-based emission order (want {})",
                events.len()
            ));
        }
        let kind = req_str(&v, line, "event")?;
        events.push(parse_event(&v, line, seq, &kind)?);
    }
    match utterance {
        Some(utterance) => Ok(FabricEvents { utterance, events }),
        None => Err("events log is empty".to_string()),
    }
}

/// One dormancy-to-live revival of a token: the successor that triggered it
/// and the SEQUENCE llr that crossed `theta_live`.
#[derive(Debug, Clone, PartialEq)]
pub struct RevivalRec {
    pub via_word: String,
    pub llr: f64,
}

/// The commit a token won: region frames, full defeating tiling words, and
/// the winner-vs-runner-up margin (`None` when unopposed).
#[derive(Debug, Clone, PartialEq)]
pub struct CommitRec {
    pub region: (u32, u32),
    pub tiling_words: Vec<String>,
    pub margin: Option<f64>,
}

/// Everything the v1 log says about one lattice token.
#[derive(Debug, Clone, PartialEq)]
pub struct FabricTokenWhy {
    pub token_id: u64,
    pub revivals: Vec<RevivalRec>,
    pub pruned_liveness: Option<f64>,
    pub evicted: Option<(f64, String)>,
    /// `(winner words, margin)` when this token lost a defeat record.
    pub defeated_by: Option<(Vec<String>, Option<f64>)>,
    /// Loser words whose defeat record this token's tiling won.
    pub beat: Vec<String>,
    pub committed: Option<CommitRec>,
}

fn present_words(tokens: &[EventToken]) -> Vec<String> {
    tokens.iter().filter_map(|t| t.word.clone()).collect()
}

fn display_word(token: &EventToken) -> String {
    if let Some(word) = &token.word {
        return word.clone();
    }
    match token.fact {
        Some(fact) => format!("fact{fact}"),
        None => "(unknown)".to_string(),
    }
}

impl FabricEvents {
    /// Collect this token's record: revivals, prune/evict liveness, defeat
    /// outcome both ways, and the commit it won, if any.
    pub fn why_for(&self, token_id: u64) -> FabricTokenWhy {
        let mut why = FabricTokenWhy {
            token_id,
            revivals: Vec::new(),
            pruned_liveness: None,
            evicted: None,
            defeated_by: None,
            beat: Vec::new(),
            committed: None,
        };
        for event in &self.events {
            match event {
                FabricEvent::Pruned {
                    token, liveness, ..
                } if token.id == Some(token_id) => {
                    why.pruned_liveness = Some(*liveness);
                }
                FabricEvent::Revived {
                    token, via, llr, ..
                } if token.id == Some(token_id) => {
                    why.revivals.push(RevivalRec {
                        via_word: display_word(via),
                        llr: *llr,
                    });
                }
                FabricEvent::Defeated {
                    token,
                    winner,
                    margin,
                    ..
                } => {
                    if token.id == Some(token_id) {
                        why.defeated_by = Some((present_words(winner), *margin));
                    } else if winner.iter().any(|w| w.id == Some(token_id))
                        && let Some(word) = &token.word
                        && !why.beat.contains(word)
                    {
                        why.beat.push(word.clone());
                    }
                }
                FabricEvent::Evicted {
                    token,
                    liveness,
                    budget,
                    ..
                } if token.id == Some(token_id) => {
                    why.evicted = Some((*liveness, budget.clone()));
                }
                FabricEvent::Committed {
                    region,
                    winner,
                    margin,
                    ..
                } if winner.iter().any(|w| w.id == Some(token_id)) => {
                    why.committed = Some(CommitRec {
                        region: *region,
                        tiling_words: present_words(winner),
                        margin: *margin,
                    });
                }
                _ => {}
            }
        }
        why
    }

    /// Committed winner token ids in commit order. Concatenating their words
    /// must reproduce the bridge transcript; fact-only winners (no lattice
    /// token) are skipped and the transcript cross-check fails loudly.
    pub fn committed_token_ids(&self) -> Vec<u64> {
        let mut ids = Vec::new();
        for event in &self.events {
            if let FabricEvent::Committed { winner, .. } = event {
                ids.extend(winner.iter().filter_map(|w| w.id));
            }
        }
        ids
    }

    /// Late rescues: revived tokens that end up committed.
    pub fn revived_and_committed(&self) -> Vec<u64> {
        let committed = self.committed_token_ids();
        let mut rescued = Vec::new();
        for event in &self.events {
            if let FabricEvent::Revived { token, .. } = event
                && let Some(id) = token.id
                && committed.contains(&id)
                && !rescued.contains(&id)
            {
                rescued.push(id);
            }
        }
        rescued
    }
}

impl FabricTokenWhy {
    /// The demo why-panel `fabric` record: revivals, dormancy outcome, the
    /// defeating tiling + margin, and explicit correction none (the v1 log
    /// carries no USER evidence by bridge design).
    pub fn record_json(&self) -> Value {
        let revivals: Vec<Value> = self
            .revivals
            .iter()
            .map(|r| serde_json::json!({"via": r.via_word, "llr": r.llr}))
            .collect();
        let defeated_by = self
            .defeated_by
            .as_ref()
            .map(|(words, margin)| serde_json::json!({"winner": words, "margin": margin}));
        let committed = self.committed.as_ref().map(|c| {
            serde_json::json!({
                "region": [c.region.0, c.region.1],
                "tiling": c.tiling_words,
                "margin": c.margin,
            })
        });
        let evicted = self
            .evicted
            .as_ref()
            .map(|(liveness, budget)| serde_json::json!({"liveness": liveness, "budget": budget}));
        serde_json::json!({
            "revivals": revivals,
            "pruned_liveness": self.pruned_liveness,
            "evicted": evicted,
            "defeated_by": defeated_by,
            "alternatives_beaten": self.beat,
            "committed": committed,
            "corrections": [],
            "correction_undo": "none",
            "corrections_note": "bridge v1 log carries no USER evidence (correction facts live in the hearsay-correct ledger)",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Miniature v1 log in exact bridge shape: a prune, a revival, a defeat,
    /// an eviction, and one commit. All words are invented for this fixture.
    fn fixture_log() -> &'static str {
        concat!(
            "{\"v\":1,\"utterance\":\"u1\",\"seq\":0,\"event\":\"pruned\",",
            "\"token\":{\"id\":9,\"word\":\"ZQX\",\"start\":0,\"end\":10},\"liveness\":-1.0}\n",
            "{\"v\":1,\"utterance\":\"u1\",\"seq\":1,\"event\":\"revived\",",
            "\"token\":{\"id\":5,\"word\":\"CAT\",\"start\":10,\"end\":20},",
            "\"via\":{\"id\":8,\"word\":\"SAT\",\"start\":20,\"end\":30},\"llr\":4.0}\n",
            "{\"v\":1,\"utterance\":\"u1\",\"seq\":2,\"event\":\"defeated\",",
            "\"token\":{\"id\":6,\"word\":\"BAT\",\"start\":10,\"end\":20},",
            "\"winner\":[{\"id\":5,\"word\":\"CAT\",\"start\":10,\"end\":20}],\"margin\":1.5}\n",
            "{\"v\":1,\"utterance\":\"u1\",\"seq\":3,\"event\":\"evicted\",",
            "\"token\":{\"id\":7,\"word\":\"MAT\",\"start\":10,\"end\":20},",
            "\"liveness\":-0.5,\"budget\":\"dormant\"}\n",
            "{\"v\":1,\"utterance\":\"u1\",\"seq\":4,\"event\":\"committed\",\"region\":[0,30],",
            "\"winner\":[{\"id\":1,\"word\":\"THE\",\"start\":0,\"end\":10},",
            "{\"id\":5,\"word\":\"CAT\",\"start\":10,\"end\":20},",
            "{\"id\":8,\"word\":\"SAT\",\"start\":20,\"end\":30}],\"margin\":1.5}\n",
        )
    }

    #[test]
    fn why_fabric_parses_v1_log() {
        let log = parse_events_jsonl(fixture_log()).expect("fixture parses");
        assert_eq!(log.utterance, "u1");
        assert_eq!(log.events.len(), 5);
        assert!(matches!(log.events[0], FabricEvent::Pruned { .. }));
        assert!(matches!(log.events[1], FabricEvent::Revived { .. }));
        assert!(matches!(log.events[2], FabricEvent::Defeated { .. }));
        assert!(matches!(log.events[3], FabricEvent::Evicted { .. }));
        assert!(matches!(log.events[4], FabricEvent::Committed { .. }));
        assert_eq!(log.committed_token_ids(), vec![1, 5, 8]);
    }

    #[test]
    fn why_fabric_revival_margin_and_tiling() {
        let log = parse_events_jsonl(fixture_log()).expect("fixture parses");
        let why = log.why_for(5);
        assert_eq!(why.token_id, 5);
        assert_eq!(why.revivals.len(), 1);
        assert_eq!(why.revivals[0].via_word, "SAT");
        assert_eq!(why.revivals[0].llr, 4.0);
        assert_eq!(why.beat, vec!["BAT".to_string()]);
        assert_eq!(why.defeated_by, None);
        let commit = why.committed.as_ref().expect("CAT committed");
        assert_eq!(commit.region, (0, 30));
        assert_eq!(commit.tiling_words, vec!["THE", "CAT", "SAT"]);
        assert_eq!(commit.margin, Some(1.5));

        let loser = log.why_for(6);
        assert_eq!(loser.revivals.len(), 0);
        assert_eq!(
            loser.defeated_by,
            Some((vec!["CAT".to_string()], Some(1.5)))
        );
        assert_eq!(loser.committed, None);

        let record = why.record_json();
        assert_eq!(record["revivals"][0]["via"], "SAT");
        assert_eq!(record["committed"]["margin"], 1.5);
        assert_eq!(record["alternatives_beaten"][0], "BAT");
        assert_eq!(record["corrections"], serde_json::json!([]));
        assert_eq!(record["correction_undo"], "none");
    }

    #[test]
    fn why_fabric_late_rescue_detection() {
        let log = parse_events_jsonl(fixture_log()).expect("fixture parses");
        assert_eq!(log.revived_and_committed(), vec![5]);
        // Pruned and evicted tokens never committed: no rescue claimed.
        assert_eq!(log.why_for(9).committed, None);
        assert_eq!(log.why_for(9).pruned_liveness, Some(-1.0));
        assert_eq!(log.why_for(7).evicted, Some((-0.5, "dormant".to_string())));
    }

    #[test]
    fn why_fabric_rejects_bad_version_seq_and_shape() {
        let bad_version = fixture_log().replacen("\"v\":1", "\"v\":2", 1);
        assert!(parse_events_jsonl(&bad_version).is_err());

        let lines: Vec<&str> = fixture_log().lines().collect();
        let dropped: String = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2)
            .map(|(_, l)| *l)
            .collect::<Vec<_>>()
            .join("\n");
        let err = parse_events_jsonl(&dropped).expect_err("seq gap must fail");
        assert!(err.contains("seq"), "{err}");

        let mixed = fixture_log().replacen("\"utterance\":\"u1\"", "\"utterance\":\"u2\"", 1);
        assert!(parse_events_jsonl(&mixed).is_err());

        assert!(parse_events_jsonl("").is_err());
        assert!(parse_events_jsonl("not json\n").is_err());
        let unknown = "{\"v\":1,\"utterance\":\"u1\",\"seq\":0,\"event\":\"napped\"}\n";
        assert!(parse_events_jsonl(unknown).is_err());
    }
}
