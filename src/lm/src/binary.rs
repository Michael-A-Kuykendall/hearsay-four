//! Binary LM archive format: fast load for large models.
//!
//! JSON archives (`persist::interpolated_to_bytes`) parse at tens of MB/s
//! through `serde_json` and force brutal count-cutoff pruning to meet a 2s
//! load budget. This format stores the same count tables with a string-id
//! vocabulary so a 72MB JSON model loads in well under 2s.
//!
//! Layout (all integers little-endian, ids are `u32` vocabulary indices):
//! ```text
//! magic:            4 bytes  b"HSLM"
//! version:          u32      (= 1)
//! kind:             u8       (0 = kn-trigram, 1 = interpolated)
//! personal_weight: f64      (kind == 1 only)
//! per component model (1 for kn, 2 for interpolated: general, personal):
//!   total_tokens: u64
//!   strings:      u32 n, then n x (u32 len, len UTF-8 bytes); the token
//!                 table (vocabulary plus every token in bigram/trigram
//!                 keys -- boundary `<s>` never enters the vocabulary)
//!   vocab:        u32 n, then n x u32 id into strings
//!   unigram:      u32 n, then n x (u32 id, u64 count)
//!   bigram:       u32 n, then n x (u32 id1, u32 id2, u64 count)
//!   trigram:      u32 n, then n x (u32 id1, u32 id2, u32 id3, u64 count)
//! ```
//! No trailing bytes are allowed after the payload. Continuation counts,
//! discounts, and history stats are NOT stored: decode re-derives them via
//! the same `finalize()` the JSON path uses, so a JSON -> binary -> load
//! round trip reproduces every count and every probability exactly.
//!
//! Encoding is deterministic for a given model instance: the token table
//! is written in intern-id order and entries are sorted by id tuple, so
//! re-encoding the same model always yields identical bytes. (Models with
//! identical counts built by different paths may assign different intern
//! ids, hence different bytes -- but identical decoded counts.)

use std::collections::BTreeSet;

use rustc_hash::FxHashMap as HashMap;

use crate::interpolate::InterpolatedLm;
use crate::kneser_ney::{KnTrigram, map_with_capacity};

/// Magic prefix of every binary archive.
pub const BINARY_MAGIC: [u8; 4] = *b"HSLM";
/// Binary format version. Decoders reject any other version.
pub const BINARY_VERSION: u32 = 1;
/// `kind` tag for a single KN trigram archive.
pub const KIND_KN: u8 = 0;
/// `kind` tag for an interpolated (general + personal) archive.
pub const KIND_INTERPOLATED: u8 = 1;

/// True when `bytes` starts with the binary magic + version header.
pub fn is_binary_archive(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..4] == BINARY_MAGIC && u32_from(&bytes[4..8]) == BINARY_VERSION
}

fn u32_from(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn with_capacity(n: usize) -> Self {
        Self {
            out: Vec::with_capacity(n),
        }
    }
    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
    }
    fn str_entry(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.bytes(s.as_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn rest(&self) -> usize {
        self.bytes.len() - self.pos
    }
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        if self.rest() < n {
            return Err(format!("binary LM archive truncated in {what}"));
        }
        let s = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }
    fn u32(&mut self, what: &str) -> Result<u32, String> {
        let b = self.take(4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self, what: &str) -> Result<u64, String> {
        let b = self.take(8, what)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn f64(&mut self, what: &str) -> Result<f64, String> {
        let b = self.take(8, what)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn str_entry(&mut self, what: &str) -> Result<&'a str, String> {
        let len = self.u32(what)? as usize;
        let b = self.take(len, what)?;
        std::str::from_utf8(b).map_err(|_| format!("binary LM archive has non-UTF8 {what}"))
    }
}

fn write_component(w: &mut Writer, model: &KnTrigram) {
    let (vocab, tokens, token_id, unigram, bigram, trigram, total_tokens) = model.binary_parts();
    // Intern ids ARE string-table indices: write the token table verbatim,
    // then the vocab subset and the id-keyed count tables (sorted by id
    // tuple for deterministic bytes).
    w.u64(total_tokens);
    w.u32(tokens.len() as u32);
    for s in tokens.iter() {
        w.str_entry(s);
    }
    w.u32(vocab.len() as u32);
    for s in vocab.iter() {
        w.u32(token_id[s.as_str()]);
    }
    let mut uni: Vec<(u32, u64)> = unigram.iter().map(|(&id, &c)| (id, c)).collect();
    uni.sort_unstable();
    w.u32(uni.len() as u32);
    for (id, c) in uni {
        w.u32(id);
        w.u64(c);
    }
    let mut bi: Vec<(u32, u32, u64)> = bigram.iter().map(|(&(a, b), &c)| (a, b, c)).collect();
    bi.sort_unstable();
    w.u32(bi.len() as u32);
    for (a, b, c) in bi {
        w.u32(a);
        w.u32(b);
        w.u64(c);
    }
    let mut tri: Vec<(u32, u32, u32, u64)> = trigram
        .iter()
        .map(|(&(a, b, c), &n)| (a, b, c, n))
        .collect();
    tri.sort_unstable();
    w.u32(tri.len() as u32);
    for (a, b, c, n) in tri {
        w.u32(a);
        w.u32(b);
        w.u32(c);
        w.u64(n);
    }
}

fn read_component(r: &mut Reader) -> Result<KnTrigram, String> {
    let total_tokens = r.u64("total_tokens")?;
    let table_len = r.u32("string table len")? as usize;
    let mut tokens: Vec<String> = Vec::with_capacity(table_len);
    for _ in 0..table_len {
        tokens.push(r.str_entry("string table word")?.to_string());
    }
    let check = |id: u32, what: &str| -> Result<u32, String> {
        if (id as usize) < tokens.len() {
            Ok(id)
        } else {
            Err(format!("binary LM archive has out-of-range {what} id {id}"))
        }
    };
    let vocab_len = r.u32("vocab len")? as usize;
    let mut vocab: BTreeSet<String> = BTreeSet::new();
    for _ in 0..vocab_len {
        let id = check(r.u32("vocab id")?, "vocab")?;
        vocab.insert(tokens[id as usize].clone());
    }
    let uni_len = r.u32("unigram len")? as usize;
    let mut unigram: HashMap<u32, u64> = map_with_capacity(uni_len);
    for _ in 0..uni_len {
        let id = check(r.u32("unigram id")?, "unigram")?;
        let c = r.u64("unigram count")?;
        unigram.insert(id, c);
    }
    let bi_len = r.u32("bigram len")? as usize;
    let mut bigram: HashMap<(u32, u32), u64> = map_with_capacity(bi_len);
    for _ in 0..bi_len {
        let a = check(r.u32("bigram id1")?, "bigram")?;
        let b = check(r.u32("bigram id2")?, "bigram")?;
        let c = r.u64("bigram count")?;
        bigram.insert((a, b), c);
    }
    let tri_len = r.u32("trigram len")? as usize;
    let mut trigram: HashMap<(u32, u32, u32), u64> = map_with_capacity(tri_len);
    for _ in 0..tri_len {
        let a = check(r.u32("trigram id1")?, "trigram")?;
        let b = check(r.u32("trigram id2")?, "trigram")?;
        let c = check(r.u32("trigram id3")?, "trigram")?;
        let n = r.u64("trigram count")?;
        trigram.insert((a, b, c), n);
    }
    Ok(KnTrigram::from_binary_parts(
        tokens,
        vocab,
        unigram,
        bigram,
        trigram,
        total_tokens,
    ))
}

fn estimated_component_bytes(model: &KnTrigram) -> usize {
    let (vocab, tokens, _, unigram, bigram, trigram, _) = model.binary_parts();
    let table_bytes: usize = tokens.iter().map(|s| 4 + s.len()).sum();
    8 + 4
        + table_bytes
        + 4
        + vocab.len() * 4
        + 4
        + unigram.len() * 12
        + 4
        + bigram.len() * 16
        + 4
        + trigram.len() * 20
}

fn write_header(w: &mut Writer, kind: u8) {
    w.bytes(&BINARY_MAGIC);
    w.u32(BINARY_VERSION);
    w.u8(kind);
}

/// Serialize one KN model to deterministic binary archive bytes.
pub fn kn_to_binary(model: &KnTrigram) -> Vec<u8> {
    let mut w = Writer::with_capacity(9 + estimated_component_bytes(model));
    write_header(&mut w, KIND_KN);
    write_component(&mut w, model);
    w.out
}

fn read_header(r: &mut Reader, want_kind: u8, want_name: &str) -> Result<(), String> {
    let magic = r.take(4, "magic")?;
    if magic != BINARY_MAGIC {
        return Err("not a binary LM archive (bad magic)".to_string());
    }
    let version = r.u32("version")?;
    if version != BINARY_VERSION {
        return Err(format!(
            "binary LM archive version {version} != {BINARY_VERSION}"
        ));
    }
    let kind = r.u8("kind")?;
    if kind != want_kind {
        return Err(format!("not a binary LM {want_name} archive"));
    }
    Ok(())
}

fn check_end(r: &Reader) -> Result<(), String> {
    if r.rest() != 0 {
        return Err(format!("binary LM archive has {} trailing bytes", r.rest()));
    }
    Ok(())
}

/// Rebuild a KN model from `kn_to_binary` bytes.
pub fn kn_from_binary(bytes: &[u8]) -> Result<KnTrigram, String> {
    let mut r = Reader::new(bytes);
    read_header(&mut r, KIND_KN, "kn-trigram")?;
    let model = read_component(&mut r)?;
    check_end(&r)?;
    Ok(model)
}

/// Serialize an interpolated LM (both components + weight) to binary bytes.
pub fn interpolated_to_binary(lm: &InterpolatedLm) -> Vec<u8> {
    let mut w = Writer::with_capacity(
        9 + 8 + estimated_component_bytes(lm.general()) + estimated_component_bytes(lm.personal()),
    );
    write_header(&mut w, KIND_INTERPOLATED);
    w.f64(lm.personal_weight());
    write_component(&mut w, lm.general());
    write_component(&mut w, lm.personal());
    w.out
}

/// Rebuild an interpolated LM from `interpolated_to_binary` bytes.
pub fn interpolated_from_binary(bytes: &[u8]) -> Result<InterpolatedLm, String> {
    let mut r = Reader::new(bytes);
    read_header(&mut r, KIND_INTERPOLATED, "interpolated")?;
    let weight = r.f64("personal_weight")?;
    let general = read_component(&mut r)?;
    let personal = read_component(&mut r)?;
    check_end(&r)?;
    InterpolatedLm::new(general, personal, weight)
}
