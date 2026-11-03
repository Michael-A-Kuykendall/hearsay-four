//! Experiment acceptance gates on fixtures (bead hearsay-four-ic1w).
//!
//! Fixture population: 4 utterances, 11 reference words. Expected totals:
//! arm A 2 errs (0.1818), arm B 2 errs (0.1818), arm C 1 err (0.0909),
//! oracle K=3 2 errs, oracle K>=5 1 err. Every WER is below 1.0.

use corpus::manifest::LibriRow;
use experiment::arms::{Arm, ArmParams, run_fabric_arm};
use experiment::fixtures::{
    all_fixtures, fixture_bigram, lattice_text, u1_revival, u2_clean, u3_bulk, u4_absent,
};
use experiment::oracle_sweep::{SWEEP_KS, render_sweep_markdown, sweep_corpus, sweep_utterance};
use experiment::sampling::{TEST200_PER_SPEAKER, TEST200_ROWS, sample_test200_rows};
use experiment::scoring::{delta_ci, one_best_path, per_utterance_errors, score_corpus};
use experiment::table::{BUDGETS, render_markdown, run_budget_table};

fn w(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn synth_manifest(speakers: usize, per_speaker: usize) -> Vec<LibriRow> {
    let mut rows = Vec::new();
    for s in 0..speakers {
        for u in 0..per_speaker {
            rows.push(LibriRow {
                id: format!("{:04}-0-{:04}", s, u),
                rel: format!("{s}/0/{s}-0-{u}.wav"),
                sec: 3.0,
                text: "stub transcript".to_string(),
            });
        }
    }
    rows
}

#[test]
fn sampling_test200_spec_holds() {
    let rows = synth_manifest(40, 6);
    let picked = sample_test200_rows(&rows).expect("TEST-200 sample");
    assert_eq!(picked.len(), TEST200_ROWS);
    let mut counts = std::collections::BTreeMap::new();
    for r in &picked {
        *counts
            .entry(r.id.split('-').next().unwrap_or("").to_string())
            .or_insert(0usize) += 1;
    }
    assert_eq!(counts.len(), 40);
    for (spk, n) in &counts {
        assert_eq!(*n, TEST200_PER_SPEAKER, "speaker {spk}");
    }
    // Deterministic: same rows, same seed, same pick.
    let again = sample_test200_rows(&rows).expect("repeat sample");
    assert_eq!(
        picked.iter().map(|r| &r.id).collect::<Vec<_>>(),
        again.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
}

#[test]
fn sampling_test200_rejects_short_manifest() {
    let rows = synth_manifest(10, 6);
    let err = sample_test200_rows(&rows).expect_err("must reject <200 rows");
    assert!(err.contains("50"), "error names the sampled count: {err}");
}

#[test]
fn oracle_sweep_fixture_gap() {
    let fxs = all_fixtures();
    let texts: Vec<String> = fxs.iter().map(|f| lattice_text(f).expect("write")).collect();
    // Round-trip: the sweep reader accepts fixture lattices.
    for t in &texts {
        lattice::read_lattice(t).expect("fixture lattice reads");
    }
    // U1: K=3 drops `cat` (rank 5 of 6); K>=5 keeps it.
    let u1 = u1_revival();
    let u1_text = lattice_text(&u1).expect("write u1");
    let rows = sweep_utterance(&u1_text, &u1.reference, &SWEEP_KS).expect("sweep u1");
    let errs: Vec<u32> = rows.iter().map(|(_, o, _, _)| o.errors).collect();
    assert_eq!(errs, vec![1, 0, 0], "u1 oracle errs at K=3/5/10");
    // Corpus: K=3 errs on u1+u4; K>=5 errs on u4 only.
    let items: Vec<(&str, &[String])> = texts
        .iter()
        .zip(fxs.iter())
        .map(|(t, f)| (t.as_str(), f.reference.as_slice()))
        .collect();
    let corpus = sweep_corpus(&items, &SWEEP_KS).expect("sweep corpus");
    let totals: Vec<(usize, u32, u32)> = corpus
        .iter()
        .map(|r| (r.k, r.errors, r.ref_len))
        .collect();
    assert_eq!(totals, vec![(3, 2, 11), (5, 1, 11), (10, 1, 11)]);
    for r in &corpus {
        assert!(r.wer() < 1.0, "K={} WER below 1.0", r.k);
    }
    // Oracle never exceeds 1-best errors per utterance (backbone property).
    for f in &fxs {
        let text = lattice_text(f).expect("write");
        let (_, oracle, _, _) = &sweep_utterance(&text, &f.reference, &[10])
            .expect("sweep")[0];
        let one = one_best_path(&f.lattice);
        let s = output::wer(&f.reference, &one);
        let one_errs = (s.substitutions + s.deletions + s.insertions) as u32;
        assert!(
            oracle.errors <= one_errs,
            "{}: oracle {} <= 1-best {one_errs}",
            f.id,
            oracle.errors
        );
    }
    println!("{}", render_sweep_markdown(&corpus));
}

#[test]
fn one_best_matches_fixture_expectations() {
    assert_eq!(
        one_best_path(&u1_revival().lattice),
        w(&["the", "bat", "sat", "down"])
    );
    assert_eq!(
        one_best_path(&u2_clean().lattice),
        w(&["up", "and", "away"])
    );
    assert_eq!(one_best_path(&u3_bulk().lattice), w(&["alpha"]));
    assert_eq!(
        one_best_path(&u4_absent().lattice),
        w(&["red", "green", "yellow"])
    );
    let empty = lattice::Lattice {
        utterance: "empty".to_string(),
        frame_shift_ms: 10.0,
        producer: "test".to_string(),
        tokens: Vec::new(),
        arcs: Vec::new(),
    };
    assert!(one_best_path(&empty).is_empty());
}

#[test]
fn arm_b_prunes_arm_c_revives_on_identical_inputs() {
    let u1 = u1_revival();
    let params = ArmParams::fixture_defaults(500);
    let b = run_fabric_arm(&u1.lattice, Arm::B, &params, &fixture_bigram).expect("arm B");
    let c = run_fabric_arm(&u1.lattice, Arm::C, &params, &fixture_bigram).expect("arm C");
    assert!(b.committed && c.committed);
    assert_eq!(b.hypothesis, w(&["the", "bat", "sat", "down"]));
    assert_eq!(c.hypothesis, w(&["the", "cat", "sat", "down"]));
    assert!(
        b.pruned.iter().any(|e| e == "cat"),
        "arm B prunes cat: {:?}",
        b.pruned
    );
    assert!(
        b.evicted.is_empty(),
        "arm B u1 has no budget evictions: {:?}",
        b.evicted
    );
    assert!(
        c.revivals.iter().any(|r| r == "cat"),
        "arm C revives cat: {:?}",
        c.revivals
    );
    assert!(
        !b.revivals.iter().any(|r| r == "cat"),
        "arm B never revives cat"
    );
}

#[test]
fn arms_agree_on_clean_bulk_absent() {
    for f in [u2_clean(), u3_bulk(), u4_absent()] {
        for &budget in &BUDGETS {
            let params = ArmParams::fixture_defaults(budget);
            let b =
                run_fabric_arm(&f.lattice, Arm::B, &params, &fixture_bigram).expect("arm B");
            let c =
                run_fabric_arm(&f.lattice, Arm::C, &params, &fixture_bigram).expect("arm C");
            assert!(b.committed && c.committed, "{} @ {budget}", f.id);
            assert_eq!(b.hypothesis, c.hypothesis, "{} @ {budget}", f.id);
        }
    }
    assert_eq!(
        run_fabric_arm(
            &u2_clean().lattice,
            Arm::C,
            &ArmParams::fixture_defaults(50),
            &fixture_bigram
        )
        .expect("c")
        .hypothesis,
        w(&["up", "and", "away"])
    );
    assert_eq!(
        run_fabric_arm(
            &u4_absent().lattice,
            Arm::C,
            &ArmParams::fixture_defaults(50),
            &fixture_bigram
        )
        .expect("c")
        .hypothesis,
        w(&["red", "green", "yellow"])
    );
}

#[test]
fn empty_lattice_commits_nothing_without_panic() {
    let empty = lattice::Lattice {
        utterance: "empty".to_string(),
        frame_shift_ms: 10.0,
        producer: "test".to_string(),
        tokens: Vec::new(),
        arcs: Vec::new(),
    };
    for arm in [Arm::B, Arm::C] {
        let o = run_fabric_arm(&empty, arm, &ArmParams::fixture_defaults(50), &fixture_bigram)
            .expect("empty run");
        assert!(!o.committed);
        assert!(o.hypothesis.is_empty());
    }
}

#[test]
fn budget_table_fixtures() {
    let fxs = all_fixtures();
    let table = run_budget_table(&fxs);
    assert_eq!(table.rows.len(), BUDGETS.len());
    for r in &table.rows {
        assert!((r.a.wer - 2.0 / 11.0).abs() < 1e-12, "A WER @ {}", r.budget);
        assert!((r.b.wer - 2.0 / 11.0).abs() < 1e-12, "B WER @ {}", r.budget);
        assert!((r.c.wer - 1.0 / 11.0).abs() < 1e-12, "C WER @ {}", r.budget);
        assert!(r.a.wer < 1.0 && r.b.wer < 1.0 && r.c.wer < 1.0);
        assert_eq!((r.b.subs, r.b.dels, r.b.inss), (2, 0, 0));
        assert_eq!((r.c.subs, r.c.dels, r.c.inss), (1, 0, 0));
        assert_eq!(r.rescues, 1, "late rescues @ {}", r.budget);
        assert!(
            (r.delta_c_minus_b.estimate - (-1.0 / 11.0)).abs() < 1e-12,
            "delta @ {}",
            r.budget
        );
        assert!(
            r.delta_c_minus_b.lo <= r.delta_c_minus_b.estimate
                && r.delta_c_minus_b.estimate <= r.delta_c_minus_b.hi,
            "CI brackets estimate @ {}",
            r.budget
        );
        let bl = r.b.peak_live.expect("B peak live");
        let bt = r.b.peak_total.expect("B peak total");
        let cl = r.c.peak_live.expect("C peak live");
        let ct = r.c.peak_total.expect("C peak total");
        assert!(bl <= r.budget, "B live peak within budget");
        assert!(bt <= r.budget, "B total peak within budget");
        assert!(cl <= r.budget, "C live peak within budget");
        assert!(ct <= r.budget, "C live+dormant within total");
    }
    // Bulk fixture binds the small budgets: peaks track the caps.
    let small = &table.rows[0];
    assert_eq!(small.budget, 50);
    assert_eq!(small.b.peak_live, Some(50));
    assert_eq!(small.c.peak_live, Some(50));
    println!("{}", render_markdown(&table));
}

#[test]
fn delta_ci_rejects_mismatched_lengths() {
    let errs = per_utterance_errors(&[w(&["a", "b"])], &[w(&["a", "c"])]);
    assert_eq!(errs, (vec![1], vec![2]));
    let ci = delta_ci(&[1, 0], &[0, 0], &[2, 2]);
    assert!((ci.estimate - (-0.25)).abs() < 1e-12);
    let totals = score_corpus(&[w(&["a", "b"])], &[w(&["a", "c"])]);
    assert_eq!(totals.subs, 1);
    assert!((totals.wer() - 0.5).abs() < 1e-12);
}
