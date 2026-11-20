//! What-if previews: capacity, fit and (for images) *measured* detectability.
//!
//! Detectability is not a formula. The payload size is hidden in an in-memory copy of the real image
//! with the same mode and parity, then the real detectors run on the result. Random bytes stand in for
//! the payload, which is faithful because encrypted data is indistinguishable from random.
//!
//! `fit_*` functions need only a sample count, so they are instant and never decode a file.

use crate::analysis::{analyze, Report};
use crate::app::config::AppConfig;
use crate::app::friendly;
use crate::carrier::{AnyCarrier, Carrier, ImageCarrier};
use crate::container::{deniable_capacity_for_len, embed_deniable, embed_single, single_capacity_for_len};
use crate::distribute::SHARE_OVERHEAD;
use crate::error::Error;
use rand::RngCore;

/// Detector readings for the untouched image and for a copy with the payload hidden.
#[derive(Clone, Copy, Debug)]
pub struct Measured {
    pub before: Report,
    pub after: Report,
}

pub const AUDIO_NOTE: &str = "detectability is only measured for images";
pub const JPEG_NOTE: &str = "this JPEG is too small for the coefficient detector to give a reading; keep the hidden file small";

fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut v);
    v
}

fn image_of(c: &AnyCarrier) -> Option<&ImageCarrier> {
    match c {
        AnyCarrier::Image(i) => Some(i),
        AnyCarrier::Wav(_) | AnyCarrier::Jpeg(_) => None,
    }
}

/// A copy of `img` with `payload_len` random bytes hidden, using cheap key derivation and a fixed random stream.
pub fn simulate_single<C: Carrier + Clone>(img: &C, payload_len: usize, cfg: &AppConfig) -> Option<C> {
    let pc = cfg.preview_config();
    if payload_len > single_capacity_for_len(img.len(), &pc) {
        return None;
    }
    let mut copy = img.clone();
    embed_single(&mut copy, b"preview", &random_bytes(payload_len), &pc).ok()?;
    Some(copy)
}

/// Same for the two-passphrase layout; `hidden_len = None` fills the second slot with noise.
pub fn simulate_hidden<C: Carrier + Clone>(img: &C, public_len: usize, hidden_len: Option<usize>, cfg: &AppConfig) -> Option<C> {
    let pc = cfg.preview_config();
    let cap = deniable_capacity_for_len(img.len(), &pc);
    if public_len > cap || hidden_len.is_some_and(|h| h > cap) {
        return None;
    }
    let mut copy = img.clone();
    let (p, h) = (random_bytes(public_len), hidden_len.map(random_bytes));
    embed_deniable(&mut copy, (b"preview-public", &p), h.as_deref().map(|h| (&b"preview-hidden"[..], h)), &pc).ok()?;
    Some(copy)
}

/// Hides `payload_len` random bytes in a copy of `img` and measures it.
/// Pass `baseline` (the reading for the untouched image) to skip re-analysing it.
pub fn measure_single(img: &ImageCarrier, payload_len: usize, cfg: &AppConfig, baseline: Option<Report>) -> Option<Measured> {
    let copy = simulate_single(img, payload_len, cfg)?;
    Some(Measured { before: baseline.unwrap_or_else(|| analyze(img)), after: analyze(&copy) })
}

/// Same as [`measure_single`] for the two-passphrase layout; `hidden_len = None` fills the second slot with noise.
pub fn measure_hidden(img: &ImageCarrier, public_len: usize, hidden_len: Option<usize>, cfg: &AppConfig, baseline: Option<Report>) -> Option<Measured> {
    let copy = simulate_hidden(img, public_len, hidden_len, cfg)?;
    Some(Measured { before: baseline.unwrap_or_else(|| analyze(img)), after: analyze(&copy) })
}

#[derive(Clone, Debug)]
pub struct EmbedPreview {
    pub capacity: usize,
    pub need: usize,
    pub fits: bool,
    /// Fraction of samples that carry message bits.
    pub fill: f64,
    pub measured: Option<Measured>,
    /// Set instead of `measured` when detectability cannot be measured (audio).
    pub detect_note: Option<&'static str>,
    pub error: Option<String>,
}

/// Capacity and fit only.
pub fn fit_embed(samples: usize, payload_len: usize, cfg: &AppConfig) -> EmbedPreview {
    let capacity = single_capacity_for_len(samples, &cfg.to_config());
    let fits = payload_len <= capacity;
    let fill = if samples == 0 { 1.0 } else { (payload_len + 32) as f64 * 8.0 / samples as f64 };
    let error = (!fits).then(|| friendly::message(&Error::Capacity { needed: payload_len, available: capacity }));
    EmbedPreview { capacity, need: payload_len, fits, fill, measured: None, detect_note: None, error }
}

pub fn preview_embed(carrier: &AnyCarrier, payload_len: usize, cfg: &AppConfig, measure: bool, baseline: Option<Report>) -> EmbedPreview {
    let mut p = fit_embed(carrier.len(), payload_len, cfg);
    if measure && p.fits {
        if let AnyCarrier::Jpeg(j) = carrier {
            p.measured = measure_jpeg_single(j, payload_len, cfg, baseline);
            if p.measured.is_none() {
                p.detect_note = Some(JPEG_NOTE);
            }
            return p;
        }
        match image_of(carrier) {
            Some(img) => p.measured = measure_single(img, payload_len, cfg, baseline),
            None => p.detect_note = Some(AUDIO_NOTE),
        }
    }
    p
}

#[derive(Clone, Debug)]
pub struct HiddenPreview {
    pub slot_capacity: usize,
    pub public_fits: bool,
    /// `None` when there is no hidden file (the slot is filled with noise).
    pub hidden_fits: Option<bool>,
    pub fill: f64,
    pub measured: Option<Measured>,
    pub detect_note: Option<&'static str>,
    pub error: Option<String>,
}

/// Capacity and fit only.
pub fn fit_hidden(samples: usize, public_len: usize, hidden_len: Option<usize>, cfg: &AppConfig) -> HiddenPreview {
    let slot_capacity = deniable_capacity_for_len(samples, &cfg.to_config());
    let public_fits = public_len <= slot_capacity;
    let hidden_fits = hidden_len.map(|h| h <= slot_capacity);
    let error = if !public_fits {
        Some(format!("The public file is too big: each slot holds {} bytes at this slot size.", slot_capacity))
    } else if hidden_fits == Some(false) {
        Some(format!("The hidden file is too big: each slot holds {} bytes at this slot size.", slot_capacity))
    } else {
        None
    };
    HiddenPreview { slot_capacity, public_fits, hidden_fits, fill: cfg.rate, measured: None, detect_note: None, error }
}

pub fn preview_hidden(carrier: &AnyCarrier, public_len: usize, hidden_len: Option<usize>, cfg: &AppConfig, measure: bool, baseline: Option<Report>) -> HiddenPreview {
    let mut p = fit_hidden(carrier.len(), public_len, hidden_len, cfg);
    if measure && p.error.is_none() {
        if let AnyCarrier::Jpeg(j) = carrier {
            p.measured = measure_jpeg_hidden(j, public_len, hidden_len, cfg, baseline);
            if p.measured.is_none() {
                p.detect_note = Some(JPEG_NOTE);
            }
            return p;
        }
        match image_of(carrier) {
            Some(img) => p.measured = measure_hidden(img, public_len, hidden_len, cfg, baseline),
            None => p.detect_note = Some(AUDIO_NOTE),
        }
    }
    p
}

#[derive(Clone, Copy, Debug)]
pub struct SplitCarrier {
    pub capacity: usize,
    pub fits: bool,
}

#[derive(Clone, Debug)]
pub struct SplitPreview {
    pub share_len: usize,
    pub carriers: Vec<SplitCarrier>,
    pub n: usize,
    pub error: Option<String>,
}

/// `samples` has one entry per chosen carrier.
pub fn preview_split(samples: &[usize], secret_len: usize, k: usize, cfg: &AppConfig) -> SplitPreview {
    let share_len = secret_len + SHARE_OVERHEAD;
    let c = cfg.to_config();
    let carriers: Vec<SplitCarrier> = samples
        .iter()
        .map(|&s| {
            let capacity = single_capacity_for_len(s, &c);
            SplitCarrier { capacity, fits: share_len <= capacity }
        })
        .collect();
    let n = carriers.len();
    let error = if n == 0 {
        Some("Select at least one image.".to_string())
    } else if k == 0 || k > n {
        Some(format!("The threshold must be between 1 and {n}."))
    } else if n > 255 {
        Some("At most 255 images are supported.".to_string())
    } else if carriers.iter().any(|c| !c.fits) {
        Some("A share does not fit in one of the selected images.".to_string())
    } else {
        None
    };
    SplitPreview { share_len, carriers, n, error }
}

/// JPEG version of [`measure_single`]: the calibrated coefficient-histogram detector instead of pixel detectors.
pub fn measure_jpeg_single(j: &crate::jpeg::JpegCarrier, payload_len: usize, cfg: &AppConfig, baseline: Option<Report>) -> Option<Measured> {
    let copy = simulate_single(j, payload_len, cfg)?;
    Some(Measured { before: baseline.or_else(|| crate::analysis::analyze_jpeg(j))?, after: crate::analysis::analyze_jpeg(&copy)? })
}

/// JPEG version of [`measure_hidden`].
pub fn measure_jpeg_hidden(j: &crate::jpeg::JpegCarrier, public_len: usize, hidden_len: Option<usize>, cfg: &AppConfig, baseline: Option<Report>) -> Option<Measured> {
    let copy = simulate_hidden(j, public_len, hidden_len, cfg)?;
    Some(Measured { before: baseline.or_else(|| crate::analysis::analyze_jpeg(j))?, after: crate::analysis::analyze_jpeg(&copy)? })
}
