use hmm::{HmmSet, Rng, SIL, SP, TrainConfig, generate};
use std::collections::BTreeMap;

fn known_set() -> HmmSet {
    let utterances = vec![vec![vec![0.0, 0.0]]];
    let mut set = HmmSet::flat_start(&["AA", "B"], &utterances, 0.01).unwrap();
    // Known generating parameters: well-separated means.
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

#[test]
fn synthetic_known_hmm_recovery() {
    let truth = known_set();
    let seq: Vec<String> = vec!["AA".into(), "B".into()];
    let mut rng = Rng::new(42);
    let mut data = Vec::new();
    for _ in 0..40 {
        data.push((seq.clone(), generate(&truth, &seq, 4, &mut rng)));
    }
    let obs: Vec<Vec<Vec<f64>>> = data.iter().map(|(_, o)| o.clone()).collect();
    let mut model = HmmSet::flat_start(&["AA", "B"], &obs, 0.01).unwrap();
    let cfg = TrainConfig {
        bw_iterations: 8,
        embedded_iterations: 3,
        var_floor_ratio: 0.01,
        optional_sp: true,
        estep_chunk: 2000,
    };
    let history = model.train_embedded(&data, &cfg).unwrap();
    assert_eq!(history.len(), 3);
    for phone in ["AA", "B"] {
        for s in 0..3 {
            for d in 0..2 {
                let rec = model.phones[phone].emissions[s].components[0].gaussian.mean[d];
                let tru = truth.phones[phone].emissions[s].components[0].gaussian.mean[d];
                assert!(
                    (rec - tru).abs() < 0.6,
                    "{phone} s{s} d{d}: recovered {rec} vs true {tru}"
                );
                assert!(model.phones[phone].emissions[s].components[0].gaussian.var[d] > 0.0);
            }
        }
    }
    eprintln!(
        "recovered AA0={:?} B0={:?} ll_history={history:?}",
        model.phones["AA"].emissions[0].components[0].gaussian.mean,
        model.phones["B"].emissions[0].components[0].gaussian.mean
    );
}

#[test]
fn flat_start_and_sp_tie() {
    let obs = vec![vec![vec![1.0, 2.0], vec![3.0, 4.0]]];
    let set = HmmSet::flat_start(&["AA"], &obs, 0.01).unwrap();
    assert_eq!(
        set.phones["AA"].emissions[0].components[0].gaussian.mean,
        set.global_mean
    );
    assert_eq!(set.phones[SP].n_states, 1);
    assert_eq!(set.emission(SP, 0).unwrap(), set.emission(SIL, 1).unwrap());
}

#[test]
fn forced_alignment_and_transcript() {
    let truth = known_set();
    let seq: Vec<String> = vec!["AA".into(), "B".into()];
    let mut rng = Rng::new(7);
    let obs = generate(&truth, &seq, 3, &mut rng);
    let (path, ll) = truth.forced_align(&seq, &obs).unwrap();
    assert_eq!(path.len(), obs.len());
    assert!(ll.is_finite());
    assert!(path.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] <= 1));
    let mut dict = BTreeMap::new();
    dict.insert("HELLO".to_string(), vec!["AA".to_string(), "B".to_string()]);
    dict.insert("WORLD".to_string(), vec!["B".to_string()]);
    let expanded = truth
        .expand_transcript(&["hello", "world"], &dict, true)
        .unwrap();
    assert_eq!(expanded, vec![SIL, "AA", "B", SP, "B", SIL]);
}

#[test]
fn model_round_trip_lossless() {
    let truth = known_set();
    let text = truth.to_text();
    let back = HmmSet::from_text(&text).unwrap();
    assert_eq!(truth, back);
    let bin = truth.to_binary();
    let back2 = HmmSet::from_binary(&bin).unwrap();
    assert_eq!(truth, back2);
    // Bitwise check on a mean value survives.
    assert_eq!(
        back2.phones["AA"].emissions[0].components[0].gaussian.mean[0].to_bits(),
        (-4.0f64).to_bits()
    );
}
