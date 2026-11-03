//! Budget table: arms A/B/C at totals {50, 100, 250, 500}.
//!
//! Per budget the table reports WER, S/D/I, corpus RTF, peak live/dormant
//! pool sizes, late rescues (words arm C commits that arm B had pruned),
//! and the paired-bootstrap delta-WER(C minus B) with its 95 pct CI.

use crate::arms::{Arm, ArmParams, run_fabric_arm};
use crate::fixtures::{self, Fixture};
use crate::scoring::{delta_ci, one_best_path, per_utterance_errors, score_corpus};
use eval::BootstrapCi;
use std::time::Instant;

/// Budget totals swept (contract §12).
pub const BUDGETS: [usize; 4] = [50, 100, 250, 500];

/// One arm's corpus totals at one budget.
#[derive(Debug, Clone)]
pub struct ArmCell {
    pub wer: f64,
    pub subs: usize,
    pub dels: usize,
    pub inss: usize,
    pub ref_len: usize,
    pub rtf: f64,
    /// Pool peaks (`None` for arm A, which is not a fabric run).
    pub peak_live: Option<usize>,
    pub peak_dormant: Option<usize>,
    pub peak_total: Option<usize>,
    pub hyps: Vec<Vec<String>>,
}

/// One budget row: all three arms plus the C-minus-B delta CI.
#[derive(Debug, Clone)]
pub struct BudgetRow {
    pub budget: usize,
    pub a: ArmCell,
    pub b: ArmCell,
    pub c: ArmCell,
    /// Late rescues: words C committed that B had pruned.
    pub rescues: usize,
    pub delta_c_minus_b: BootstrapCi,
}

/// The full A/B/C by budget table.
#[derive(Debug, Clone)]
pub struct BudgetTable {
    pub rows: Vec<BudgetRow>,
}

/// Run arms A/B/C at every budget over the given utterances.
pub fn run_budget_table(fxs: &[Fixture]) -> BudgetTable {
    let refs: Vec<Vec<String>> = fxs.iter().map(|f| f.reference.clone()).collect();
    let total_span: f64 = fxs
        .iter()
        .map(|f| {
            let frames = f
                .lattice
                .tokens
                .iter()
                .map(|t| t.end)
                .max()
                .unwrap_or(0)
                .saturating_sub(
                    f.lattice.tokens.iter().map(|t| t.start).min().unwrap_or(0),
                );
            frames as f64 * f.lattice.frame_shift_ms / 1000.0
        })
        .sum();
    let t0 = Instant::now();
    let a_hyps: Vec<Vec<String>> = fxs.iter().map(|f| one_best_path(&f.lattice)).collect();
    let a_elapsed = t0.elapsed().as_secs_f64();
    let a_tot = score_corpus(&refs, &a_hyps);
    let (_, denoms) = per_utterance_errors(&refs, &a_hyps);
    let a_cell = ArmCell {
        wer: a_tot.wer(),
        subs: a_tot.subs,
        dels: a_tot.dels,
        inss: a_tot.inss,
        ref_len: a_tot.ref_len,
        rtf: if total_span > 0.0 {
            a_elapsed / total_span
        } else {
            0.0
        },
        peak_live: None,
        peak_dormant: None,
        peak_total: None,
        hyps: a_hyps,
    };
    let mut rows = Vec::new();
    for &budget in &BUDGETS {
        let params = ArmParams::fixture_defaults(budget);
        let mut b_hyps = Vec::new();
        let mut c_hyps = Vec::new();
        let (mut bl, mut bd, mut bt, mut cl, mut cd, mut ct) =
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
        let (mut be, mut bs, mut ce, mut cs) = (0.0, 0.0, 0.0, 0.0);
        let mut rescues = 0usize;
        for f in fxs {
            let bo = run_fabric_arm(&f.lattice, Arm::B, &params, &fixtures::fixture_bigram)
                .expect("arm B run");
            let co = run_fabric_arm(&f.lattice, Arm::C, &params, &fixtures::fixture_bigram)
                .expect("arm C run");
            bl = bl.max(bo.peak_live);
            bd = bd.max(bo.peak_dormant);
            bt = bt.max(bo.peak_total);
            cl = cl.max(co.peak_live);
            cd = cd.max(co.peak_dormant);
            ct = ct.max(co.peak_total);
            be += bo.elapsed_sec;
            bs += bo.span_sec;
            ce += co.elapsed_sec;
            cs += co.span_sec;
            for w in &co.hypothesis {
                if bo.pruned.iter().any(|e| e == w) || bo.evicted.iter().any(|e| e == w) {
                    rescues += 1;
                }
            }
            b_hyps.push(bo.hypothesis);
            c_hyps.push(co.hypothesis);
        }
        let b_tot = score_corpus(&refs, &b_hyps);
        let c_tot = score_corpus(&refs, &c_hyps);
        let (b_errs, _) = per_utterance_errors(&refs, &b_hyps);
        let (c_errs, _) = per_utterance_errors(&refs, &c_hyps);
        rows.push(BudgetRow {
            budget,
            a: a_cell.clone(),
            b: ArmCell {
                wer: b_tot.wer(),
                subs: b_tot.subs,
                dels: b_tot.dels,
                inss: b_tot.inss,
                ref_len: b_tot.ref_len,
                rtf: if bs > 0.0 { be / bs } else { 0.0 },
                peak_live: Some(bl),
                peak_dormant: Some(bd),
                peak_total: Some(bt),
                hyps: b_hyps,
            },
            c: ArmCell {
                wer: c_tot.wer(),
                subs: c_tot.subs,
                dels: c_tot.dels,
                inss: c_tot.inss,
                ref_len: c_tot.ref_len,
                rtf: if cs > 0.0 { ce / cs } else { 0.0 },
                peak_live: Some(cl),
                peak_dormant: Some(cd),
                peak_total: Some(ct),
                hyps: c_hyps,
            },
            rescues,
            delta_c_minus_b: delta_ci(&b_errs, &c_errs, &denoms),
        });
    }
    BudgetTable { rows }
}

fn peaks(cell: &ArmCell) -> (String, String) {
    match (cell.peak_live, cell.peak_dormant) {
        (Some(l), Some(d)) => (l.to_string(), d.to_string()),
        _ => ("n/a".to_string(), "n/a".to_string()),
    }
}

/// Render the budget table as Markdown.
pub fn render_markdown(table: &BudgetTable) -> String {
    let mut s = String::from(
        "| budget | arm | WER | S | D | I | RTF | peak live | peak dormant | late rescues | delta-WER(C-B) + 95% CI |\n",
    );
    s.push_str(
        "|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for r in &table.rows {
        for (name, cell) in [("A", &r.a), ("B", &r.b), ("C", &r.c)] {
            let (pl, pd) = peaks(cell);
            let (rescues, delta) = if name == "C" {
                (
                    r.rescues.to_string(),
                    format!(
                        "{:+.4} [{:+.4}, {:+.4}]",
                        r.delta_c_minus_b.estimate,
                        r.delta_c_minus_b.lo,
                        r.delta_c_minus_b.hi
                    ),
                )
            } else {
                ("--".to_string(), "--".to_string())
            };
            s.push_str(&format!(
                "| {} | {} | {:.4} | {} | {} | {} | {:.4} | {} | {} | {} | {} |\n",
                r.budget, name, cell.wer, cell.subs, cell.dels, cell.inss, cell.rtf, pl, pd,
                rescues, delta
            ));
        }
    }
    s
}
