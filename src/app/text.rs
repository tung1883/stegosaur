//! Cover-text helpers for front ends: a cache of trained models (training builds a multi-thousand-line
//! corpus, so it must not happen on every keystroke), plus the measured capacity and realism figures.

use crate::app::secret::Secret;
use crate::corpora::Style;
use crate::crypto::KdfParams;
use crate::error::Result;
use crate::linguistic::{decode, encode, NgramModel};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

type Key = (Style, usize, usize);

fn cache() -> &'static Mutex<HashMap<Key, Arc<NgramModel>>> {
    static C: OnceLock<Mutex<HashMap<Key, Arc<NgramModel>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn all_styles() -> [Style; 5] {
    [Style::Prose, Style::Syslog, Style::Postgres, Style::Nginx, Style::Env]
}

pub fn style_name(s: Style) -> &'static str {
    match s {
        Style::Prose => "prose",
        Style::Syslog => "syslog",
        Style::Postgres => "postgres",
        Style::Nginx => "nginx",
        Style::Env => "env",
    }
}

/// Orders that make a difference for a style; prose and env are fixed at one measured setting.
pub fn adjustable_order(style: Style) -> bool {
    matches!(style, Style::Syslog | Style::Postgres | Style::Nginx)
}

/// Trained model for a style, built once and shared.
pub fn model(style: Style, order: Option<usize>, top_k: Option<usize>) -> Arc<NgramModel> {
    let key = (style, order.unwrap_or(style.default_order()), top_k.unwrap_or(style.default_top_k()));
    if let Some(m) = cache().lock().unwrap().get(&key) {
        return m.clone();
    }
    let built = Arc::new(style.model(None, Some(key.2), Some(key.1)));
    cache().lock().unwrap().entry(key).or_insert(built).clone()
}

/// Encrypts `data` under `pass` and spells it out as text in the given style.
pub fn hide(style: Style, order: Option<usize>, pass: &Secret, data: &[u8], kdf: KdfParams) -> Result<String> {
    let m = model(style, order, None);
    encode(&*m, pass.as_bytes(), data, kdf)
}

/// Inverse of [`hide`]; needs the same style and order.
pub fn reveal(style: Style, order: Option<usize>, pass: &Secret, text: &str, kdf: KdfParams) -> Result<Vec<u8>> {
    let m = model(style, order, None);
    decode(&*m, pass.as_bytes(), text, kdf)
}

/// Measured capacity and realism for a style at an order (see the order sweep in the test suite).
#[derive(Clone, Copy, Debug)]
pub struct TextStats {
    pub bits_per_word: f64,
    /// Share of generated lines whose shape matches the training corpus; not measured for prose or env.
    pub well_formed_percent: Option<u8>,
    pub words_per_line: f64,
}

const BPW_SYSLOG: [f64; 6] = [1.83, 1.35, 1.09, 0.85, 0.60, 0.42];
const BPW_POSTGRES: [f64; 6] = [1.78, 1.48, 1.06, 0.84, 0.58, 0.47];
const BPW_NGINX: [f64; 6] = [1.62, 1.30, 1.24, 1.14, 0.87, 0.49];
const WELL_SYSLOG: [u8; 6] = [31, 75, 87, 95, 100, 100];
const WELL_POSTGRES: [u8; 6] = [80, 100, 100, 100, 100, 100];
const WELL_NGINX: [u8; 6] = [11, 100, 100, 100, 100, 100];

pub fn stats(style: Style, order: usize) -> TextStats {
    let i = order.clamp(1, 6) - 1;
    match style {
        Style::Prose => TextStats { bits_per_word: 2.59, well_formed_percent: None, words_per_line: 14.0 },
        Style::Env => TextStats { bits_per_word: 0.57, well_formed_percent: None, words_per_line: 1.2 },
        Style::Syslog => TextStats { bits_per_word: BPW_SYSLOG[i], well_formed_percent: Some(WELL_SYSLOG[i]), words_per_line: 14.0 },
        Style::Postgres => TextStats { bits_per_word: BPW_POSTGRES[i], well_formed_percent: Some(WELL_POSTGRES[i]), words_per_line: 15.0 },
        Style::Nginx => TextStats { bits_per_word: BPW_NGINX[i], well_formed_percent: Some(WELL_NGINX[i]), words_per_line: 20.0 },
    }
}

/// Expected size of the cover text for a message: (words, lines). The 32 bytes are the nonce, tag and length frame.
///
/// The model works in tokens, and for the log and config styles every line break is a token too, so
/// words are the tokens that remain after the line breaks are taken out.
pub fn estimate(style: Style, order: usize, message_len: usize) -> (usize, usize) {
    let s = stats(style, order);
    let tokens = (((message_len + 32) * 8) as f64 / s.bits_per_word).ceil() as usize;
    match style {
        Style::Prose => (tokens, ((tokens as f64 / s.words_per_line).ceil() as usize).max(1)),
        _ => {
            let lines = ((tokens as f64 / (s.words_per_line + 1.0)).ceil() as usize).max(1);
            (tokens.saturating_sub(lines), lines)
        }
    }
}
