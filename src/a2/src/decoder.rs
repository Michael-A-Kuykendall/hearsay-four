//! One-stage connected-word DTW token-passing decoder (Ney family).
//!
//! Frame-synchronous Viterbi over word templates + SIL/SP silence. Token
//! state (per bead `hearsay-four-5n2--connected-word-decoder`): unit in
//! {word, SIL, SP}, template position, start frame, path cost, backpointer,
//! LM history (empty in A2; GARBAGE deferred). DTW cells are NOT fabric
//! facts — the search graph lives and dies inside [`decode`].
//!
//! Local transitions (input-synchronous, each step consumes 1 input frame):
//! within-word stay/step/skip with a slope cap, word-end to word-start
//! (+ word-insertion penalty), word-end to SIL/SP, SIL self-loop, SP short
//! pause (1..=2 frames). Duration + expansion guards enforced at word end.
//! Path-length normalization reported as cost/frame (ranking uses raw cost:
//! all complete paths span the same T).

use crate::templates::TemplateSet;
use fse::dtw::frame_distance;
use std::collections::HashMap;

/// Decoder hyper-parameters (tuned on DEV only; see A2-CONNECTED-DTW.md).
#[derive(Clone, Debug)]
pub struct DecoderConfig {
    /// Keep tokens within `best + beam` per frame (cost units).
    pub beam: f64,
    /// Hard cap on live tokens per frame.
    pub max_live: usize,
    /// Word-insertion penalty added on every word start.
    pub wip: f64,
    /// Min word duration in frames (10 ms each).
    pub min_word_frames: usize,
    /// Max word duration in frames.
    pub max_word_frames: usize,
    /// Max template advance per input frame (slope/step cap).
    pub max_step: usize,
    /// Max consecutive template stays (slope cap).
    pub max_stay: u8,
    /// Max word_frames / template_frames expansion at word end.
    pub max_expansion: f64,
    /// Max inter-word silence gap (frames) admitted as lattice arcs.
    pub max_sp_frames: usize,
    /// Require final token in SIL (utterances end in silence).
    pub sil_endpoints: bool,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        DecoderConfig {
            beam: 400.0,
            max_live: 2000,
            wip: 15.0,
            min_word_frames: 15,
            max_word_frames: 400,
            max_step: 2,
            max_stay: 2,
            max_expansion: 3.0,
            max_sp_frames: 30,
            sil_endpoints: true,
        }
    }
}

/// Search unit: word template instance, SIL, or short pause.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Unit {
    Word(usize),
    Sil,
    Sp,
}

/// Live token: full bead state (lm_history empty in A2).
#[derive(Clone, Debug)]
struct Token {
    unit: Unit,
    /// Template position (0 for SIL/SP).
    pos: usize,
    /// Start frame of the current unit.
    start: u32,
    /// Path cost to here (acoustic + WIPs).
    cost: f64,
    /// Path cost just before the current word started (for local word cost).
    entry: f64,
    /// Backpointer: (frame, index in that frame's pruned column).
    bp_frame: u32,
    bp_idx: u32,
    /// Consecutive template stays (slope state).
    stay: u8,
    /// Words completed before the current unit.
    words: u32,
}

impl Token {
    fn no_bp() -> (u32, u32) {
        (u32::MAX, u32::MAX)
    }
}

/// One word hypothesis (1-best path segment or lattice token source).
#[derive(Clone, Debug)]
pub struct WordHyp {
    pub word: String,
    pub instance: usize,
    pub start: u32,
    pub end: u32,
    /// Cumulative path cost at word end (acoustic + WIPs).
    pub cost: f64,
    /// Local word acoustic cost (excludes entry path + WIP).
    pub local: f64,
    /// Path-length-normalized local cost (local / frames).
    pub norm: f64,
}

/// Decode result: 1-best + word-end events (lattice source) + diagnostics.
#[derive(Clone, Debug)]
pub struct DecodeResult {
    pub words: Vec<WordHyp>,
    /// All word-end events within the beam (lattice source, K-filtered later).
    pub events: Vec<WordHyp>,
    /// Total path cost of the 1-best (raw).
    pub cost: f64,
    /// Normalized 1-best cost (cost / input frames).
    pub norm_cost: f64,
    /// Peak live tokens across frames (gate: <= max_live by construction).
    pub peak_live: usize,
    /// Input frames.
    pub frames: usize,
}

type StateKey = (Unit, usize);

/// Frame-synchronous one-stage decode of untrimmed phrase `frames`.
pub fn decode(
    set: &TemplateSet,
    frames: &[Vec<f64>],
    cfg: &DecoderConfig,
) -> Result<DecodeResult, String> {
    if frames.is_empty() {
        return Err("decode: empty input frames".to_string());
    }
    if set.templates.is_empty() {
        return Err("decode: empty template set".to_string());
    }
    let t_len = frames.len();
    let mut columns: Vec<Vec<Token>> = Vec::with_capacity(t_len);
    let mut events: Vec<WordHyp> = Vec::new();
    let mut peak_live = 0usize;

    // t = 0 seed: every word start + SIL.
    let mut seed: HashMap<StateKey, Token> = HashMap::new();
    let d_sil = frame_distance(&frames[0], &set.sil_mean);
    seed.insert(
        (Unit::Sil, 0),
        Token {
            unit: Unit::Sil,
            pos: 0,
            start: 0,
            cost: d_sil,
            entry: 0.0,
            bp_frame: Token::no_bp().0,
            bp_idx: Token::no_bp().1,
            stay: 0,
            words: 0,
        },
    );
    for (m, tmpl) in set.templates.iter().enumerate() {
        if tmpl.frames.is_empty() {
            continue;
        }
        let d = frame_distance(&frames[0], &tmpl.frames[0]);
        seed.insert(
            (Unit::Word(m), 0),
            Token {
                unit: Unit::Word(m),
                pos: 0,
                start: 0,
                cost: d + cfg.wip,
                entry: 0.0,
                bp_frame: Token::no_bp().0,
                bp_idx: Token::no_bp().1,
                stay: 0,
                words: 0,
            },
        );
    }
    let col0 = prune(seed, cfg);
    if col0.is_empty() {
        return Err("decode: all seeds pruned at t=0".to_string());
    }
    peak_live = peak_live.max(col0.len());
    columns.push(col0);

    for t in 1..t_len {
        let prev = &columns[t - 1];
        let mut curr: HashMap<StateKey, Token> = HashMap::new();
        // Best path cost entering a fresh word at t (recomputed per (m) below
        // from each predecessor; word-start fan-out loops instances).
        for (pi, tok) in prev.iter().enumerate() {
            match tok.unit {
                Unit::Word(m) => {
                    let tmpl = &set.templates[m];
                    let tlen = tmpl.frames.len();
                    if tok.pos >= tlen {
                        continue;
                    }
                    // Within-word transitions.
                    if tok.stay < cfg.max_stay {
                        relax(
                            &mut curr,
                            (Unit::Word(m), tok.pos),
                            Token {
                                unit: Unit::Word(m),
                                pos: tok.pos,
                                start: tok.start,
                                cost: tok.cost + frame_distance(&frames[t], &tmpl.frames[tok.pos]),
                                entry: tok.entry,
                                bp_frame: (t - 1) as u32,
                                bp_idx: pi as u32,
                                stay: tok.stay + 1,
                                words: tok.words,
                            },
                        );
                    }
                    for step in 1..=cfg.max_step {
                        let u = tok.pos + step;
                        if u >= tlen {
                            break;
                        }
                        relax(
                            &mut curr,
                            (Unit::Word(m), u),
                            Token {
                                unit: Unit::Word(m),
                                pos: u,
                                start: tok.start,
                                cost: tok.cost + frame_distance(&frames[t], &tmpl.frames[u]),
                                entry: tok.entry,
                                bp_frame: (t - 1) as u32,
                                bp_idx: pi as u32,
                                stay: 0,
                                words: tok.words,
                            },
                        );
                    }
                    // Word end: record event + fan out.
                    if tok.pos == tlen - 1 {
                        let end = (t - 1) as u32;
                        let span = end.saturating_sub(tok.start) + 1;
                        let dur_ok = (span as usize) >= cfg.min_word_frames
                            && (span as usize) <= cfg.max_word_frames
                            && (span as f64) <= cfg.max_expansion * tlen as f64;
                        if dur_ok {
                            let local = (tok.cost - tok.entry - cfg.wip).max(0.0);
                            events.push(WordHyp {
                                word: tmpl.word.clone(),
                                instance: m,
                                start: tok.start,
                                end,
                                cost: tok.cost,
                                local,
                                norm: local / span as f64,
                            });
                            // To SIL at t.
                            relax(
                                &mut curr,
                                (Unit::Sil, 0),
                                Token {
                                    unit: Unit::Sil,
                                    pos: 0,
                                    start: t as u32,
                                    cost: tok.cost + frame_distance(&frames[t], &set.sil_mean),
                                    entry: tok.cost,
                                    bp_frame: (t - 1) as u32,
                                    bp_idx: pi as u32,
                                    stay: 0,
                                    words: tok.words + 1,
                                },
                            );
                            // To SP at t (1-frame pause, may extend).
                            relax(
                                &mut curr,
                                (Unit::Sp, 0),
                                Token {
                                    unit: Unit::Sp,
                                    pos: 0,
                                    start: t as u32,
                                    cost: tok.cost + frame_distance(&frames[t], &set.sil_mean),
                                    entry: tok.cost,
                                    bp_frame: (t - 1) as u32,
                                    bp_idx: pi as u32,
                                    stay: 0,
                                    words: tok.words + 1,
                                },
                            );
                            // Direct word-to-word (0-frame gap).
                            for (m2, tmpl2) in set.templates.iter().enumerate() {
                                if tmpl2.frames.is_empty() {
                                    continue;
                                }
                                relax(
                                    &mut curr,
                                    (Unit::Word(m2), 0),
                                    Token {
                                        unit: Unit::Word(m2),
                                        pos: 0,
                                        start: t as u32,
                                        cost: tok.cost
                                            + frame_distance(&frames[t], &tmpl2.frames[0])
                                            + cfg.wip,
                                        entry: tok.cost,
                                        bp_frame: (t - 1) as u32,
                                        bp_idx: pi as u32,
                                        stay: 0,
                                        words: tok.words + 1,
                                    },
                                );
                            }
                        }
                    }
                }
                Unit::Sil => {
                    // Self-loop (unbounded; acoustics prune speech).
                    relax(
                        &mut curr,
                        (Unit::Sil, 0),
                        Token {
                            unit: Unit::Sil,
                            pos: 0,
                            start: tok.start,
                            cost: tok.cost + frame_distance(&frames[t], &set.sil_mean),
                            entry: tok.entry,
                            bp_frame: (t - 1) as u32,
                            bp_idx: pi as u32,
                            stay: 0,
                            words: tok.words,
                        },
                    );
                    // SIL to word start.
                    for (m2, tmpl2) in set.templates.iter().enumerate() {
                        if tmpl2.frames.is_empty() {
                            continue;
                        }
                        relax(
                            &mut curr,
                            (Unit::Word(m2), 0),
                            Token {
                                unit: Unit::Word(m2),
                                pos: 0,
                                start: t as u32,
                                cost: tok.cost
                                    + frame_distance(&frames[t], &tmpl2.frames[0])
                                    + cfg.wip,
                                entry: tok.cost,
                                bp_frame: (t - 1) as u32,
                                bp_idx: pi as u32,
                                stay: 0,
                                words: tok.words,
                            },
                        );
                    }
                }
                Unit::Sp => {
                    // SP may extend to 2 frames total, then must exit.
                    let run = (t as u32).saturating_sub(tok.start) + 1;
                    if run <= 2 {
                        relax(
                            &mut curr,
                            (Unit::Sp, 0),
                            Token {
                                unit: Unit::Sp,
                                pos: 0,
                                start: tok.start,
                                cost: tok.cost + frame_distance(&frames[t], &set.sil_mean),
                                entry: tok.entry,
                                bp_frame: (t - 1) as u32,
                                bp_idx: pi as u32,
                                stay: 0,
                                words: tok.words,
                            },
                        );
                    }
                    // SP exit to word start or SIL.
                    for (m2, tmpl2) in set.templates.iter().enumerate() {
                        if tmpl2.frames.is_empty() {
                            continue;
                        }
                        relax(
                            &mut curr,
                            (Unit::Word(m2), 0),
                            Token {
                                unit: Unit::Word(m2),
                                pos: 0,
                                start: t as u32,
                                cost: tok.cost
                                    + frame_distance(&frames[t], &tmpl2.frames[0])
                                    + cfg.wip,
                                entry: tok.cost,
                                bp_frame: (t - 1) as u32,
                                bp_idx: pi as u32,
                                stay: 0,
                                words: tok.words,
                            },
                        );
                    }
                    relax(
                        &mut curr,
                        (Unit::Sil, 0),
                        Token {
                            unit: Unit::Sil,
                            pos: 0,
                            start: t as u32,
                            cost: tok.cost + frame_distance(&frames[t], &set.sil_mean),
                            entry: tok.entry,
                            bp_frame: (t - 1) as u32,
                            bp_idx: pi as u32,
                            stay: 0,
                            words: tok.words,
                        },
                    );
                }
            }
        }
        let col = prune(curr, cfg);
        if col.is_empty() {
            return Err(format!(
                "decode: beam empty at t={t}/{t_len} (beam={} max_live={} events={})",
                cfg.beam,
                cfg.max_live,
                events.len()
            ));
        }
        peak_live = peak_live.max(col.len());
        columns.push(col);
    }

    // Trailing word ends at T-1 (events for words ending on the last frame).
    let last = &columns[t_len - 1];
    for tok in last.iter() {
        if let Unit::Word(m) = tok.unit {
            let tlen = set.templates[m].frames.len();
            if tok.pos == tlen - 1 {
                let end = (t_len - 1) as u32;
                let span = end.saturating_sub(tok.start) + 1;
                let dur_ok = (span as usize) >= cfg.min_word_frames
                    && (span as usize) <= cfg.max_word_frames
                    && (span as f64) <= cfg.max_expansion * tlen as f64;
                if dur_ok
                    && !events
                        .iter()
                        .any(|e| e.instance == m && e.start == tok.start && e.end == end)
                {
                    let local = (tok.cost - tok.entry - cfg.wip).max(0.0);
                    events.push(WordHyp {
                        word: set.templates[m].word.clone(),
                        instance: m,
                        start: tok.start,
                        end,
                        cost: tok.cost,
                        local,
                        norm: local / span as f64,
                    });
                }
            }
        }
    }

    // Final: best SIL token if endpoints required, else global best word/SIL.
    let mut best: Option<(usize, &Token)> = None;
    for (i, tok) in last.iter().enumerate() {
        let ok = if cfg.sil_endpoints {
            matches!(tok.unit, Unit::Sil)
        } else {
            true
        };
        if !ok {
            continue;
        }
        // Word-final tokens must satisfy duration to close the utterance.
        if let Unit::Word(m) = tok.unit {
            let tlen = set.templates[m].frames.len();
            if tok.pos != tlen - 1 {
                continue;
            }
            let span = (t_len - 1) as u32 - tok.start + 1;
            if (span as usize) < cfg.min_word_frames || (span as usize) > cfg.max_word_frames {
                continue;
            }
        }
        if best.map(|(_, b)| tok.cost < b.cost).unwrap_or(true) {
            best = Some((i, tok));
        }
    }
    let (mut bi, btok) = best.ok_or_else(|| {
        format!(
            "decode: no final token (sil_endpoints={} last_live={} events={})",
            cfg.sil_endpoints,
            last.len(),
            events.len()
        )
    })?;
    let cost = btok.cost;
    let mut bt = (t_len - 1) as u32;
    let mut words_rev: Vec<WordHyp> = Vec::new();
    // Backtrace: first Word(m) encounter per (m,start) going back marks a word.
    let mut seen: std::collections::HashSet<(usize, u32)> = std::collections::HashSet::new();
    loop {
        let tok = &columns[bt as usize][bi];
        if let Unit::Word(m) = tok.unit
            && seen.insert((m, tok.start))
        {
            // End frame = furthest frame of this word on the backtrace =
            // the frame where we first met it going back.
            let local = (tok.cost - tok.entry - cfg.wip).max(0.0);
            // Reconstruct end: walk forward is unavailable; approximate
            // end as the backtrace frame of first encounter.
            words_rev.push(WordHyp {
                word: set.templates[m].word.clone(),
                instance: m,
                start: tok.start,
                end: bt,
                cost: tok.cost,
                local,
                norm: local / ((bt - tok.start + 1).max(1) as f64),
            });
        }
        if tok.bp_frame == u32::MAX {
            break;
        }
        bt = tok.bp_frame;
        bi = tok.bp_idx as usize;
    }
    words_rev.reverse();
    // Patch word ends: each word ends where the next starts (minus gap), last
    // word keeps its backtrace end. Backtrace ends are exact for the last
    // frame of each word only when the path exits immediately; the event
    // table holds exact ends — reconcile by matching (instance,start).
    for w in words_rev.iter_mut() {
        if let Some(e) = events
            .iter()
            .filter(|e| e.instance == w.instance && e.start == w.start)
            .min_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap())
        {
            w.end = e.end;
            w.cost = e.cost;
            w.local = e.local;
            w.norm = e.norm;
        }
    }
    Ok(DecodeResult {
        words: words_rev,
        events,
        norm_cost: cost / t_len as f64,
        cost,
        peak_live,
        frames: t_len,
    })
}

fn relax(map: &mut HashMap<StateKey, Token>, key: StateKey, tok: Token) {
    match map.get(&key) {
        Some(old) if old.cost <= tok.cost => {}
        _ => {
            map.insert(key, tok);
        }
    }
}

fn prune(map: HashMap<StateKey, Token>, cfg: &DecoderConfig) -> Vec<Token> {
    if map.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<Token> = map.into_values().collect();
    let best = v.iter().map(|t| t.cost).fold(f64::INFINITY, f64::min);
    v.retain(|t| t.cost <= best + cfg.beam);
    if v.len() > cfg.max_live {
        v.sort_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap());
        v.truncate(cfg.max_live);
    }
    v
}

/// Emit an A5-format word lattice from decode events (K = max kept per end).
///
/// Tokens keep the best instance per (word,start,end); per end-frame the top
/// `k_max` by norm cost survive. The 1-best chain (`one_best`) is ALWAYS kept
/// (source `a2-1best`, exempt from K truncation, chain arcs added) so the
/// lattice realizes the 1-best path and oracle WER <= 1-best WER by
/// construction. Arcs join temporally adjacent tokens with a gap tolerance
/// of `max_sp_frames + 2`. Generate ONCE per acoustic config; K sweeps
/// filter this output (preserving `a2-1best`).
pub fn emit_lattice(
    utterance: &str,
    events: &[WordHyp],
    one_best: &[WordHyp],
    k_max: usize,
    max_sp_frames: usize,
    frame_shift_ms: f64,
) -> Result<lattice::Lattice, String> {
    // Dedup to best instance per (word,start,end).
    let mut best: HashMap<(String, u32, u32), &WordHyp> = HashMap::new();
    for e in events.iter().chain(one_best.iter()) {
        let k = (e.word.clone(), e.start, e.end);
        match best.get(&k) {
            Some(old) if old.norm <= e.norm => {}
            _ => {
                best.insert(k, e);
            }
        }
    }
    // Group by end frame, keep top k_max by norm + all must-keep.
    let mut by_end: HashMap<u32, Vec<&WordHyp>> = HashMap::new();
    for e in best.values() {
        by_end.entry(e.end).or_default().push(*e);
    }
    let mut kept: Vec<&WordHyp> = Vec::new();
    for v in by_end.values_mut() {
        v.sort_by(|a, b| a.norm.partial_cmp(&b.norm).unwrap());
        v.truncate(k_max);
        kept.append(v);
    }
    // Re-add must-keep tokens dropped by truncation.
    for w in one_best {
        let k = (w.word.clone(), w.start, w.end);
        if !kept
            .iter()
            .any(|e| e.word == w.word && e.start == w.start && e.end == w.end)
            && let Some(e) = best.get(&k)
        {
            kept.push(*e);
        }
    }
    kept.sort_by_key(|e| (e.start, e.end));
    let on_best: std::collections::HashSet<(String, u32, u32)> = one_best
        .iter()
        .map(|w| (w.word.clone(), w.start, w.end))
        .collect();
    let mut tokens = Vec::with_capacity(kept.len());
    for (i, e) in kept.iter().enumerate() {
        if !e.norm.is_finite() {
            return Err(format!(
                "non-finite norm for {}:{}-{}",
                e.word, e.start, e.end
            ));
        }
        let src = if on_best.contains(&(e.word.clone(), e.start, e.end)) {
            "a2-1best"
        } else {
            "a2"
        };
        tokens.push(lattice::Token {
            id: i as u64,
            word: e.word.clone(),
            start: e.start,
            end: e.end,
            source: src.to_string(),
            acoustic_llr: -e.norm,
            lexical_llr: None,
            speaker_llr: None,
            verify_llr: None,
            user_llr: None,
        });
    }
    // Arcs via end-frame index (gap in [0, max_sp+2]).
    let gap_tol = max_sp_frames as u32 + 2;
    let mut by_end_id: HashMap<u32, Vec<u64>> = HashMap::new();
    for t in &tokens {
        by_end_id.entry(t.end).or_default().push(t.id);
    }
    // Index starts for forward scan: for each token, predecessors end in
    // [start-gap_tol, start].
    let mut arcs = Vec::new();
    for t in &tokens {
        let lo = t.start.saturating_sub(gap_tol);
        for end in lo..=t.start {
            if let Some(ids) = by_end_id.get(&end) {
                for &from in ids {
                    if from != t.id {
                        arcs.push(lattice::Arc { from, to: t.id });
                    }
                }
            }
        }
    }
    // Explicit 1-best chain arcs (guarantee the backbone path exists even
    // when a gap exceeds the tolerance).
    let id_of = |w: &WordHyp| {
        tokens
            .iter()
            .find(|t| t.word == w.word && t.start == w.start && t.end == w.end)
            .map(|t| t.id)
    };
    for pair in one_best.windows(2) {
        if let (Some(f), Some(t)) = (id_of(&pair[0]), id_of(&pair[1])) {
            arcs.push(lattice::Arc { from: f, to: t });
        }
    }
    arcs.sort_by_key(|a| (a.from, a.to));
    arcs.dedup_by(|a, b| a.from == b.from && a.to == b.to);
    Ok(lattice::Lattice {
        utterance: utterance.to_string(),
        frame_shift_ms,
        producer: "a2-dev".to_string(),
        tokens,
        arcs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::TemplateSet;

    fn synth_set() -> TemplateSet {
        // dim=2. A rises along x, B along y; SIL at origin.
        let a = vec![vec![0.0, 5.0], vec![1.0, 5.0], vec![2.0, 5.0]];
        let b = vec![vec![5.0, 0.0], vec![5.0, 1.0], vec![5.0, 2.0]];
        TemplateSet::synthetic(vec![("alpha", a), ("beta", b)], vec![5.0, 5.0])
    }

    fn cfg_tiny() -> DecoderConfig {
        DecoderConfig {
            beam: 100.0,
            max_live: 500,
            wip: 1.0,
            min_word_frames: 1,
            max_word_frames: 20,
            max_step: 2,
            max_stay: 2,
            max_expansion: 4.0,
            max_sp_frames: 4,
            sil_endpoints: false,
        }
    }

    #[test]
    fn decodes_concat_in_order() {
        let set = synth_set();
        // Phrase = alpha + beta frames exactly.
        let mut frames = Vec::new();
        frames.extend(set.templates[0].frames.clone());
        frames.extend(set.templates[1].frames.clone());
        let res = decode(&set, &frames, &cfg_tiny()).expect("decode");
        let words: Vec<&str> = res.words.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(words, vec!["alpha", "beta"]);
        assert!(res.peak_live <= 500);
    }

    #[test]
    fn single_word_roundtrip() {
        let set = synth_set();
        let frames = set.templates[1].frames.clone();
        let res = decode(&set, &frames, &cfg_tiny()).expect("decode");
        assert_eq!(res.words.len(), 1);
        assert_eq!(res.words[0].word, "beta");
    }

    #[test]
    fn empty_input_errors_loudly() {
        let set = synth_set();
        assert!(decode(&set, &[], &cfg_tiny()).is_err());
    }

    #[test]
    fn lattice_emit_dedups_and_arcs() {
        let ev = vec![
            WordHyp {
                word: "a".into(),
                instance: 0,
                start: 0,
                end: 2,
                cost: 3.0,
                local: 3.0,
                norm: 1.0,
            },
            WordHyp {
                word: "a".into(),
                instance: 1,
                start: 0,
                end: 2,
                cost: 6.0,
                local: 6.0,
                norm: 2.0,
            },
            WordHyp {
                word: "b".into(),
                instance: 2,
                start: 3,
                end: 5,
                cost: 9.0,
                local: 3.0,
                norm: 1.0,
            },
        ];
        let lat = emit_lattice("u", &ev, &ev[..1], 20, 30, 10.0).expect("emit");
        assert_eq!(lat.tokens.len(), 2);
        assert_eq!(lat.arcs.len(), 1);
        let text = lattice::write_lattice(&lat).expect("write");
        let back = lattice::read_lattice(&text).expect("read");
        assert_eq!(back.tokens.len(), 2);
    }
}
