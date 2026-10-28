//! R2b gate tests (bead hearsay-four-0bv): checkpoint selection on DEV PER,
//! mixture splitting driven by DEV, parameter provenance validator.
//! The real-partition gate consumes D1 partition manifests (use-verified
//! sets only). Features here are generated deterministically per real
//! utterance id from a known HMM (machinery test); the real-audio gate that
//! extracts fse features from better-voice audio lives in
//! real_data_gate_tests.rs.
use hmm::{HmmSet, Rng, TrainConfig, UttData, generate, paired_ci, phone_edit_distance};

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

fn make_data(truth: &HmmSet, n: usize, seed: u64) -> Vec<UttData> {
    let seqs: Vec<Vec<String>> = vec![
        vec!["AA".into(), "B".into()],
        vec!["B".into(), "AA".into()],
        vec!["AA".into(), "B".into(), "AA".into()],
    ];
    let mut rng = Rng::new(seed);
    (0..n)
        .map(|i| {
            let seq = seqs[i % seqs.len()].clone();
            (
                format!("utt:{i}"),
                seq.clone(),
                generate(truth, &seq, 4, &mut rng),
            )
        })
        .collect()
}

fn plain(data: &[UttData]) -> Vec<(Vec<String>, Vec<Vec<f64>>)> {
    data.iter()
        .map(|(_, s, o)| (s.clone(), o.clone()))
        .collect()
}

#[test]
fn checkpoint_selection_beats_flat_start_paired_ci() {
    let truth = truth_set();
    let train = make_data(&truth, 40, 42);
    let dev_full = make_data(&truth, 24, 99);
    let dev = plain(&dev_full);
    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&["AA", "B"], &obs, 0.01).unwrap();
    let cfg = TrainConfig {
        bw_iterations: 6,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
    };
    let run = model
        .train_checkpoints(&train, &dev, &cfg, &[2, 4, 8])
        .unwrap();
    assert!(
        run.checkpoints.len() >= 7,
        "one checkpoint per BW iteration plus flat start"
    );
    assert_eq!(run.checkpoints[0].iteration, 0);
    let selected = &run.checkpoints[run.selected];
    eprintln!(
        "flat PER={} selected PER={} (iter {}, {} mixtures)",
        run.flat_dev_per, selected.dev_per, selected.iteration, selected.mixtures
    );
    assert!(
        selected.dev_per < run.flat_dev_per,
        "selected checkpoint must beat flat start on DEV PER"
    );
    let flat_rep = run.checkpoints[0].model.phone_error_rate(&dev).unwrap();
    let sel_rep = selected.model.phone_error_rate(&dev).unwrap();
    let diffs: Vec<f64> = sel_rep
        .per_utterance
        .iter()
        .zip(&flat_rep.per_utterance)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, _lo, hi) = paired_ci(&diffs);
    eprintln!("paired diff mean={mean} CI hi={hi}");
    assert!(
        mean < 0.0 && hi < 0.0,
        "paired CI must lie below 0 (mean {mean}, hi {hi})"
    );
    assert_eq!(
        run.split_decisions.len(),
        3,
        "splitting decision recorded for 2, 4, 8"
    );
    assert_eq!(run.split_decisions[0].to_mixtures, 2);
    assert_eq!(run.split_decisions[1].to_mixtures, 4);
    assert_eq!(run.split_decisions[2].to_mixtures, 8);
    for d in &run.split_decisions {
        assert!(d.dev_per_before >= 0.0 && d.dev_per_after >= 0.0);
        if d.accepted {
            assert!(d.dev_per_after < d.dev_per_before);
        }
    }
    selected.model.validate_provenance().unwrap();
    model.validate_provenance().unwrap();
}

#[test]
fn provenance_validator_and_sufficient_statistics() {
    let truth = truth_set();
    let train = make_data(&truth, 12, 5);
    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&["AA", "B"], &obs, 0.01).unwrap();
    model.validate_provenance().unwrap();
    let one = TrainConfig {
        bw_iterations: 2,
        ..TrainConfig::default()
    };
    model.baum_welch_ids(&train, &one).unwrap();
    model.validate_provenance().unwrap();
    // Sufficient statistics reproduce the stored mean for an occupied component.
    let comp = &model.phones["AA"].emissions[0].components[0];
    assert!(comp.stats.count > 0.0);
    assert!(!comp.stats.provenance.utterances.is_empty());
    assert!(comp.stats.provenance.frames > 0);
    for d in 0..2 {
        let m = comp.stats.sum[d] / comp.stats.count;
        assert!((m - comp.gaussian.mean[d]).abs() < 1e-9);
    }
    // Stripping provenance must fail validation.
    let mut broken = model.clone();
    broken.phones.get_mut("AA").unwrap().emissions[0].components[0]
        .stats
        .provenance
        .utterances
        .clear();
    assert!(broken.validate_provenance().is_err());
}

#[test]
fn mixture_splitting_grows_weights_normalised_provenance_kept() {
    let truth = truth_set();
    let train = make_data(&truth, 12, 5);
    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&["AA", "B"], &obs, 0.01).unwrap();
    for target in [2usize, 4, 8] {
        model.split_mixtures_to(target);
        for (name, h) in &model.phones {
            if h.tied_to.is_some() {
                continue;
            }
            for mix in &h.emissions {
                assert_eq!(mix.components.len(), target, "{name} mixture count");
                let w: f64 = mix.components.iter().map(|c| c.weight).sum();
                assert!((w - 1.0).abs() < 1e-9, "weights normalised");
            }
        }
        model.validate_provenance().unwrap();
    }
    let _ = train;
}

#[test]
fn paired_ci_and_edit_distance_units() {
    assert_eq!(
        phone_edit_distance(&["AA".into(), "B".into()], &["AA".into(), "B".into()]),
        0
    );
    assert_eq!(
        phone_edit_distance(&["AA".into(), "B".into()], &["AA".into()]),
        1
    );
    assert_eq!(
        phone_edit_distance(&["AA".into()], &["B".into(), "AA".into()]),
        1
    );
    let (mean, lo, hi) = paired_ci(&[-0.5, -0.4, -0.6, -0.5, -0.45]);
    assert!(mean < 0.0 && hi < 0.0 && lo < mean);
    let (m2, lo2, hi2) = paired_ci(&[0.1, -0.1, 0.05, -0.05]);
    assert!(lo2 < m2 && m2 < hi2);
}

fn hash_id(s: &str) -> u64 {
    let mut h = 1469598103934665603u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h.max(1)
}

#[test]
fn real_partition_checkpoint_gate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let load = |name: &str| -> serde_json::Value {
        let text =
            std::fs::read_to_string(root.join(format!("data/partitions/{name}.json"))).unwrap();
        serde_json::from_str(&text).unwrap()
    };
    let train_j = load("train");
    let dev_j = load("dev");
    let excluded_j = load("excluded");
    let excluded_sets: Vec<String> = excluded_j["excluded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["dataset"].as_str().unwrap().to_string())
        .collect();
    let ids = |v: &serde_json::Value| -> Vec<(String, String)> {
        v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["path"].as_str().unwrap().to_string(),
                    e["dataset"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    let train_entries = ids(&train_j);
    let dev_entries = ids(&dev_j);
    let train_paths: std::collections::BTreeSet<&str> =
        train_entries.iter().map(|(p, _)| p.as_str()).collect();
    for (p, ds) in &dev_entries {
        assert!(!train_paths.contains(p.as_str()), "TRAIN/DEV disjoint (D1)");
        assert!(
            !excluded_sets.contains(ds),
            "DEV consumes only use-verified data"
        );
    }
    for (_, ds) in &train_entries {
        assert!(
            !excluded_sets.contains(ds),
            "TRAIN consumes only use-verified data"
        );
    }
    eprintln!(
        "D1 consumed: TRAIN {} entries, DEV {} entries; excluded sets off-limits: {}",
        train_entries.len(),
        dev_entries.len(),
        excluded_sets.len()
    );
    let truth = truth_set();
    let seqs: Vec<Vec<String>> = vec![vec!["AA".into(), "B".into()], vec!["B".into(), "AA".into()]];
    let build =
        |entries: &[(String, String)], cap: usize| -> Vec<(String, Vec<String>, Vec<Vec<f64>>)> {
            entries
                .iter()
                .take(cap)
                .map(|(p, _)| {
                    let mut rng = Rng::new(hash_id(p));
                    let seq = seqs[(hash_id(p) % 2) as usize].clone();
                    (p.clone(), seq.clone(), generate(&truth, &seq, 4, &mut rng))
                })
                .collect()
        };
    let train = build(&train_entries, 30);
    let dev_full = build(&dev_entries, 16);
    let dev = plain(&dev_full);
    let obs: Vec<Vec<Vec<f64>>> = train.iter().map(|(_, _, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&["AA", "B"], &obs, 0.01).unwrap();
    let cfg = TrainConfig {
        bw_iterations: 6,
        embedded_iterations: 1,
        var_floor_ratio: 0.01,
        optional_sp: true,
    };
    let run = model
        .train_checkpoints(&train, &dev, &cfg, &[2, 4, 8])
        .unwrap();
    let selected = &run.checkpoints[run.selected];
    eprintln!(
        "real-partition gate: flat PER={} selected PER={} splits={:?}",
        run.flat_dev_per, selected.dev_per, run.split_decisions
    );
    assert!(selected.dev_per < run.flat_dev_per);
    let flat_rep = run.checkpoints[0].model.phone_error_rate(&dev).unwrap();
    let sel_rep = selected.model.phone_error_rate(&dev).unwrap();
    let diffs: Vec<f64> = sel_rep
        .per_utterance
        .iter()
        .zip(&flat_rep.per_utterance)
        .map(|(a, b)| a - b)
        .collect();
    let (mean, _lo, hi) = paired_ci(&diffs);
    assert!(
        mean < 0.0 && hi < 0.0,
        "paired CI below 0 on D1 DEV ids (mean {mean}, hi {hi})"
    );
    selected.model.validate_provenance().unwrap();
    assert_eq!(run.split_decisions.len(), 3);
    // Provenance points at real D1 utterance ids.
    let comp = &selected.model.phones["AA"].emissions[0].components[0];
    assert!(
        comp.stats
            .provenance
            .utterances
            .iter()
            .all(|u| train_paths.contains(u.as_str()))
    );
}
