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
    /// Word-internal triphone expansion (default false). When true, the
    /// trie is built over per-position unit keys (`L-C+R` word-internally,
    /// biphone at the edges, monophone for single-phone words) and each
    /// arc's acoustics are looked up in the tied model, falling back to
    /// the center monophone for units absent from the AM. Word identity
    /// and LM scoring still see words only — units never leak past the
    /// acoustic lookup. Cross-word contexts are excluded.
    pub triphone: bool,
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
            triphone: false,
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
    /// Creation sequence within one decode; links predecessor records.
    seq: u64,
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

/// Word-end hypotheses kept per end frame in k-best lattices.
const KBEST_PER_END_FRAME: usize = 10;

/// One recorded word-end hypothesis: every word end created during token
/// passing is recorded with the full path score; the k-best lattice keeps
/// the top [`KBEST_PER_END_FRAME`] per end frame by `path_score`, plus the
/// 1-best path links so the 1-best transcript stays reproducible.
#[derive(Clone, Debug)]
struct WordEndHyp {
    seq: u64,
    word: String,
    start: u32,
    end: u32,
    /// log P(frames[start..end] | word model): transitions + emissions.
    acoustic: f64,
    /// Full path score at the word end (acoustic + LM + penalties).
    /// Ranking only: never written into the lattice (contract §6).
    path_score: f64,
    prev_seq: Option<u64>,
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
    /// Frontend guard: the acoustic model must have trained on the features
    /// this config selects (CMVN id when `cfg.cmvn`, raw id otherwise).
    /// Legacy unstamped models warn loudly and proceed; an explicit
    /// mismatch is refused loudly.
    fn guard_frontend(am: &HmmSet, am_path: &Path, cfg: &DecoderConfig) -> Result<(), String> {
        let selected = if cfg.cmvn {
            fse::HMM_FRONTEND_ID
        } else {
            fse::HMM_FRONTEND_ID_RAW
        };
        if let Err(e) =
            hmm::check_frontend(&am.frontend_id, selected, &am_path.display().to_string())
        {
            let msg = format!("FRONTEND GUARD: {e:?}");
            eprintln!("{msg}");
            return Err(msg);
        }
        Ok(())
    }

    /// Load a decoder from model files.
    ///
    /// - `am_path`: acoustic `model.json` (interfaces-v1 §I2 schema).
    /// - `lm_path`: optional `lm::persist` file (plain or interpolated).
    /// - `dict_text`: CMUdict-format pronunciation text.
    ///
    /// Words whose phones are outside the lexicon alphabet or missing from
    /// the acoustic inventory are skipped loudly (see `skipped_words`);
    /// malformed dict lines fail the load. An acoustic model stamped for
    /// the other frontend (see `cfg.cmvn`) is refused loudly; legacy
    /// unstamped models warn and proceed. In triphone mode (`cfg.triphone`)
    /// the coverage check still maps centers: unit keys absent from the AM
    /// fall back to their center monophone at search time, so only a
    /// missing center skips a word.
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
        Self::guard_frontend(&am, am_path, &cfg)?;
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
            if cfg.triphone {
                for key in hmm::triphone::unit_keys_for_word(&entry.phones) {
                    if !topo.contains_key(&key) {
                        topo.insert(key.clone(), Self::topology(&am, &key)?);
                    }
                }
            } else {
                for phone in &entry.phones {
                    if !topo.contains_key(phone) {
                        topo.insert(phone.clone(), Self::topology(&am, phone)?);
                    }
                }
            }
        }
        let trie = if cfg.triphone {
            PronTrie::build_triphone(&entries)
        } else {
            PronTrie::build(&entries)
        };
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
    /// a surviving set of zero fails the load. Frontend-guard refusal is
    /// the same as [`MooreDecoder::load`]: a model stamped for the other
    /// frontend (see `cfg.cmvn`) fails; legacy unstamped models proceed.
    /// In triphone mode (`cfg.triphone`) the coverage check still maps
    /// centers: unit keys absent from the AM fall back to their center
    /// monophone at search time, so only a missing center skips an entry.
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
        Self::guard_frontend(&am, am_path, &cfg)?;
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
    /// Decode 16 kHz mono PCM to a k-best word lattice: top
    /// [`KBEST_PER_END_FRAME`] word ends per end frame by path score plus
    /// the 1-best path, arcs from predecessor links, one SIL twin per word
    /// token. Word `acoustic_llr` is the contract §3 ACOUSTIC llr (word
    /// model minus SIL null over exactly the token frames, no LM term).
    /// The 1-best path is always present; use [`Recognizer::decode`] for
    /// the plain 1-best lattice.
    pub fn decode_kbest(&self, pcm16k: &[i16]) -> Lattice {
        let frames = self.frontend(pcm16k);
        self.decode_inner_kbest(&frames).0
    }
    /// Clear the streaming buffer (next `stream` chunk starts fresh).
    pub fn reset_stream(&mut self) {
        self.stream_buf.clear();
        self.cmvn_live = fse::CmvnLive::new();
        self.cmvn_windows = 0;
    }

    /// Resolve a trie arc label to the acoustic inventory key: the label
    /// itself when the model carries it, else its center monophone
    /// (`L-C+R`/`L-C`/`C+R` → `C`; monophone labels are their own
    /// centers, so this is a no-op outside triphone mode).
    fn acoustic_key<'a>(am: &'a HmmSet, key: &'a str) -> &'a str {
        if am.phones.contains_key(key) {
            key
        } else {
            hmm::triphone::center_of_unit(key)
        }
    }

    fn topology(am: &HmmSet, key: &str) -> Result<PhoneTopology, String> {
        let resolved = Self::acoustic_key(am, key);
        let h = am
            .phones
            .get(resolved)
            .ok_or_else(|| format!("phone {key} missing from AM"))?;
        let n = h.n_states;
        if n == 0 || h.trans.len() != n + 2 || h.trans.iter().any(|r| r.len() != n + 2) {
            return Err(format!("phone {key}: bad transition shape"));
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
            .and_then(|key| {
                self.am
                    .emission(Self::acoustic_key(&self.am, key), state)
                    .ok()
            })
            .map(|mixture| mixture.log_prob(frame))
            .unwrap_or(f64::NEG_INFINITY);
        cache.insert((node, state), score);
        score
    }
    /// Per-frame SIL null scores: best-state-per-frame emission log-prob
    /// under the SIL HMM (contract §3 ACOUSTIC null). `None` when the
    /// loaded AM has no usable SIL phone.
    fn sil_frame_nulls(&self, frames: &[fse::Frame]) -> Option<Vec<f64>> {
        let sil = self.am.phones.get("SIL")?;
        if sil.n_states == 0 {
            return None;
        }
        let mut nulls = Vec::with_capacity(frames.len());
        for frame in frames {
            let mut best = f64::NEG_INFINITY;
            let mut found = false;
            for s in 0..sil.n_states {
                if let Ok(mix) = self.am.emission("SIL", s) {
                    found = true;
                    best = best.max(mix.log_prob(frame));
                }
            }
            if !found {
                return None;
            }
            nulls.push(best);
        }
        Some(nulls)
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
        seq_next: &mut u64,
        mut record: Option<&mut Vec<WordEndHyp>>,
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
                let seq = *seq_next;
                *seq_next += 1;
                let acoustic = tok.word_acoustic + topo.exit_lp;
                let path_score = tok.score + topo.exit_lp + lmscore;
                if let Some(hyps) = record.as_mut() {
                    hyps.push(WordEndHyp {
                        seq,
                        word: word.clone(),
                        start: tok.word_start,
                        end: t_next,
                        acoustic,
                        path_score,
                        prev_seq: tok.path.as_ref().map(|p| p.seq),
                    });
                }
                let link = Rc::new(WordLink {
                    seq,
                    word: word.clone(),
                    start: tok.word_start,
                    end: t_next,
                    acoustic,
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
    /// Build the k-best word lattice from recorded word-end hypotheses:
    /// top [`KBEST_PER_END_FRAME`] per end frame by path score plus the
    /// 1-best path, arcs from predecessor links, one SIL twin per word
    /// token (same span, `acoustic_llr` 0). Word `acoustic_llr` is the
    /// contract §3 ACOUSTIC llr: word-model log-score minus the SIL-null
    /// sum over exactly the token frames, no LM term. Falls back to the
    /// legacy 1-best lattice when the AM has no SIL phone.
    fn lattice_from_kbest(
        &self,
        frames: &[fse::Frame],
        hyps: &[WordEndHyp],
        best: &Option<Rc<WordLink>>,
    ) -> Lattice {
        let nframes = frames.len() as u32;
        let mut best_seqs: HashSet<u64> = HashSet::new();
        let mut cursor = best.as_ref();
        while let Some(link) = cursor {
            best_seqs.insert(link.seq);
            cursor = link.prev.as_ref();
        }
        let kept = select_kbest(hyps, KBEST_PER_END_FRAME, &best_seqs);
        let nulls = match self.sil_frame_nulls(frames) {
            Some(nulls) => nulls,
            None => return self.lattice_from_path(best, nframes),
        };
        let mut id_of: HashMap<u64, u64> = HashMap::new();
        let mut tokens: Vec<Token> = Vec::with_capacity(kept.len() * 2);
        for h in &kept {
            let id = tokens.len() as u64 + 1;
            id_of.insert(h.seq, id);
            let null_sum: f64 = (h.start..h.end)
                .map(|t| nulls.get(t as usize).copied().unwrap_or(f64::NEG_INFINITY))
                .sum();
            tokens.push(Token {
                id,
                word: h.word.clone(),
                start: h.start,
                end: h.end,
                source: PRODUCER.to_string(),
                acoustic_llr: finite_llr(h.acoustic - null_sum),
                lexical_llr: None,
                speaker_llr: None,
                verify_llr: None,
                user_llr: None,
            });
        }
        let mut arcs: Vec<Arc> = Vec::new();
        let mut incoming: HashMap<u64, u64> = HashMap::new();
        for h in &kept {
            let id = id_of[&h.seq];
            if let Some(pred) = h.prev_seq.and_then(|p| id_of.get(&p)) {
                arcs.push(Arc {
                    from: *pred,
                    to: id,
                });
                incoming.insert(id, *pred);
            }
        }
        let n_word = tokens.len();
        for i in 0..n_word {
            let (start, end) = (tokens[i].start, tokens[i].end);
            let word_id = i as u64 + 1;
            let id = tokens.len() as u64 + 1;
            if let Some(pred) = incoming.get(&word_id) {
                arcs.push(Arc {
                    from: *pred,
                    to: id,
                });
            }
            tokens.push(Token {
                id,
                word: "SIL".to_string(),
                start,
                end,
                source: PRODUCER.to_string(),
                acoustic_llr: 0.0,
                lexical_llr: None,
                speaker_llr: None,
                verify_llr: None,
                user_llr: None,
            });
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
        let mut seq_next: u64 = 0;
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
                    &mut seq_next,
                    None,
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
                let seq = seq_next;
                seq_next += 1;
                let link = Rc::new(WordLink {
                    seq,
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
    /// Token passing with word-end recording; emits the k-best lattice.
    /// Search is identical to [`MooreDecoder::decode_inner`] (same beam,
    /// prune, and best-path selection); the two must stay in sync.
    fn decode_inner_kbest(&self, frames: &[fse::Frame]) -> (Lattice, f64, u32) {
        let nframes = frames.len() as u32;
        if frames.is_empty() || self.trie.is_empty() {
            return (self.empty_lattice(nframes), f64::NEG_INFINITY, 0);
        }
        let mut hyps: Vec<WordEndHyp> = Vec::new();
        let mut seq_next: u64 = 0;
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
                    &mut seq_next,
                    Some(&mut hyps),
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
                let seq = seq_next;
                seq_next += 1;
                let acoustic = tok.word_acoustic + topo.exit_lp;
                hyps.push(WordEndHyp {
                    seq,
                    word: word.clone(),
                    start: tok.word_start,
                    end: nframes,
                    acoustic,
                    path_score: score,
                    prev_seq: tok.path.as_ref().map(|p| p.seq),
                });
                let link = Rc::new(WordLink {
                    seq,
                    word: word.clone(),
                    start: tok.word_start,
                    end: nframes,
                    acoustic,
                    prev: tok.path.clone(),
                });
                let better = best.as_ref().map(|(s, _)| score > *s).unwrap_or(true);
                if better {
                    best = Some((score, Some(link)));
                }
            }
        }
        match best {
            Some((score, path)) => (
                self.lattice_from_kbest(frames, &hyps, &path),
                score,
                nframes,
            ),
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

    /// Feed one chunk; returns the current 1-best partial over the buffer.
    ///
    /// Chunking/latency contract (v0): a chunk is any-length 16 kHz mono
    /// PCM and appends to an internal buffer (empty chunks re-decode
    /// unchanged). Each call re-decodes the whole buffer, so per-chunk
    /// latency is ~one batch decode of the audio so far and total work is
    /// quadratic in utterance length; incremental search is follow-up work.
    /// Returns exactly one partial: the 1-best over the whole buffer.
    /// CMVN (when `cfg.cmvn`) uses the cumulative [`fse::CmvnLive`]
    /// estimate over the buffer, applied past ~1 s (100 frames) and raw
    /// below that; with `cfg.cmvn == false` the final partial equals batch
    /// `decode` exactly. Call [`MooreDecoder::reset_stream`] between
    /// utterances so neither audio nor CMVN state leaks across them.
    fn stream(&mut self, chunk: &[i16]) -> Vec<Partial> {
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

/// Keep top `k` hyps per end frame by path score (ties broken by word,
/// start, then seq, so the order is total and deterministic), plus every
/// 1-best-path seq. Returns kept hyps ordered by (end, path score desc,
/// word, start, seq).
fn select_kbest(hyps: &[WordEndHyp], k: usize, best_seqs: &HashSet<u64>) -> Vec<WordEndHyp> {
    let mut by_end: BTreeMap<u32, Vec<&WordEndHyp>> = BTreeMap::new();
    for h in hyps {
        by_end.entry(h.end).or_default().push(h);
    }
    let mut kept: Vec<WordEndHyp> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    for group in by_end.values_mut() {
        group.sort_by(|a, b| {
            b.path_score
                .partial_cmp(&a.path_score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.word.cmp(&b.word))
                .then_with(|| a.start.cmp(&b.start))
                .then_with(|| a.seq.cmp(&b.seq))
        });
        for h in group.iter().take(k) {
            if seen.insert(h.seq) {
                kept.push((*h).clone());
            }
        }
    }
    // 1-best backfill: every best-path link stays, with its chain intact.
    for h in hyps {
        if best_seqs.contains(&h.seq) && seen.insert(h.seq) {
            kept.push(h.clone());
        }
    }
    kept.sort_by(|a, b| {
        a.end
            .cmp(&b.end)
            .then_with(|| {
                b.path_score
                    .partial_cmp(&a.path_score)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| a.word.cmp(&b.word))
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.seq.cmp(&b.seq))
    });
    kept
}

/// Clamp a lattice llr to finite values, preserving the sign of infinity:
/// +inf (null impossible) saturates high, -inf/NaN saturates low.
fn finite_llr(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else if value == f64::INFINITY {
        1e9
    } else {
        -1e9
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

    // --- Word-internal triphone fixtures (no data/ needed) ---
    //
    // Two-word vocab: AB = [AH, B], AD = [AH, D]. Single-state phones,
    // unit variance, means separated on dim 0 (AH=0, B=+5, D=-5). Two
    // synthetic frames: f0 AH-like, f1 nearer B than D — so the monophone
    // 1-best is AB, and a tied model can flip it to AD through unit means
    // alone (same search code path, no LM).

    fn triphone_fixture_phone(name: &str, mean0: f64) -> hmm::PhoneHmm {
        let dim = fse::FEATURE_DIM;
        let mut mean = vec![0.0; dim];
        mean[0] = mean0;
        hmm::PhoneHmm {
            name: name.to_string(),
            n_states: 1,
            emissions: vec![hmm::Mixture::single(
                hmm::Gaussian {
                    mean,
                    var: vec![1.0; dim],
                },
                hmm::ParamStats::zeros(dim),
            )],
            trans: vec![
                vec![0.0, 1.0, 0.0],
                vec![0.0, 0.5, 0.5],
                vec![0.0, 0.0, 0.0],
            ],
            tied_to: None,
        }
    }

    fn triphone_fixture_mono() -> HmmSet {
        let dim = fse::FEATURE_DIM;
        HmmSet {
            dim,
            phones: [("AH", 0.0), ("B", 5.0), ("D", -5.0)]
                .into_iter()
                .map(|(p, m)| (p.to_string(), triphone_fixture_phone(p, m)))
                .collect(),
            global_mean: vec![0.0; dim],
            global_var: vec![1.0; dim],
            var_floor_ratio: 0.01,
            frontend_id: None,
        }
    }

    fn triphone_fixture_entries() -> Vec<PronEntry> {
        ["AB", "AD"]
            .into_iter()
            .map(|w| PronEntry {
                phones: w
                    .chars()
                    .map(|c| match c {
                        'A' => "AH".to_string(),
                        other => other.to_string(),
                    })
                    .collect(),
                word: w.to_string(),
            })
            .collect()
    }

    fn triphone_fixture_frames() -> Vec<fse::Frame> {
        let f0 = [0.0; fse::FEATURE_DIM];
        let mut f1 = [0.0; fse::FEATURE_DIM];
        f1[0] = 1.0;
        vec![f0, f1]
    }

    fn triphone_wide_cfg() -> DecoderConfig {
        DecoderConfig {
            beam: 1e9,
            max_tokens: 1_000_000,
            ..DecoderConfig::default()
        }
    }

    fn triphone_best_words(lat: &Lattice) -> Vec<&str> {
        lat.tokens.iter().map(|t| t.word.as_str()).collect()
    }

    #[test]
    fn triphone_tied_decode_differs_from_monophone_only_by_acoustics() {
        let mono = triphone_fixture_mono();
        let entries = triphone_fixture_entries();
        let frames = triphone_fixture_frames();
        let cfg = triphone_wide_cfg();
        let mono_dec =
            MooreDecoder::assemble(mono.clone(), None, entries.clone(), Vec::new(), cfg.clone())
                .expect("assemble mono");
        let (mono_lat, mono_score, _) = mono_dec.decode_inner(&frames);
        assert_eq!(triphone_best_words(&mono_lat), vec!["AB"]);

        // Tied model: same centers, unit means that flip the 1-best to AD.
        let mut tied = mono.clone();
        for (unit, mean0) in [("AH+B", -5.0), ("AH-B", 5.0), ("AH+D", 0.0), ("AH-D", -5.0)] {
            tied.phones
                .insert(unit.to_string(), triphone_fixture_phone(unit, mean0));
        }
        let tri_cfg = DecoderConfig {
            triphone: true,
            ..cfg.clone()
        };
        let tied_dec =
            MooreDecoder::assemble(tied.clone(), None, entries.clone(), Vec::new(), tri_cfg)
                .expect("assemble tied");
        assert!(
            tied_dec.trie.nodes.len() > mono_dec.trie.nodes.len(),
            "triphone trie must expand beyond the monophone trie"
        );
        let (tied_lat, _, _) = tied_dec.decode_inner(&frames);
        assert_eq!(
            triphone_best_words(&tied_lat),
            vec!["AD"],
            "tied 1-best must flip via unit acoustic scores"
        );

        // Control: the SAME tied AM with triphone off ignores units and
        // reproduces the monophone result bit-exactly — the search code
        // path is shared, so the flip comes only from unit acoustics.
        let ctrl_dec =
            MooreDecoder::assemble(tied, None, entries, Vec::new(), cfg).expect("assemble ctrl");
        let (ctrl_lat, ctrl_score, _) = ctrl_dec.decode_inner(&frames);
        assert_eq!(triphone_best_words(&ctrl_lat), vec!["AB"]);
        assert_eq!(
            ctrl_score, mono_score,
            "tied AM + triphone off must equal the monophone decode"
        );
    }

    #[test]
    fn triphone_unseen_units_fall_back_to_monophone() {
        let mono = triphone_fixture_mono();
        let entries = triphone_fixture_entries();
        let frames = triphone_fixture_frames();
        let cfg = triphone_wide_cfg();
        let mono_dec =
            MooreDecoder::assemble(mono.clone(), None, entries.clone(), Vec::new(), cfg.clone())
                .expect("assemble mono");
        let (mono_lat, mono_score, _) = mono_dec.decode_inner(&frames);
        // Triphone trie over a pure monophone AM: every unit falls back.
        let tri_cfg = DecoderConfig {
            triphone: true,
            ..cfg.clone()
        };
        let fb_dec = MooreDecoder::assemble(mono, None, entries, Vec::new(), tri_cfg)
            .expect("assemble fallback");
        let (fb_lat, fb_score, _) = fb_dec.decode_inner(&frames);
        assert_eq!(triphone_best_words(&fb_lat), triphone_best_words(&mono_lat));
        assert_eq!(
            fb_score, mono_score,
            "full fallback must reproduce monophone scores exactly"
        );
    }

    #[test]
    fn triphone_partial_units_fall_back_per_arc() {
        let mono = triphone_fixture_mono();
        let entries = triphone_fixture_entries();
        let frames = triphone_fixture_frames();
        let cfg = triphone_wide_cfg();
        let tri_cfg = || DecoderConfig {
            triphone: true,
            ..cfg.clone()
        };

        // Pure fallback baseline: monophone AM, triphone trie.
        let pure_dec =
            MooreDecoder::assemble(mono.clone(), None, entries.clone(), Vec::new(), tri_cfg())
                .expect("assemble pure");
        let (pure_lat, _, _) = pure_dec.decode_inner(&frames);
        assert_eq!(triphone_best_words(&pure_lat), vec!["AB"]);

        // Partial tied AM carrying ONE unit (AH-D tuned to f1): the present
        // arc scores the unit (flipping the 1-best), absent arcs fall back.
        let mut partial = mono.clone();
        partial
            .phones
            .insert("AH-D".to_string(), triphone_fixture_phone("AH-D", 1.0));
        let part_dec =
            MooreDecoder::assemble(partial, None, entries.clone(), Vec::new(), tri_cfg())
                .expect("assemble partial");
        let (part_lat, part_score, _) = part_dec.decode_inner(&frames);
        assert_eq!(
            triphone_best_words(&part_lat),
            vec!["AD"],
            "the one present unit must score (1-best flips)"
        );

        // Full tied AM whose other units are exact center clones: identical
        // scores prove the absent arcs fell back to their centers.
        let mut full = mono.clone();
        for (unit, mean0) in [("AH+B", 0.0), ("AH-B", 5.0), ("AH+D", 0.0), ("AH-D", 1.0)] {
            full.phones
                .insert(unit.to_string(), triphone_fixture_phone(unit, mean0));
        }
        let full_dec = MooreDecoder::assemble(full, None, entries, Vec::new(), tri_cfg())
            .expect("assemble full");
        let (full_lat, full_score, _) = full_dec.decode_inner(&frames);
        assert_eq!(triphone_best_words(&full_lat), vec!["AD"]);
        assert_eq!(
            part_score, full_score,
            "partial tied AM must equal the center-clone-filled AM"
        );
    }
}
