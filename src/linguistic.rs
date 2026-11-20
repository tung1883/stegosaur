//! Linguistic steganography: ciphertext bits choose which word a language model writes next.
//!
//! At each step the model proposes its top-K next words with probabilities. A Huffman code
//! over those candidates maps ciphertext bits to a word, so likely words consume few bits and
//! unlikely words many. Because ciphertext bits are uniform, the chosen words follow the
//! model's own distribution. The decoder replays the model, finds each word in the candidate
//! set, and reads the bits back.
//!
//! `LanguageModel` is a trait so an LLM-backed model can be plugged in; the shipped
//! implementation is a word-level n-gram model (deterministic, offline, fully testable).

use crate::crypto::{self, derive, master_key, KdfParams, NONCE_LEN, TAG_LEN};
use crate::error::{Error, Result};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub const BOS: u32 = 0;
const PUNCT: &[char] = &[',', '.', ';', ':', '!', '?'];

pub trait LanguageModel {
    /// Next-word candidates (id, weight) for the given context, in a deterministic order.
    /// Must be a pure function of `context`: encoder and decoder both call it.
    fn candidates(&self, context: &[u32]) -> Vec<(u32, u64)>;
    fn word(&self, id: u32) -> &str;
    fn id(&self, word: &str) -> Option<u32>;
    /// How text is split into model tokens; must invert `detokenize`.
    fn tokenize(&self, text: &str) -> Vec<String> {
        tokenize(text)
    }
    fn detokenize(&self, tokens: &[&str]) -> String {
        detokenize(tokens)
    }
    /// Token after which the encoder may stop once the payload is written.
    fn end_token(&self) -> Option<u32> {
        self.id(".")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenizerKind {
    /// Words with punctuation split off; for sentences.
    Prose,
    /// Whitespace-separated fields with line breaks as tokens; for logs and config files.
    Lines,
}

pub const NEWLINE: &str = "\n";

pub fn tokenize_lines(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    for (i, line) in parts.iter().enumerate() {
        out.extend(line.split_whitespace().map(str::to_string));
        if i + 1 < parts.len() {
            out.push(NEWLINE.to_string());
        }
    }
    out
}

pub fn detokenize_lines(tokens: &[&str]) -> String {
    let mut s = String::new();
    for t in tokens {
        if *t == NEWLINE {
            s.push('\n');
        } else {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push(' ');
            }
            s.push_str(t);
        }
    }
    s
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else if PUNCT.contains(&ch) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(ch.to_string());
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn detokenize(tokens: &[&str]) -> String {
    let mut s = String::new();
    for (i, t) in tokens.iter().enumerate() {
        let is_punct = t.len() == 1 && t.chars().next().map_or(false, |c| PUNCT.contains(&c));
        if i > 0 && !is_punct {
            s.push(' ');
        }
        s.push_str(t);
    }
    s
}

pub struct NgramModel {
    vocab: Vec<String>,
    index: HashMap<String, u32>,
    /// `levels[k]` maps a context of k + 1 tokens to its continuations; the last level is the model order.
    levels: Vec<HashMap<Vec<u32>, Vec<(u32, u64)>>>,
    uni: Vec<(u32, u64)>,
    kind: TokenizerKind,
    /// Contexts with fewer candidates back off to a shorter context. Prose uses 2 to keep bit capacity up;
    /// structured text uses 1 so forced words keep lines well-formed.
    min_cands: usize,
}

pub const DEFAULT_ORDER: usize = 2;

impl NgramModel {
    /// Trains a prose model on `corpus`, keeping the `top_k` most frequent continuations per context.
    pub fn train(corpus: &str, top_k: usize) -> Self {
        Self::train_with(corpus, top_k, TokenizerKind::Prose)
    }

    pub fn train_with(corpus: &str, top_k: usize, kind: TokenizerKind) -> Self {
        Self::train_ordered(corpus, top_k, kind, DEFAULT_ORDER)
    }

    /// `order` is the number of preceding tokens the model conditions on (1 = bigram, 2 = trigram, ...).
    /// Higher orders keep lines well-formed but leave fewer free choices, so capacity drops.
    pub fn train_ordered(corpus: &str, top_k: usize, kind: TokenizerKind, order: usize) -> Self {
        let order = order.max(1);
        let mut vocab = vec!["<s>".to_string()];
        let mut index: HashMap<String, u32> = HashMap::new();
        index.insert("<s>".into(), BOS);
        let mut ids = vec![BOS; order];
        let tokens = match kind {
            TokenizerKind::Prose => tokenize(corpus),
            TokenizerKind::Lines => tokenize_lines(corpus),
        };
        for t in tokens {
            let id = *index.entry(t.clone()).or_insert_with(|| {
                vocab.push(t);
                (vocab.len() - 1) as u32
            });
            ids.push(id);
        }
        let mut counts: Vec<HashMap<Vec<u32>, HashMap<u32, u64>>> = vec![HashMap::new(); order];
        let mut uni: HashMap<u32, u64> = HashMap::new();
        for i in order..ids.len() {
            for len in 1..=order {
                *counts[len - 1].entry(ids[i - len..i].to_vec()).or_default().entry(ids[i]).or_default() += 1;
            }
            *uni.entry(ids[i]).or_default() += 1;
        }
        let finish = |m: HashMap<u32, u64>| -> Vec<(u32, u64)> {
            let mut v: Vec<(u32, u64)> = m.into_iter().filter(|(id, _)| *id != BOS).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.truncate(top_k);
            v
        };
        Self {
            vocab,
            index,
            levels: counts
                .into_iter()
                .map(|lvl| lvl.into_iter().map(|(k, v)| (k, finish(v))).collect())
                .collect(),
            uni: finish(uni),
            kind,
            min_cands: if kind == TokenizerKind::Lines { 1 } else { 2 },
        }
    }

    pub fn order(&self) -> usize {
        self.levels.len()
    }

    pub fn default_english() -> Self {
        Self::train(include_str!("../assets/corpus.txt"), 16)
    }
}

impl LanguageModel for NgramModel {
    fn candidates(&self, context: &[u32]) -> Vec<(u32, u64)> {
        let order = self.levels.len();
        let mut padded = vec![BOS; order];
        padded.extend_from_slice(context);
        let end = padded.len();
        for len in (1..=order).rev() {
            if let Some(c) = self.levels[len - 1].get(&padded[end - len..]).filter(|c| c.len() >= self.min_cands) {
                return c.clone();
            }
        }
        self.uni.clone()
    }


    fn word(&self, id: u32) -> &str {
        &self.vocab[id as usize]
    }

    fn id(&self, word: &str) -> Option<u32> {
        self.index.get(word).copied()
    }

    fn tokenize(&self, text: &str) -> Vec<String> {
        match self.kind {
            TokenizerKind::Prose => tokenize(text),
            TokenizerKind::Lines => tokenize_lines(text),
        }
    }

    fn detokenize(&self, tokens: &[&str]) -> String {
        match self.kind {
            TokenizerKind::Prose => detokenize(tokens),
            TokenizerKind::Lines => detokenize_lines(tokens),
        }
    }

    fn end_token(&self) -> Option<u32> {
        match self.kind {
            TokenizerKind::Prose => self.id("."),
            TokenizerKind::Lines => self.id(NEWLINE),
        }
    }
}

/// Deterministic Huffman code lengths -> codes (bit vectors, 0 = left). A single symbol gets an empty code.
fn huffman_codes(weights: &[u64]) -> Vec<Vec<u8>> {
    let n = weights.len();
    if n == 1 {
        return vec![vec![]];
    }
    // node index < n are leaves; others internal. children[i - n] = (left, right)
    let mut children: Vec<(usize, usize)> = Vec::new();
    let mut heap = BinaryHeap::new();
    for (i, &w) in weights.iter().enumerate() {
        heap.push(Reverse((w.max(1), i)));
    }
    let mut next = n;
    while heap.len() > 1 {
        let Reverse((w1, a)) = heap.pop().unwrap();
        let Reverse((w2, b)) = heap.pop().unwrap();
        children.push((a, b));
        heap.push(Reverse((w1 + w2, next)));
        next += 1;
    }
    let root = next - 1;
    let mut codes = vec![Vec::new(); n];
    let mut stack = vec![(root, Vec::<u8>::new())];
    while let Some((node, path)) = stack.pop() {
        if node < n {
            codes[node] = path;
        } else {
            let (l, r) = children[node - n];
            let mut lp = path.clone();
            lp.push(0);
            let mut rp = path;
            rp.push(1);
            stack.push((l, lp));
            stack.push((r, rp));
        }
    }
    codes
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().flat_map(|b| (0..8).map(move |k| (b >> (7 - k)) & 1)).collect()
}

fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    bits.chunks_exact(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b)).collect()
}

fn text_keys(pass: &[u8], kdf: KdfParams) -> Result<([u8; 32], [u8; 4])> {
    if pass.is_empty() {
        return Err(Error::Invalid("passphrase must not be empty".into()));
    }
    let master = master_key(pass, b"stegosaur/text/v1", kdf)?;
    let m = derive(&master, "len-mask");
    Ok((derive(&master, "aead"), [m[0], m[1], m[2], m[3]]))
}

/// Writes cover text that carries `data`, encrypted under `pass`.
pub fn encode<M: LanguageModel>(model: &M, pass: &[u8], data: &[u8], kdf: KdfParams) -> Result<String> {
    let (aead, mask) = text_keys(pass, kdf)?;
    let sealed = crypto::seal(&aead, data)?;
    let masked = (sealed.len() as u32) ^ u32::from_le_bytes(mask);
    let mut frame = masked.to_le_bytes().to_vec();
    frame.extend_from_slice(&sealed);
    let bits = bytes_to_bits(&frame);

    let mut ctx: Vec<u32> = vec![BOS, BOS];
    let mut out: Vec<u32> = Vec::new();
    let mut pos = 0usize;
    let period = model.end_token();
    let mut tail = 0usize;
    loop {
        let cands = model.candidates(&ctx);
        if cands.is_empty() {
            return Err(Error::Invalid("language model produced no candidates".into()));
        }
        let weights: Vec<u64> = cands.iter().map(|c| c.1).collect();
        let codes = huffman_codes(&weights);
        let chosen = if pos < bits.len() {
            let mut prefix: Vec<u8> = Vec::new();
            loop {
                if let Some(i) = codes.iter().position(|c| *c == prefix) {
                    break i;
                }
                prefix.push(if pos < bits.len() { bits[pos] } else { 0 });
                pos += 1;
            }
        } else {
            // Payload done: sample words by model probability until a sentence ends.
            // (Argmax can cycle forever; the decoder stops reading once the frame is complete.)
            tail += 1;
            let total: u64 = weights.iter().sum();
            let mut r = rand::Rng::gen_range(&mut rand::thread_rng(), 0..total.max(1));
            weights
                .iter()
                .position(|&w| {
                    if r < w {
                        true
                    } else {
                        r -= w;
                        false
                    }
                })
                .unwrap_or(0)
        };
        let id = cands[chosen].0;
        out.push(id);
        ctx.push(id);
        if pos >= bits.len() && Some(id) == period {
            break;
        }
        if tail > 200 {
            if let Some(p) = period {
                out.push(p);
            }
            break;
        }
    }
    let words: Vec<&str> = out.iter().map(|&i| model.word(i)).collect();
    Ok(model.detokenize(&words))
}

/// Recovers the payload from cover text produced by [`encode`] with the same model.
pub fn decode<M: LanguageModel>(model: &M, pass: &[u8], text: &str, kdf: KdfParams) -> Result<Vec<u8>> {
    let (aead, mask) = text_keys(pass, kdf)?;
    let mut ctx: Vec<u32> = vec![BOS, BOS];
    let mut bits: Vec<u8> = Vec::new();
    for tok in model.tokenize(text) {
        let id = model.id(&tok).ok_or(Error::NoPayload)?;
        let cands = model.candidates(&ctx);
        let weights: Vec<u64> = cands.iter().map(|c| c.1).collect();
        let codes = huffman_codes(&weights);
        let i = cands.iter().position(|c| c.0 == id).ok_or(Error::NoPayload)?;
        bits.extend_from_slice(&codes[i]);
        ctx.push(id);
        if bits.len() >= 32 {
            let head = bits_to_bytes(&bits[..32]);
            let need = (u32::from_le_bytes([head[0], head[1], head[2], head[3]]) ^ u32::from_le_bytes(mask)) as usize;
            if need >= NONCE_LEN + TAG_LEN && bits.len() >= (4 + need) * 8 {
                break;
            }
        }
    }
    let bytes = bits_to_bytes(&bits);
    if bytes.len() < 4 {
        return Err(Error::NoPayload);
    }
    let sealed_len = (u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) ^ u32::from_le_bytes(mask)) as usize;
    if sealed_len < NONCE_LEN + TAG_LEN || 4 + sealed_len > bytes.len() {
        return Err(Error::NoPayload);
    }
    crypto::open(&aead, &bytes[4..4 + sealed_len])
}

/// Average payload bits carried per word for `model` over cover text `text`.
pub fn bits_per_word<M: LanguageModel>(model: &M, text: &str) -> f64 {
    let mut ctx: Vec<u32> = vec![BOS, BOS];
    let (mut bits, mut words) = (0usize, 0usize);
    for tok in model.tokenize(text) {
        let Some(id) = model.id(&tok) else { continue };
        let cands = model.candidates(&ctx);
        let codes = huffman_codes(&cands.iter().map(|c| c.1).collect::<Vec<_>>());
        if let Some(i) = cands.iter().position(|c| c.0 == id) {
            bits += codes[i].len();
        }
        words += 1;
        ctx.push(id);
    }
    if words == 0 { 0.0 } else { bits as f64 / words as f64 }
}
