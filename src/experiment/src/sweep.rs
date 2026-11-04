//! DEV tuning and TEST sweep drivers for arms B/C (synthetic dry run).
//!
//! Pipeline (bead `hearsay-four-fmyc--sweep-drivers-scorer`): tune arm B on a
//! synthetic DEV set (arm C reuses the identical params), sweep synthetic
//! TEST-200 over budgets x arms {B, C} through the real fabric bridge,
//! score paired-bootstrap delta-WER, and render the fixed-schema TSV.
//!
//! The ONLY real-data swap point is [`load_experiment_feed`]: implement its
//! `Real` branch (manifest lattices plus the real language model) after the
//! P0+bridge merge and every downstream consumer — tuning, sweep, scoring,
//! TSV — runs unchanged.

use crate::arms::{Arm, ArmParams, run_fabric_arm};
use crate::scoring::{delta_ci, per_utterance_errors, score_corpus};
use eval::BootstrapCi;
use hearsay::SequenceScorer;
use lattice::{Arc, Lattice, Token};
use std::path::PathBuf;

/// One sweep utterance: lattice plus reference transcript.
#[derive(Debug, Clone)]
pub struct SweepUtt {
    pub id: String,
    pub lattice: Lattice,
    pub reference: Vec<String>,
    /// Arm-A 1-best baseline from the `wer_devclean` cell TSV (`None` on
    /// synthetic feeds, which have no decoder baseline).
    pub arm_a: Option<ArmABaseline>,
}

/// Arm-A baseline for one utterance: decoder 1-best hypothesis plus the
/// cell's own audio/decode seconds (arm-A RTF needs no fabric run).
#[derive(Debug, Clone)]
pub struct ArmABaseline {
    pub hyp: Vec<String>,
    pub audio_sec: f64,
    pub decode_sec: f64,
}

/// Language-model source behind a feed.
#[derive(Debug, Clone)]
pub enum FeedLm {
    /// Deterministic bigram over the synthetic vocabulary.
    Synthetic,
    /// Real interpolated KN LM (`lm-r1v2-interpolated.json`), boxed: the
    /// 72 MB model must not ride the enum by value (clippy
    /// `large_enum_variant`).
    Real(Box<lm::InterpolatedLm>),
}

impl FeedLm {
    /// Score `llr_SEQ(a -> b)` for word labels (`"<s>"` = utterance start).
    /// The real variant delegates to [`hearsay::InterpolatedScorer`]
    /// (bigram-over-unigram LLR), exactly as `real_table` does.
    pub fn score(&self, a: &str, b: &str) -> f64 {
        match self {
            FeedLm::Synthetic => synthetic_bigram(a, b),
            FeedLm::Real(lm) => {
                let scorer = hearsay::InterpolatedScorer::new(lm);
                scorer.sequence_llr(if a == "<s>" { None } else { Some(a) }, b)
            }
        }
    }
}

/// Utterance source for a tuning/sweep run.
#[derive(Debug, Clone)]
pub enum FeedSource {
    /// Deterministic synthetic lattices (dry run; no audio needed).
    Synthetic {
        dev_n: usize,
        test_n: usize,
        seed: u64,
    },
    /// Real DEV/TEST manifests plus a lattice directory, the interpolated
    /// LM, and the `wer_devclean` arm-A cell TSVs; see
    /// [`load_experiment_feed`].
    Real {
        dev_manifest: PathBuf,
        test_manifest: PathBuf,
        lat_dir: PathBuf,
        lm_path: PathBuf,
        dev_cell: PathBuf,
        test_cell: PathBuf,
    },
}

/// DEV + TEST utterances plus the scoring LM.
#[derive(Debug, Clone)]
pub struct ExperimentFeed {
    pub dev: Vec<SweepUtt>,
    pub test: Vec<SweepUtt>,
    pub lm: FeedLm,
}

/// THE SEAM: the single function that swaps the synthetic dry-run feed for
/// real data. `Synthetic` builds deterministic lattices plus
/// [`synthetic_bigram`]; `Real` reads the two 4-column manifest TSVs
/// (`id`, `rel`, `seconds`, `transcript`), loads `{id}.lat.jsonl` per row
/// through [`lattice::read_lattice`], attaches the real interpolated LM,
/// and joins the arm-A cell TSVs row-for-row (same order as the manifest).
/// Falsification gate: manifest ids must be Librispeech-shaped
/// (`<spk>-<chap>-<utt>`, all digits) — synthetic-shaped rows fail loudly.
pub fn load_experiment_feed(src: &FeedSource) -> Result<ExperimentFeed, String> {
    match src {
        FeedSource::Synthetic {
            dev_n,
            test_n,
            seed,
        } => Ok(ExperimentFeed {
            dev: synthetic_corpus(*dev_n, seed ^ 0x9e37_79b9_7f4a_7c15, "dev"),
            test: synthetic_corpus(*test_n, *seed, "test"),
            lm: FeedLm::Synthetic,
        }),
        FeedSource::Real {
            dev_manifest,
            test_manifest,
            lat_dir,
            lm_path,
            dev_cell,
            test_cell,
        } => {
            let lm = lm::persist::load_interpolated(lm_path)
                .map_err(|e| format!("real feed: LM {}: {e}", lm_path.display()))?;
            let dev = load_real_set(dev_manifest, dev_cell, lat_dir, "dev")?;
            let test = load_real_set(test_manifest, test_cell, lat_dir, "test")?;
            Ok(ExperimentFeed {
                dev,
                test,
                lm: FeedLm::Real(Box::new(lm)),
            })
        }
    }
}
/// Librispeech utterance id shape: three dash-separated all-digit groups
/// (`1272-128104-0001`). Synthetic ids (`dev-0000`) fail this check, so a
/// real feed built over synthetic-shaped rows errors instead of silently
/// scoring the wrong population.
fn is_librispeech_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 3
        && !parts[0].is_empty()
        && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
}
/// Load one real set: manifest rows joined row-for-row to the arm-A cell
/// TSV (same order required) plus `{id}.lat.jsonl` per row.
fn load_real_set(
    manifest: &std::path::Path,
    cell: &std::path::Path,
    lat_dir: &std::path::Path,
    name: &str,
) -> Result<Vec<SweepUtt>, String> {
    let text = std::fs::read_to_string(manifest)
        .map_err(|e| format!("real feed {name}: manifest {}: {e}", manifest.display()))?;
    let mut ids: Vec<String> = Vec::new();
    let mut refs: Vec<Vec<String>> = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(id), Some(_), Some(_), Some(t)) => {
                if !is_librispeech_id(id) {
                    return Err(format!(
                        "real feed {name}: manifest line {} id {id:?} is not Librispeech-shaped",
                        lineno + 1
                    ));
                }
                ids.push(id.to_string());
                refs.push(t.split_whitespace().map(str::to_string).collect());
            }
            _ => {
                return Err(format!(
                    "real feed {name}: manifest line {} malformed",
                    lineno + 1
                ));
            }
        }
    }
    if ids.is_empty() {
        return Err(format!("real feed {name}: empty manifest"));
    }
    let cell_text = std::fs::read_to_string(cell)
        .map_err(|e| format!("real feed {name}: cell {}: {e}", cell.display()))?;
    let mut baselines: Vec<ArmABaseline> = Vec::new();
    for (lineno, line) in cell_text.lines().enumerate() {
        if lineno == 0 {
            if !line.starts_with("utt\terrors") {
                return Err(format!("real feed {name}: cell header: {line}"));
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 7 {
            return Err(format!(
                "real feed {name}: cell line {} has {} cols, want 7",
                lineno + 1,
                cols.len()
            ));
        }
        if baselines.len() >= ids.len() || cols[0] != ids[baselines.len()] {
            let want = ids
                .get(baselines.len())
                .map(String::as_str)
                .unwrap_or("(none)");
            return Err(format!(
                "real feed {name}: cell row {} id {:?} != manifest {:?} (join order)",
                lineno + 1,
                cols[0],
                want
            ));
        }
        let audio_sec: f64 = cols[4]
            .parse()
            .map_err(|_| format!("real feed {name}: cell line {} bad audio_sec", lineno + 1))?;
        let decode_sec: f64 = cols[5]
            .parse()
            .map_err(|_| format!("real feed {name}: cell line {} bad decode_sec", lineno + 1))?;
        baselines.push(ArmABaseline {
            hyp: cols[6].split_whitespace().map(str::to_string).collect(),
            audio_sec,
            decode_sec,
        });
    }
    if baselines.len() != ids.len() {
        return Err(format!(
            "real feed {name}: cell covers {} utts, manifest has {}",
            baselines.len(),
            ids.len()
        ));
    }
    let mut out = Vec::with_capacity(ids.len());
    for ((id, reference), arm_a) in ids.into_iter().zip(refs).zip(baselines) {
        let p = lat_dir.join(format!("{id}.lat.jsonl"));
        let lt = std::fs::read_to_string(&p)
            .map_err(|e| format!("real feed {name}: lattice {}: {e}", p.display()))?;
        let lattice =
            lattice::read_lattice(&lt).map_err(|e| format!("real feed {name}: {id}: {e}"))?;
        out.push(SweepUtt {
            id,
            lattice,
            reference,
            arm_a: Some(arm_a),
        });
    }
    Ok(out)
}

/// Minimal splitmix64 stream (dependency-free deterministic RNG, mirroring
/// the style of `eval`'s resampling stream).
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        debug_assert!(n >= 1);
        (self.next_u64() % n as u64) as usize
    }

    fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (f64::from((self.next_u64() >> 11) as u32) / f64::from(u32::MAX))
    }
}

/// One revival template: `carrier correct right1 right2` with competing
/// same-span distractors for the `correct` slot (mirrors `u1-revival`).
struct Pattern {
    carrier: &'static str,
    correct: &'static str,
    right1: &'static str,
    right2: &'static str,
    distractors: &'static [&'static str],
}

const PATTERNS: [Pattern; 3] = [
    Pattern {
        carrier: "the",
        correct: "cat",
        right1: "sat",
        right2: "down",
        distractors: &["bat", "hat", "mat", "rat", "cab", "fog", "log", "cap"],
    },
    Pattern {
        carrier: "a",
        correct: "dog",
        right1: "ran",
        right2: "fast",
        distractors: &["hog", "fog", "log", "cog", "dot", "dig", "dug"],
    },
    Pattern {
        carrier: "they",
        correct: "sang",
        right1: "all",
        right2: "night",
        distractors: &["rang", "bang", "hang", "fang", "sank"],
    },
];

/// Clean carrier sentences (single backbone path; every arm commits them).
const CLEAN_SENTS: [&[&str]; 3] = [
    &["up", "and", "away"],
    &["sun", "and", "moon"],
    &["go", "far", "away"],
];

/// Extra same-span tokens in bulk utterances: 57..=64 plus the backbone, so
/// the live pool binds budget 50 and exercises budget evictions.
const BULK_EXTRA_MIN: usize = 57;
const BULK_EXTRA_SPAN: usize = 8;

/// Deterministic bigram over the synthetic vocabulary: rewards the correct
/// continuations, penalizes distractors after the carrier (so arm B prunes
/// the correct slot word and arm C can revive it), defaults to -1.0.
pub fn synthetic_bigram(a: &str, b: &str) -> f64 {
    for p in &PATTERNS {
        if a == "<s>" && b == p.carrier {
            return 1.0;
        }
        if a == p.carrier && b == p.correct {
            return -1.5;
        }
        if a == p.carrier && p.distractors.contains(&b) {
            return -5.0;
        }
        if a == p.correct && b == p.right1 {
            return 4.0;
        }
        if a == p.right1 && b == p.right2 {
            return 2.0;
        }
    }
    for s in &CLEAN_SENTS {
        if a == "<s>" && b == s[0] {
            return 1.0;
        }
        if a == s[0] && b == s[1] {
            return 2.0;
        }
        if a == s[1] && b == s[2] {
            return 2.0;
        }
    }
    // Absent template: the lattice path exists; the model need not punish it.
    if (a, b) == ("<s>", "red") || (a, b) == ("red", "green") || (a, b) == ("green", "yellow") {
        return 1.0;
    }
    -1.0
}

fn tok(id: u64, word: &str, start: u32, end: u32, source: &str, acoustic_llr: f64) -> Token {
    Token {
        id,
        word: word.to_string(),
        start,
        end,
        source: source.to_string(),
        acoustic_llr,
        lexical_llr: None,
        speaker_llr: None,
        verify_llr: None,
        user_llr: None,
    }
}

fn lat(utterance: &str, tokens: Vec<Token>, arcs: Vec<(u64, u64)>) -> Lattice {
    Lattice {
        utterance: utterance.to_string(),
        frame_shift_ms: 10.0,
        producer: "experiment-synthetic".to_string(),
        tokens,
        arcs: arcs
            .into_iter()
            .map(|(from, to)| Arc { from, to })
            .collect(),
    }
}

fn words(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// Revival utterance: the correct slot word has low acoustics but strong
/// right context; 2..=pool distractors have higher acoustics.
fn synth_revival(rng: &mut Rng, id: &str, pat: &Pattern) -> SweepUtt {
    let n_dist = 2 + rng.below(pat.distractors.len() - 1);
    let rot = rng.below(pat.distractors.len());
    let mut tokens = vec![tok(1, pat.carrier, 0, 10, "synth-1best", 5.0)];
    tokens.push(tok(
        2,
        pat.correct,
        10,
        20,
        "synth-alt",
        rng.range_f64(0.5, 1.5),
    ));
    tokens.push(tok(100, pat.right1, 20, 30, "synth-1best", 5.0));
    tokens.push(tok(101, pat.right2, 30, 40, "synth-1best", 5.0));
    let mut arcs = vec![(1, 2), (2, 100), (100, 101)];
    for k in 0..n_dist {
        let d = pat.distractors[(rot + k) % pat.distractors.len()];
        let tid = 3 + k as u64;
        tokens.push(tok(tid, d, 10, 20, "synth-alt", rng.range_f64(2.0, 4.5)));
        arcs.push((1, tid));
        arcs.push((tid, 100));
    }
    SweepUtt {
        id: id.to_string(),
        lattice: lat(id, tokens, arcs),
        reference: words(&[pat.carrier, pat.correct, pat.right1, pat.right2]),
        arm_a: None,
    }
}

/// Clean utterance: single backbone path, no alternatives.
fn synth_clean(id: &str, sent: &[&str]) -> SweepUtt {
    let tokens: Vec<Token> = sent
        .iter()
        .enumerate()
        .map(|(k, w)| {
            tok(
                k as u64 + 1,
                w,
                k as u32 * 10,
                k as u32 * 10 + 10,
                "synth-1best",
                5.0,
            )
        })
        .collect();
    let arcs: Vec<(u64, u64)> = (1..sent.len() as u64).map(|k| (k, k + 1)).collect();
    SweepUtt {
        id: id.to_string(),
        lattice: lat(id, tokens, arcs),
        reference: words(sent),
        arm_a: None,
    }
}

/// Absent utterance: the lattice commits `yellow`; the reference wants
/// `blue` (generation miss — every arm errs).
fn synth_absent(id: &str) -> SweepUtt {
    let tokens = vec![
        tok(1, "red", 0, 10, "synth-1best", 5.0),
        tok(2, "green", 10, 20, "synth-1best", 5.0),
        tok(3, "yellow", 20, 30, "synth-1best", 4.0),
    ];
    SweepUtt {
        id: id.to_string(),
        lattice: lat(id, tokens, vec![(1, 2), (2, 3)]),
        reference: words(&["red", "green", "blue"]),
        arm_a: None,
    }
}

/// Bulk utterance: backbone plus 57..=64 same-span competitors, no arcs
/// (mirrors `u3-bulk`): every arm commits the backbone, pools bind.
fn synth_bulk(rng: &mut Rng, id: &str) -> SweepUtt {
    let mut tokens = vec![tok(1, "alpha", 0, 10, "synth-1best", 5.0)];
    let n = BULK_EXTRA_MIN + rng.below(BULK_EXTRA_SPAN);
    for i in 0..n {
        tokens.push(tok(
            2 + i as u64,
            &format!("w{:03}", i + 1),
            0,
            10,
            "synth-alt",
            2.0,
        ));
    }
    SweepUtt {
        id: id.to_string(),
        lattice: lat(id, tokens, Vec::new()),
        reference: words(&["alpha"]),
        arm_a: None,
    }
}

/// Build `n` deterministic synthetic utterances (`prefix` = `"dev"`/`"test"`).
/// Mix per index decade: 40% revival, 30% clean, 20% absent, 10% bulk.
pub fn synthetic_corpus(n: usize, seed: u64, prefix: &str) -> Vec<SweepUtt> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let id = format!("{prefix}-{i:04}");
        let u = match i % 10 {
            0..=3 => synth_revival(&mut rng, &id, &PATTERNS[i % PATTERNS.len()]),
            4..=6 => synth_clean(&id, CLEAN_SENTS[i % CLEAN_SENTS.len()]),
            7..=8 => synth_absent(&id),
            _ => synth_bulk(&mut rng, &id),
        };
        out.push(u);
    }
    out
}

/// DEV grid over arm-B tunables (arm C reuses the winner identically).
#[derive(Debug, Clone)]
pub struct TuningGrid {
    pub w_acoustic: Vec<f64>,
    pub w_sequence: Vec<f64>,
    pub theta_live: Vec<f64>,
    pub delta: Vec<f64>,
}

impl Default for TuningGrid {
    fn default() -> Self {
        Self {
            w_acoustic: vec![0.5, 1.0, 2.0],
            w_sequence: vec![1.0],
            theta_live: vec![-1.0, 0.0, 1.0],
            delta: vec![0.0, 1.0],
        }
    }
}

impl TuningGrid {
    /// Coarse real-data grid: `theta_live` x `w_sequence` x `delta` (18
    /// candidates), `w_acoustic` pinned at the fixture default.
    pub fn real_coarse() -> Self {
        Self {
            w_acoustic: vec![1.0],
            w_sequence: vec![0.5, 1.0, 2.0],
            theta_live: vec![-1.0, 0.0, 1.0],
            delta: vec![0.0, 1.0],
        }
    }

    /// Grid candidates in evaluation order (outer `w_acoustic`, inner
    /// `delta`). `h`/`h_max` stay at [`ArmParams::fixture_defaults`].
    pub fn candidates(&self, budget: usize) -> Vec<ArmParams> {
        let mut out = Vec::new();
        for &wa in &self.w_acoustic {
            for &ws in &self.w_sequence {
                for &t in &self.theta_live {
                    for &d in &self.delta {
                        let mut p = ArmParams::fixture_defaults(budget);
                        p.w_acoustic = wa;
                        p.w_sequence = ws;
                        p.theta_live = t;
                        p.delta = d;
                        out.push(p);
                    }
                }
            }
        }
        out
    }
}

/// Winner of DEV tuning plus its DEV error totals and full scoreboard.
#[derive(Debug, Clone)]
pub struct TuningResult {
    /// Winning tunables; `budget` is unset (0) — the sweep sets it per row.
    pub params: ArmParams,
    /// Winner DEV errors summed over the tuning budgets.
    pub dev_errors: u32,
    /// DEV reference length summed over the tuning budgets.
    pub dev_ref_len: u32,
    pub candidates: usize,
    /// `(candidate, total DEV errors)` in grid order; `None` = decode failed.
    pub scoreboard: Vec<(ArmParams, Option<u32>)>,
}

/// Tune arm B on DEV: each candidate's total DEV errors summed over
/// `budgets`; lowest wins, ties resolve to the earliest grid candidate
/// (deterministic). Candidates that fail to decode score `None` and are
/// skipped; all-failing is `Err`. Candidates evaluate in parallel.
pub fn tune_arm_b(
    dev: &[SweepUtt],
    lm: &FeedLm,
    budgets: &[usize],
    grid: &TuningGrid,
) -> Result<TuningResult, String> {
    let cands = grid.candidates(0);
    if cands.is_empty() || dev.is_empty() || budgets.is_empty() {
        return Err("tune_arm_b needs a non-empty grid, DEV set, and budget list".to_string());
    }
    let ref_len: u32 =
        dev.iter().map(|u| u.reference.len() as u32).sum::<u32>() * budgets.len() as u32;
    let scored: Vec<Option<u32>> = par_map(&cands, &|p: &ArmParams| {
        let mut errs = 0u32;
        for &budget in budgets {
            let mut q = p.clone();
            q.budget = budget;
            let lm_fn = |a: &str, b: &str| lm.score(a, b);
            for u in dev {
                match run_fabric_arm(&u.lattice, Arm::B, &q, &lm_fn) {
                    Ok(o) => {
                        let s = output::wer(&u.reference, &o.hypothesis);
                        errs += (s.substitutions + s.deletions + s.insertions) as u32;
                    }
                    Err(_) => return None,
                }
            }
        }
        Some(errs)
    });
    let mut best: Option<(usize, u32)> = None;
    for (k, errs) in scored.iter().enumerate() {
        if let Some(e) = errs
            && best.is_none_or(|(_, b)| *e < b)
        {
            best = Some((k, *e));
        }
    }
    let (k, e) = best.ok_or_else(|| "tune_arm_b: every candidate failed to decode".to_string())?;
    let scoreboard: Vec<(ArmParams, Option<u32>)> = cands.into_iter().zip(scored).collect();
    Ok(TuningResult {
        params: scoreboard[k].0.clone(),
        dev_errors: e,
        dev_ref_len: ref_len,
        candidates: scoreboard.len(),
        scoreboard,
    })
}

/// One (budget, arm) cell of the TEST sweep.
#[derive(Debug, Clone)]
pub struct SweepCell {
    pub budget: usize,
    pub arm: Arm,
    pub wer: f64,
    pub subs: usize,
    pub dels: usize,
    pub inss: usize,
    pub ref_len: usize,
    pub rtf: f64,
    /// Fabric pool peaks (`None` on arm-A rows: the decoder baseline never
    /// touches the fabric; rendered `n/a`).
    pub peak_live: Option<usize>,
    pub peak_dormant: Option<usize>,
    /// Pair-level (C-vs-B) stats at this budget: populated on C rows,
    /// `None` on B rows (rendered `NA`).
    pub rescues: Option<usize>,
    pub delta: Option<BootstrapCi>,
}

/// Late-rescue counter: committed words the other arm had pruned or evicted.
/// Multiset: a twice-committed rescued word counts twice — the same
/// convention as [`crate::table::run_budget_table`].
pub fn count_late_rescues(committed: &[String], pruned: &[String], evicted: &[String]) -> usize {
    committed
        .iter()
        .filter(|w| pruned.contains(*w) || evicted.contains(*w))
        .count()
}

/// Per-utterance decode record kept for cell assembly. Public so the
/// 16-worker queue (`cell_decode` shards + `sweep_collect`) assembles
/// identical cells from per-task files.
#[derive(Debug, Clone)]
pub struct CellRun {
    pub hyp: Vec<String>,
    pub pruned: Vec<String>,
    pub evicted: Vec<String>,
    pub peak_live: usize,
    pub peak_dormant: usize,
    pub elapsed: f64,
    pub span: f64,
}

/// Sweep TEST over `budgets` x arms {B, C} with the DEV-tuned params (C uses
/// params identical to B), plus an arm-A baseline row per budget when every
/// TEST utt carries one (real feeds). One task per (budget, arm,
/// utterance), decoded in parallel and assembled in row order
/// (budget-major, A then B then C). Any decode failure aborts the sweep
/// with the utterance id.
pub fn sweep_test(
    test: &[SweepUtt],
    lm: &FeedLm,
    tuned: &ArmParams,
    budgets: &[usize],
) -> Result<Vec<SweepCell>, String> {
    if test.is_empty() || budgets.is_empty() {
        return Err("sweep_test needs a non-empty TEST set and budget list".to_string());
    }
    let refs: Vec<Vec<String>> = test.iter().map(|u| u.reference.clone()).collect();
    let denoms: Vec<u32> = refs.iter().map(|r| r.len() as u32).collect();
    let mut meta: Vec<(usize, Arm, ArmParams)> = Vec::new();
    for &budget in budgets {
        for arm in [Arm::B, Arm::C] {
            let mut p = tuned.clone();
            p.budget = budget;
            meta.push((budget, arm, p));
        }
    }
    struct Task {
        cell: usize,
        utt: usize,
    }
    let mut tasks = Vec::new();
    for (cell, _) in meta.iter().enumerate() {
        for (utt, _) in test.iter().enumerate() {
            tasks.push(Task { cell, utt });
        }
    }
    let results: Vec<Result<CellRun, String>> = par_map(&tasks, &|t: &Task| {
        let (budget, arm, params) = &meta[t.cell];
        let u = &test[t.utt];
        let lm_fn = |a: &str, b: &str| lm.score(a, b);
        match run_fabric_arm(&u.lattice, *arm, params, &lm_fn) {
            Ok(o) => Ok(CellRun {
                hyp: o.hypothesis,
                pruned: o.pruned,
                evicted: o.evicted,
                peak_live: o.peak_live,
                peak_dormant: o.peak_dormant,
                elapsed: o.elapsed_sec,
                span: o.span_sec,
            }),
            Err(e) => Err(format!("{} arm {arm:?} budget {budget}: {e}", u.id)),
        }
    });
    let mut per_cell: Vec<Vec<CellRun>> = (0..meta.len()).map(|_| Vec::new()).collect();
    for (t, r) in tasks.iter().zip(results) {
        per_cell[t.cell].push(r?);
    }
    // Arm-A baseline rows (real feeds only: every utt carries a baseline).
    let arm_a: Option<Vec<ArmABaseline>> = test
        .iter()
        .map(|u| u.arm_a.clone())
        .collect::<Option<Vec<_>>>();
    let mut out = Vec::with_capacity(meta.len() + budgets.len());
    for (k, &budget) in budgets.iter().enumerate() {
        if let Some(base) = &arm_a {
            out.push(assemble_arm_a_cell(budget, &refs, base));
        }
        let b_runs = &per_cell[2 * k];
        let c_runs = &per_cell[2 * k + 1];
        let b_hyps: Vec<Vec<String>> = b_runs.iter().map(|r| r.hyp.clone()).collect();
        let c_hyps: Vec<Vec<String>> = c_runs.iter().map(|r| r.hyp.clone()).collect();
        let b_tot = score_corpus(&refs, &b_hyps);
        let c_tot = score_corpus(&refs, &c_hyps);
        let (b_errs, _) = per_utterance_errors(&refs, &b_hyps);
        let (c_errs, _) = per_utterance_errors(&refs, &c_hyps);
        let rescues: usize = c_runs
            .iter()
            .zip(b_runs.iter())
            .map(|(c, b)| count_late_rescues(&c.hyp, &b.pruned, &b.evicted))
            .sum();
        let ci = delta_ci(&b_errs, &c_errs, &denoms);
        out.push(assemble_cell(budget, Arm::B, &b_tot, b_runs, None, None));
        out.push(assemble_cell(
            budget,
            Arm::C,
            &c_tot,
            c_runs,
            Some(rescues),
            Some(ci),
        ));
    }
    Ok(out)
}

/// Assemble one B/C cell from per-utterance runs (shared by the
/// in-process sweep and the queue collector: one scorer, one schema).
pub fn assemble_cell(
    budget: usize,
    arm: Arm,
    tot: &crate::scoring::WerTotals,
    runs: &[CellRun],
    rescues: Option<usize>,
    delta: Option<BootstrapCi>,
) -> SweepCell {
    let elapsed: f64 = runs.iter().map(|r| r.elapsed).sum();
    let span: f64 = runs.iter().map(|r| r.span).sum();
    SweepCell {
        budget,
        arm,
        wer: tot.wer(),
        subs: tot.subs,
        dels: tot.dels,
        inss: tot.inss,
        ref_len: tot.ref_len,
        rtf: if span > 0.0 { elapsed / span } else { 0.0 },
        peak_live: Some(runs.iter().map(|r| r.peak_live).max().unwrap_or(0)),
        peak_dormant: Some(runs.iter().map(|r| r.peak_dormant).max().unwrap_or(0)),
        rescues,
        delta,
    }
}

/// Assemble the arm-A baseline cell at one budget (identical WER at every
/// budget: the decoder baseline is budget-free). Peaks are `None`.
pub fn assemble_arm_a_cell(
    budget: usize,
    refs: &[Vec<String>],
    base: &[ArmABaseline],
) -> SweepCell {
    let hyps: Vec<Vec<String>> = base.iter().map(|b| b.hyp.clone()).collect();
    let tot = score_corpus(refs, &hyps);
    let audio: f64 = base.iter().map(|b| b.audio_sec).sum();
    let decode: f64 = base.iter().map(|b| b.decode_sec).sum();
    SweepCell {
        budget,
        arm: Arm::A,
        wer: tot.wer(),
        subs: tot.subs,
        dels: tot.dels,
        inss: tot.inss,
        ref_len: tot.ref_len,
        rtf: if audio > 0.0 { decode / audio } else { 0.0 },
        peak_live: None,
        peak_dormant: None,
        rescues: None,
        delta: None,
    }
}

/// Fixed TSV schema (the run's falsification gate: any missing column fails).
pub const SWEEP_TSV_HEADER: &str =
    "budget\tarm\tWER\tS\tD\tI\tRTF\tpeak_live\tpeak_dormant\tlate_rescues\tdWER\tCI_lo\tCI_hi";

fn arm_name(arm: Arm) -> &'static str {
    match arm {
        Arm::A => "A",
        Arm::B => "B",
        Arm::C => "C",
    }
}

/// Render sweep cells as TSV under [`SWEEP_TSV_HEADER`]. Pair-level columns
/// (`late_rescues`, `dWER`, `CI_lo`, `CI_hi`) render on C rows; A/B rows
/// show `NA` (the delta is defined C-minus-B at the row's budget). Peaks
/// render `n/a` on arm-A rows (no fabric run).
pub fn render_sweep_tsv(cells: &[SweepCell]) -> String {
    let mut s = String::from(SWEEP_TSV_HEADER);
    s.push('\n');
    for c in cells {
        let (resc, dw, lo, hi) = match (c.rescues, c.delta) {
            (Some(r), Some(d)) => (
                r.to_string(),
                format!("{:.4}", d.estimate),
                format!("{:.4}", d.lo),
                format!("{:.4}", d.hi),
            ),
            _ => (
                "NA".to_string(),
                "NA".to_string(),
                "NA".to_string(),
                "NA".to_string(),
            ),
        };
        let pl = c
            .peak_live
            .map(|p| p.to_string())
            .unwrap_or("n/a".to_string());
        let pd = c
            .peak_dormant
            .map(|p| p.to_string())
            .unwrap_or("n/a".to_string());
        s.push_str(&format!(
            "{}\t{}\t{:.4}\t{}\t{}\t{}\t{:.4}\t{pl}\t{pd}\t{resc}\t{dw}\t{lo}\t{hi}\n",
            c.budget,
            arm_name(c.arm),
            c.wer,
            c.subs,
            c.dels,
            c.inss,
            c.rtf,
        ));
    }
    s
}

/// Scorer self-checks (paired-bootstrap contract):
/// (a) identical transcripts give delta exactly 0 with a CI covering 0;
/// (b) swapped arm labels flip the sign — estimate exactly, CI endpoints to
/// 1e-9 (quantile interpolation order differs under negation, so the
/// endpoints agree up to float rounding, not bit-identically).
/// Returns a human-readable report on success, `Err(reason)` on failure.
pub fn selfcheck_scorer() -> Result<String, String> {
    let r = |ws: &[&str]| ws.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let refs = vec![
        r(&["the", "cat", "sat"]),
        r(&["a", "dog", "ran"]),
        r(&["up", "and", "away"]),
        r(&["red", "green", "blue"]),
        r(&["they", "sang", "well"]),
        r(&["go", "far", "away"]),
    ];
    let (e_id, d_id) = per_utterance_errors(&refs, &refs);
    let ci_id = delta_ci(&e_id, &e_id, &d_id);
    if ci_id.estimate != 0.0 {
        return Err(format!(
            "identical transcripts: delta {} != 0",
            ci_id.estimate
        ));
    }
    if !(ci_id.lo <= 0.0 && 0.0 <= ci_id.hi) {
        return Err(format!(
            "identical transcripts: CI [{}, {}] misses 0",
            ci_id.lo, ci_id.hi
        ));
    }
    let base = vec![
        r(&["the", "bat", "sat"]),
        r(&["a", "dog", "ran"]),
        r(&["up", "and", "away"]),
        r(&["red", "green", "yellow"]),
        r(&["they", "sang", "well"]),
        r(&["go", "far", "home"]),
    ];
    let new = vec![
        r(&["the", "cat", "sat"]),
        r(&["a", "dog", "ran"]),
        r(&["up", "and", "away"]),
        r(&["red", "green", "yellow"]),
        r(&["they", "sang", "badly"]),
        r(&["go", "far", "away"]),
    ];
    let (e_base, denoms) = per_utterance_errors(&refs, &base);
    let (e_new, _) = per_utterance_errors(&refs, &new);
    let fwd = delta_ci(&e_base, &e_new, &denoms);
    let rev = delta_ci(&e_new, &e_base, &denoms);
    if fwd.estimate == 0.0 {
        return Err("self-check pair is degenerate (delta 0)".to_string());
    }
    if fwd.estimate != -rev.estimate {
        return Err(format!(
            "swap: forward {} vs reversed {}",
            fwd.estimate, rev.estimate
        ));
    }
    if (fwd.lo + rev.hi).abs() > 1e-9 || (fwd.hi + rev.lo).abs() > 1e-9 {
        return Err(format!(
            "swap CI mismatch: [{}, {}] vs [{}, {}]",
            fwd.lo, fwd.hi, rev.lo, rev.hi
        ));
    }
    Ok(format!(
        "identical delta {:+.4} CI [{:+.4}, {:+.4}] covers 0; swap {:+.4} -> {:+.4}",
        ci_id.estimate, ci_id.lo, ci_id.hi, fwd.estimate, rev.estimate
    ))
}

/// Parallel map over `items` with deterministic (index-order) assembly.
/// Caps at 8 workers to stay polite on shared lane boxes.
fn par_map<T: Sync, R: Send>(items: &[T], f: &(dyn Fn(&T) -> R + Sync)) -> Vec<R> {
    if items.is_empty() {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8)
        .min(items.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let chunk = items.len().div_ceil(workers);
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for piece in items.chunks(chunk) {
            handles.push(s.spawn(|| piece.iter().map(f).collect::<Vec<R>>()));
        }
        let mut out = Vec::with_capacity(items.len());
        for h in handles {
            out.extend(h.join().expect("sweep worker panicked"));
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tsv_header_is_exact() {
        assert_eq!(
            SWEEP_TSV_HEADER,
            "budget\tarm\tWER\tS\tD\tI\tRTF\tpeak_live\tpeak_dormant\tlate_rescues\tdWER\tCI_lo\tCI_hi"
        );
    }

    #[test]
    fn tsv_na_placement_pair_stats_on_c_rows_only() {
        let ci = delta_ci(&[1, 0], &[0, 0], &[2, 2]);
        let b = SweepCell {
            budget: 50,
            arm: Arm::B,
            wer: 0.25,
            subs: 1,
            dels: 0,
            inss: 0,
            ref_len: 4,
            rtf: 0.01,
            peak_live: Some(12),
            peak_dormant: Some(0),
            rescues: None,
            delta: None,
        };
        let c = SweepCell {
            budget: 50,
            arm: Arm::C,
            wer: 0.0,
            subs: 0,
            dels: 0,
            inss: 0,
            ref_len: 4,
            rtf: 0.02,
            peak_live: Some(12),
            peak_dormant: Some(3),
            rescues: Some(1),
            delta: Some(ci),
        };
        let tsv = render_sweep_tsv(&[b, c]);
        let lines: Vec<&str> = tsv.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], SWEEP_TSV_HEADER);
        assert!(
            lines[1].ends_with("\tNA\tNA\tNA\tNA"),
            "B row: {}",
            lines[1]
        );
        assert!(
            lines[1].starts_with("50\tB\t0.2500\t1\t0\t0\t"),
            "B row: {}",
            lines[1]
        );
        assert!(
            lines[2].starts_with("50\tC\t0.0000\t0\t0\t0\t"),
            "C row: {}",
            lines[2]
        );
        assert!(
            lines[2].contains("\t1\t-0.2500\t"),
            "C row carries rescues + delta: {}",
            lines[2]
        );
    }

    #[test]
    fn late_rescues_count_multiset_matches_table_convention() {
        let committed = w(&["cat", "cat", "sat"]);
        let pruned = w(&["cat"]);
        assert_eq!(count_late_rescues(&committed, &pruned, &[]), 2);
        assert_eq!(count_late_rescues(&committed, &[], &w(&["sat"])), 1);
        assert_eq!(count_late_rescues(&committed, &[], &[]), 0);
    }

    #[test]
    fn scorer_selfcheck_passes() {
        let report = selfcheck_scorer().expect("self-checks pass");
        assert!(report.contains("covers 0"), "{report}");
        assert!(report.contains("swap"), "{report}");
    }

    #[test]
    fn synthetic_corpus_is_deterministic_and_seeded() {
        let a = synthetic_corpus(20, 7, "test");
        let b = synthetic_corpus(20, 7, "test");
        let c = synthetic_corpus(20, 8, "test");
        assert_eq!(a.len(), 20);
        assert_eq!(a[0].reference, w(&["the", "cat", "sat", "down"]));
        let texts = |us: &[SweepUtt]| {
            us.iter()
                .map(|u| {
                    (
                        u.id.clone(),
                        lattice::write_lattice(&u.lattice).expect("write"),
                        u.reference.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&a), texts(&b));
        assert_ne!(texts(&a), texts(&c));
        // Mix per decade: revival/clean/absent/bulk all present.
        assert_eq!(a[9].reference, w(&["alpha"]));
        assert_eq!(a[8].reference, w(&["red", "green", "blue"]));
    }

    #[test]
    fn feed_synthetic_sizes_and_real_unwired() {
        let feed = load_experiment_feed(&FeedSource::Synthetic {
            dev_n: 6,
            test_n: 11,
            seed: 1,
        })
        .expect("synthetic feed");
        assert_eq!(feed.dev.len(), 6);
        assert_eq!(feed.test.len(), 11);
        assert!(feed.dev[0].id.starts_with("dev-"));
        assert!(feed.test[0].id.starts_with("test-"));
        let err = load_experiment_feed(&FeedSource::Real {
            dev_manifest: PathBuf::from("dev.tsv"),
            test_manifest: PathBuf::from("test.tsv"),
            lat_dir: PathBuf::from("lat"),
            lm_path: PathBuf::from("lm.json"),
            dev_cell: PathBuf::from("dev-cell.tsv"),
            test_cell: PathBuf::from("test-cell.tsv"),
        })
        .expect_err("Real with missing inputs errors loudly");
        assert!(err.contains("real feed"), "{err}");
    }

    #[test]
    fn real_feed_rejects_synthetic_shaped_ids() {
        assert!(is_librispeech_id("1272-128104-0001"));
        assert!(is_librispeech_id("1089-134686-0006"));
        assert!(!is_librispeech_id("dev-0000"));
        assert!(!is_librispeech_id("test-0042"));
        assert!(!is_librispeech_id("abc"));
        assert!(!is_librispeech_id(""));
        assert!(!is_librispeech_id("1272-128104"));
    }

    #[test]
    fn tune_arm_b_is_deterministic() {
        let dev = synthetic_corpus(4, 42, "dev");
        let lm = FeedLm::Synthetic;
        let grid = TuningGrid {
            w_acoustic: vec![0.5, 1.0],
            w_sequence: vec![1.0],
            theta_live: vec![0.0],
            delta: vec![0.0],
        };
        let r1 = tune_arm_b(&dev, &lm, &[250], &grid).expect("tune");
        let r2 = tune_arm_b(&dev, &lm, &[250], &grid).expect("tune again");
        assert_eq!(r1.candidates, 2);
        assert_eq!(r1.scoreboard.len(), 2);
        assert_eq!(r1.dev_errors, r2.dev_errors);
        assert_eq!(r1.params.w_acoustic, r2.params.w_acoustic);
        assert_eq!(r1.params.theta_live, r2.params.theta_live);
        assert_eq!(r1.params.delta, r2.params.delta);
    }

    #[test]
    fn sweep_test_smoke_one_budget() {
        let test = synthetic_corpus(4, 99, "test");
        let lm = FeedLm::Synthetic;
        let tuned = ArmParams::fixture_defaults(0);
        let cells = sweep_test(&test, &lm, &tuned, &[250]).expect("sweep");
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].arm, Arm::B);
        assert_eq!(cells[1].arm, Arm::C);
        assert!(cells[0].rescues.is_none() && cells[0].delta.is_none());
        assert!(cells[1].rescues.is_some() && cells[1].delta.is_some());
        for c in &cells {
            assert!(c.wer < 1.0, "WER below 1.0: {}", c.wer);
            assert!(c.peak_live.unwrap_or(0) <= 250);
        }
        let tsv = render_sweep_tsv(&cells);
        assert_eq!(tsv.lines().count(), 3);
    }

    #[test]
    fn synthetic_revival_prune_revive_gap() {
        // First decade opens with 4 revival utterances (patterns 0,1,2,0).
        let utts = synthetic_corpus(10, 1234, "test");
        let lm = FeedLm::Synthetic;
        let params = ArmParams::fixture_defaults(500);
        let lm_fn = |a: &str, b: &str| lm.score(a, b);
        let mut gaps = 0;
        for u in utts.iter().take(4) {
            let b = run_fabric_arm(&u.lattice, Arm::B, &params, &lm_fn).expect("B");
            let c = run_fabric_arm(&u.lattice, Arm::C, &params, &lm_fn).expect("C");
            let correct = &u.reference[1];
            let b_pruned = b.pruned.iter().any(|e| e == correct);
            let c_commits = c.hypothesis.iter().any(|e| e == correct);
            eprintln!(
                "{} correct={correct} b_pruned={b_pruned} c_commits={c_commits} b={:?} c={:?}",
                u.id, b.hypothesis, c.hypothesis
            );
            if b_pruned && c_commits {
                gaps += 1;
            }
        }
        assert_eq!(
            gaps, 4,
            "every revival utt prunes-then-revives under fixture defaults"
        );
    }
}
