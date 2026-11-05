//! P2 why-this-word view (bead hearsay-four-ofy): per committed word, the
//! complete evidence chain behind the commit, rendered for a reviewer and
//! exportable as JSON with the transcript.
//!
//! Substrate: the existing [`WordRecord`] provenance record — extended,
//! never replaced. [`WhyThisWord`] embeds the record verbatim and adds the
//! decision-layer explanation consumed from A3 token facts (fabric
//! `token` module: label, frame span, source) and A4b contract terms
//! (fabric `decision` module, spec 1.0): acoustic `llr` and frames, LM
//! probability and history, speaker/adaptation contribution, alternatives
//! with scores, the defeating tiling and margin, revival events, and the
//! user facts involved.
//!
//! Every score shown is a tiling-score `S(T)` component (contract §4),
//! recomputed from currently active justifications. Liveness `L(h)`
//! (contract §5) is never computed, stored, or rendered here.

use crate::{ChainError, Transcript, WordRecord};
use fabric::FactId;
use fabric::decision::{DecisionFabric, SourceClass, spans_conflict};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One source-class term of a token's tiling score (§3, §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassTerm {
    /// Source class name (contract §3).
    pub class: String,
    /// Strongest active `llr`, or `None` for an absent term (§3 rule 2).
    pub llr: Option<f64>,
    /// Frozen per-class weight `w_c` (§12).
    pub weight: f64,
    /// Weighted contribution `w_c * llr` (0.0 when absent).
    pub weighted: f64,
    /// Declared null the `llr` was computed against (§3 rule 1).
    pub null: String,
}

/// Acoustic evidence for the committed word: `llr` over exactly the
/// token's frames (§3 ACOUSTIC), with the A3 token span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcousticWhy {
    pub llr: Option<f64>,
    pub start_frame: u32,
    pub end_frame: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub null: String,
    /// A3 `WORD_TOKEN.source` for this occurrence, when recorded.
    pub source: Option<String>,
}

/// LM evidence: the SEQUENCE term (§3, §4) with its history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LmWhy {
    /// Word history ending at the predecessor (`<s>` first).
    pub history: Vec<String>,
    /// Predecessor word label; `None` is the `<s>` boundary pair.
    pub predecessor: Option<String>,
    pub sequence_llr: Option<f64>,
    pub sequence_weighted: f64,
    /// `P(word | history)` from the caller's LM (contract §3 SEQUENCE
    /// definition); the contract stores only the `llr` against the
    /// unigram null, so the probability itself is supplied by the LM
    /// owner and rendered alongside it.
    pub lm_probability: Option<f64>,
    pub null: String,
}

/// A defeated alternative with its tiling-score components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlternativeWhy {
    pub text: String,
    pub token_id: u64,
    /// This token's per-token `S(T)` total inside the runner-up tiling.
    pub tiling_component_total: f64,
    pub acoustic_llr: Option<f64>,
    pub sequence_llr: Option<f64>,
}

/// Defeat information (§10): the tiling that committed, and its margin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefeatWhy {
    pub defeating_tiling: Vec<String>,
    pub defeating_tiling_ids: Vec<u64>,
    pub defeating_tiling_score: f64,
    /// `None` when the winner was unopposed (no runner-up tiling).
    pub margin: Option<f64>,
    /// Labels of non-winning tokens defeated in the committed region.
    pub defeated_tokens: Vec<String>,
}

/// One revival event (§9.4) that lifted this token before commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevivalWhy {
    pub via_text: String,
    pub via_token_id: u64,
    pub llr: f64,
}

/// The complete why-this-word explanation for one committed word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhyThisWord {
    /// The substrate record, embedded verbatim (extend, do not replace).
    pub record: WordRecord,
    /// A3/A4b token fact id of the committed word.
    pub token_id: u64,
    pub acoustic: AcousticWhy,
    pub lm: LmWhy,
    pub speaker: ClassTerm,
    pub lexical: ClassTerm,
    pub verify: ClassTerm,
    pub user: ClassTerm,
    /// USER correction fact ids gating the USER term (§3 rules 4-5).
    pub user_facts: Vec<u64>,
    pub alternatives: Vec<AlternativeWhy>,
    /// Why `alternatives` is empty, when it is (e.g. unopposed winner).
    pub alternatives_note: String,
    pub defeat: DefeatWhy,
    pub revivals: Vec<RevivalWhy>,
    /// Weighted acoustic term `w_acoustic * llr` in `S(T)`.
    pub acoustic_weighted: f64,
    /// This token's per-token `S(T)` total in the winning tiling.
    pub tiling_token_total: f64,
    /// Winning tiling total `S(T)`.
    pub tiling_total: f64,
    /// Two-tier lexicon provenance tag (`cmudict`/`librispeech-g2p`) from
    /// the committed token, when recorded. Additive: exports written before
    /// this field still deserialize to `None`.
    #[serde(default)]
    pub lexicon_source: Option<String>,
}

/// Why-view construction / completeness failures.
#[derive(Debug, Clone, PartialEq)]
pub enum WhyError {
    RecordInvalid(ChainError),
    UnknownToken(u64),
    MissingAcoustic(u64),
    MissingSequence(u64),
    MissingLmProbability(u64),
    NonFinite(&'static str),
    Incomplete(String),
}

fn class_name(class: SourceClass) -> &'static str {
    match class {
        SourceClass::Acoustic => "acoustic",
        SourceClass::Lexical => "lexical",
        SourceClass::Sequence => "sequence",
        SourceClass::Speaker => "speaker",
        SourceClass::Verify => "verify",
        SourceClass::User => "user",
    }
}

fn finite(v: f64, field: &'static str) -> Result<f64, WhyError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(WhyError::NonFinite(field))
    }
}

impl WhyThisWord {
    /// Build the complete explanation for one committed word from the
    /// decision fabric's live state. `winning` is the committed tiling,
    /// `runner_up` the defeated runner-up tiling (if any), `margin` the
    /// §10 commit margin (`None` when unopposed), and `lm_probabilities`
    /// the LM owner's `P(word | history)` per token fact id.
    pub fn build(
        record: WordRecord,
        fabric: &DecisionFabric,
        token: FactId,
        winning: &[FactId],
        runner_up: Option<&[FactId]>,
        margin: Option<f64>,
        lm_probabilities: &BTreeMap<FactId, f64>,
    ) -> Result<Self, WhyError> {
        record.validate().map_err(WhyError::RecordInvalid)?;
        let tok = fabric
            .token(token)
            .ok_or(WhyError::UnknownToken(token.0))?
            .clone();
        let pos = winning
            .iter()
            .position(|t| *t == token)
            .ok_or(WhyError::UnknownToken(token.0))?;
        let prev = pos.checked_sub(1).map(|i| winning[i]);
        let breakdown = fabric.token_breakdown(prev, token);
        let sequence_llr = breakdown.sequence_llr;
        let token_total = breakdown.total;
        let get = |c: SourceClass| -> ClassTerm {
            let t = breakdown
                .terms
                .iter()
                .find(|t| t.class == c)
                .expect("every unary class has a term");
            ClassTerm {
                class: class_name(c).to_string(),
                llr: t.llr,
                weight: t.weight,
                weighted: t.weighted,
                null: c.declared_null().to_string(),
            }
        };
        let acoustic_llr = fabric
            .published_acoustic_llr(token)
            .ok_or(WhyError::MissingAcoustic(token.0))?;
        finite(acoustic_llr, "acoustic.llr")?;
        let sequence_llr = sequence_llr.ok_or(WhyError::MissingSequence(token.0))?;
        finite(sequence_llr, "lm.sequence_llr")?;
        let lm_probability = lm_probabilities
            .get(&token)
            .copied()
            .ok_or(WhyError::MissingLmProbability(token.0))?;
        if !lm_probability.is_finite()
            || !(0.0..=1.0).contains(&lm_probability)
            || lm_probability == 0.0
        {
            return Err(WhyError::NonFinite("lm.lm_probability"));
        }
        let history: Vec<String> = if pos == 0 {
            vec!["<s>".to_string()]
        } else {
            let mut h = vec!["<s>".to_string()];
            h.extend(winning[..pos].iter().map(|t| {
                fabric
                    .token(*t)
                    .map(|x| x.label.clone())
                    .unwrap_or_default()
            }));
            h
        };
        let source = match fabric.fabric().token_fact(token) {
            Some(fabric::TokenFact::Word(w)) => Some(w.source.clone()),
            _ => None,
        };
        let lexicon_source = match fabric.fabric().token_fact(token) {
            Some(fabric::TokenFact::Word(w)) => w.lexicon_source.clone(),
            _ => None,
        };
        let mut alternatives = Vec::new();
        if let Some(ru) = runner_up {
            for (i, alt) in ru.iter().enumerate() {
                if let (Some(a), Some(_)) = (fabric.token(*alt), fabric.token(token))
                    && spans_conflict(a, &tok)
                {
                    let aprev = i.checked_sub(1).map(|j| ru[j]);
                    let ab = fabric.token_breakdown(aprev, *alt);
                    alternatives.push(AlternativeWhy {
                        text: a.label.clone(),
                        token_id: alt.0,
                        tiling_component_total: finite(ab.total, "alternative.total")?,
                        acoustic_llr: fabric.published_acoustic_llr(*alt),
                        sequence_llr: ab.sequence_llr,
                    });
                }
            }
        }
        let alternatives_note = if alternatives.is_empty() {
            if runner_up.is_none() {
                "no runner-up tiling: winner was unopposed".to_string()
            } else {
                "no runner-up token conflicts with this span".to_string()
            }
        } else {
            String::new()
        };
        let defeated_tokens: Vec<String> = fabric
            .events()
            .iter()
            .filter_map(|e| match e {
                fabric::decision::DecisionEvent::Defeated {
                    token: t, winner, ..
                } if winner == winning => fabric.token(*t).map(|x| x.label.clone()),
                _ => None,
            })
            .collect();
        let revivals = fabric
            .revivals_for(token)
            .into_iter()
            .map(|(via, llr)| RevivalWhy {
                via_text: fabric
                    .token(via)
                    .map(|x| x.label.clone())
                    .unwrap_or_default(),
                via_token_id: via.0,
                llr,
            })
            .collect();
        let (_, _, tiling_total) = fabric.tiling_breakdown(winning);
        let why = Self {
            record,
            token_id: token.0,
            acoustic: AcousticWhy {
                llr: Some(acoustic_llr),
                start_frame: tok.start_frame,
                end_frame: tok.end_frame,
                start_ms: 0,
                end_ms: 0,
                null: SourceClass::Acoustic.declared_null().to_string(),
                source,
            },
            lm: LmWhy {
                history,
                predecessor: prev
                    .map(|p| fabric.token(p).map(|x| x.label.clone()).unwrap_or_default()),
                sequence_llr: Some(sequence_llr),
                sequence_weighted: fabric.params().w_sequence * sequence_llr,
                lm_probability: Some(lm_probability),
                null: SourceClass::Sequence.declared_null().to_string(),
            },
            speaker: get(SourceClass::Speaker),
            lexical: get(SourceClass::Lexical),
            verify: get(SourceClass::Verify),
            user: get(SourceClass::User),
            user_facts: fabric
                .user_correction_facts(token)
                .iter()
                .map(|f| f.0)
                .collect(),
            alternatives,
            alternatives_note,
            defeat: DefeatWhy {
                defeating_tiling: winning
                    .iter()
                    .map(|t| {
                        fabric
                            .token(*t)
                            .map(|x| x.label.clone())
                            .unwrap_or_default()
                    })
                    .collect(),
                defeating_tiling_ids: winning.iter().map(|t| t.0).collect(),
                defeating_tiling_score: finite(tiling_total, "defeat.score")?,
                margin,
                defeated_tokens,
            },
            revivals,
            acoustic_weighted: get(SourceClass::Acoustic).weighted,
            tiling_token_total: finite(token_total, "tiling_token_total")?,
            tiling_total: finite(tiling_total, "tiling_total")?,
            lexicon_source,
        };
        let mut why = why;
        why.acoustic.start_ms = why.record.start_ms;
        why.acoustic.end_ms = why.record.end_ms;
        why.validate_complete()?;
        Ok(why)
    }

    /// Completeness gate for one word: the substrate record validates,
    /// acoustic + LM terms are present and finite, every rendered score
    /// is a finite `S(T)` component, and the per-token total reconciles
    /// with its components. Absent unary classes render as explicit
    /// absent terms (§3 rule 2), not as missing fields.
    pub fn validate_complete(&self) -> Result<(), WhyError> {
        self.record.validate().map_err(WhyError::RecordInvalid)?;
        let a = self
            .acoustic
            .llr
            .ok_or(WhyError::MissingAcoustic(self.token_id))?;
        finite(a, "acoustic.llr")?;
        if self.acoustic.end_frame <= self.acoustic.start_frame {
            return Err(WhyError::Incomplete("acoustic frames empty".into()));
        }
        let s = self
            .lm
            .sequence_llr
            .ok_or(WhyError::MissingSequence(self.token_id))?;
        finite(s, "lm.sequence_llr")?;
        let p = self
            .lm
            .lm_probability
            .ok_or(WhyError::MissingLmProbability(self.token_id))?;
        if !p.is_finite() || !(0.0..=1.0).contains(&p) || p == 0.0 {
            return Err(WhyError::NonFinite("lm.lm_probability"));
        }
        if self.lm.history.is_empty() {
            return Err(WhyError::Incomplete("lm history empty".into()));
        }
        for t in [&self.speaker, &self.lexical, &self.verify, &self.user] {
            finite(t.weighted, "term.weighted")?;
            if let Some(v) = t.llr {
                finite(v, "term.llr")?;
            }
        }
        if self.user.llr.is_some() && self.user_facts.is_empty() {
            return Err(WhyError::Incomplete("user term without user facts".into()));
        }
        if let Some(m) = self.defeat.margin {
            finite(m, "defeat.margin")?;
            if m < 0.0 {
                return Err(WhyError::Incomplete("negative margin".into()));
            }
        }
        let sum = self.acoustic_weighted
            + self.lexical.weighted
            + self.speaker.weighted
            + self.verify.weighted
            + self.user.weighted
            + self.lm.sequence_weighted;
        if (sum - self.tiling_token_total).abs() > 1e-9 {
            return Err(WhyError::Incomplete(format!(
                "tiling token total {sum} != components {}",
                self.tiling_token_total
            )));
        }
        Ok(())
    }

    /// One-screen rendering: why this word and not the alternatives.
    /// Scores are `S(T)` tiling components only; liveness `L` is never
    /// shown.
    pub fn render(&self) -> String {
        let r = &self.record;
        let mut out = format!(
            "WHY {:?} [{}ms-{}ms] frames {}-{} token {}\n",
            r.text,
            r.start_ms,
            r.end_ms,
            self.acoustic.start_frame,
            self.acoustic.end_frame,
            self.token_id
        );
        out.push_str(&format!("LEXICON: source={:?}\n", self.lexicon_source));
        out.push_str(&format!(
            "ACOUSTIC: llr {:?} over frames {}-{} (null: {})\n",
            self.acoustic.llr,
            self.acoustic.start_frame,
            self.acoustic.end_frame,
            self.acoustic.null
        ));
        out.push_str(&format!(
            "LM: P(word|history)={:?} history={:?} sequence_llr={:?} weighted={:.4} (null: {})\n",
            self.lm.lm_probability,
            self.lm.history,
            self.lm.sequence_llr,
            self.lm.sequence_weighted,
            self.lm.null
        ));
        for t in [&self.speaker, &self.lexical, &self.verify, &self.user] {
            match t.llr {
                Some(v) => out.push_str(&format!(
                    "{}: llr={:.4} w={:.4} weighted={:.4} (null: {})\n",
                    t.class.to_uppercase(),
                    v,
                    t.weight,
                    t.weighted,
                    t.null
                )),
                None => out.push_str(&format!(
                    "{}: absent term (no active evidence)\n",
                    t.class.to_uppercase()
                )),
            }
        }
        out.push_str(&format!("USER FACTS: {:?}\n", self.user_facts));
        out.push_str("ALTERNATIVES:\n");
        if self.alternatives.is_empty() {
            out.push_str(&format!("  ({})\n", self.alternatives_note));
        }
        for a in &self.alternatives {
            out.push_str(&format!(
                "  - {:?} token {} tiling-component {:.4} acoustic {:?} seq {:?}\n",
                a.text, a.token_id, a.tiling_component_total, a.acoustic_llr, a.sequence_llr
            ));
        }
        for h in &r.losing {
            out.push_str(&format!(
                "  - record-losing {:?} score {:.2}\n",
                h.text, h.score
            ));
        }
        out.push_str(&format!(
            "DEFEAT: tiling {:?} S(T)={:.4} margin={:?} defeated={:?}\n",
            self.defeat.defeating_tiling,
            self.defeat.defeating_tiling_score,
            self.defeat.margin,
            self.defeat.defeated_tokens
        ));
        out.push_str("REVIVALS:\n");
        if self.revivals.is_empty() {
            out.push_str("  (none)\n");
        }
        for v in &self.revivals {
            out.push_str(&format!(
                "  - via {:?} token {} llr={:.4}\n",
                v.via_text, v.via_token_id, v.llr
            ));
        }
        out.push_str(&format!(
            "TILING SCORE S(T): token={:.4} tiling={:.4} (tiling components only; liveness L is never shown)\n",
            self.tiling_token_total, self.tiling_total
        ));
        out.push_str(&r.render());
        out
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        use serde::ser::Error as _;
        self.validate_complete()
            .map_err(|e| serde_json::Error::custom(format!("{e:?}")))?;
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// JSON export: transcript plus one explanation per committed word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhyExport {
    pub transcript: Transcript,
    pub explanations: Vec<WhyThisWord>,
}

impl WhyExport {
    pub fn new(transcript: Transcript, explanations: Vec<WhyThisWord>) -> Result<Self, WhyError> {
        if transcript.words.len() != explanations.len() {
            return Err(WhyError::Incomplete(format!(
                "transcript has {} words, {} explanations",
                transcript.words.len(),
                explanations.len()
            )));
        }
        let export = Self {
            transcript,
            explanations,
        };
        export.validate_all()?;
        Ok(export)
    }

    /// Gate: every committed word validates and renders complete.
    /// Returns (words checked, words complete).
    pub fn validate_all(&self) -> Result<(usize, usize), WhyError> {
        let mut complete = 0usize;
        for (word, why) in self.transcript.words.iter().zip(&self.explanations) {
            word.validate().map_err(WhyError::RecordInvalid)?;
            if why.record.text != word.text
                || why.record.start_ms != word.start_ms
                || why.record.end_ms != word.end_ms
            {
                return Err(WhyError::Incomplete(format!(
                    "explanation for {:?} does not match transcript word {:?}",
                    why.record.text, word.text
                )));
            }
            why.validate_complete()?;
            complete += 1;
        }
        Ok((self.transcript.words.len(), complete))
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        use serde::ser::Error as _;
        self.validate_all()
            .map_err(|e| serde_json::Error::custom(format!("{e:?}")))?;
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod why_tests {
    use super::*;
    use crate::{
        ChainLayer, ChainLevel, Evidence, EvidenceKind, Hypothesis, HypothesisStatus,
        JustificationRef, Node, ProvenanceChain, SessionMeta, Transcript,
    };
    use fabric::decision::{CommitOutcome, DecisionParams, TokenKind};
    use std::collections::BTreeSet;

    fn np() -> BTreeSet<FactId> {
        BTreeSet::new()
    }

    fn record_for(
        d: &mut DecisionFabric,
        token: FactId,
        text: &str,
        span_ms: (u64, u64),
        losing_text: &str,
    ) -> WordRecord {
        let f1 = d.fabric_mut().intern();
        let f2 = d.fabric_mut().intern();
        d.fabric_mut().assert_assumption(f1);
        d.fabric_mut().assert_assumption(f2);
        let score = d.fabric_mut().intern();
        d.fabric_mut().assert_assumption(score);
        let chain = ProvenanceChain::new(vec![
            ChainLayer {
                level: ChainLevel::Frame,
                nodes: vec![
                    Node {
                        id: f1.0,
                        label: "FRAME:0-10ms".into(),
                        support: vec![],
                        active: true,
                        rating: 90.0,
                    },
                    Node {
                        id: f2.0,
                        label: "FRAME:10-20ms".into(),
                        support: vec![],
                        active: true,
                        rating: 90.0,
                    },
                ],
            },
            ChainLayer {
                level: ChainLevel::Lexical,
                nodes: vec![Node {
                    id: token.0,
                    label: format!("WORD:{text}"),
                    support: vec![JustificationRef {
                        justification_id: format!("j-lex-{text}"),
                        premises: vec![f1.0, f2.0],
                        unconditional: false,
                    }],
                    active: true,
                    rating: 80.0,
                }],
            },
            ChainLayer {
                level: ChainLevel::Sequence,
                nodes: vec![Node {
                    id: score.0,
                    label: format!("SEQ:{text}"),
                    support: vec![JustificationRef {
                        justification_id: format!("j-seq-{text}"),
                        premises: vec![token.0],
                        unconditional: false,
                    }],
                    active: true,
                    rating: 70.0,
                }],
            },
        ]);
        WordRecord::committed_from_live(
            text,
            span_ms,
            0.9,
            Hypothesis::new(text, 0.9, HypothesisStatus::Committed),
            vec![Hypothesis::new(
                losing_text,
                0.4,
                HypothesisStatus::Retracted,
            )],
            Evidence::new(EvidenceKind::Lexical, "tiling margin decided", 0.8),
            chain,
            80.0,
            "regression evidence summary",
        )
        .expect("valid record")
    }

    struct RegressionRun {
        fabric: DecisionFabric,
        winner: Vec<FactId>,
        runner_up: Option<Vec<FactId>>,
        margin: Option<f64>,
        probs: BTreeMap<FactId, f64>,
        records: Vec<WordRecord>,
        correction: FactId,
    }

    fn regression_run() -> RegressionRun {
        let params = DecisionParams {
            theta_live: 6.0,
            delta: 0.5,
            ..DecisionParams::default()
        };
        let mut d = DecisionFabric::new(params);
        let wreath = d.mint_token("wreath", TokenKind::Word, 0, 10).expect("t");
        let reef = d.mint_token("reef", TokenKind::Word, 0, 10).expect("t");
        let now = d.mint_token("now", TokenKind::Word, 10, 20).expect("t");
        d.add_unary(wreath, SourceClass::Acoustic, 3.0, np())
            .expect("e");
        d.add_unary(reef, SourceClass::Acoustic, 2.8, np())
            .expect("e");
        d.add_unary(reef, SourceClass::Lexical, 1.2, np())
            .expect("e");
        d.add_unary(reef, SourceClass::Speaker, 0.6, np())
            .expect("e");
        d.add_unary(now, SourceClass::Acoustic, 2.0, np())
            .expect("e");
        d.add_boundary_sequence(reef, 0.3).expect("e");
        d.add_boundary_sequence(wreath, 0.2).expect("e");
        d.add_sequence(wreath, now, 0.1, np()).expect("e");
        // §7 v1.4: intended tiling pairs ride lattice arcs.
        d.add_lattice_arc(wreath, now).expect("arc");
        d.add_lattice_arc(reef, now).expect("arc");
        // Right context arrives while reef is dormant: revival event.
        d.attach_context_for_new_token(now, &|a, b| {
            if a.label == "reef" && b.label == "now" {
                5.0
            } else {
                0.0
            }
        });
        // Then the USER correction lands on the revived token.
        let correction = d.fabric_mut().intern();
        d.fabric_mut().assert_assumption(correction);
        d.add_user(reef, correction).expect("user");
        let outcome = d.commit_region((0, 20), 40);
        let (winner, runner_up, margin) = match outcome {
            CommitOutcome::Committed {
                winner,
                runner_up,
                margin,
                ..
            } => (winner.tokens, runner_up.map(|r| r.tokens), margin),
            other => panic!("regression region must commit, got {other:?}"),
        };
        assert_eq!(
            winner
                .iter()
                .map(|t| d.token(*t).expect("tok").label.as_str())
                .collect::<Vec<_>>(),
            vec!["reef", "now"],
            "USER + LM context must make reef the winner"
        );
        let mut probs = BTreeMap::new();
        probs.insert(reef, 0.42);
        probs.insert(now, 0.35);
        let r1 = record_for(&mut d, reef, "reef", (0, 100), "wreath");
        let r2 = record_for(&mut d, now, "now", (100, 200), "wreath");
        RegressionRun {
            fabric: d,
            winner,
            runner_up,
            margin,
            probs,
            records: vec![r1, r2],
            correction,
        }
    }

    fn build_all() -> (WhyExport, FactId) {
        let run = regression_run();
        let mut whys = Vec::new();
        for rec in &run.records {
            let token = run.winner[whys.len()];
            whys.push(
                WhyThisWord::build(
                    rec.clone(),
                    &run.fabric,
                    token,
                    &run.winner,
                    run.runner_up.as_deref(),
                    run.margin,
                    &run.probs,
                )
                .expect("complete why"),
            );
        }
        let transcript = Transcript::new(
            run.records.clone(),
            SessionMeta {
                speaker_id: "spk1".into(),
                recording_ref: "regression.wav".into(),
                started_ms: 0,
            },
            Some("A".into()),
        );
        (
            WhyExport::new(transcript, whys).expect("export"),
            run.correction,
        )
    }

    #[test]
    fn regression_every_committed_word_renders_complete_chain() {
        let (export, correction) = build_all();
        let (checked, complete) = export.validate_all().expect("validate");
        assert_eq!(checked, 2);
        assert_eq!(complete, checked, "validator must pass 100%");
        for w in &export.transcript.words {
            w.validate().expect("provenance validator");
        }
        let reef = &export.explanations[0];
        assert_eq!(reef.acoustic.llr, Some(2.8));
        assert_eq!(
            (reef.acoustic.start_frame, reef.acoustic.end_frame),
            (0, 10)
        );
        assert_eq!(reef.lm.history, vec!["<s>".to_string()]);
        assert_eq!(reef.lm.lm_probability, Some(0.42));
        assert_eq!(reef.speaker.llr, Some(0.6));
        assert!(
            reef.user_facts.contains(&correction.0),
            "user facts involved"
        );
        // Unopposed (§10, amendment 1.2): no runner-up tiling, hence no
        // alternatives; the dormant rival renders as defeated, not alternative.
        assert!(reef.alternatives.is_empty());
        assert_eq!(
            reef.alternatives_note,
            "no runner-up tiling: winner was unopposed"
        );
        assert!(reef.defeat.defeated_tokens.contains(&"wreath".to_string()));
        assert!(!reef.revivals.is_empty(), "revival events must render");
        assert_eq!(reef.revivals[0].via_text, "now");
        // Post-§8-amendment-1.1 the winner is unopposed (wreath is dormant,
        // hence not a live tiling), so §10 amendment 1.2 reports no margin:
        // absence, not +inf.
        assert_eq!(reef.defeat.margin, None);
        assert!(reef.defeat.defeating_tiling.contains(&"reef".to_string()));
        let screen = reef.render();
        for section in [
            "ACOUSTIC:",
            "LM:",
            "SPEAKER:",
            "ALTERNATIVES:",
            "DEFEAT:",
            "REVIVALS:",
            "USER FACTS:",
            "TILING SCORE",
        ] {
            assert!(
                screen.contains(section),
                "render missing {section}:\n{screen}"
            );
        }
        assert!(screen.contains("liveness L is never shown"));
        assert!(
            !screen.contains("L(h) ="),
            "liveness value must never render"
        );
        let now_why = &export.explanations[1];
        assert_eq!(
            now_why.lm.history,
            vec!["<s>".to_string(), "reef".to_string()]
        );
        assert_eq!(now_why.lm.predecessor.as_deref(), Some("reef"));
    }

    #[test]
    fn regression_tiling_breakdown_matches_tiling_score() {
        let run = regression_run();
        let (_, _, total) = run.fabric.tiling_breakdown(&run.winner);
        assert!((total - run.fabric.tiling_score(&run.winner)).abs() < 1e-9);
        let why = WhyThisWord::build(
            run.records[0].clone(),
            &run.fabric,
            run.winner[0],
            &run.winner,
            run.runner_up.as_deref(),
            run.margin,
            &run.probs,
        )
        .expect("why");
        assert!((why.tiling_total - run.fabric.tiling_score(&run.winner)).abs() < 1e-9);
    }

    #[test]
    fn lexicon_source_renders_and_survives_json_round_trip() {
        let run = regression_run();
        let mut why = WhyThisWord::build(
            run.records[0].clone(),
            &run.fabric,
            run.winner[0],
            &run.winner,
            run.runner_up.as_deref(),
            run.margin,
            &run.probs,
        )
        .expect("why");
        // The decision-harness tokens record no lexicon tag.
        assert_eq!(why.lexicon_source, None);
        assert!(
            why.render().contains("LEXICON: source=None"),
            "render must show the (unrecorded) lexicon tag"
        );
        // A recorded tag renders and survives JSON.
        why.lexicon_source = Some("librispeech-g2p".to_string());
        assert!(
            why.render()
                .contains("LEXICON: source=Some(\"librispeech-g2p\")"),
            "render must show the recorded lexicon tag"
        );
        let back = WhyThisWord::from_json(&why.to_json().expect("json")).expect("parse");
        assert_eq!(back.lexicon_source.as_deref(), Some("librispeech-g2p"));
        // Additive field: exports written before it still parse to None.
        let mut v: serde_json::Value =
            serde_json::from_str(&why.to_json().expect("json")).expect("parse");
        v.as_object_mut().expect("object").remove("lexicon_source");
        let legacy =
            WhyThisWord::from_json(&serde_json::to_string(&v).expect("json")).expect("parse");
        assert_eq!(legacy.lexicon_source, None);
    }

    #[test]
    fn json_export_shape_with_transcript() {
        let (export, _) = build_all();
        let json = export.to_json().expect("json");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(v["transcript"]["words"].as_array().expect("words").len(), 2);
        assert_eq!(v["transcript"]["words"][0]["text"], "reef");
        let e = &v["explanations"][0];
        for key in [
            "record",
            "token_id",
            "acoustic",
            "lm",
            "speaker",
            "lexical",
            "verify",
            "user",
            "user_facts",
            "alternatives",
            "defeat",
            "revivals",
            "tiling_token_total",
            "tiling_total",
            "acoustic_weighted",
            "lexicon_source",
        ] {
            assert!(e.get(key).is_some(), "export missing key {key}");
        }
        assert_eq!(e["acoustic"]["llr"], 2.8);
        assert_eq!(e["acoustic"]["start_frame"], 0);
        assert_eq!(e["lm"]["history"][0], "<s>");
        assert_eq!(e["lm"]["lm_probability"], 0.42);
        // Unopposed commit (§10, amendment 1.2): margin is absent (null).
        assert!(e["defeat"]["margin"].is_null());
        assert!(
            e["defeat"]["defeating_tiling"]
                .as_array()
                .expect("tiling")
                .len()
                == 2
        );
        let back = WhyExport::from_json(&json).expect("round trip");
        assert_eq!(back.validate_all().expect("revalidate"), (2, 2));
    }

    #[test]
    fn incomplete_chains_are_rejected() {
        let run = regression_run();
        let empty: BTreeMap<FactId, f64> = BTreeMap::new();
        assert!(matches!(
            WhyThisWord::build(
                run.records[0].clone(),
                &run.fabric,
                run.winner[0],
                &run.winner,
                run.runner_up.as_deref(),
                run.margin,
                &empty
            ),
            Err(WhyError::MissingLmProbability(_))
        ));
        let ghost = FactId(999_999);
        assert!(matches!(
            WhyThisWord::build(
                run.records[0].clone(),
                &run.fabric,
                ghost,
                &[ghost],
                None,
                None,
                &empty
            ),
            Err(WhyError::UnknownToken(_))
        ));
    }
}
