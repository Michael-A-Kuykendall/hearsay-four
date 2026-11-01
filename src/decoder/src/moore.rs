//! `MooreDecoder`: minimal pronunciation-trie + token-passing `Recognizer`.
//!
//! Search: per-frame-synchronous Viterbi over (trie node, HMM state, LM
//! history) tokens with beam + per-frame cap pruning. Emissions come from
//! the file-loaded `HmmSet`; word-end transitions add LM log-prob (when an
//! LM file was supplied) plus word penalty. Output is a linear 1-best
//! lattice in the existing `lattice` types.
//!
//! v0 limitations (follow-up work, not bugs): single-best lattice only (no
//! alternates); `acoustic_llr` is the raw segment log-score, uncalibrated;
//! no explicit silence model (words tile the utterance back-to-back);
//! `stream` re-decodes the whole buffer per chunk (quadratic, correct).

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use hmm::HmmSet;
use lattice::{Arc, Lattice, Token};
use lm::persist::{interpolated_from_bytes, kn_from_bytes};
use lm::{InterpolatedLm, KnTrigram};

use crate::trie::{PronEntry, PronTrie};
use crate::{Partial, Recognizer};

/// Producer/source tag written into emitted lattices.
pub const PRODUCER: &str = "decoder-r5a-v0";

/// LM history sentinel for "no word yet" (scores as `lm::BOS_TOKEN`).
const NO_WORD: u32 = u32::MAX;

/// Search configuration. All scores are natural-log domain.
#[derive(Debug, Clone)]
pub struct DecoderConfig {
    /// Log-domain beam width around the per-frame best (default 150).
    pub beam: f64,
    /// Maximum tokens kept per frame after pruning (default 2000).
    pub max_tokens: usize,
    /// Per-word additive bonus/penalty at word end (default 0).
    pub word_penalty: f64,
    /// LM log-prob weight (default 1).
    pub lm_weight: f64,
    /// Trie-node LM-lookahead weight in the pruning rank (default 1).
    ///
    /// Lookahead guides which tokens survive the beam; stored path scores
    /// stay exact. 0 disables lookahead (rank = path score).
    pub lookahead_weight: f64,
    /// Per-utterance CMVN on static features before deltas (hmm-fe2,
    /// default true). Set false ONLY to decode pre-CMVN acoustic models.
    pub cmvn: bool,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            beam: 150.0,
            max_tokens: 2000,
            word_penalty: 0.0,
            lm_weight: 1.0,
            lookahead_weight: 1.0,
            cmvn: true,
        }
    }
}

/// Either LM file flavor `lm::persist` reads (interfaces-v1 §I3).
enum LmKind {
    Kn(Box<KnTrigram>),
    Ix(Box<InterpolatedLm>),
}

impl LmKind {
    fn log_prob(&self, w1: &str, w2: &str, w: &str) -> f64 {
        let p = match self {
            LmKind::Kn(m) => m.prob(w1, w2, w),
            LmKind::Ix(m) => m.prob(w1, w2, w),
        };
        p.max(1e-12).ln()
    }
    /// Static per-word prior for trie-node lookahead (continuation unigram;
    /// convex mix with the same weight as [`InterpolatedLm::prob`]).
    fn unigram_prob(&self, w: &str) -> f64 {
        match self {
            LmKind::Kn(m) => m.unigram_prob(w),
            LmKind::Ix(m) => {
                let lambda = m.personal_weight();
                ((1.0 - lambda) * m.general().unigram_prob(w)
                    + lambda * m.personal().unigram_prob(w))
                .clamp(0.0, 1.0)
            }
        }
    }
}

/// Per-phone HMM topology in log domain.
///
/// `hmm` transition matrices are `(n+2)x(n+2)`: index 0 is entry, `1..=n`
/// are the emitting states, `n+1` is exit.
#[derive(Debug, Clone)]
struct PhoneTopology {
    n_states: usize,
    loop_lp: Vec<f64>,
    next_lp: Vec<f64>,
    exit_lp: f64,
}

/// One completed word on a hypothesis path.
#[derive(Clone)]
struct WordLink {
    word: String,
    start: u32,
    end: u32,
    acoustic: f64,
    prev: Option<Rc<WordLink>>,
}

/// One Viterbi token: (trie node, HMM state, LM history) + path.
/// Keyed by `(node, state, w1, w2)`; recombination is exact per key.
#[derive(Clone)]
struct VToken {
    score: f64,
    word_start: u32,
    word_acoustic: f64,
    path: Option<Rc<WordLink>>,
}

type Key = (usize, usize, u32, u32);

/// Trie decoder over a file-loaded acoustic model and optional LM file.
pub struct MooreDecoder {
    am: HmmSet,
    topo: HashMap<String, PhoneTopology>,
    trie: PronTrie,
    /// Per-trie-node LM lookahead (max unigram log-prob over the subtree),
    /// aligned with `trie.nodes`; all zeros when no LM is loaded.
    lookahead: Vec<f64>,
    id_words: Vec<String>,
    word_ids: HashMap<String, u32>,
    lm: Option<LmKind>,
    cfg: DecoderConfig,
    skipped_words: Vec<String>,
    stream_buf: Vec<i16>,
    /// Running CMVN estimate for the live path (cumulative over the buffer).
    cmvn_live: fse::CmvnLive,
    /// Buffer windows already folded into `cmvn_live`.
    cmvn_windows: usize,
}

impl MooreDecoder {
    /// Load a decoder from model files.
    ///
    /// - `am_path`: acoustic `model.json` (interfaces-v1 §I2 schema).
    /// - `lm_path`: optional `lm::persist` file (plain or interpolated).
    /// - `dict_text`: CMUdict-format pronunciation text.
    ///
    /// Words whose phones are outside the lexicon alphabet or missing from
    /// the acoustic inventory are skipped loudly (see `skipped_words`);
    /// malformed dict lines fail the load.
    pub fn load(
        am_path: &Path,
        lm_path: Option<&Path>,
        dict_text: &str,
        cfg: DecoderConfig,
    ) -> Result<Self, String> {
        let am = HmmSet::load_text(am_path)
            .map_err(|e| format!("acoustic model {}: {e:?}", am_path.display()))?;
        if am.dim != fse::FEATURE_DIM {
            return Err(format!(
                "acoustic dim {} != frontend dim {}",
                am.dim,
                fse::FEATURE_DIM
            ));
        }
        let mut entries: Vec<PronEntry> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for (lineno, line) in dict_text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(";;;") {
                continue;
            }
            match lexicon::parse_cmudict_line(line) {
                Some(entry) => {
                    if !lexicon::phones_in_alphabet(&entry.phones) {
                        skipped.push(format!("{} (non-alphabet phone)", entry.word));
                        continue;
                    }
                    let missing: Vec<&str> = entry
                        .phones
                        .iter()
                        .map(String::as_str)
                        .filter(|p| !am.phones.contains_key(*p))
                        .collect();
                    if !missing.is_empty() {
                        skipped.push(format!(
                            "{} (missing from AM: {})",
                            entry.word,
                            missing.join(",")
                        ));
                        continue;
                    }
                    entries.push(PronEntry {
                        word: entry.word,
                        phones: entry.phones,
                    });
                }
                None => {
                    return Err(format!("dict line {} malformed: {line:?}", lineno + 1));
                }
            }
        }
        if entries.is_empty() {
            return Err("dict produced zero usable entries".to_string());
        }
        let lm = match lm_path {
            None => None,
            Some(path) => Some(Self::load_lm(path)?),
        };
        Self::assemble(am, lm, entries, skipped, cfg)
    }
    /// Assemble a decoder from a loaded AM, an optional LM, and pronunciation
    /// entries. Shared by [`MooreDecoder::load`] (CMUdict text) and
    /// [`MooreDecoder::load_with_vocab`] (programmatic vocab).
    fn assemble(
        am: HmmSet,
        lm: Option<LmKind>,
        entries: Vec<PronEntry>,
        skipped: Vec<String>,
        cfg: DecoderConfig,
    ) -> Result<Self, String> {
        let mut topo: HashMap<String, PhoneTopology> = HashMap::new();
        for entry in &entries {
            for phone in &entry.phones {
                if !topo.contains_key(phone) {
                    topo.insert(phone.clone(), Self::topology(&am, phone)?);
                }
            }
        }
        let trie = PronTrie::build(&entries);
        let lookahead = match &lm {
            None => vec![0.0; trie.nodes.len()],
            Some(lm) => {
                let unk = lm.unigram_prob(lm::UNK_TOKEN).max(1e-12).ln();
                let mut scores = HashMap::with_capacity(entries.len());
                for entry in &entries {
                    scores.insert(
                        entry.word.clone(),
                        lm.unigram_prob(&entry.word.to_lowercase()).max(1e-12).ln(),
                    );
                }
                trie.build_lookahead(&scores, unk)
            }
        };
        let mut id_words: Vec<String> = entries.iter().map(|e| e.word.clone()).collect();
        id_words.sort();
        id_words.dedup();
        let word_ids: HashMap<String, u32> = id_words
            .iter()
            .enumerate()
            .map(|(i, w)| (w.clone(), i as u32))
            .collect();
        Ok(Self {
            am,
            topo,
            trie,
            lookahead,
            id_words,
            word_ids,
            lm,
            cfg,
            skipped_words: skipped,
            stream_buf: Vec::new(),
            cmvn_live: fse::CmvnLive::new(),
            cmvn_windows: 0,
        })
    }
    /// Load a decoder from an acoustic model file, an optional LM file, and
    /// programmatic pronunciation entries (the large-vocab path).
    ///
    /// Entries with non-alphabet phones or phones missing from the acoustic
    /// inventory are skipped loudly (see [`MooreDecoder::skipped_words`]);
    /// a surviving set of zero fails the load.
    pub fn load_with_vocab(
        am_path: &Path,
        lm_path: Option<&Path>,
        entries: &[PronEntry],
        cfg: DecoderConfig,
    ) -> Result<Self, String> {
        let am = HmmSet::load_text(am_path)
            .map_err(|e| format!("acoustic model {}: {e:?}", am_path.display()))?;
        if am.dim != fse::FEATURE_DIM {
            return Err(format!(
                "acoustic dim {} != frontend dim {}",
                am.dim,
                fse::FEATURE_DIM
            ));
        }
        let mut kept: Vec<PronEntry> = Vec::with_capacity(entries.len());
        let mut skipped: Vec<String> = Vec::new();
        for entry in entries {
            if entry.phones.is_empty() || !lexicon::phones_in_alphabet(&entry.phones) {
                skipped.push(format!("{} (non-alphabet phone)", entry.word));
                continue;
            }
            let missing: Vec<&str> = entry
                .phones
                .iter()
                .map(String::as_str)
                .filter(|p| !am.phones.contains_key(*p))
                .collect();
            if !missing.is_empty() {
                skipped.push(format!(
                    "{} (missing from AM: {})",
                    entry.word,
                    missing.join(",")
                ));
                continue;
            }
            kept.push(entry.clone());
        }
        if kept.is_empty() {
            return Err("vocab produced zero usable entries".to_string());
        }
        let lm = match lm_path {
            None => None,
            Some(path) => Some(Self::load_lm(path)?),
        };
        Self::assemble(am, lm, kept, skipped, cfg)
    }
    /// Words skipped at load with reasons. Empty means full coverage.
    pub fn skipped_words(&self) -> &[String] {
        &self.skipped_words
    }

    /// Number of distinct words in the search trie.
    pub fn vocab_size(&self) -> usize {
        self.trie.word_count()
    }

    /// True when an LM file was loaded (LM inside search).
    pub fn has_lm(&self) -> bool {
        self.lm.is_some()
    }

    /// Clear the streaming buffer (next `stream` chunk starts fresh).
    pub fn reset_stream(&mut self) {
        self.stream_buf.clear();
        self.cmvn_live = fse::CmvnLive::new();
        self.cmvn_windows = 0;
    }

    fn topology(am: &HmmSet, phone: &str) -> Result<PhoneTopology, String> {
        let h = am
            .phones
            .get(phone)
            .ok_or_else(|| format!("phone {phone} missing from AM"))?;
        let n = h.n_states;
        if n == 0 || h.trans.len() != n + 2 || h.trans.iter().any(|r| r.len() != n + 2) {
            return Err(format!("phone {phone}: bad transition shape"));
        }
        let mut loop_lp = vec![0.0; n];
        let mut next_lp = vec![f64::NEG_INFINITY; n];
        for s in 0..n {
            loop_lp[s] = h.trans[s + 1][s + 1].max(1e-300).ln();
            if s + 1 < n {
                next_lp[s] = h.trans[s + 1][s + 2].max(1e-300).ln();
            }
        }
        let exit_lp = h.trans[n][n + 1].max(1e-300).ln();
        Ok(PhoneTopology {
            n_states: n,
            loop_lp,
            next_lp,
            exit_lp,
        })
    }

    fn load_lm(path: &Path) -> Result<LmKind, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("lm {}: {e}", path.display()))?;
        if let Ok(model) = interpolated_from_bytes(&bytes) {
            return Ok(LmKind::Ix(Box::new(model)));
        }
        if let Ok(model) = kn_from_bytes(&bytes) {
            return Ok(LmKind::Kn(Box::new(model)));
        }
        Err(format!(
            "lm {}: neither interpolated nor plain archive",
            path.display()
        ))
    }

    /// Canonical frontend: 16 kHz mono → 39-dim MFCC frames (same `fse`
    /// pipeline the acoustic model trained on). CMVN per `cfg.cmvn`.
    fn frontend(&self, pcm16k: &[i16]) -> Vec<fse::Frame> {
        if pcm16k.is_empty() {
            return Vec::new();
        }
        let windows = fse::window_audio(pcm16k, 16_000);
        if windows.is_empty() {
            return Vec::new();
        }
        if self.cfg.cmvn {
            fse::extract_features_cmvn(&windows, 16_000).unwrap_or_default()
        } else {
            fse::extract_features(&windows, 16_000).unwrap_or_default()
        }
    }

    /// Live frontend: like `frontend`, but CMVN uses the cumulative running
    /// estimate over the buffer (new windows folded per call), applied only
    /// past ~1 s (100 frames); raw when `cfg.cmvn` is false.
    fn frontend_live(&mut self, pcm16k: &[i16]) -> Vec<fse::Frame> {
        if pcm16k.is_empty() {
            return Vec::new();
        }
        let windows = fse::window_audio(pcm16k, 16_000);
        if windows.is_empty() {
            return Vec::new();
        }
        if !self.cfg.cmvn {
            return fse::extract_features(&windows, 16_000).unwrap_or_default();
        }
        let Ok(statics) = fse::extract_statics(&windows, 16_000) else {
            return Vec::new();
        };
        let fresh = self.cmvn_windows.min(statics.len());
        self.cmvn_live.update(&statics[fresh..]);
        self.cmvn_windows = statics.len();
        let stats = if statics.len() >= 100 {
            self.cmvn_live.stats()
        } else {
            None
        };
        let borrowed = stats.as_ref().map(|(m, s)| (m.as_slice(), s.as_slice()));
        fse::extract_features_with_stats(&windows, 16_000, borrowed).unwrap_or_default()
    }

    fn lm_log_prob(&self, w1: u32, w2: u32, word: &str) -> f64 {
        match &self.lm {
            None => 0.0,
            Some(lm) => {
                let h1 = if w1 == NO_WORD {
                    lm::BOS_TOKEN.to_string()
                } else {
                    self.id_words[w1 as usize].to_lowercase()
                };
                let h2 = if w2 == NO_WORD {
                    lm::BOS_TOKEN.to_string()
                } else {
                    self.id_words[w2 as usize].to_lowercase()
                };
                lm.log_prob(&h1, &h2, &word.to_lowercase())
            }
        }
    }

    fn emit_cached(
        &self,
        cache: &mut HashMap<(usize, usize), f64>,
        node: usize,
        state: usize,
        frame: &fse::Frame,
    ) -> f64 {
        if let Some(&score) = cache.get(&(node, state)) {
            return score;
        }
        let score = self.trie.nodes[node]
            .phone
            .as_ref()
            .and_then(|phone| self.am.emission(phone, state).ok())
            .map(|mixture| mixture.log_prob(frame))
            .unwrap_or(f64::NEG_INFINITY);
        cache.insert((node, state), score);
        score
    }

    fn insert_tok(next: &mut BTreeMap<Key, VToken>, key: Key, tok: VToken) {
        match next.get(&key) {
            Some(old) if old.score >= tok.score => {}
            _ => {
                next.insert(key, tok);
            }
        }
    }

    /// Expand one token at frame `t` into frame `t_next` (emission frame).
    #[allow(clippy::too_many_arguments)]
    fn expand(
        &self,
        next: &mut BTreeMap<Key, VToken>,
        cache: &mut HashMap<(usize, usize), f64>,
        node: usize,
        state: usize,
        w1: u32,
        w2: u32,
        tok: &VToken,
        frame: &fse::Frame,
        t_next: u32,
    ) {
        let phone = match &self.trie.nodes[node].phone {
            Some(phone) => phone,
            None => return,
        };
        let topo = match self.topo.get(phone) {
            Some(topo) => topo,
            None => return,
        };
        // Self-loop.
        {
            let emit = self.emit_cached(cache, node, state, frame);
            let mut nt = tok.clone();
            nt.score += topo.loop_lp[state] + emit;
            nt.word_acoustic += topo.loop_lp[state] + emit;
            Self::insert_tok(next, (node, state, w1, w2), nt);
        }
        // Next state, or phone exit.
        if state + 1 < topo.n_states {
            let emit = self.emit_cached(cache, node, state + 1, frame);
            let mut nt = tok.clone();
            nt.score += topo.next_lp[state] + emit;
            nt.word_acoustic += topo.next_lp[state] + emit;
            Self::insert_tok(next, (node, state + 1, w1, w2), nt);
        } else {
            // Continue into child phones (longer words sharing the prefix).
            for &child in &self.trie.nodes[node].children {
                let emit = self.emit_cached(cache, child, 0, frame);
                let mut nt = tok.clone();
                nt.score += topo.exit_lp + emit;
                nt.word_acoustic += topo.exit_lp + emit;
                Self::insert_tok(next, (child, 0, w1, w2), nt);
            }
            // Word ends: apply LM + penalty, restart at trie root.
            for word in &self.trie.nodes[node].words {
                let wid = self.word_ids[word];
                let lmscore =
                    self.cfg.lm_weight * self.lm_log_prob(w1, w2, word) + self.cfg.word_penalty;
                let link = Rc::new(WordLink {
                    word: word.clone(),
                    start: tok.word_start,
                    end: t_next,
                    acoustic: tok.word_acoustic + topo.exit_lp,
                    prev: tok.path.clone(),
                });
                for &child in &self.trie.nodes[0].children {
                    let emit = self.emit_cached(cache, child, 0, frame);
                    let nt = VToken {
                        score: tok.score + topo.exit_lp + lmscore + emit,
                        word_start: t_next,
                        word_acoustic: emit,
                        path: Some(link.clone()),
                    };
                    Self::insert_tok(next, (child, 0, w2, wid), nt);
                }
            }
        }
    }

    /// Beam + histogram prune. Tokens rank by path score plus the weighted
    /// trie-node LM lookahead; stored scores are never modified, so the
    /// search objective stays exact with or without lookahead.
    fn prune(&self, toks: &mut BTreeMap<Key, VToken>) {
        if toks.is_empty() {
            return;
        }
        let rank = |node: usize, tok: &VToken| {
            tok.score + self.cfg.lookahead_weight * self.lookahead.get(node).copied().unwrap_or(0.0)
        };
        let best = toks
            .iter()
            .map(|(&(node, _, _, _), t)| rank(node, t))
            .fold(f64::NEG_INFINITY, f64::max);
        toks.retain(|&(node, _, _, _), t| rank(node, t) >= best - self.cfg.beam);
        if toks.len() > self.cfg.max_tokens {
            let mut ranked: Vec<(Key, f64)> =
                toks.iter().map(|(k, t)| (*k, rank(k.0, t))).collect();
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            ranked.truncate(self.cfg.max_tokens);
            let keep: HashSet<Key> = ranked.into_iter().map(|(k, _)| k).collect();
            toks.retain(|k, _| keep.contains(k));
        }
    }

    fn empty_lattice(&self, nframes: u32) -> Lattice {
        let _ = nframes;
        Lattice {
            utterance: "decode-empty".to_string(),
            frame_shift_ms: fse::SHIFT_MS,
            producer: PRODUCER.to_string(),
            tokens: Vec::new(),
            arcs: Vec::new(),
        }
    }

    fn lattice_from_path(&self, path: &Option<Rc<WordLink>>, nframes: u32) -> Lattice {
        let mut links: Vec<&WordLink> = Vec::new();
        let mut cursor = path.as_ref();
        while let Some(link) = cursor {
            links.push(link);
            cursor = link.prev.as_ref();
        }
        links.reverse();
        let mut tokens: Vec<Token> = Vec::with_capacity(links.len());
        let mut arcs: Vec<Arc> = Vec::new();
        for (i, link) in links.iter().enumerate() {
            let id = (i + 1) as u64;
            tokens.push(Token {
                id,
                word: link.word.clone(),
                start: link.start,
                end: link.end,
                source: PRODUCER.to_string(),
                // v0: raw segment log-score, uncalibrated (NOT a true llr).
                acoustic_llr: finite_or(link.acoustic, -1e9),
                lexical_llr: None,
                speaker_llr: None,
                verify_llr: None,
                user_llr: None,
            });
            if i > 0 {
                arcs.push(Arc {
                    from: id - 1,
                    to: id,
                });
            }
        }
        Lattice {
            utterance: format!("decode-{nframes}frames"),
            frame_shift_ms: fse::SHIFT_MS,
            producer: PRODUCER.to_string(),
            tokens,
            arcs,
        }
    }

    /// Decode to (lattice, total path score, frame count).
    fn decode_inner(&self, frames: &[fse::Frame]) -> (Lattice, f64, u32) {
        let nframes = frames.len() as u32;
        if frames.is_empty() || self.trie.is_empty() {
            return (self.empty_lattice(nframes), f64::NEG_INFINITY, 0);
        }
        let mut cache: HashMap<(usize, usize), f64> = HashMap::new();
        let mut cur: BTreeMap<Key, VToken> = BTreeMap::new();
        for &child in &self.trie.nodes[0].children {
            let emit = self.emit_cached(&mut cache, child, 0, &frames[0]);
            cur.insert(
                (child, 0, NO_WORD, NO_WORD),
                VToken {
                    score: emit,
                    word_start: 0,
                    word_acoustic: emit,
                    path: None,
                },
            );
        }
        self.prune(&mut cur);
        for t in 0..frames.len().saturating_sub(1) {
            let mut next: BTreeMap<Key, VToken> = BTreeMap::new();
            let mut cache: HashMap<(usize, usize), f64> = HashMap::new();
            let t_next = (t + 1) as u32;
            for (&(node, state, w1, w2), tok) in cur.iter() {
                self.expand(
                    &mut next,
                    &mut cache,
                    node,
                    state,
                    w1,
                    w2,
                    tok,
                    &frames[t + 1],
                    t_next,
                );
            }
            cur = next;
            self.prune(&mut cur);
            if cur.is_empty() {
                break;
            }
        }
        // Flush: complete trailing words ending exactly at the final frame.
        let mut best: Option<(f64, Option<Rc<WordLink>>)> = None;
        for ((node, state, w1, w2), tok) in cur.iter() {
            let words = &self.trie.nodes[*node].words;
            if words.is_empty() {
                continue;
            }
            let phone = match &self.trie.nodes[*node].phone {
                Some(phone) => phone,
                None => continue,
            };
            let topo = match self.topo.get(phone) {
                Some(topo) => topo,
                None => continue,
            };
            if *state + 1 != topo.n_states {
                continue;
            }
            for word in words {
                let lmscore =
                    self.cfg.lm_weight * self.lm_log_prob(*w1, *w2, word) + self.cfg.word_penalty;
                let score = tok.score + topo.exit_lp + lmscore;
                let link = Rc::new(WordLink {
                    word: word.clone(),
                    start: tok.word_start,
                    end: nframes,
                    acoustic: tok.word_acoustic + topo.exit_lp,
                    prev: tok.path.clone(),
                });
                let better = best.as_ref().map(|(s, _)| score > *s).unwrap_or(true);
                if better {
                    best = Some((score, Some(link)));
                }
            }
        }
        match best {
            Some((score, path)) => (self.lattice_from_path(&path, nframes), score, nframes),
            None => {
                // No word completed: emit the best prefix path (may be empty).
                let tail = cur
                    .values()
                    .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(Ordering::Equal));
                match tail {
                    Some(tok) => (
                        self.lattice_from_path(&tok.path, nframes),
                        tok.score,
                        nframes,
                    ),
                    None => (self.empty_lattice(nframes), f64::NEG_INFINITY, nframes),
                }
            }
        }
    }
}

impl Recognizer for MooreDecoder {
    fn decode(&self, pcm16k: &[i16]) -> Lattice {
        let frames = self.frontend(pcm16k);
        self.decode_inner(&frames).0
    }

    fn stream(&mut self, chunk: &[i16]) -> Vec<Partial> {
        // v0: re-decode the whole buffer per chunk. Quadratic but correct;
        // incremental search is follow-up work.
        self.stream_buf.extend_from_slice(chunk);
        let buffered = self.stream_buf.clone();
        let frames = self.frontend_live(&buffered);
        let (lattice, score, nframes) = self.decode_inner(&frames);
        let text = lattice
            .tokens
            .iter()
            .map(|t| t.word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        vec![Partial {
            text,
            frame_end: nframes.saturating_sub(1),
            score,
        }]
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn am_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/si_base/model.json")
    }

    /// Lookahead wiring: with an LM, per-node values are finite unigram
    /// log-probs with the root at the max; without an LM, all zeros.
    #[test]
    fn lookahead_wired_from_lm() {
        let dict = "HOTBED HH AA T B EH D\nRIPPLE R IH P AH L\n";
        let plain = MooreDecoder::load(&am_path(), None, dict, DecoderConfig::default())
            .expect("load without lm");
        assert!(plain.lookahead.iter().all(|&v| v == 0.0));

        let sentences: Vec<Vec<String>> = vec![
            "a hotbed".split(' ').map(str::to_string).collect(),
            "b hotbed".split(' ').map(str::to_string).collect(),
            "c ripple".split(' ').map(str::to_string).collect(),
        ];
        let model = KnTrigram::train(&sentences);
        assert!(
            model.unigram_prob("hotbed") > model.unigram_prob("ripple"),
            "toy LM must rank hotbed above ripple"
        );
        let lm_path = std::env::temp_dir().join("decoder-lookahead-test-lm.kn");
        lm::persist::save_kn(&model, &lm_path).expect("save test lm");
        let with_lm =
            MooreDecoder::load(&am_path(), Some(&lm_path), dict, DecoderConfig::default())
                .expect("load with lm");
        std::fs::remove_file(&lm_path).ok();
        assert_eq!(with_lm.lookahead.len(), with_lm.trie.nodes.len());
        assert!(with_lm.lookahead.iter().all(|v| v.is_finite()));
        assert!(with_lm.lookahead.iter().all(|&v| v <= 0.0));
        // Root carries the max; the hotbed branch outranks ripple's.
        let max = with_lm
            .lookahead
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(with_lm.lookahead[0], max);
        let h = with_lm.trie.child(0, "HH").expect("HH child");
        let r = with_lm.trie.child(0, "R").expect("R child");
        assert!(
            with_lm.lookahead[h] > with_lm.lookahead[r],
            "hotbed branch {:?} must outrank ripple branch {:?}",
            with_lm.lookahead[h],
            with_lm.lookahead[r]
        );
    }
}
