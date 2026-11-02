//! Lane B wave 1a gates: tying determinism, question-path record+serialize,
//! boundary backoff. All stats are fixed synthetic (no randomness, no audio).

use hmm::triphone::{
    MAX_PROV_UTTS, TreeSide, TriStateStats, Triphone, TyingConfig, accumulate_stats, base_phone,
    bound_provenance, build_tying, cap_model_provenance, center_of_unit, default_questions,
    merge_stats, parse_triphone, triphones_for_word, unit_key, unit_keys_for_word,
};
use hmm::{HmmSet, Provenance};
use std::collections::{BTreeMap, BTreeSet};

fn stat(left: &str, center: &str, right: &str, state: usize, mean: f64) -> TriStateStats {
    let dim = 2;
    let count = 100.0;
    TriStateStats {
        key: Triphone {
            left: Some(left.to_string()),
            center: center.to_string(),
            right: Some(right.to_string()),
        },
        state,
        count,
        sum: vec![mean * count; dim],
        sum_sq: vec![(1.0 + mean * mean) * count; dim],
        utterances: BTreeSet::from(["utt:fixed".to_string()]),
        frames: 100,
    }
}

/// Two well-separated clusters by left context (stops at +5, vowels at -5).
fn fixed_stats() -> Vec<TriStateStats> {
    vec![
        stat("B", "AE", "T", 1, 5.0),
        stat("D", "AE", "T", 1, 5.0),
        stat("IY", "AE", "T", 1, -5.0),
        stat("UW", "AE", "T", 1, -5.0),
    ]
}

#[test]
fn tying_determinism_on_fixed_stats() {
    let questions = default_questions();
    let cfg = TyingConfig::default();
    let stats = fixed_stats();
    let once = build_tying(&stats, &questions, &cfg);
    // The stop/vowel split must actually fire (gain >> threshold).
    assert!(
        once.len() >= 2,
        "expected a split, got {} tied states",
        once.len()
    );
    // Same input twice -> bit-identical tying (ids, members, paths, counts).
    let twice = build_tying(&stats, &questions, &cfg);
    assert_eq!(
        serde_json::to_string(&once).unwrap(),
        serde_json::to_string(&twice).unwrap()
    );
    // Input order must not matter either.
    let mut rev = stats.clone();
    rev.reverse();
    let shuffled = build_tying(&rev, &questions, &cfg);
    assert_eq!(
        serde_json::to_string(&once).unwrap(),
        serde_json::to_string(&shuffled).unwrap()
    );
    // Every member sits in exactly one tied state.
    let mut seen: Vec<Triphone> = once.iter().flat_map(|t| t.members.clone()).collect();
    seen.sort();
    let mut want: Vec<Triphone> = stats.iter().map(|s| s.key.clone()).collect();
    want.sort();
    assert_eq!(seen, want);
}

#[test]
fn question_path_recorded_and_serialized() {
    let questions = default_questions();
    let cfg = TyingConfig::default();
    let tied = build_tying(&fixed_stats(), &questions, &cfg);
    assert!(!tied.is_empty());
    // Replay every path: each member must survive its leaf's root-to-leaf walk.
    for leaf in &tied {
        for member in &leaf.members {
            assert_eq!(&member.center, &leaf.center);
            for step in &leaf.path {
                let q = questions
                    .iter()
                    .find(|q| q.name == step.question)
                    .unwrap_or_else(|| panic!("unknown question {}", step.question));
                let ctx = match step.side {
                    TreeSide::Left => member.left.as_deref(),
                    TreeSide::Right => member.right.as_deref(),
                };
                let hit = ctx.map(|c| q.matches(c)).unwrap_or(false);
                assert_eq!(
                    hit, step.answer,
                    "member {member:?} fails step {step:?} of leaf {}",
                    leaf.id
                );
            }
        }
        // Non-trivial leaves carry a non-empty path.
        if tied.len() > 1 {
            assert!(
                !leaf.path.is_empty(),
                "leaf {} has members but no path",
                leaf.id
            );
        }
    }
    // Serialize round-trip preserves ids, members, paths, counts exactly.
    let json = serde_json::to_string_pretty(&tied).unwrap();
    assert!(json.contains("\"question\""));
    let back: Vec<hmm::triphone::TiedState> = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_string(&back).unwrap(),
        serde_json::to_string(&tied).unwrap()
    );
}

#[test]
fn boundary_backoff() {
    // Word edges have no outside context; keys back off tri->bi->mono.
    let w: Vec<String> = ["B", "AE", "T"].iter().map(|s| s.to_string()).collect();
    let tris = triphones_for_word(&w);
    assert_eq!(tris.len(), 3);
    assert_eq!(tris[0].left, None);
    assert_eq!(tris[0].right, Some("AE".to_string()));
    assert_eq!(tris[2].left, Some("AE".to_string()));
    assert_eq!(tris[2].right, None);
    assert!(tris[1].is_full());
    assert_eq!(
        unit_keys_for_word(&w),
        vec!["B+AE".to_string(), "B-AE+T".to_string(), "AE-T".to_string()]
    );
    // Single-phone word -> bare monophone key.
    let one: Vec<String> = vec!["AH".to_string()];
    assert_eq!(unit_keys_for_word(&one), vec!["AH".to_string()]);
    // Key helpers.
    assert_eq!(center_of_unit("B-AE+T"), "AE");
    assert_eq!(center_of_unit("B-AE"), "AE");
    assert_eq!(center_of_unit("AE+T"), "AE");
    assert_eq!(center_of_unit("AE"), "AE");
    let full = parse_triphone("B-AE+T").unwrap();
    assert_eq!(full.center, "AE");
    assert_eq!(unit_key(&full), "B-AE+T");
    assert_eq!(parse_triphone("B-AE"), None);
    assert_eq!(parse_triphone("AE"), None);
    assert_eq!(base_phone("AH0"), "AH");

    // accumulate_stats skips SIL frames and keys boundary phones as biphones.
    let utterances = vec![vec![vec![0.0, 0.0]]];
    let model = HmmSet::flat_start(&["B", "AE", "T"], &utterances, 0.01).unwrap();
    let seq: Vec<String> = ["SIL", "B", "AE", "T"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Composite layout: SIL 0..3, B 3..6, AE 6..9, T 9..12.
    let path = vec![1, 4, 7, 10];
    let obs = vec![
        vec![0.1, 0.2], // SIL s1 -> skipped
        vec![1.0, 1.0], // B s1, word pos 0 -> B+AE
        vec![2.0, 2.0], // AE s1, word pos 1 -> B-AE+T
        vec![3.0, 3.0], // T s1, word pos 2 -> AE-T
    ];
    let got = accumulate_stats(&model, "utt:7", &seq, &[(1, 4)], &path, &obs).unwrap();
    assert_eq!(got.len(), 3);
    let key = |l: Option<&str>, c: &str, r: Option<&str>| Triphone {
        left: l.map(str::to_string),
        center: c.to_string(),
        right: r.map(str::to_string),
    };
    let b = &got[&(key(None, "B", Some("AE")), 1)];
    assert_eq!(b.count, 1.0);
    assert_eq!(b.sum, vec![1.0, 1.0]);
    assert!(b.utterances.contains("utt:7"));
    let ae = &got[&(key(Some("B"), "AE", Some("T")), 1)];
    assert_eq!(ae.count, 1.0);
    assert_eq!(ae.sum, vec![2.0, 2.0]);
    let t = &got[&(key(Some("AE"), "T", None), 1)];
    assert_eq!(t.count, 1.0);
    assert_eq!(t.sum, vec![3.0, 3.0]);
    for (tri, _) in got.keys() {
        assert_ne!(tri.center, "SIL");
    }
}

#[test]
fn subset_sampler_is_deterministic() {
    // Same (nrows, size, seed) twice -> identical picks.
    let a = hmm::triphone::sample_subset(285, 285, 20261101);
    let b = hmm::triphone::sample_subset(285, 285, 20261101);
    assert_eq!(a, b);
    // Full-size pick is a permutation of all rows.
    let mut sorted = a.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..285).collect::<Vec<_>>());
    // Different seed shuffles differently.
    let c = hmm::triphone::sample_subset(285, 285, 7);
    assert_ne!(a, c);
    // Fewer rows is a prefix of more rows under the same seed.
    let few = hmm::triphone::sample_subset(285, 30, 20261101);
    assert_eq!(few, a[..30]);
    // Size caps at nrows; empty stays empty.
    assert_eq!(hmm::triphone::sample_subset(10, 99, 1).len(), 10);
    assert!(hmm::triphone::sample_subset(0, 5, 1).is_empty());
    assert!(hmm::triphone::sample_subset(10, 0, 1).is_empty());
}

#[test]
fn provenance_is_bounded() {
    // bound_provenance keeps the first MAX ids, frames untouched.
    let mut p = Provenance {
        utterances: (0..50).map(|i| format!("utt:{i:03}")).collect(),
        frames: 1234,
    };
    bound_provenance(&mut p);
    assert_eq!(p.utterances.len(), MAX_PROV_UTTS);
    assert_eq!(p.frames, 1234);
    assert_eq!(p.utterances[0], "utt:000");
    // Under the cap: untouched, order preserved.
    let mut q = Provenance {
        utterances: vec!["utt:zzz".to_string(), "utt:aaa".to_string()],
        frames: 7,
    };
    bound_provenance(&mut q);
    assert_eq!(
        q.utterances,
        vec!["utt:zzz".to_string(), "utt:aaa".to_string()]
    );

    // merge_stats never grows a cell past the cap; frames still add up.
    let key = Triphone {
        left: Some("B".to_string()),
        center: "AE".to_string(),
        right: Some("T".to_string()),
    };
    let cell = |tag: &str| TriStateStats {
        key: key.clone(),
        state: 1,
        count: 1.0,
        sum: vec![0.0; 2],
        sum_sq: vec![0.0; 2],
        utterances: (0..6).map(|i| format!("{tag}:{i}")).collect(),
        frames: 6,
    };
    let mut pooled: BTreeMap<(Triphone, usize), TriStateStats> = BTreeMap::new();
    pooled.insert((key.clone(), 1), cell("a"));
    let mut one: BTreeMap<(Triphone, usize), TriStateStats> = BTreeMap::new();
    one.insert((key.clone(), 1), cell("b"));
    merge_stats(&mut pooled, one);
    let got = &pooled[&(key, 1)];
    assert_eq!(got.frames, 12);
    assert_eq!(got.utterances.len(), MAX_PROV_UTTS);

    // cap_model_provenance keeps validate_provenance green on a real model.
    let utterances = vec![vec![vec![0.0, 0.0]]];
    let mut model = HmmSet::flat_start(&["B"], &utterances, 0.01).unwrap();
    for h in model.phones.values_mut() {
        for mix in h.emissions.iter_mut() {
            for comp in mix.components.iter_mut() {
                comp.stats.provenance.utterances = (0..50).map(|i| format!("utt:{i:03}")).collect();
            }
        }
    }
    let (before, after) = cap_model_provenance(&mut model);
    assert!(before > after);
    assert!(after > 0);
    model.validate_provenance().unwrap();
}
