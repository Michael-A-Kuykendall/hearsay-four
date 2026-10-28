//! Owner enrollment + MAP adaptation tests (bead
//! hearsay-four-jem--owner-enrollment-adaptation). Synthetic fixtures only —
//! no audio. The real-audio owner gate lives in owner_enroll_gate_tests.rs.
use hmm::adapt::{
    AdaptFact, AdaptKey, AdaptOverlay, AdaptStage, AdaptStats, collect_fact, select_stage, tune_tau,
};
use hmm::{HmmSet, Rng, TrainConfig, generate};
use std::collections::BTreeMap;

fn truth_set() -> HmmSet {
    let utterances = vec![vec![vec![0.0, 0.0]]];
    let mut set = HmmSet::flat_start(&["AA", "B"], &utterances, 0.01).unwrap();
    for (i, m) in [[-4.0, -4.0], [-3.0, -4.0], [-2.0, -4.0]]
        .iter()
        .enumerate()
    {
        set.phones.get_mut("AA").unwrap().emissions[i].components[0]
            .gaussian
            .mean = m.to_vec();
        set.phones.get_mut("AA").unwrap().emissions[i].components[0]
            .gaussian
            .var = vec![0.09, 0.09];
    }
    for (i, m) in [[4.0, 4.0], [5.0, 4.0], [6.0, 4.0]].iter().enumerate() {
        set.phones.get_mut("B").unwrap().emissions[i].components[0]
            .gaussian
            .mean = m.to_vec();
        set.phones.get_mut("B").unwrap().emissions[i].components[0]
            .gaussian
            .var = vec![0.09, 0.09];
    }
    set
}

fn seqs() -> Vec<Vec<String>> {
    vec![
        vec!["AA".into(), "B".into()],
        vec!["B".into(), "AA".into()],
        vec!["AA".into(), "B".into(), "AA".into()],
    ]
}

fn hand_fact(id: &str, key: AdaptKey, n: f64, sum: Vec<f64>, sumsq: Vec<f64>) -> AdaptFact {
    let mut stats = BTreeMap::new();
    stats.insert(
        key,
        AdaptStats {
            n,
            sum,
            sumsq,
            frames: 10,
        },
    );
    AdaptFact {
        id: id.into(),
        hyp_id: format!("ref:{id}"),
        frames: 10,
        stats,
    }
}

#[test]
fn map_stage1_formula_matches_bead_spec() {
    // mu = (tau * mu_base + sum_x) / (tau + n), variance stays base.
    let base = truth_set();
    let key = AdaptKey {
        phone: "AA".into(),
        state: 0,
        comp: 0,
    };
    // mu_base = [-4, -4]; tau = 3, n = 1, sum = [2, 6]:
    // mu = ([-12, -12] + [2, 6]) / 4 = [-2.5, -1.5].
    let fact = hand_fact("f1", key, 1.0, vec![2.0, 6.0], vec![4.0, 36.0]);
    let mut overlay = AdaptOverlay::new(base.clone(), 3.0, AdaptStage::MeansOnly).unwrap();
    overlay.assert_fact(fact).unwrap();
    let adapted = overlay.adapted_model();
    let g = &adapted.phones["AA"].emissions[0].components[0].gaussian;
    assert!((g.mean[0] - -2.5).abs() < 1e-12, "mean[0] = {}", g.mean[0]);
    assert!((g.mean[1] - -1.5).abs() < 1e-12, "mean[1] = {}", g.mean[1]);
    assert_eq!(g.var, vec![0.09, 0.09], "stage 1 keeps base variance");
    // Untouched components are bit-identical to base.
    assert_eq!(
        adapted.phones["B"].emissions[2].components[0].gaussian,
        base.phones["B"].emissions[2].components[0].gaussian
    );
    // Base itself is immutable.
    assert_eq!(overlay.base().clone(), base);
}

#[test]
fn stage2_variance_formula() {
    // var_new = (tau * (var_b + mu_b^2) + sumsq) / (tau + n) - mu_new^2.
    let base = truth_set();
    let key = AdaptKey {
        phone: "B".into(),
        state: 1,
        comp: 0,
    };
    // mu_b = [5, 4], var_b = [0.09, 0.09]; tau = 1, n = 1,
    // sum = [7, 6], sumsq = [49, 36].
    // mu_new = ([5,4] + [7,6]) / 2 = [6, 5].
    // dim0: (1 * (0.09 + 25) + 49) / 2 - 36 = 37.045 - 36 = 1.045.
    // dim1: (1 * (0.09 + 16) + 36) / 2 - 25 = 26.045 - 25 = 1.045.
    let fact = hand_fact("f2", key, 1.0, vec![7.0, 6.0], vec![49.0, 36.0]);
    let mut overlay = AdaptOverlay::new(base, 1.0, AdaptStage::MeansAndVariances).unwrap();
    overlay.assert_fact(fact).unwrap();
    let g = &overlay.adapted_model().phones["B"].emissions[1].components[0].gaussian;
    assert!((g.mean[0] - 6.0).abs() < 1e-12);
    assert!((g.mean[1] - 5.0).abs() < 1e-12);
    assert!((g.var[0] - 1.045).abs() < 1e-9, "var[0] = {}", g.var[0]);
    assert!((g.var[1] - 1.045).abs() < 1e-9, "var[1] = {}", g.var[1]);
}

#[test]
fn tau_zero_gives_ml_estimate_and_large_tau_stays_near_base() {
    let base = truth_set();
    let key = AdaptKey {
        phone: "AA".into(),
        state: 2,
        comp: 0,
    };
    let fact = hand_fact("f", key.clone(), 4.0, vec![8.0, 12.0], vec![16.0, 36.0]);
    let mut ml = AdaptOverlay::new(base.clone(), 0.0, AdaptStage::MeansOnly).unwrap();
    ml.assert_fact(fact.clone()).unwrap();
    let g = &ml.adapted_model().phones["AA"].emissions[2].components[0].gaussian;
    assert!((g.mean[0] - 2.0).abs() < 1e-12);
    assert!((g.mean[1] - 3.0).abs() < 1e-12);
    let mut stiff = AdaptOverlay::new(base.clone(), 1e12, AdaptStage::MeansOnly).unwrap();
    stiff.assert_fact(fact).unwrap();
    let g = &stiff.adapted_model().phones["AA"].emissions[2].components[0].gaussian;
    let b = &base.phones["AA"].emissions[2].components[0].gaussian;
    assert!((g.mean[0] - b.mean[0]).abs() < 1e-6);
    assert!((g.mean[1] - b.mean[1]).abs() < 1e-6);
}

#[test]
fn retraction_restores_base_exactly() {
    let base = truth_set();
    let mut rng = Rng::new(7);
    let seq_list = seqs();
    let mut overlay = AdaptOverlay::new(base.clone(), 5.0, AdaptStage::MeansOnly).unwrap();
    let mut ids = Vec::new();
    for (i, seq) in seq_list.iter().enumerate() {
        let obs = generate(&base, seq, 4, &mut rng);
        let fact = collect_fact(&base, &format!("u{i}"), &format!("ref:u{i}"), seq, &obs).unwrap();
        assert!(!fact.stats.is_empty());
        ids.push(fact.id.clone());
        overlay.assert_fact(fact).unwrap();
    }
    assert_ne!(overlay.adapted_model(), base);
    for id in &ids {
        overlay.retract_fact(id).unwrap();
    }
    assert!(overlay.is_empty());
    assert_eq!(
        overlay.adapted_model(),
        base,
        "retract-all must restore the base bit-exactly"
    );
}

#[test]
fn retract_one_equals_adapt_without_it() {
    let base = truth_set();
    let mut rng = Rng::new(11);
    let seq_list = seqs();
    let facts: Vec<AdaptFact> = seq_list
        .iter()
        .enumerate()
        .map(|(i, seq)| {
            let obs = generate(&base, seq, 4, &mut rng);
            collect_fact(&base, &format!("w{i}"), &format!("ref:w{i}"), seq, &obs).unwrap()
        })
        .collect();
    let mut full = AdaptOverlay::new(base.clone(), 5.0, AdaptStage::MeansOnly).unwrap();
    for f in &facts {
        full.assert_fact(f.clone()).unwrap();
    }
    let dropped = facts[1].id.clone();
    full.retract_fact(&dropped).unwrap();
    let mut partial = AdaptOverlay::new(base.clone(), 5.0, AdaptStage::MeansOnly).unwrap();
    for f in [&facts[0], &facts[2]] {
        partial.assert_fact(f.clone()).unwrap();
    }
    assert_eq!(full.adapted_model(), partial.adapted_model());
    assert_eq!(full.totals(), partial.totals());
}
#[test]
fn fabric_backed_retract_restores_base_exactly() {
    let base = truth_set();
    let mut rng = Rng::new(13);
    let seq_list = seqs();
    let mut overlay = AdaptOverlay::new(base.clone(), 5.0, AdaptStage::MeansOnly).unwrap();
    let mut ids = Vec::new();
    for (i, seq) in seq_list.iter().enumerate() {
        let obs = generate(&base, seq, 4, &mut rng);
        let fact = collect_fact(&base, &format!("f{i}"), &format!("ref:f{i}"), seq, &obs).unwrap();
        ids.push(fact.id.clone());
        overlay.assert_fact(fact).unwrap();
    }
    // Every contribution is an active fabric fact.
    for id in &ids {
        assert!(
            overlay.fabric_is_active(id),
            "{id} must be active in fabric"
        );
        assert_eq!(overlay.fabric_support_count(id), 1);
    }
    assert_ne!(overlay.adapted_model(), base);
    // Retract one: the fabric deactivates it but preserves provenance.
    overlay.retract_fact(&ids[0]).unwrap();
    assert!(!overlay.fabric_is_active(&ids[0]));
    assert_eq!(
        overlay.fabric_justification_count(&ids[0]),
        1,
        "retraction preserves the justification"
    );
    // Serialization preserves the fabric active set.
    let text = serde_json::to_string(&overlay).unwrap();
    let back: AdaptOverlay = serde_json::from_str(&text).unwrap();
    assert_eq!(overlay, back);
    assert_eq!(overlay.adapted_model(), back.adapted_model());
    // An unrelated assert must not resurrect the retracted fact.
    let extra_obs = generate(&base, &seq_list[0], 4, &mut rng);
    let extra = collect_fact(&base, "fx", "ref:fx", &seq_list[0], &extra_obs).unwrap();
    let extra_id = extra.id.clone();
    overlay.assert_fact(extra).unwrap();
    assert!(
        !overlay.fabric_is_active(&ids[0]),
        "retracted fact must stay inactive"
    );
    assert!(overlay.fabric_is_active(&extra_id));
    // Retract-all restores the base bit-exactly.
    overlay.retract_fact(&ids[1]).unwrap();
    overlay.retract_fact(&ids[2]).unwrap();
    overlay.retract_fact(&extra_id).unwrap();
    assert!(overlay.is_empty());
    assert!(overlay.totals().is_empty());
    assert_eq!(
        overlay.adapted_model(),
        base,
        "fabric-backed retract-all must restore the base exactly"
    );
}
#[test]
fn overlay_rejects_duplicates_unknown_ids_dim_mismatch_and_bad_tau() {
    let base = truth_set();
    let key = AdaptKey {
        phone: "AA".into(),
        state: 0,
        comp: 0,
    };
    let fact = hand_fact("dup", key.clone(), 1.0, vec![0.0, 0.0], vec![0.0, 0.0]);
    let mut overlay = AdaptOverlay::new(base.clone(), 2.0, AdaptStage::MeansOnly).unwrap();
    overlay.assert_fact(fact.clone()).unwrap();
    assert!(overlay.assert_fact(fact).is_err(), "duplicate id must fail");
    assert!(
        overlay.retract_fact("enroll:nope").is_err(),
        "unknown id must fail"
    );
    let bad = hand_fact("bad", key, 1.0, vec![0.0, 0.0, 0.0], vec![0.0; 3]);
    assert!(overlay.assert_fact(bad).is_err(), "dim mismatch must fail");
    assert!(AdaptOverlay::new(base.clone(), -1.0, AdaptStage::MeansOnly).is_err());
    assert!(AdaptOverlay::new(base, f64::NAN, AdaptStage::MeansOnly).is_err());
}

#[test]
fn collect_fact_rejects_empty_obs_dim_mismatch_and_unknown_phone() {
    let base = truth_set();
    let seq = vec!["AA".to_string()];
    assert!(collect_fact(&base, "u", "ref:u", &seq, &[]).is_err());
    assert!(collect_fact(&base, "u", "ref:u", &seq, &[vec![0.0, 0.0, 0.0]]).is_err());
    assert!(collect_fact(&base, "u", "ref:u", &["ZZZ".to_string()], &[vec![0.0, 0.0]],).is_err());
}

#[test]
fn adapted_model_keeps_provenance_and_sp_tie() {
    let base = truth_set();
    let mut rng = Rng::new(3);
    let seq = vec!["AA".into(), "B".into()];
    let obs = generate(&base, &seq, 6, &mut rng);
    let fact = collect_fact(&base, "sp1", "ref:sp1", &seq, &obs).unwrap();
    let mut overlay = AdaptOverlay::new(base, 4.0, AdaptStage::MeansOnly).unwrap();
    overlay.assert_fact(fact.clone()).unwrap();
    let adapted = overlay.adapted_model();
    adapted.validate_provenance().unwrap();
    // SP stays tied to the (possibly adapted) SIL centre.
    assert_eq!(
        adapted.phones["SP"].emissions[0],
        adapted.phones["SIL"].emissions[1]
    );
    // Touched components cite the enrollment fact.
    let mut cited = 0usize;
    for h in adapted.phones.values() {
        if h.tied_to.is_some() {
            continue;
        }
        for mix in &h.emissions {
            for comp in &mix.components {
                if comp.stats.provenance.utterances.contains(&fact.id) {
                    cited += 1;
                }
            }
        }
    }
    assert!(cited > 0, "adapted params must cite the enrollment fact");
}

#[test]
fn overlay_serialization_round_trip() {
    let base = truth_set();
    let mut rng = Rng::new(5);
    let seq = vec!["B".into(), "AA".into()];
    let obs = generate(&base, &seq, 5, &mut rng);
    let fact = collect_fact(&base, "ser1", "ref:ser1", &seq, &obs).unwrap();
    let mut overlay = AdaptOverlay::new(base, 2.5, AdaptStage::MeansOnly).unwrap();
    overlay.assert_fact(fact).unwrap();
    let text = serde_json::to_string(&overlay).unwrap();
    let back: AdaptOverlay = serde_json::from_str(&text).unwrap();
    assert_eq!(overlay, back);
    assert_eq!(overlay.adapted_model(), back.adapted_model());
}

/// Overlapping two-phone truth: AA and B means 1.0 apart with unit variance,
/// so a speaker shift along dim 0 genuinely moves owner frames across the
/// base decision boundary (well-separated phones would decode perfectly with
/// or without adaptation, proving nothing).
fn overlap_truth() -> HmmSet {
    let utterances = vec![vec![vec![0.0, 0.0]]];
    let mut set = HmmSet::flat_start(&["AA", "B"], &utterances, 0.01).unwrap();
    for (i, m) in [[0.0, 0.0], [0.3, 0.0], [0.6, 0.0]].iter().enumerate() {
        set.phones.get_mut("AA").unwrap().emissions[i].components[0]
            .gaussian
            .mean = m.to_vec();
        set.phones.get_mut("AA").unwrap().emissions[i].components[0]
            .gaussian
            .var = vec![1.0, 1.0];
    }
    for (i, m) in [[1.6, 0.0], [1.9, 0.0], [2.2, 0.0]].iter().enumerate() {
        set.phones.get_mut("B").unwrap().emissions[i].components[0]
            .gaussian
            .mean = m.to_vec();
        set.phones.get_mut("B").unwrap().emissions[i].components[0]
            .gaussian
            .var = vec![1.0, 1.0];
    }
    set
}

#[test]
fn tune_tau_improves_shifted_speaker_and_reports_ci() {
    // Base speaker at truth; owner shifted by +1.5 in dim 0, pushing owner-AA
    // onto the base-B means. MAP enrollment on owner-train must beat base on
    // owner-dev with a CI excluding 0.
    let cfg = TrainConfig::default();
    let truth = overlap_truth();
    let mut owner = truth.clone();
    for h in owner.phones.values_mut() {
        if h.tied_to.is_some() {
            continue;
        }
        for mix in h.emissions.iter_mut() {
            for comp in mix.components.iter_mut() {
                comp.gaussian.mean[0] += 1.5;
            }
        }
    }
    let seq_list = seqs();
    let mut rng = Rng::new(21);
    let mut train: Vec<(Vec<String>, Vec<Vec<f64>>)> = Vec::new();
    for i in 0..24 {
        let seq = seq_list[i % seq_list.len()].clone();
        train.push((seq.clone(), generate(&truth, &seq, 8, &mut rng)));
    }
    let obs_all: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, o)| o.clone()).collect();
    let mut base = HmmSet::flat_start(&["AA", "B"], &obs_all, cfg.var_floor_ratio).unwrap();
    base.train_embedded(&train, &cfg).unwrap();
    assert!(base.phone_error_rate(&train).is_ok());

    let mut rng = Rng::new(99);
    let mut enroll: Vec<(String, Vec<String>, Vec<Vec<f64>>)> = Vec::new();
    let mut dev: Vec<(Vec<String>, Vec<Vec<f64>>)> = Vec::new();
    for i in 0..48 {
        let seq = seq_list[i % seq_list.len()].clone();
        let frames = generate(&owner, &seq, 8, &mut rng);
        if i < 24 {
            enroll.push((format!("own{i}"), seq, frames));
        } else {
            dev.push((seq, frames));
        }
    }
    let facts: Vec<AdaptFact> = enroll
        .iter()
        .map(|(id, seq, frames)| {
            collect_fact(&base, id, &format!("ref:{id}"), seq, frames).unwrap()
        })
        .collect();
    let taus = vec![0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
    let decision = tune_tau(&base, &facts, &dev, &taus, AdaptStage::MeansOnly).unwrap();
    eprintln!(
        "synthetic owner: base PER={:.4} adapted PER={:.4} tau={} diff mean={:.4} CI=[{:.4},{:.4}] keep={}",
        decision.dev_per_base,
        decision.dev_per_adapted,
        decision.best_tau,
        decision.mean_diff,
        decision.ci_lo,
        decision.ci_hi,
        decision.keep
    );
    assert!(
        decision.keep,
        "MAP must beat base on shifted-speaker DEV with CI excluding 0"
    );
}

#[test]
fn select_stage_prefers_means_only_unless_variance_wins() {
    let cfg = TrainConfig::default();
    let truth = truth_set();
    let mut rng = Rng::new(31);
    let seq_list = seqs();
    let mut train: Vec<(Vec<String>, Vec<Vec<f64>>)> = Vec::new();
    for i in 0..18 {
        let seq = seq_list[i % seq_list.len()].clone();
        train.push((seq.clone(), generate(&truth, &seq, 6, &mut rng)));
    }
    let obs_all: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, o)| o.clone()).collect();
    let mut base = HmmSet::flat_start(&["AA", "B"], &obs_all, cfg.var_floor_ratio).unwrap();
    base.train_embedded(&train, &cfg).unwrap();
    let mut rng = Rng::new(77);
    let mut facts = Vec::new();
    let mut dev = Vec::new();
    for i in 0..24 {
        let seq = seq_list[i % seq_list.len()].clone();
        let frames = generate(&truth, &seq, 6, &mut rng);
        if i < 14 {
            facts.push(
                collect_fact(&base, &format!("s{i}"), &format!("ref:s{i}"), &seq, &frames).unwrap(),
            );
        } else {
            dev.push((seq, frames));
        }
    }
    let taus = vec![1.0, 5.0, 20.0, 100.0];
    let sel = select_stage(&base, &facts, &dev, &taus).unwrap();
    // Rule check, not outcome check: stage 2 is selected only when it beats
    // stage 1 with a CI excluding 0.
    if sel.selected == AdaptStage::MeansAndVariances {
        assert!(sel.stage2_mean_diff < 0.0 && sel.stage2_ci_hi < 0.0);
    } else {
        assert!(
            !(sel.stage2.dev_per_adapted < sel.stage1.dev_per_adapted && sel.stage2_ci_hi < 0.0),
            "stage 2 won on the rule but was not selected"
        );
    }
    eprintln!(
        "stage select: s1 PER={:.4} (tau {}) s2 PER={:.4} (tau {}) selected={:?} s2-vs-s1 CI=[{:.4},{:.4}]",
        sel.stage1.dev_per_adapted,
        sel.stage1.best_tau,
        sel.stage2.dev_per_adapted,
        sel.stage2.best_tau,
        sel.selected,
        sel.stage2_ci_lo,
        sel.stage2_ci_hi
    );
}
