//! Bridge contract tests: synthetic lattices plus the 5 dev5 lattices.
//!
//! `data/lattices/dev5/*.lat.jsonl` comes from the decoder-kbest-word-lattice
//! bead and may not exist yet; the lattice-file tests below decode the real 5
//! when the directory is present (and assert there are exactly 5), else they
//! decode the lattice crate's D0 fixture plus 4 synthetic lattices and note
//! the fallback on stderr. Either way every decoded utterance must yield a
//! non-empty transcript and a non-empty v1 event log. The LM resolves the
//! same way: `data/lm-text/r1v2-full/lm-r1v2-interpolated.json` when present,
//! else a tiny trained interpolated model.

use fabric::decision::DecisionParams;
use hearsay::{BridgeOutcome, InterpolatedScorer, SequenceScorer, decode_lattice};
use lattice::{Lattice, read_lattice};
use lm::interpolate::InterpolatedLm;
use lm::kneser_ney::KnTrigram;
use lm::persist::load_interpolated;
use std::collections::HashMap;
use std::path::PathBuf;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_dir() -> PathBuf {
    crate_dir().join("../..")
}

/// Deterministic stub scorer: fixed boundary LLR, per-pair table, default.
struct MapScorer {
    boundary: f64,
    pairs: HashMap<(String, String), f64>,
    default: f64,
}

impl SequenceScorer for MapScorer {
    fn sequence_llr(&self, prev: Option<&str>, word: &str) -> f64 {
        match prev {
            None => self.boundary,
            Some(p) => self
                .pairs
                .get(&(p.to_string(), word.to_string()))
                .copied()
                .unwrap_or(self.default),
        }
    }
}

fn map_scorer(boundary: f64, pairs: &[(&str, &str, f64)], default: f64) -> MapScorer {
    MapScorer {
        boundary,
        pairs: pairs
            .iter()
            .map(|(a, b, v)| ((a.to_string(), b.to_string()), *v))
            .collect(),
        default,
    }
}

/// Build lattice JSONL in the on-disk shape, then read it through the real
/// `read_lattice` so tests exercise the full parse path.
fn lat_text(utterance: &str, toks: &[(u64, &str, u32, u32, f64)], arcs: &[(u64, u64)]) -> String {
    let mut s = format!(
        "{{\"format\":\"hearsay-word-lattice\",\"version_major\":1,\"version_minor\":0,\
         \"utterance\":\"{utterance}\",\"frame_shift_ms\":10.0,\"producer\":\"p0tb-test\"}}\n"
    );
    for (id, w, st, en, a) in toks {
        s.push_str(&format!(
            "{{\"t\":{{\"id\":{id},\"word\":\"{w}\",\"start\":{st},\"end\":{en},\
             \"source\":\"t\",\"acoustic_llr\":{a}}}}}\n"
        ));
    }
    for (f, t) in arcs {
        s.push_str(&format!("{{\"a\":{{\"from\":{f},\"to\":{t}}}}}\n"));
    }
    s
}

fn read(text: &str) -> Lattice {
    read_lattice(text).expect("test lattice reads")
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn synthetic_two_word_chain_commits_exact_score() {
    // §4 exactly-once: S(T) = acoustic sum + one boundary pair + one arc
    // pair = 3.0 + 4.0 + 0.5 + 1.5 = 9.0.
    let lat = read(&lat_text(
        "two-word",
        &[(1, "hello", 0, 30, 3.0), (2, "world", 30, 60, 4.0)],
        &[(1, 2)],
    ));
    let scorer = map_scorer(0.5, &[("hello", "world", 1.5)], 0.0);
    let out = decode_lattice(&lat, &scorer, DecisionParams::default()).expect("decodes");
    assert_eq!(
        out.transcript,
        vec!["hello".to_string(), "world".to_string()]
    );
    assert_eq!(out.commits.len(), 1);
    assert!(approx(out.commits[0].winner_score, 9.0));
    assert_eq!(out.commits[0].margin, None);
    assert!(out.commits[0].defeated_words.is_empty());
    assert!(out.commits[0].flushed);
    assert!(out.gaps.is_empty());
    assert!(out.uncommitted_ids.is_empty());
    let lines: Vec<&str> = out.events_jsonl.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"event\":\"committed\""));
}

#[test]
fn lm_sequence_flips_acoustic_winner_with_margin() {
    // Acoustics prefer alpha (5.0 vs 2.0); the SEQUENCE pair terms prefer
    // the beta path by more. S(alpha,gamma) = 5+1+0-1 = 5,
    // S(beta,gamma) = 2+1+0+5 = 8, margin 3. Both paths stay live so the
    // margin is numeric (not the unopposed None).
    let lat = read(&lat_text(
        "flip",
        &[
            (1, "alpha", 0, 30, 5.0),
            (2, "beta", 0, 30, 2.0),
            (3, "gamma", 30, 60, 1.0),
        ],
        &[(1, 3), (2, 3)],
    ));
    let scorer = map_scorer(
        0.0,
        &[("alpha", "gamma", -1.0), ("beta", "gamma", 5.0)],
        0.0,
    );
    let out = decode_lattice(&lat, &scorer, DecisionParams::default()).expect("decodes");
    assert_eq!(
        out.transcript,
        vec!["beta".to_string(), "gamma".to_string()]
    );
    assert_eq!(out.commits.len(), 1);
    let m = out.commits[0].margin.expect("numeric margin");
    assert!(approx(m, 3.0));
    assert_eq!(out.commits[0].defeated_words, vec!["alpha".to_string()]);
    assert!(out.events_jsonl.contains("\"event\":\"defeated\""));
    assert!(out.events_jsonl.contains("\"event\":\"committed\""));
}

#[test]
fn dormant_predecessor_revives_via_successor() {
    // echo's acoustics (-5.0) leave it dormant until foxtrot's arrival
    // attaches the +10 pair (§9): L(echo) = -5 + 10 = 5 >= theta_live.
    let lat = read(&lat_text(
        "revive",
        &[(1, "echo", 0, 30, -5.0), (2, "foxtrot", 30, 60, 1.0)],
        &[(1, 2)],
    ));
    let scorer = map_scorer(0.0, &[("echo", "foxtrot", 10.0)], 0.0);
    let out = decode_lattice(&lat, &scorer, DecisionParams::default()).expect("decodes");
    assert_eq!(
        out.transcript,
        vec!["echo".to_string(), "foxtrot".to_string()]
    );
    let revived: Vec<&str> = out
        .events_jsonl
        .lines()
        .filter(|l| l.contains("\"event\":\"revived\""))
        .collect();
    assert_eq!(revived.len(), 1);
    assert!(revived[0].contains("\"word\":\"echo\""));
    assert!(revived[0].contains("\"via\""));
}

#[test]
fn arm_b_prunes_below_threshold_and_never_revives() {
    // Same inputs as the arm-C revival test, but under Arm::B echo is
    // beam-pruned at its own intake (before foxtrot's +10 can save it):
    // no revived event, a pruned event instead, and foxtrot commits alone
    // past the recorded gap.
    use hearsay::Arm;
    let lat = read(&lat_text(
        "prune",
        &[(1, "echo", 0, 30, -5.0), (2, "foxtrot", 30, 60, 1.0)],
        &[(1, 2)],
    ));
    let scorer = map_scorer(0.0, &[("echo", "foxtrot", 10.0)], 0.0);
    let params = DecisionParams {
        arm: Arm::B,
        ..Default::default()
    };
    let out = decode_lattice(&lat, &scorer, params).expect("decodes");
    assert_eq!(out.transcript, vec!["foxtrot".to_string()]);
    assert_eq!(out.gaps, vec![(0, 30)]);
    assert_eq!(out.uncommitted_ids, vec![1]);
    assert!(!out.events_jsonl.contains("\"event\":\"revived\""));
    let pruned: Vec<&str> = out
        .events_jsonl
        .lines()
        .filter(|l| l.contains("\"event\":\"pruned\""))
        .collect();
    assert_eq!(pruned.len(), 1);
    assert!(pruned[0].contains("\"word\":\"echo\""));
}

#[test]
fn frame_gap_is_recorded_and_transcript_concatenates() {
    // No token covers [30,90): the first region commits at the stream
    // frontier (utterance end 120 >= 30 + H), the gap records, the second
    // region flushes.
    let lat = read(&lat_text(
        "gap",
        &[(1, "one", 0, 30, 2.0), (2, "two", 90, 120, 2.0)],
        &[],
    ));
    let scorer = map_scorer(0.0, &[], 0.0);
    let out = decode_lattice(&lat, &scorer, DecisionParams::default()).expect("decodes");
    assert_eq!(out.transcript, vec!["one".to_string(), "two".to_string()]);
    assert_eq!(out.commits.len(), 2);
    assert!(!out.commits[0].flushed);
    assert!(out.commits[1].flushed);
    assert_eq!(out.gaps, vec![(30, 90)]);
    assert!(out.uncommitted_ids.is_empty());
}

#[test]
fn d0_fixture_decodes_with_defeat() {
    let path = crate_dir().join("../lattice/tests/fixtures/d0_words.lat.jsonl");
    let text = std::fs::read_to_string(&path).expect("D0 fixture present");
    let lat = read_lattice(&text).expect("D0 fixture reads");
    let scorer = map_scorer(0.0, &[], 0.0);
    let out = decode_lattice(&lat, &scorer, DecisionParams::default()).expect("decodes");
    assert_eq!(
        out.transcript,
        vec![
            "impeachment",
            "survival",
            "process",
            "intense",
            "reflection",
            "kinship",
            "resented"
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );
    assert_eq!(out.commits.len(), 1);
    assert_eq!(out.commits[0].margin, None);
    assert_eq!(
        out.commits[0].defeated_words,
        vec!["methodical".to_string()]
    );
    assert!(out.gaps.is_empty());
    assert!(out.uncommitted_ids.is_empty());
    assert!(!out.events_jsonl.is_empty());
}

#[test]
fn empty_lattice_fails_loudly() {
    let lat = Lattice {
        utterance: "empty".to_string(),
        frame_shift_ms: 10.0,
        producer: "p0tb-test".to_string(),
        tokens: vec![],
        arcs: vec![],
    };
    let scorer = map_scorer(0.0, &[], 0.0);
    let err = decode_lattice(&lat, &scorer, DecisionParams::default()).expect_err("must fail");
    assert!(err.contains("no tokens"));
}
#[test]
fn finite_budgets_bind_peaks_and_evict_in_both_arms() {
    // Six live tokens against tight caps: peaks stay within budget and
    // evictions are logged. Arm C keeps a dormant pool; arm B holds no
    // dormant tokens at all (a4 R3).
    use hearsay::Arm;
    let toks = [
        (1, "alpha", 0, 10, 2.0),
        (2, "bravo", 10, 20, 2.0),
        (3, "charlie", 20, 30, 2.0),
        (4, "delta", 30, 40, 2.0),
        (5, "echo", 40, 50, 2.0),
        (6, "foxtrot", 50, 60, 2.0),
    ];
    let arcs = [(1, 2), (2, 3), (3, 4), (4, 5), (5, 6)];
    let scorer = map_scorer(0.0, &[], 0.0);
    let lat_c = read(&lat_text("bind-c", &toks, &arcs));
    let out_c = decode_lattice(
        &lat_c,
        &scorer,
        DecisionParams {
            arm: Arm::C,
            live_budget: 2,
            dormant_budget: 2,
            combined_budget: Some(3),
            ..Default::default()
        },
    )
    .expect("decodes");
    assert!(out_c.peak_live <= 2, "peak_live {}", out_c.peak_live);
    assert!(
        out_c.peak_dormant <= 2,
        "peak_dormant {}",
        out_c.peak_dormant
    );
    assert!(out_c.peak_total <= 3, "peak_total {}", out_c.peak_total);
    let evicted_c = out_c
        .events_jsonl
        .lines()
        .filter(|l| l.contains("\"event\":\"evicted\""))
        .count();
    assert!(evicted_c >= 3, "evicted {evicted_c}");
    let lat_b = read(&lat_text("bind-b", &toks, &arcs));
    let out_b = decode_lattice(
        &lat_b,
        &scorer,
        DecisionParams {
            arm: Arm::B,
            live_budget: 2,
            dormant_budget: 0,
            combined_budget: None,
            ..Default::default()
        },
    )
    .expect("decodes");
    assert!(out_b.peak_live <= 2, "peak_live {}", out_b.peak_live);
    assert_eq!(out_b.peak_dormant, 0);
    assert!(!out_b.events_jsonl.contains("\"event\":\"revived\""));
}
#[test]
fn dangling_arc_fails_loudly() {
    // read_lattice already rejects dangling arcs, so build the struct
    // directly to exercise the bridge's own guard.
    let lat = Lattice {
        utterance: "dangle".to_string(),
        frame_shift_ms: 10.0,
        producer: "p0tb-test".to_string(),
        tokens: vec![lattice::Token {
            id: 1,
            word: "solo".to_string(),
            start: 0,
            end: 30,
            source: "t".to_string(),
            acoustic_llr: 1.0,
            lexical_llr: None,
            speaker_llr: None,
            verify_llr: None,
            user_llr: None,
        }],
        arcs: vec![lattice::Arc { from: 1, to: 99 }],
    };
    let scorer = map_scorer(0.0, &[], 0.0);
    let err = decode_lattice(&lat, &scorer, DecisionParams::default()).expect_err("must fail");
    assert!(err.contains("dangles"));
}

#[test]
fn interpolated_scorer_is_bigram_over_unigram() {
    let general = KnTrigram::train(&[
        vec!["hello".to_string(), "world".to_string()],
        vec!["hello".to_string(), "there".to_string()],
    ]);
    let personal = KnTrigram::train(&[vec!["hello".to_string(), "world".to_string()]]);
    let lm = InterpolatedLm::new(general, personal, 0.25).expect("weight in range");
    let s = InterpolatedScorer::new(&lm);
    let mix = |g: f64, p: f64| (0.75 * g + 0.25 * p).clamp(0.0, 1.0).max(1e-12);
    let c = mix(
        lm.general().bigram_prob("hello", "world"),
        lm.personal().bigram_prob("hello", "world"),
    );
    let u = mix(
        lm.general().unigram_prob("world"),
        lm.personal().unigram_prob("world"),
    );
    assert!(approx(s.sequence_llr(Some("hello"), "world"), (c / u).ln()));
    // Boundary history is <s>, matching both the contract (§4) and the LM's
    // own BOS_TOKEN.
    let c0 = mix(
        lm.general().bigram_prob("<s>", "hello"),
        lm.personal().bigram_prob("<s>", "hello"),
    );
    let u0 = mix(
        lm.general().unigram_prob("hello"),
        lm.personal().unigram_prob("hello"),
    );
    assert!(approx(s.sequence_llr(None, "hello"), (c0 / u0).ln()));
    assert!(s.sequence_llr(Some("zzz-unknown"), "world").is_finite());
}

/// Sorted `*.lat.jsonl` paths under the dev5 directory, or `None` when the
/// decoder bead has not landed its lattices yet.
fn dev5_paths() -> Option<Vec<PathBuf>> {
    let dir = workspace_dir().join("data/lattices/dev5");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    paths.sort();
    Some(paths)
}

fn tiny_interpolated() -> InterpolatedLm {
    let general = KnTrigram::train(&[
        vec!["the".to_string(), "cat".to_string()],
        vec!["a".to_string(), "dog".to_string()],
    ]);
    let personal = KnTrigram::train(&[vec!["the".to_string(), "cat".to_string()]]);
    InterpolatedLm::new(general, personal, 0.5).expect("weight in range")
}

fn resolve_lm() -> InterpolatedLm {
    let path = workspace_dir().join("data/lm-text/r1v2-full/lm-r1v2-interpolated.json");
    match std::fs::metadata(&path) {
        Ok(_) => load_interpolated(&path).expect("shipped LM loads"),
        Err(_) => {
            eprintln!("shipped LM absent; using tiny trained model");
            tiny_interpolated()
        }
    }
}

/// Four synthetic lattices in varied shapes, standing in for dev5 until the
/// decoder bead lands. Together with the D0 fixture they form the same
/// 5-lattice gate population.
fn synthetic_five() -> Vec<(String, Lattice)> {
    let d0 =
        std::fs::read_to_string(crate_dir().join("../lattice/tests/fixtures/d0_words.lat.jsonl"))
            .expect("D0 fixture present");
    vec![
        (
            "d0-fixture-001".to_string(),
            read_lattice(&d0).expect("D0 reads"),
        ),
        (
            "synth-chain".to_string(),
            read(&lat_text(
                "synth-chain",
                &[(1, "red", 0, 20, 2.0), (2, "blue", 20, 45, 2.5)],
                &[(1, 2)],
            )),
        ),
        (
            "synth-rival".to_string(),
            read(&lat_text(
                "synth-rival",
                &[
                    (1, "fast", 0, 25, 3.0),
                    (2, "vast", 0, 25, 2.0),
                    (3, "lane", 25, 50, 3.0),
                ],
                &[(1, 3), (2, 3)],
            )),
        ),
        (
            "synth-single".to_string(),
            read(&lat_text("synth-single", &[(1, "alone", 5, 40, 1.5)], &[])),
        ),
        (
            "synth-gap".to_string(),
            read(&lat_text(
                "synth-gap",
                &[(1, "first", 0, 20, 2.0), (2, "second", 60, 90, 2.0)],
                &[],
            )),
        ),
    ]
}

/// The 5-lattice gate population: real dev5 when present (asserted to be
/// exactly 5), else D0 + 4 synthetics.
fn gate_population() -> Vec<(String, Lattice)> {
    match dev5_paths() {
        Some(paths) => {
            assert_eq!(paths.len(), 5, "dev5 holds exactly 5 lattices");
            paths
                .iter()
                .map(|p| {
                    let text = std::fs::read_to_string(p).expect("dev5 lattice reads");
                    let lat = read_lattice(&text).expect("dev5 lattice parses");
                    (lat.utterance.clone(), lat)
                })
                .collect()
        }
        None => {
            eprintln!("data/lattices/dev5 absent: gating on D0 + 4 synthetic lattices");
            synthetic_five()
        }
    }
}

fn decode_all(pop: &[(String, Lattice)], lm: &InterpolatedLm) -> Vec<BridgeOutcome> {
    let scorer = InterpolatedScorer::new(lm);
    pop.iter()
        .map(|(name, lat)| {
            decode_lattice(lat, &scorer, DecisionParams::default())
                .unwrap_or_else(|e| panic!("lattice {name} decodes: {e}"))
        })
        .collect()
}

#[test]
fn fabric_transcript_on_5_dev5() {
    let pop = gate_population();
    assert_eq!(pop.len(), 5);
    let lm = resolve_lm();
    for out in decode_all(&pop, &lm) {
        assert!(
            !out.transcript.is_empty(),
            "utterance {} has a transcript",
            out.utterance
        );
        assert!(
            !out.commits.is_empty(),
            "utterance {} committed at least one region",
            out.utterance
        );
    }
}

#[test]
fn non_empty_event_log() {
    let pop = gate_population();
    let lm = resolve_lm();
    for out in decode_all(&pop, &lm) {
        let lines: Vec<&str> = out.events_jsonl.lines().collect();
        assert!(
            !lines.is_empty(),
            "utterance {} logs at least one event",
            out.utterance
        );
        let mut committed = 0;
        for (seq, line) in lines.iter().enumerate() {
            let v: serde_json::Value =
                serde_json::from_str(line).expect("event line parses as JSON");
            assert_eq!(v["v"], 1);
            assert_eq!(v["utterance"], out.utterance);
            assert_eq!(v["seq"], seq);
            let kind = v["event"].as_str().expect("event kind is a string");
            assert!(["pruned", "revived", "defeated", "evicted", "committed"].contains(&kind));
            if kind == "committed" {
                committed += 1;
            }
        }
        assert_eq!(committed, out.commits.len());
    }
}

#[test]
fn render_unknown_fact_falls_back_to_fact_id() {
    use fabric::FactId;
    use fabric::decision::DecisionEvent;
    use hearsay::render_events_jsonl;
    use std::collections::BTreeMap;
    let refs = BTreeMap::new();
    let jsonl = render_events_jsonl(
        "u",
        &[DecisionEvent::Revived {
            token: FactId(7),
            via: FactId(9),
            llr: 1.0,
        }],
        &refs,
    );
    assert!(jsonl.contains("\"fact\":7"));
    assert!(jsonl.contains("\"fact\":9"));
}
