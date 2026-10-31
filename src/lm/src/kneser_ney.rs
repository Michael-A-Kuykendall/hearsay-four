//! Trigram language model with modified Kneser-Ney discounting
//! (Chen & Goodman 1999), bead hearsay-four-baj.
//!
//! Counts, continuation counts, and per-order discounts are stored in
//! sorted (`BTreeMap`) maps so the model is inspectable and serializes
//! deterministically. Sentences are padded `<s> <s> ... </s>`.
//!
//! Recursion (interpolated modified KN):
//! - Trigram level uses raw counts `c(w1,w2,w)`.
//! - Bigram level uses continuation counts
//!   `cc(w2,w) = |{v : c(v,w2,w) > 0}|`.
//! - Unigram level uses continuation counts
//!   `P(w) = N1+(.,w) / N1+(.,.)`, `N1+(.,w) = |{v : c(v,w) > 0}|`
//!   over bigrams; `<unk>` and `</s>` always carry mass because `<unk>`
//!   is seeded with one pseudo-predecessor and `</s>` follows `<s>`.
//! - Discounts per order from counts-of-counts:
//!   `Y = n1/(n1+2*n2)`, `D1 = 1-2Y*n2/n1`, `D2 = 2-3Y*n3/n2`,
//!   `D3 = 3-4Y*n4/n3`, clamped to `[0, count]`; any degenerate
//!   (missing/zero/non-finite) component falls back to `0.5` scaled
//!   (`D1=0.5, D2=1.0, D3=1.5` fallback set uses 0.5/1.0/1.5 only when
//!   the whole estimate is degenerate; per-component fallback is
//!   `0.5 * count-index`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::normalize::{BOS_TOKEN, EOS_TOKEN, UNK_TOKEN};

/// Per-order modified-KN discounts `D1, D2, D3plus`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Discounts {
    pub d1: f64,
    pub d2: f64,
    pub d3: f64,
}

impl Discounts {
    pub fn for_count(&self, c: u64) -> f64 {
        match c {
            0 => 0.0,
            1 => self.d1,
            2 => self.d2,
            _ => self.d3,
        }
    }
}

fn estimate_discounts(counts: &HashMap<u64, u64>) -> Discounts {
    // counts: count-value -> number of n-grams with that count (n1..n4 used)
    let n = |k: u64| *counts.get(&k).unwrap_or(&0) as f64;
    let (n1, n2, n3, n4) = (n(1), n(2), n(3), n(4));
    let fallback = Discounts {
        d1: 0.5,
        d2: 1.0,
        d3: 1.5,
    };
    if n1 <= 0.0 || n2 <= 0.0 || n3 <= 0.0 {
        return fallback;
    }
    let y = n1 / (n1 + 2.0 * n2);
    let d1 = 1.0 - 2.0 * y * (n2 / n1);
    let d2 = 2.0 - 3.0 * y * (n3 / n2);
    let d3 = if n4 > 0.0 {
        3.0 - 4.0 * y * (n4 / n3)
    } else {
        1.5
    };
    let fix = |d: f64, c: f64, fb: f64| {
        if d.is_finite() && (0.0..=c).contains(&d) {
            d
        } else {
            fb
        }
    };
    Discounts {
        d1: fix(d1, 1.0, 0.5),
        d2: fix(d2, 2.0, 1.0),
        d3: fix(d3, 3.0, 1.5),
    }
}

fn counts_of_counts<I: Iterator<Item = u64>>(it: I) -> HashMap<u64, u64> {
    let mut m = HashMap::new();
    for c in it {
        if c > 0 {
            *m.entry(c).or_insert(0) += 1;
        }
    }
    m
}

/// Inspectable snapshot: every count and discount, deterministically ordered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectableModel {
    pub vocabulary: Vec<String>,
    pub unigram_counts: BTreeMap<String, u64>,
    pub bigram_counts: BTreeMap<String, u64>,
    pub trigram_counts: BTreeMap<String, u64>,
    /// Live-only: skipped in v2 archives (load recomputes it exactly via
    /// `finalize()`); dropping this redundant map cuts archive bytes ~35%.
    #[serde(skip)]
    pub bigram_continuation_counts: BTreeMap<String, u64>,
    pub trigram_discounts: Discounts,
    pub bigram_discounts: Discounts,
    pub total_tokens: u64,
}

/// Trigram modified Kneser-Ney model over normalized token sentences.
#[derive(Debug, Clone, Default)]
pub struct KnTrigram {
    unigram: HashMap<String, u64>,
    bigram: HashMap<(String, String), u64>,
    trigram: HashMap<(String, String, String), u64>,
    vocab: BTreeSet<String>,
    total_tokens: u64,
    // continuation structures, rebuilt in finalize()
    bigram_cont: HashMap<(String, String), u64>, // (u,w) -> |{v: c(v,u,w)>0}|
    unigram_cont: HashMap<String, u64>,          // w -> |{v: c(v,w)>0}| (bigrams)
    unigram_cont_total: u64,
    tri_discounts: Discounts,
    bi_discounts: Discounts,
    // Per-history (total, n1, n2, n3plus) follower-type stats, rebuilt in
    // finalize(). Caches only: prob()/bigram_prob() arithmetic is unchanged.
    tri_hist_stats: HashMap<(String, String), (u64, u64, u64, u64)>,
    bi_hist_stats: HashMap<String, (u64, u64, u64, u64)>,
}

/// Perplexity of a model on held-out normalized sentences.
/// `predicted_tokens` counts every predicted token including `</s>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerplexityReport {
    pub sentences: usize,
    pub predicted_tokens: u64,
    pub log_prob: f64,
    pub perplexity: f64,
}

fn key2(a: &str, b: &str) -> String {
    format!("{a} {b}")
}
fn key3(a: &str, b: &str, c: &str) -> String {
    format!("{a} {b} {c}")
}

impl KnTrigram {
    pub fn new() -> Self {
        Self::default()
    }

    /// Train on normalized sentences (tokens without boundary padding).
    pub fn train(sentences: &[Vec<String>]) -> Self {
        let mut m = Self::new();
        m.vocab.insert(EOS_TOKEN.into());
        m.vocab.insert(UNK_TOKEN.into());
        for sent in sentences {
            let mut padded: Vec<String> = vec![BOS_TOKEN.into(), BOS_TOKEN.into()];
            padded.extend(sent.iter().cloned());
            padded.push(EOS_TOKEN.into());
            for w in &padded {
                if w != BOS_TOKEN {
                    m.vocab.insert(w.clone());
                    *m.unigram.entry(w.clone()).or_insert(0) += 1;
                    m.total_tokens += 1;
                }
            }
            for p in padded.windows(2) {
                *m.bigram.entry((p[0].clone(), p[1].clone())).or_insert(0) += 1;
            }
            for t in padded.windows(3) {
                *m.trigram
                    .entry((t[0].clone(), t[1].clone(), t[2].clone()))
                    .or_insert(0) += 1;
            }
        }
        m.finalize();
        m
    }

        fn finalize(&mut self) {
        // Bigram continuation counts: distinct predecessors v per (u,w).
        // Trigram keys are unique, so each key contributes exactly one
        // predecessor (no dedup set). Every (u,w) here is also a stored
        // bigram, which bounds the map.
        let mut cont: HashMap<(String, String), u64> =
            HashMap::with_capacity(self.bigram.len());
        for key in self.trigram.keys() {
            *cont.entry((key.1.clone(), key.2.clone())).or_insert(0) += 1;
        }
        self.bigram_cont = cont;
        // Unigram continuation: distinct predecessors v per w, via sort +
        // linear dedup. Same counts as per-word predecessor sets, without
        // cloning every bigram string twice into nested sets.
        let mut pairs: Vec<(&str, &str)> = Vec::with_capacity(self.bigram.len());
        for (v, w) in self.bigram.keys() {
            if w != BOS_TOKEN {
                pairs.push((w.as_str(), v.as_str()));
            }
        }
        pairs.sort_unstable();
        let mut unigram_cont: HashMap<String, u64> =
            HashMap::with_capacity(self.vocab.len());
        let mut i = 0;
        while i < pairs.len() {
            let w = pairs[i].0;
            let mut n = 0u64;
            let mut prev: Option<&str> = None;
            while i < pairs.len() && pairs[i].0 == w {
                if Some(pairs[i].1) != prev {
                    n += 1;
                    prev = Some(pairs[i].1);
                }
                i += 1;
            }
            unigram_cont.insert(w.to_string(), n);
        }
        self.unigram_cont = unigram_cont;
        // Guarantee <unk> and </s> carry continuation mass.
        self.unigram_cont.entry(UNK_TOKEN.into()).or_insert(1);
        self.unigram_cont.entry(EOS_TOKEN.into()).or_insert(1);
        self.unigram_cont_total = self.unigram_cont.values().sum();
        self.tri_discounts = estimate_discounts(&counts_of_counts(self.trigram.values().copied()));
        self.bi_discounts =
            estimate_discounts(&counts_of_counts(self.bigram_cont.values().copied()));
        // History follower-type stats (same buckets prob() used to rescan for).
        let mut tri_stats: HashMap<(String, String), (u64, u64, u64, u64)> =
            HashMap::with_capacity(self.bigram.len());
        for ((a, b, _), &c) in &self.trigram {
            let e = tri_stats.entry((a.clone(), b.clone())).or_default();
            e.0 += c;
            match c {
                1 => e.1 += 1,
                2 => e.2 += 1,
                _ => e.3 += 1,
            }
        }
        self.tri_hist_stats = tri_stats;
        let mut bi_stats: HashMap<String, (u64, u64, u64, u64)> =
            HashMap::with_capacity(self.vocab.len());
        for ((u, _), &cc) in &self.bigram_cont {
            let e = bi_stats.entry(u.clone()).or_default();
            e.0 += cc;
            match cc {
                1 => e.1 += 1,
                2 => e.2 += 1,
                _ => e.3 += 1,
            }
        }
        self.bi_hist_stats = bi_stats;
    }

    pub fn vocabulary(&self) -> &BTreeSet<String> {
        &self.vocab
    }

    /// Prediction vocabulary: vocab without `<s>` (never predicted).
    pub fn prediction_vocabulary(&self) -> Vec<&str> {
        self.vocab
            .iter()
            .map(|s| s.as_str())
            .filter(|w| *w != BOS_TOKEN)
            .collect()
    }
    pub fn trigram_discounts(&self) -> Discounts {
        self.tri_discounts
    }
    pub fn bigram_discounts(&self) -> Discounts {
        self.bi_discounts
    }
    pub fn count_unigram(&self, w: &str) -> u64 {
        self.unigram.get(w).copied().unwrap_or(0)
    }
    pub fn count_bigram(&self, a: &str, b: &str) -> u64 {
        self.bigram.get(&(a.into(), b.into())).copied().unwrap_or(0)
    }
    pub fn count_trigram(&self, a: &str, b: &str, c: &str) -> u64 {
        self.trigram
            .get(&(a.into(), b.into(), c.into()))
            .copied()
            .unwrap_or(0)
    }

    fn map_word<'a>(&'a self, w: &'a str) -> &'a str {
        if w == BOS_TOKEN || self.vocab.contains(w) {
            w
        } else {
            UNK_TOKEN
        }
    }

    /// Continuation unigram `P(w)`.
    pub fn unigram_prob(&self, w: &str) -> f64 {
        let w = self.map_word(w);
        if self.unigram_cont_total == 0 {
            let v = self.prediction_vocabulary().len().max(1) as f64;
            return 1.0 / v;
        }
        self.unigram_cont.get(w).copied().unwrap_or(0) as f64 / self.unigram_cont_total as f64
    }

    /// Modified KN `P(w | u)` (bigram level, continuation counts).
    pub fn bigram_prob(&self, u: &str, w: &str) -> f64 {
        let u = self.map_word(u);
        let w = self.map_word(w);
        let lower = self.unigram_prob(w);
        // cc(u,.) total and follower type counts by continuation count.
        let (total, n1, n2, n3) = self.bi_hist_stats.get(u).copied().unwrap_or((0, 0, 0, 0));
        if total == 0 {
            return lower;
        }
        let cc = self
            .bigram_cont
            .get(&(u.into(), w.into()))
            .copied()
            .unwrap_or(0);
        let d = self.bi_discounts.for_count(cc);
        let lambda_num = self.bi_discounts.d1 * n1 as f64
            + self.bi_discounts.d2 * n2 as f64
            + self.bi_discounts.d3 * n3 as f64;
        let p = ((cc as f64 - d).max(0.0) / total as f64) + (lambda_num / total as f64) * lower;
        p.clamp(0.0, 1.0)
    }

    /// Modified KN `P(w | w1, w2)` (trigram level, raw counts).
    pub fn prob(&self, w1: &str, w2: &str, w: &str) -> f64 {
        let w1 = self.map_word(w1).to_string();
        let w2 = self.map_word(w2).to_string();
        let w = self.map_word(w).to_string();
        let lower = self.bigram_prob(&w2, &w);
        let (total, n1, n2, n3) = self
            .tri_hist_stats
            .get(&(w1.clone(), w2.clone()))
            .copied()
            .unwrap_or((0, 0, 0, 0));
        if total == 0 {
            return lower;
        }
        let c = self.count_trigram(&w1, &w2, &w);
        let d = self.tri_discounts.for_count(c);
        let lambda_num = self.tri_discounts.d1 * n1 as f64
            + self.tri_discounts.d2 * n2 as f64
            + self.tri_discounts.d3 * n3 as f64;
        let p = ((c as f64 - d).max(0.0) / total as f64) + (lambda_num / total as f64) * lower;
        p.clamp(0.0, 1.0)
    }

    /// Log probability of a normalized sentence (with boundary padding).
    pub fn sentence_log_prob(&self, sent: &[String]) -> f64 {
        let mut padded: Vec<String> = vec![BOS_TOKEN.into(), BOS_TOKEN.into()];
        padded.extend(sent.iter().cloned());
        padded.push(EOS_TOKEN.into());
        let mut lp = 0.0;
        for t in padded.windows(3) {
            lp += self.prob(&t[0], &t[1], &t[2]).max(1e-12).ln();
        }
        lp
    }

    /// Inspectable, deterministically ordered snapshot of all counts.
    pub fn inspectable(&self) -> InspectableModel {
        InspectableModel {
            vocabulary: self.vocab.iter().cloned().collect(),
            unigram_counts: self.unigram.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            bigram_counts: self
                .bigram
                .iter()
                .map(|((a, b), v)| (key2(a, b), *v))
                .collect(),
            trigram_counts: self
                .trigram
                .iter()
                .map(|((a, b, c), v)| (key3(a, b, c), *v))
                .collect(),
            bigram_continuation_counts: self
                .bigram_cont
                .iter()
                .map(|((a, b), v)| (key2(a, b), *v))
                .collect(),
            trigram_discounts: self.tri_discounts,
            bigram_discounts: self.bi_discounts,
            total_tokens: self.total_tokens,
        }
    }

    /// Rebuild a model from an inspectable snapshot (persistence reload).
    /// Keys are space-joined tokens; normalized tokens never contain a
    /// space, so the split is unambiguous. Continuation counts, discounts,
    /// and history stats are re-derived by `finalize()`, which reproduces
    /// the snapshot's stored values for any snapshot taken from a model.
        pub fn from_inspectable(snap: &InspectableModel) -> Result<Self, String> {
        let mut m = Self::new();
        m.vocab = snap.vocabulary.iter().cloned().collect();
        m.total_tokens = snap.total_tokens;
        m.unigram = HashMap::with_capacity(snap.unigram_counts.len());
        m.bigram = HashMap::with_capacity(snap.bigram_counts.len());
        m.trigram = HashMap::with_capacity(snap.trigram_counts.len());
        for (k, &v) in &snap.unigram_counts {
            m.unigram.insert(k.clone(), v);
        }
        for (k, &v) in &snap.bigram_counts {
            let p: Vec<&str> = k.split(' ').collect();
            if p.len() != 2 {
                return Err(format!("bad bigram key {k:?}"));
            }
            m.bigram.insert((p[0].into(), p[1].into()), v);
        }
        for (k, &v) in &snap.trigram_counts {
            let p: Vec<&str> = k.split(' ').collect();
            if p.len() != 3 {
                return Err(format!("bad trigram key {k:?}"));
            }
            m.trigram.insert((p[0].into(), p[1].into(), p[2].into()), v);
        }
        m.finalize();
        Ok(m)
    }

    /// Number of stored trigram entries (the prunable size).
    pub fn trigram_entry_count(&self) -> usize {
        self.trigram.len()
    }
    /// Number of stored bigram entries (kept whole by entropy pruning).
    pub fn bigram_entry_count(&self) -> usize {
        self.bigram.len()
    }
    /// Counts-of-counts histograms `(bigrams, trigrams)`: raw-count value
    /// -> number of distinct entries carrying it. Used to pick
    /// count-cutoff thresholds without serializing the model.
    pub fn count_histograms(&self) -> (BTreeMap<u64, u64>, BTreeMap<u64, u64>) {
        let mut bi: BTreeMap<u64, u64> = BTreeMap::new();
        for &c in self.bigram.values() {
            *bi.entry(c).or_insert(0) += 1;
        }
        let mut tri: BTreeMap<u64, u64> = BTreeMap::new();
        for &c in self.trigram.values() {
            *tri.entry(c).or_insert(0) += 1;
        }
        (bi, tri)
    }
    /// Perplexity on held-out normalized sentences (R1b gate metric).
    /// Empty input yields infinite perplexity.
    pub fn perplexity(&self, sentences: &[Vec<String>]) -> PerplexityReport {
        let mut log_prob = 0.0;
        let mut tokens: u64 = 0;
        for s in sentences {
            log_prob += self.sentence_log_prob(s);
            tokens += s.len() as u64 + 1; // +1 for predicted </s>
        }
        let perplexity = if tokens == 0 {
            f64::INFINITY
        } else {
            (-log_prob / tokens as f64).exp()
        };
        PerplexityReport {
            sentences: sentences.len(),
            predicted_tokens: tokens,
            log_prob,
            perplexity,
        }
    }

    /// Entropy pruning to a trigram-entry budget (Stolcke-style, R1b).
    ///
    /// Each trigram `(w1,w2,w)` with count `c` is scored by its total
    /// log-likelihood gain over its backed-off bigram probability on the
    /// training counts themselves:
    /// `score = c * (ln P(w|w1,w2) - ln P(w|w2))`. Entries with the
    /// lowest score (rare, or no better than backoff) are removed first;
    /// ties break on the token key, so pruning is deterministic. Only
    /// trigram entries are removed: unigram/bigram raw counts stay, and
    /// continuation counts, discounts, and history stats are re-estimated
    /// by `finalize()`. A budget at or above the current entry count
    /// returns an unchanged clone.
    pub fn entropy_pruned(&self, max_trigram_entries: usize) -> Self {
        if self.trigram.len() <= max_trigram_entries {
            return self.clone();
        }
        let mut scored: Vec<(f64, String, String, String)> = self
            .trigram
            .iter()
            .map(|((a, b, c), &cnt)| {
                let p = self.prob(a, b, c).max(1e-12);
                let pb = self.bigram_prob(b, c).max(1e-12);
                (
                    cnt as f64 * (p.ln() - pb.ln()),
                    a.clone(),
                    b.clone(),
                    c.clone(),
                )
            })
            .collect();
        scored.sort_by(|x, y| {
            x.0.partial_cmp(&y.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| (&x.1, &x.2, &x.3).cmp(&(&y.1, &y.2, &y.3)))
        });
        let remove = self.trigram.len() - max_trigram_entries;
        let drop_set: HashSet<(String, String, String)> = scored
            .iter()
            .take(remove)
            .map(|(_, a, b, c)| (a.clone(), b.clone(), c.clone()))
            .collect();
        let mut m = self.clone();
        m.trigram.retain(|k, _| !drop_set.contains(k));
        m.finalize();
        m
    }
    /// Count-cutoff pruning (R1-v2): drop bigram entries with raw count
    /// below `min_bigram_count` and trigram entries below
    /// `min_trigram_count`, then re-estimate continuation counts,
    /// discounts, and history stats via `finalize()`. Thresholds at or
    /// below 1 keep every entry of that order (a no-op clone). Pruning
    /// is a pure filter, so it is deterministic. Removed n-grams back
    /// off exactly as unseen ones do (standard cutoff semantics);
    /// unigram counts are untouched.
    pub fn count_cutoff_pruned(
        &self,
        min_bigram_count: u64,
        min_trigram_count: u64,
    ) -> Self {
        let mut m = self.clone();
        if min_bigram_count > 1 {
            m.bigram.retain(|_, c| *c >= min_bigram_count);
        }
        if min_trigram_count > 1 {
            m.trigram.retain(|_, c| *c >= min_trigram_count);
        }
        m.finalize();
        m
    }
}
