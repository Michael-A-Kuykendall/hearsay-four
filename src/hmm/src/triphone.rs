//! Word-internal triphones + decision-tree state tying (lane B, wave 1a).
//!
//! Scope: word-INTERNAL contexts only. A phone at a word edge has no
//! left/right neighbour inside the word, so its unit falls back along
//! triphone -> biphone -> monophone. SIL/SP are never triphone centers;
//! their monophone models are kept as-is.
//!
//! Memory: stats accumulate per utterance and merge into one pooled map,
//! so RSS scales with the triphone inventory (tens of thousands), not the
//! corpus. Trees grow from pooled sufficient stats only. Provenance is
//! capped (see MAX_PROV_UTTS) to hold the 8 GB RSS gate at 1 h scale.

use crate::{
    Gaussian, HmmError, HmmSet, Mixture, ParamStats, PerReport, PhoneHmm, Provenance, Result, SIL,
    SP, phone_edit_distance,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Hard cap on provenance utterance ids kept per stat cell / parameter.
///
/// The CMVN monophone checkpoint carries tens of millions of provenance
/// ids (~740 MB of JSON); uncapped unions across the 1 h subset blew a
/// prior run past 23 GB RSS. Every provenance set in this module keeps at
/// most the first `MAX_PROV_UTTS` ids in sorted order (deterministic);
/// `frames` counts always stay exact. The cap keeps
/// `validate_provenance` green (it requires a non-empty id list).
pub const MAX_PROV_UTTS: usize = 8;

/// Truncate a provenance pointer to [`MAX_PROV_UTTS`] ids, keeping the
/// lexicographically first distinct ids; `frames` is untouched. Shorter
/// lists keep their original order.
pub fn bound_provenance(p: &mut Provenance) {
    if p.utterances.len() > MAX_PROV_UTTS {
        let set: BTreeSet<String> = p.utterances.drain(..).collect();
        p.utterances = set.into_iter().take(MAX_PROV_UTTS).collect();
    }
}

/// Cap provenance on every component of a model in place. Returns
/// (utterance ids before, after) for logging.
pub fn cap_model_provenance(model: &mut HmmSet) -> (usize, usize) {
    let (mut before, mut after) = (0usize, 0usize);
    for h in model.phones.values_mut() {
        for mix in h.emissions.iter_mut() {
            for comp in mix.components.iter_mut() {
                before += comp.stats.provenance.utterances.len();
                bound_provenance(&mut comp.stats.provenance);
                after += comp.stats.provenance.utterances.len();
            }
        }
    }
    (before, after)
}

/// One context-dependent unit. `None` on a side means word boundary
/// (no context on that side); such units back off, see [`unit_key`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Triphone {
    pub left: Option<String>,
    pub center: String,
    pub right: Option<String>,
}

impl Triphone {
    pub fn is_full(&self) -> bool {
        self.left.is_some() && self.right.is_some()
    }
}

/// Expand one word's phone string into per-position triphone keys.
/// Position 0 has no left context, the last position no right context.
pub fn triphones_for_word(phones: &[String]) -> Vec<Triphone> {
    let n = phones.len();
    phones
        .iter()
        .enumerate()
        .map(|(i, c)| Triphone {
            left: if i > 0 {
                Some(phones[i - 1].clone())
            } else {
                None
            },
            center: c.clone(),
            right: if i + 1 < n {
                Some(phones[i + 1].clone())
            } else {
                None
            },
        })
        .collect()
}

/// Backoff unit key: `L-C+R` (full) -> `L-C` / `C+R` (biphone) -> `C` (mono).
pub fn unit_key(t: &Triphone) -> String {
    match (&t.left, &t.right) {
        (Some(l), Some(r)) => format!("{l}-{}+{r}", t.center),
        (Some(l), None) => format!("{l}-{}", t.center),
        (None, Some(r)) => format!("{}+{r}", t.center),
        (None, None) => t.center.clone(),
    }
}

/// Per-position backoff unit keys for one word.
pub fn unit_keys_for_word(word: &[String]) -> Vec<String> {
    triphones_for_word(word).iter().map(unit_key).collect()
}

/// Center phone of a unit key (`L-C+R`, `L-C`, `C+R`, or `C`).
pub fn center_of_unit(key: &str) -> &str {
    let after_left = key.split_once('-').map(|(_, r)| r).unwrap_or(key);
    after_left
        .split_once('+')
        .map(|(c, _)| c)
        .unwrap_or(after_left)
}

/// Parse a full `L-C+R` key back into a [`Triphone`]; biphone/mono keys
/// return `None`.
pub fn parse_triphone(key: &str) -> Option<Triphone> {
    let (l, rest) = key.split_once('-')?;
    let (c, r) = rest.split_once('+')?;
    if l.is_empty() || c.is_empty() || r.is_empty() {
        return None;
    }
    Some(Triphone {
        left: Some(l.to_string()),
        center: c.to_string(),
        right: Some(r.to_string()),
    })
}

/// Base phone for question matching: uppercase, trailing stress digits
/// stripped (`AH0` -> `AH`; the r3v2 inventory is already stress-free).
pub fn base_phone(p: &str) -> String {
    p.trim_end_matches(|c: char| c.is_ascii_digit())
        .to_uppercase()
}

/// A phonetic question: a named set of base ARPABET phones. During tree
/// growing each question is tried on both the left and the right context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub name: String,
    pub phones: BTreeSet<String>,
}

impl Question {
    pub fn matches(&self, phone: &str) -> bool {
        self.phones.contains(&base_phone(phone))
    }
}

fn q(name: &str, members: &[&str]) -> Question {
    Question {
        name: name.to_string(),
        phones: members.iter().map(|s| s.to_string()).collect(),
    }
}

/// Hand-written ARPABET question set (stops, fricatives, nasals, vowel
/// height/front-back/rounding, voicing, place). Order is fixed; ties in
/// tree growing resolve to the earliest question, keeping tying
/// deterministic.
pub fn default_questions() -> Vec<Question> {
    vec![
        q("stop", &["B", "D", "G", "P", "T", "K"]),
        q("voiced_stop", &["B", "D", "G"]),
        q("voiceless_stop", &["P", "T", "K"]),
        q(
            "fricative",
            &["F", "V", "TH", "DH", "S", "Z", "SH", "ZH", "HH"],
        ),
        q("voiced_fricative", &["V", "DH", "Z", "ZH"]),
        q("voiceless_fricative", &["F", "TH", "S", "SH", "HH"]),
        q("affricate", &["CH", "JH"]),
        q("nasal", &["M", "N", "NG"]),
        q("liquid", &["L", "R"]),
        q("glide", &["W", "Y"]),
        q(
            "vowel",
            &[
                "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH",
                "UW",
            ],
        ),
        q("high_vowel", &["IY", "IH", "UH", "UW"]),
        q("mid_vowel", &["EH", "ER", "EY", "OW", "AH", "AO"]),
        q("low_vowel", &["AE", "AA", "AW", "AY", "OY"]),
        q("front_vowel", &["IY", "IH", "EH", "AE", "EY", "AY"]),
        q("central_vowel", &["AH", "ER"]),
        q("back_vowel", &["UW", "UH", "OW", "AO", "AA", "OY", "AW"]),
        q("rounded", &["UW", "UH", "OW", "AO", "OY", "AW"]),
        q(
            "voiced",
            &[
                "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH",
                "UW", "B", "D", "G", "V", "DH", "Z", "ZH", "JH", "M", "N", "NG", "L", "R", "W",
                "Y",
            ],
        ),
        q(
            "voiceless",
            &["P", "T", "K", "F", "TH", "S", "SH", "CH", "HH"],
        ),
        q(
            "obstruent",
            &[
                "B", "D", "G", "P", "T", "K", "F", "V", "TH", "DH", "S", "Z", "SH", "ZH", "HH",
                "CH", "JH",
            ],
        ),
        q(
            "sonorant",
            &[
                "M", "N", "NG", "L", "R", "W", "Y", "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER",
                "EY", "IH", "IY", "OW", "OY", "UH", "UW",
            ],
        ),
        q("labial", &["B", "P", "M", "W", "F", "V"]),
        q(
            "coronal",
            &[
                "D", "T", "N", "S", "Z", "SH", "ZH", "CH", "JH", "TH", "DH", "L", "R",
            ],
        ),
        q("velar", &["G", "K", "NG"]),
        q("silence", &["SIL", "SP"]),
    ]
}

/// Which context side a tree question was asked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeSide {
    Left,
    Right,
}

/// One root-to-leaf step: the question, the side, and the branch taken
/// (`true` = context in the question set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeStep {
    pub question: String,
    pub side: TreeSide,
    pub answer: bool,
}

/// Sufficient stats for one (triphone, state) cell, pooled over frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriStateStats {
    pub key: Triphone,
    pub state: usize,
    pub count: f64,
    pub sum: Vec<f64>,
    pub sum_sq: Vec<f64>,
    pub utterances: BTreeSet<String>,
    pub frames: u64,
}

/// One tied state (senone): pooled members plus the question path that
/// produced it. The path is serialized with the state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiedState {
    pub id: usize,
    pub center: String,
    pub state: usize,
    pub members: Vec<Triphone>,
    pub path: Vec<TreeStep>,
    pub count: f64,
}

/// Tree-growing stopping rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TyingConfig {
    /// Split only if the likelihood gain exceeds this.
    pub gain_threshold: f64,
    /// Split only if both children hold at least this occupancy.
    pub min_occupancy: f64,
    /// Hard cap on leaves per (center, state) tree.
    pub max_leaves: usize,
    /// Absolute per-dim variance floor for pooled likelihoods.
    pub var_floor: f64,
}

impl Default for TyingConfig {
    fn default() -> Self {
        Self {
            gain_threshold: 10.0,
            min_occupancy: 10.0,
            max_leaves: 32,
            var_floor: 1e-6,
        }
    }
}

fn pooled_ll(count: f64, sum: &[f64], sum_sq: &[f64], var_floor: f64) -> f64 {
    if count <= 0.0 {
        return 0.0;
    }
    let tau_ln = std::f64::consts::TAU.ln();
    let mut ll = 0.0;
    for d in 0..sum.len() {
        let mean = sum[d] / count;
        let var = (sum_sq[d] / count - mean * mean).max(var_floor);
        ll += -0.5 * count * (tau_ln + var.ln() + 1.0);
    }
    ll
}

fn pool(items: &[&TriStateStats]) -> (f64, Vec<f64>, Vec<f64>) {
    let dim = items.first().map(|s| s.sum.len()).unwrap_or(0);
    let (mut n, mut sum, mut sq) = (0.0, vec![0.0; dim], vec![0.0; dim]);
    for s in items {
        n += s.count;
        for d in 0..dim {
            sum[d] += s.sum[d];
            sq[d] += s.sum_sq[d];
        }
    }
    (n, sum, sq)
}

struct Node {
    members: Vec<usize>,
    path: Vec<TreeStep>,
}

/// Best split so far: (question idx, side, gain, yes-members, no-members).
type BestSplit = (usize, TreeSide, f64, Vec<usize>, Vec<usize>);

/// Grow one decision tree over a single (center, state) group.
/// Deterministic: members sorted, questions in order, strict `>` keeps
/// the earliest best split on ties.
fn grow_tree(items: &[&TriStateStats], questions: &[Question], cfg: &TyingConfig) -> Vec<Node> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| items[a].key.cmp(&items[b].key));
    let max_leaves = cfg.max_leaves.max(1);
    let mut stack = vec![Node {
        members: order,
        path: Vec::new(),
    }];
    let mut leaves: Vec<Node> = Vec::new();
    while let Some(node) = stack.pop() {
        if leaves.len() + stack.len() + 1 >= max_leaves || node.members.len() < 2 {
            leaves.push(node);
            continue;
        }
        let group: Vec<&TriStateStats> = node.members.iter().map(|&i| items[i]).collect();
        let (pn, ps, pq) = pool(&group);
        let parent_ll = pooled_ll(pn, &ps, &pq, cfg.var_floor);
        let mut best: Option<BestSplit> = None;
        for (qi, question) in questions.iter().enumerate() {
            for side in [TreeSide::Left, TreeSide::Right] {
                let mut yes: Vec<usize> = Vec::new();
                let mut no: Vec<usize> = Vec::new();
                for &i in &node.members {
                    let ctx = match side {
                        TreeSide::Left => items[i].key.left.as_deref(),
                        TreeSide::Right => items[i].key.right.as_deref(),
                    };
                    let hit = ctx.map(|c| question.matches(c)).unwrap_or(false);
                    if hit {
                        yes.push(i);
                    } else {
                        no.push(i);
                    }
                }
                if yes.is_empty() || no.is_empty() {
                    continue;
                }
                let yg: Vec<&TriStateStats> = yes.iter().map(|&i| items[i]).collect();
                let ng: Vec<&TriStateStats> = no.iter().map(|&i| items[i]).collect();
                let (yn, ys, yq) = pool(&yg);
                let (nn, ns, nq) = pool(&ng);
                if yn < cfg.min_occupancy || nn < cfg.min_occupancy {
                    continue;
                }
                let gain = pooled_ll(yn, &ys, &yq, cfg.var_floor)
                    + pooled_ll(nn, &ns, &nq, cfg.var_floor)
                    - parent_ll;
                let better = best.as_ref().map(|b| gain > b.2).unwrap_or(true);
                if better {
                    best = Some((qi, side, gain, yes, no));
                }
            }
        }
        match best {
            Some((qi, side, gain, yes, no)) if gain > cfg.gain_threshold => {
                let qname = questions[qi].name.clone();
                let mut path_yes = node.path.clone();
                path_yes.push(TreeStep {
                    question: qname.clone(),
                    side,
                    answer: true,
                });
                let mut path_no = node.path;
                path_no.push(TreeStep {
                    question: qname,
                    side,
                    answer: false,
                });
                stack.push(Node {
                    members: no,
                    path: path_no,
                });
                stack.push(Node {
                    members: yes,
                    path: path_yes,
                });
            }
            _ => leaves.push(node),
        }
    }
    leaves
}

/// Build tied states from pooled triphone stats. Only full (word-internal)
/// triphones with positive occupancy enter trees; ids are assigned in
/// (center, state, members) order.
pub fn build_tying(
    stats: &[TriStateStats],
    questions: &[Question],
    cfg: &TyingConfig,
) -> Vec<TiedState> {
    let mut groups: BTreeMap<(String, usize), Vec<&TriStateStats>> = BTreeMap::new();
    for s in stats {
        if s.count > 0.0 && s.key.is_full() {
            groups
                .entry((s.key.center.clone(), s.state))
                .or_default()
                .push(s);
        }
    }
    let mut tied: Vec<TiedState> = Vec::new();
    for ((center, state), items) in &groups {
        for leaf in grow_tree(items, questions, cfg) {
            let group: Vec<&TriStateStats> = leaf.members.iter().map(|&i| items[i]).collect();
            let (count, _, _) = pool(&group);
            let mut members: Vec<Triphone> =
                leaf.members.iter().map(|&i| items[i].key.clone()).collect();
            members.sort();
            tied.push(TiedState {
                id: 0,
                center: center.clone(),
                state: *state,
                members,
                path: leaf.path,
                count,
            });
        }
    }
    tied.sort_by(|a, b| (&a.center, a.state, &a.members).cmp(&(&b.center, b.state, &b.members)));
    for (i, t) in tied.iter_mut().enumerate() {
        t.id = i;
    }
    tied
}

/// Accumulate per-(triphone, state) sufficient stats from one Viterbi
/// alignment. `phones_seq` is the full sequence given to `forced_align`
/// (SIL/SP allowed); `word_spans` are per-word `[start, end)` seq-position
/// spans (SIL/SP excluded). Frames aligned to SIL/SP are skipped: boundary
/// phones back off and SIL/SP stay monophone.
pub fn accumulate_stats(
    model: &HmmSet,
    utt_id: &str,
    phones_seq: &[String],
    word_spans: &[(usize, usize)],
    path: &[usize],
    obs: &[Vec<f64>],
) -> Result<BTreeMap<(Triphone, usize), TriStateStats>> {
    let mut out: BTreeMap<(Triphone, usize), TriStateStats> = BTreeMap::new();
    if obs.is_empty() {
        return Ok(out);
    }
    if path.len() != obs.len() {
        return Err(HmmError::DimensionMismatch);
    }
    if obs[0].len() != model.dim {
        return Err(HmmError::DimensionMismatch);
    }
    let mut layout: Vec<(usize, usize)> = Vec::new();
    for (pos, p) in phones_seq.iter().enumerate() {
        let h = model
            .phones
            .get(p)
            .ok_or_else(|| HmmError::UnknownPhone(p.clone()))?;
        for s in 0..h.n_states {
            layout.push((pos, s));
        }
    }
    for (t, frame) in obs.iter().enumerate() {
        let (seqpos, state) = *layout
            .get(path[t])
            .ok_or_else(|| HmmError::InvalidModel(format!("path idx {}", path[t])))?;
        let span = word_spans.iter().find(|(s, e)| seqpos >= *s && seqpos < *e);
        let Some((start, end)) = span else {
            continue;
        };
        let word = &phones_seq[*start..*end];
        let pos = seqpos - *start;
        let tris = triphones_for_word(word);
        let tri = tris[pos].clone();
        let entry = out
            .entry((tri.clone(), state))
            .or_insert_with(|| TriStateStats {
                key: tri,
                state,
                count: 0.0,
                sum: vec![0.0; model.dim],
                sum_sq: vec![0.0; model.dim],
                utterances: BTreeSet::new(),
                frames: 0,
            });
        entry.count += 1.0;
        for d in 0..model.dim {
            entry.sum[d] += frame[d];
            entry.sum_sq[d] += frame[d] * frame[d];
        }
        if entry.utterances.len() < MAX_PROV_UTTS {
            entry.utterances.insert(utt_id.to_string());
        }
        entry.frames += 1;
    }
    Ok(out)
}

/// Merge per-utterance stat maps into the pooled map (streaming caller
/// pattern: accumulate one utterance, merge, drop frames).
pub fn merge_stats(
    pooled: &mut BTreeMap<(Triphone, usize), TriStateStats>,
    one: BTreeMap<(Triphone, usize), TriStateStats>,
) {
    for (k, v) in one {
        match pooled.get_mut(&k) {
            Some(e) => {
                e.count += v.count;
                for d in 0..e.sum.len() {
                    e.sum[d] += v.sum[d];
                    e.sum_sq[d] += v.sum_sq[d];
                }
                e.utterances.extend(v.utterances);
                while e.utterances.len() > MAX_PROV_UTTS {
                    e.utterances.pop_last();
                }
                e.frames += v.frames;
            }
            None => {
                pooled.insert(k, v);
            }
        }
    }
}

/// Build a triphone [`HmmSet`] from a monophone model plus tying.
///
/// * Monophone entries (including SIL/SP) are cloned untouched.
/// * Every unit in `units` becomes an entry: full triphones take their
///   state's mixture from the tied senone's pooled single Gaussian;
///   biphone/monophone backoff units clone the center monophone's states.
/// * Unseen triphones (no tied state) fall back to the center monophone.
pub fn build_tied_model(
    mono: &HmmSet,
    stats: &BTreeMap<(Triphone, usize), TriStateStats>,
    tied: &[TiedState],
    units: &BTreeSet<String>,
) -> Result<HmmSet> {
    let mut member_of: BTreeMap<(Triphone, usize), usize> = BTreeMap::new();
    for t in tied {
        for m in &t.members {
            member_of.insert((m.clone(), t.state), t.id);
        }
    }
    let mut by_id: BTreeMap<usize, &TiedState> = BTreeMap::new();
    for t in tied {
        by_id.insert(t.id, t);
    }
    let mut model = mono.clone();
    for unit in units {
        if model.phones.contains_key(unit) {
            continue;
        }
        let center = center_of_unit(unit).to_string();
        let proto = model
            .phones
            .get(&center)
            .ok_or_else(|| HmmError::UnknownPhone(center.clone()))?
            .clone();
        let tri = parse_triphone(unit);
        let mut emissions = Vec::with_capacity(proto.n_states);
        for s in 0..proto.n_states {
            let tied_mix = tri.as_ref().and_then(|t| {
                member_of
                    .get(&(t.clone(), s))
                    .and_then(|id| pooled_mixture(mono, stats, by_id.get(id)?, s))
            });
            match tied_mix {
                Some(m) => emissions.push(m),
                None => emissions.push(proto.emissions[s].clone()),
            }
        }
        model.phones.insert(
            unit.clone(),
            PhoneHmm {
                name: unit.clone(),
                n_states: proto.n_states,
                emissions,
                trans: proto.trans.clone(),
                tied_to: None,
            },
        );
    }
    Ok(model)
}

fn pooled_mixture(
    mono: &HmmSet,
    stats: &BTreeMap<(Triphone, usize), TriStateStats>,
    tied: &TiedState,
    state: usize,
) -> Option<Mixture> {
    let dim = mono.dim;
    let (mut count, mut sum, mut sq) = (0.0, vec![0.0; dim], vec![0.0; dim]);
    let mut utts: BTreeSet<String> = BTreeSet::new();
    let mut frames = 0u64;
    for m in &tied.members {
        let s = stats.get(&(m.clone(), state))?;
        count += s.count;
        for d in 0..dim {
            sum[d] += s.sum[d];
            sq[d] += s.sum_sq[d];
        }
        utts.extend(s.utterances.iter().cloned());
        frames += s.frames;
    }
    if count <= 1e-8 || utts.is_empty() {
        return None;
    }
    let mut mean = vec![0.0; dim];
    let mut var = vec![0.0; dim];
    for d in 0..dim {
        mean[d] = sum[d] / count;
        var[d] = (sq[d] / count - mean[d] * mean[d]).max(mono.global_var[d] * mono.var_floor_ratio);
    }
    Some(Mixture::single(
        Gaussian { mean, var },
        ParamStats {
            count,
            sum,
            sum_sq: sq,
            provenance: Provenance {
                utterances: utts.into_iter().take(MAX_PROV_UTTS).collect(),
                frames,
            },
        },
    ))
}

/// Pool each tied state's member mixtures back to their occupancy-weighted
/// average (per component index) so tied states stay tied across BW
/// rounds. Exact at 1 mixture; past splits the per-index correspondence
/// is an approximation (members split identically from identical starts,
/// so it holds early).
pub fn retie_averaging(model: &mut HmmSet, tied: &[TiedState]) -> Result<()> {
    for t in tied {
        let keys: Vec<String> = t.members.iter().map(unit_key).collect();
        let ncomp = model
            .phones
            .get(&keys[0])
            .ok_or_else(|| HmmError::UnknownPhone(keys[0].clone()))?
            .emissions[t.state]
            .components
            .len();
        let dim = model.dim;
        for c in 0..ncomp {
            let (mut count, mut sum, mut sq) = (0.0, vec![0.0; dim], vec![0.0; dim]);
            let mut utts: BTreeSet<String> = BTreeSet::new();
            let mut frames = 0u64;
            let mut ok = true;
            for k in &keys {
                let mix = &model
                    .phones
                    .get(k)
                    .ok_or_else(|| HmmError::UnknownPhone(k.clone()))?
                    .emissions[t.state];
                if mix.components.len() != ncomp {
                    ok = false;
                    break;
                }
                let st = &mix.components[c].stats;
                count += st.count;
                for d in 0..dim {
                    sum[d] += st.sum[d];
                    sq[d] += st.sum_sq[d];
                }
                utts.extend(st.provenance.utterances.iter().cloned());
                frames += st.provenance.frames;
            }
            if !ok || count <= 1e-8 {
                continue;
            }
            let mut mean = vec![0.0; dim];
            let mut var = vec![0.0; dim];
            for d in 0..dim {
                mean[d] = sum[d] / count;
                var[d] = (sq[d] / count - mean[d] * mean[d])
                    .max(model.global_var[d] * model.var_floor_ratio);
            }
            let pooled = ParamStats {
                count,
                sum,
                sum_sq: sq,
                provenance: Provenance {
                    utterances: utts.into_iter().take(MAX_PROV_UTTS).collect(),
                    frames,
                },
            };
            let gauss = Gaussian { mean, var };
            let mut wsum = 0.0;
            for k in &keys {
                wsum += model.phones[k].emissions[t.state].components[c].weight;
            }
            let weight = wsum / keys.len() as f64;
            for k in &keys {
                let comp = &mut model
                    .phones
                    .get_mut(k)
                    .ok_or_else(|| HmmError::UnknownPhone(k.clone()))?
                    .emissions[t.state]
                    .components[c];
                comp.gaussian = gauss.clone();
                comp.stats = pooled.clone();
                comp.weight = weight;
            }
        }
        for k in &keys {
            let mix = &mut model
                .phones
                .get_mut(k)
                .ok_or_else(|| HmmError::UnknownPhone(k.clone()))?
                .emissions[t.state];
            let tot: f64 = mix.components.iter().map(|c| c.weight).sum();
            if tot > 0.0 {
                for c in mix.components.iter_mut() {
                    c.weight /= tot;
                }
            }
        }
    }
    Ok(())
}

/// Deterministic corpus subset sampler (shared train-bin helper).
///
/// Fisher-Yates shuffle of row indices `0..nrows` driven by [`crate::Rng`]
/// seeded with `seed`, returning the first `size` picks. A pure function
/// of `(nrows, size, seed)`: the caller filters usable rows first, so the
/// same (manifest, lexicon, size, seed) always yields the same utterance
/// list. Taking fewer rows is a prefix of taking more under the same seed.
pub fn sample_subset(nrows: usize, size: usize, seed: u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..nrows).collect();
    let mut rng = crate::Rng::new(seed);
    for i in (1..idx.len()).rev() {
        let j = (rng.next_f64() * (i + 1) as f64) as usize;
        idx.swap(i, j.min(i));
    }
    idx.truncate(size.min(nrows));
    idx
}

/// Deterministic disjoint train/PER split: one shuffle on the same stream
/// as [`sample_subset`], first `train_size` rows are train, the next
/// `per_size` are PER. PER never overlaps train (train-on-test guard).
pub fn sample_disjoint(
    nrows: usize,
    train_size: usize,
    per_size: usize,
    seed: u64,
) -> (Vec<usize>, Vec<usize>) {
    let all = sample_subset(nrows, train_size.saturating_add(per_size), seed);
    let t = train_size.min(all.len());
    (all[..t].to_vec(), all[t..].to_vec())
}

/// Decode one observation sequence with a tied (monophone + unit) model
/// and map decoded units to center phones.
///
/// Pure function of (model, obs): the loop inventory is the model's full
/// phone set (monophones plus every training-observed unit), fixed for
/// every utterance. It takes no reference, so the decode cannot see
/// reference word identities or boundaries.
pub fn decode_centers(model: &HmmSet, obs: &[Vec<f64>]) -> Result<Vec<String>> {
    let hyp_units = model.decode_phones(obs)?;
    Ok(hyp_units
        .iter()
        .map(|k| center_of_unit(k).to_string())
        .filter(|p| p != SIL && p != SP)
        .collect())
}

/// PER for a tied model through the SAME free-loop path as the monophone
/// [`HmmSet::phone_error_rate`]: full model inventory, uniform loop,
/// centers scored. Returns per-utterance PERs for bootstrap CIs.
pub fn tied_phone_error_rate(
    model: &HmmSet,
    data: &[(Vec<String>, Vec<Vec<f64>>)],
) -> Result<PerReport> {
    let strip = |seq: &[String]| -> Vec<String> {
        seq.iter()
            .filter(|p| *p != SIL && *p != SP)
            .cloned()
            .collect()
    };
    let mut per_utterance = Vec::with_capacity(data.len());
    let (mut errors, mut total) = (0usize, 0usize);
    for (seq, obs) in data {
        let reference = strip(seq);
        let hyp = decode_centers(model, obs)?;
        let e = phone_edit_distance(&reference, &hyp);
        let pu = if reference.is_empty() {
            0.0
        } else {
            e as f64 / reference.len() as f64
        };
        errors += e;
        total += reference.len();
        per_utterance.push(pu);
    }
    Ok(PerReport {
        per: if total == 0 {
            0.0
        } else {
            errors as f64 / total as f64
        },
        per_utterance,
        total_errors: errors,
        total_ref_phones: total,
    })
}
