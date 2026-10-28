//! Correction facts + exact retraction gate (bead
//! `hearsay-four-f56--correction-retraction`).
//!
//! Property: apply then undo N random corrections -> ledger model state
//! byte-identical to before; every item provenanced to its correction event.
//! Consistency: ledger LM increments equal `KnTrigram` count diffs (R1b) and
//! ledger MAP totals equal `AdaptOverlay` totals (R4).

use fabric::correction::{
    CorrectionError, CorrectionInput, CorrectionLedger, MapContribution, MapKey, MapStats,
};
use fabric::decision::{DecisionFabric, DecisionParams, SourceClass, TokenKind};
use fabric::{Fabric, FactId, TokenFact};
use hmm::HmmSet;
use hmm::adapt::{AdaptFact, AdaptKey, AdaptOverlay, AdaptStage, AdaptStats};
use lm::KnTrigram;
use std::collections::{BTreeMap, BTreeSet};

/// Deterministic xorshift64* (fixed seeds: every property run reproduces).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0);
        self.next() % n
    }

    fn unit(&mut self) -> f64 {
        const SCALE: f64 = (1u64 << 53) as f64;
        ((self.next() >> 11) as f64) / SCALE
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            let pick = self.below(i as u64 + 1) as usize;
            values.swap(i, pick);
        }
    }
}

const WORDS: [&str; 12] = [
    "the", "reef", "read", "wreck", "red", "quick", "brown", "fox", "audio", "ocean", "radio",
    "north",
];
const PHONES: [&str; 10] = ["AA", "B", "D", "EH", "F", "IY", "K", "L", "OW", "R"];

fn pick_word(rng: &mut Rng) -> String {
    WORDS[rng.below(WORDS.len() as u64) as usize].to_string()
}

fn pick_phone(rng: &mut Rng) -> String {
    PHONES[rng.below(PHONES.len() as u64) as usize].to_string()
}

fn random_sentence(rng: &mut Rng, word: &str) -> Vec<String> {
    let len = 1 + rng.below(5) as usize;
    let mut out: Vec<String> = (0..len).map(|_| pick_word(rng)).collect();
    let at = rng.below(out.len() as u64 + 1) as usize;
    out.insert(at, word.to_string());
    out
}

fn random_phones(rng: &mut Rng) -> Vec<String> {
    let len = 1 + rng.below(4) as usize;
    (0..len).map(|_| pick_phone(rng)).collect()
}

fn random_map(rng: &mut Rng, dim: usize) -> Vec<MapContribution> {
    let count = rng.below(4) as usize;
    (0..count)
        .map(|_| {
            let key = MapKey {
                phone: pick_phone(rng),
                state: rng.below(3) as usize,
                comp: rng.below(2) as usize,
            };
            let stats = MapStats {
                n: 0.25 + 2.75 * rng.unit(),
                sum: (0..dim).map(|_| (rng.unit() - 0.5) * 4.0).collect(),
                sumsq: (0..dim).map(|_| rng.unit() * 8.0).collect(),
                frames: 1 + rng.below(30),
            };
            MapContribution { key, stats }
        })
        .collect()
}

fn random_correction(rng: &mut Rng, dim: usize, seq: u64) -> CorrectionInput {
    let word = pick_word(rng);
    let observed = random_phones(rng);
    // Lexicon variants: 0..=2 random ones, plus the observed sequence itself
    // half the time (the no-variant-evidence case).
    let mut variants: Vec<Vec<String>> = (0..rng.below(3)).map(|_| random_phones(rng)).collect();
    if rng.below(2) == 0 {
        variants.push(observed.clone());
    }
    let start = seq * 7 + rng.below(5);
    CorrectionInput {
        corrected_word: word.clone(),
        corrected_sentence: random_sentence(rng, &word),
        start_frame: start,
        end_frame: start + 1 + rng.below(20),
        observed_phones: observed,
        lexicon_variants: variants,
        map: random_map(rng, dim),
    }
}

fn test_input() -> CorrectionInput {
    CorrectionInput {
        corrected_word: "reef".to_string(),
        corrected_sentence: vec!["the".to_string(), "reef".to_string()],
        start_frame: 5,
        end_frame: 15,
        observed_phones: vec!["R".to_string(), "IY".to_string(), "F".to_string()],
        lexicon_variants: vec![vec!["R".to_string(), "IY".to_string(), "F".to_string()]],
        map: Vec::new(),
    }
}

/// Every live item is provenanced to its correction event.
fn assert_item_provenance(fabric: &Fabric, ledger: &CorrectionLedger, event: FactId) {
    let record = ledger.get(event).expect("live correction record");
    assert_eq!(record.event, event);
    assert!(fabric.is_active(event), "event is an active assumption");
    // Item 1: token occurrence justified by the event.
    match fabric.token_fact(record.token) {
        Some(TokenFact::Word(word)) => {
            assert_eq!(word.word, record.word);
            assert_eq!(word.start_frame, record.start_frame);
            assert_eq!(word.end_frame, record.end_frame);
            assert_eq!(word.source, "user-correction");
        }
        other => panic!("corrected token payload: {other:?}"),
    }
    assert!(
        fabric
            .justifications(record.token)
            .iter()
            .any(|j| j.premises.contains(&event)),
        "token justified by its event"
    );
    assert!(fabric.is_active(record.token));
    // Item 3: variant evidence justified by the event.
    if let Some(variant) = record.variant {
        let content = fabric.type_content(variant).expect("variant type content");
        assert_eq!(
            content,
            format!(
                "PRON_VARIANT:{}:{}",
                record.word,
                record.observed_phones.join(" ")
            )
        );
        assert!(
            fabric
                .justifications(variant)
                .iter()
                .any(|j| j.premises.contains(&event)),
            "variant justified by its event"
        );
        assert!(fabric.is_active(variant));
    }
    // Items 2+4: filed under the event in the live ledger.
    assert!(ledger.live_events().contains(&event));
}

#[test]
fn apply_undo_random_corrections_is_byte_identical() {
    const DIM: usize = 4;
    let mut rng = Rng(0xC0FF_EE12_3456_7890);
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(DIM);

    // Non-empty "before" state: 25 baseline corrections stay live throughout.
    let mut baseline = Vec::new();
    for seq in 0..25u64 {
        let input = random_correction(&mut rng, DIM, seq);
        baseline.push(
            ledger
                .apply(&mut fabric, &input)
                .expect("baseline apply")
                .event,
        );
    }
    assert_eq!(ledger.len(), 25);
    let bytes_before = ledger.state_bytes();
    let lm_before = ledger.lm_totals();
    let map_before = ledger.map_totals();

    // N random corrections.
    let mut events = Vec::new();
    for seq in 0..200u64 {
        let input = random_correction(&mut rng, DIM, 1000 + seq);
        events.push(ledger.apply(&mut fabric, &input).expect("apply").event);
    }
    assert_eq!(ledger.len(), 225);

    // Every live item carries provenance to its event.
    for event in ledger.live_events() {
        assert_item_provenance(&fabric, &ledger, event);
    }

    // Undo in shuffled order; tokens had only USER support, so they go inactive.
    rng.shuffle(&mut events);
    for event in &events {
        let record = ledger.undo(&mut fabric, *event).expect("undo");
        assert!(!fabric.is_active(*event), "event retracted");
        assert!(!fabric.is_active(record.token), "USER-only token withdrawn");
    }

    // Model state byte-identical to before; baseline still live and provenanced.
    assert_eq!(ledger.state_bytes(), bytes_before);
    assert_eq!(ledger.lm_totals(), lm_before);
    assert_eq!(ledger.map_totals(), map_before);
    assert_eq!(ledger.live_events(), baseline);
    for event in &baseline {
        assert_item_provenance(&fabric, &ledger, *event);
    }

    // Second cycle guards against first-cycle-only passes.
    let mut again = Vec::new();
    for seq in 0..50u64 {
        let input = random_correction(&mut rng, DIM, 5000 + seq);
        again.push(ledger.apply(&mut fabric, &input).expect("apply").event);
    }
    rng.shuffle(&mut again);
    for event in &again {
        ledger.undo(&mut fabric, *event).expect("undo");
    }
    assert_eq!(ledger.state_bytes(), bytes_before);
    assert_eq!(ledger.lm_totals(), lm_before);
    assert_eq!(ledger.map_totals(), map_before);
}

#[test]
fn variant_evidence_iff_acoustics_disagree_with_all_lexicon_prons() {
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(2);
    // Observed matches a lexicon pronunciation: no new-variant evidence.
    let applied = ledger.apply(&mut fabric, &test_input()).expect("apply");
    assert_eq!(applied.variant, None);
    // Observed matches none: variant evidence with event provenance.
    let mut disagree = test_input();
    disagree.observed_phones = vec!["R".to_string(), "IY".to_string(), "V".to_string()];
    let applied = ledger.apply(&mut fabric, &disagree).expect("apply");
    let variant = applied.variant.expect("variant evidence");
    assert_eq!(
        fabric.type_content(variant).unwrap(),
        "PRON_VARIANT:reef:R IY V"
    );
    assert_eq!(fabric.support_count(variant), 1);
    ledger.undo(&mut fabric, applied.event).expect("undo");
    assert!(!fabric.is_active(variant));
    // Provenance preserved: the justification set survives retraction.
    assert_eq!(fabric.justifications(variant).len(), 1);
}

#[test]
fn shared_variant_survives_single_undo() {
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(2);
    let mut first = test_input();
    first.start_frame = 0;
    first.end_frame = 10;
    first.observed_phones = vec!["R".to_string(), "IY".to_string(), "V".to_string()];
    let mut second = first.clone();
    second.start_frame = 20;
    second.end_frame = 30;
    let a = ledger.apply(&mut fabric, &first).expect("apply");
    let b = ledger.apply(&mut fabric, &second).expect("apply");
    // Hash-consed type fact: one variant, two independent supports.
    assert_eq!(a.variant, b.variant);
    let variant = a.variant.expect("variant");
    assert_ne!(a.token, b.token, "occurrences never merge");
    assert_eq!(fabric.support_count(variant), 2);
    ledger.undo(&mut fabric, a.event).expect("undo");
    assert!(
        fabric.is_active(variant),
        "survives on the second correction"
    );
    assert_eq!(fabric.support_count(variant), 1);
    ledger.undo(&mut fabric, b.event).expect("undo");
    assert!(!fabric.is_active(variant));
}

#[test]
fn token_survives_undo_on_acoustic_support() {
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(2);
    let acoustic = fabric.intern();
    fabric.assert_assumption(acoustic);
    let applied = ledger.apply(&mut fabric, &test_input()).expect("apply");
    let mut premises = BTreeSet::new();
    premises.insert(acoustic);
    fabric.assert(applied.token, premises, false);
    assert_eq!(fabric.support_count(applied.token), 2);
    ledger.undo(&mut fabric, applied.event).expect("undo");
    assert!(
        fabric.is_active(applied.token),
        "acoustic support keeps the token"
    );
    assert_eq!(fabric.support_count(applied.token), 1);
    // The USER justification is still listed (provenance) but no longer active.
    assert_eq!(fabric.justifications(applied.token).len(), 2);
}

fn count_diff(
    after: &BTreeMap<String, u64>,
    before: &BTreeMap<String, u64>,
) -> BTreeMap<String, u64> {
    let mut diff = BTreeMap::new();
    for (key, count) in after {
        let grown = count - before.get(key).copied().unwrap_or(0);
        if grown > 0 {
            diff.insert(key.clone(), grown);
        }
    }
    diff
}

#[test]
fn lm_increments_match_kn_trigram_count_diffs() {
    let base_sentences = vec![
        vec!["the".to_string(), "quick".to_string()],
        vec!["the".to_string(), "reef".to_string()],
        vec!["read".to_string(), "the".to_string(), "radio".to_string()],
    ];
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(2);
    let mut rng = Rng(0x5EED_1234_5678_9ABC);
    let mut correction_sentences = Vec::new();
    for seq in 0..10u64 {
        let input = random_correction(&mut rng, 2, seq);
        let applied = ledger.apply(&mut fabric, &input).expect("apply");
        correction_sentences.push(applied.sentence.clone());
    }
    let base = KnTrigram::train(&base_sentences);
    let mut extended_input = base_sentences.clone();
    extended_input.extend(correction_sentences.iter().cloned());
    let extended = KnTrigram::train(&extended_input);
    let before = base.inspectable();
    let after = extended.inspectable();
    let totals = ledger.lm_totals();
    assert_eq!(
        count_diff(&after.unigram_counts, &before.unigram_counts),
        totals.unigram
    );
    assert_eq!(
        count_diff(&after.bigram_counts, &before.bigram_counts),
        totals.bigram
    );
    assert_eq!(
        count_diff(&after.trigram_counts, &before.trigram_counts),
        totals.trigram
    );
    // Predicted tokens grow by one </s> plus one token per word.
    let expected_tokens: u64 = correction_sentences
        .iter()
        .map(|s| s.len() as u64 + 1)
        .sum();
    assert_eq!(after.total_tokens - before.total_tokens, expected_tokens);
}

#[test]
fn map_totals_match_adapt_overlay_totals() {
    const DIM: usize = 3;
    let base = HmmSet::flat_start(
        &["AA", "B", "D"],
        &[vec![vec![0.1, 0.2, 0.3], vec![0.0, 0.1, -0.1]]],
        0.01,
    )
    .expect("flat start");
    let mut overlay = AdaptOverlay::new(base, 10.0, AdaptStage::MeansOnly).expect("overlay");
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(DIM);
    let mut rng = Rng(0x0BAD_F00D_DEAD_BEEF);
    for seq in 0..8u64 {
        let input = random_correction(&mut rng, DIM, seq);
        ledger.apply(&mut fabric, &input).expect("apply");
    }
    // Mirror every live correction as an enrollment fact. Zero-padded ids
    // keep overlay iteration (string order) aligned with ledger iteration
    // (numeric FactId order), so the float folds are bit-identical.
    for event in ledger.live_events() {
        let record = ledger.get(event).expect("record");
        let mut stats = BTreeMap::new();
        for (key, value) in &record.map {
            stats.insert(
                AdaptKey {
                    phone: key.phone.clone(),
                    state: key.state,
                    comp: key.comp,
                },
                AdaptStats {
                    n: value.n,
                    sum: value.sum.clone(),
                    sumsq: value.sumsq.clone(),
                    frames: value.frames,
                },
            );
        }
        let frames = record.map.values().map(|s| s.frames).sum();
        overlay
            .assert_fact(AdaptFact {
                id: format!("correction:{:020}", event.0),
                hyp_id: "ref:correction".to_string(),
                frames,
                stats,
            })
            .expect("assert fact");
    }
    let overlay_totals = overlay.totals();
    let ledger_totals = ledger.map_totals();
    assert_eq!(overlay_totals.len(), ledger_totals.len());
    for (key, value) in &ledger_totals {
        let other = overlay_totals
            .get(&AdaptKey {
                phone: key.phone.clone(),
                state: key.state,
                comp: key.comp,
            })
            .expect("overlay key");
        assert_eq!(other.n, value.n, "n for {key:?}");
        assert_eq!(other.sum, value.sum, "sum for {key:?}");
        assert_eq!(other.sumsq, value.sumsq, "sumsq for {key:?}");
        assert_eq!(other.frames, value.frames, "frames for {key:?}");
    }
}

#[test]
fn decision_user_term_composes_with_undo() {
    let mut decision = DecisionFabric::new(DecisionParams::default());
    let mut ledger = CorrectionLedger::new(2);
    let applied = ledger
        .apply(decision.fabric_mut(), &test_input())
        .expect("apply");
    decision
        .register_token(applied.token, "reef", TokenKind::Word, 5, 15)
        .expect("register");
    decision
        .add_unary(applied.token, SourceClass::Acoustic, 1.0, BTreeSet::new())
        .expect("acoustic");
    decision
        .add_user(applied.token, applied.event)
        .expect("user");
    assert_eq!(
        decision.unary_sum(applied.token),
        1.0 + decision.params().u0
    );
    assert_eq!(
        decision.user_correction_facts(applied.token),
        vec![applied.event]
    );
    ledger
        .undo(decision.fabric_mut(), applied.event)
        .expect("undo");
    assert_eq!(
        decision.unary_sum(applied.token),
        1.0,
        "USER term gone with its event"
    );
    assert!(decision.user_correction_facts(applied.token).is_empty());
}

#[test]
fn apply_validates_before_interning_anything() {
    let mut fabric = Fabric::new();
    let mut ledger = CorrectionLedger::new(2);
    let facts_before = fabric.provenance_stats().total_facts;
    let mut bad = test_input();
    bad.end_frame = bad.start_frame;
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::BadSpan { start: 5, end: 5 })
    );
    bad = test_input();
    bad.corrected_word.clear();
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::EmptyWord)
    );
    bad = test_input();
    bad.corrected_sentence.clear();
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::EmptySentence)
    );
    bad = test_input();
    bad.corrected_sentence.push(String::new());
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::EmptyToken)
    );
    bad = test_input();
    bad.observed_phones.clear();
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::EmptyObservation)
    );
    bad = test_input();
    bad.map.push(MapContribution {
        key: MapKey {
            phone: "AA".to_string(),
            state: 0,
            comp: 0,
        },
        stats: MapStats {
            n: 1.0,
            sum: vec![0.0; 3],
            sumsq: vec![0.0; 2],
            frames: 1,
        },
    });
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::DimMismatch {
            expected: 2,
            got: 3
        })
    );
    bad = test_input();
    bad.map.push(MapContribution {
        key: MapKey {
            phone: "AA".to_string(),
            state: 0,
            comp: 0,
        },
        stats: MapStats {
            n: f64::NAN,
            sum: vec![0.0; 2],
            sumsq: vec![0.0; 2],
            frames: 1,
        },
    });
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::NonFiniteStat)
    );
    bad = test_input();
    bad.map.push(MapContribution {
        key: MapKey {
            phone: "AA".to_string(),
            state: 0,
            comp: 0,
        },
        stats: MapStats {
            n: -1.0,
            sum: vec![0.0; 2],
            sumsq: vec![0.0; 2],
            frames: 1,
        },
    });
    assert_eq!(
        ledger.apply(&mut fabric, &bad),
        Err(CorrectionError::NegativeN)
    );
    // No partial facts: fabric untouched, ledger empty.
    assert_eq!(fabric.provenance_stats().total_facts, facts_before);
    assert!(ledger.is_empty());
    // Unknown undo.
    let ghost = fabric.intern();
    assert_eq!(
        ledger.undo(&mut fabric, ghost),
        Err(CorrectionError::UnknownCorrection(ghost))
    );
}
