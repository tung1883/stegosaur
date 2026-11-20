use crate::carrier::{is_audio_ext, is_image_ext, is_jpeg_ext, webp_is_lossless, AnyCarrier, Carrier};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CarrierKind {
    Png,
    Bmp,
    Tiff,
    Webp,
    Ppm,
    Jpeg,
    Wav,
    Flac,
}

impl CarrierKind {
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Wav | Self::Flac)
    }

    /// True for formats stored as pixels, where the classical LSB detectors apply.
    /// (JPEG data lives in DCT coefficients, so pixel detectors say nothing about it.)
    pub fn has_pixel_detectors(self) -> bool {
        !self.is_audio() && self != Self::Jpeg
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Dir,
    /// A lossless image or 16-bit PCM audio file that can carry hidden data.
    Carrier(CarrierKind),
    /// Looks like a carrier but cannot hold data (lossy format, unreadable, wrong sample format).
    Unusable,
    Other,
}

#[derive(Clone, Debug)]
pub struct CarrierInfo {
    pub kind: CarrierKind,
    pub width: u32,
    pub height: u32,
    /// Number of LSB-capable samples; the only input capacity depends on.
    pub samples: usize,
    /// "1024x768" or "44.1 kHz, 2 ch".
    pub summary: String,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    /// Present for usable carriers.
    pub info: Option<CarrierInfo>,
    /// Why a file is unusable, for display next to it.
    pub note: Option<String>,
}

fn extension(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

/// Reads only the file header, so scanning a folder of large images is fast.
pub fn carrier_info(path: &Path) -> Result<CarrierInfo> {
    match extension(path).as_deref() {
        Some(ext) if is_image_ext(ext) => {
            if ext == "webp" {
                let mut head = vec![0u8; 4096];
                let n = std::io::Read::read(&mut std::fs::File::open(path)?, &mut head)?;
                if !webp_is_lossless(&head[..n]) {
                    return Err(Error::Unsupported("lossy WebP: hidden data would not survive".into()));
                }
            }
            let (width, height) = image::image_dimensions(path)?;
            Ok(CarrierInfo {
                kind: match ext {
                    "png" => CarrierKind::Png,
                    "bmp" => CarrierKind::Bmp,
                    "webp" => CarrierKind::Webp,
                    "ppm" => CarrierKind::Ppm,
                    _ => CarrierKind::Tiff,
                },
                width,
                height,
                samples: width as usize * height as usize * 3,
                summary: format!("{width}x{height}"),
            })
        }
        Some("wav") => {
            let r = hound::WavReader::open(path)?;
            let spec = r.spec();
            if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
                return Err(Error::Unsupported("only 16-bit PCM WAV files can carry data".into()));
            }
            Ok(CarrierInfo {
                kind: CarrierKind::Wav,
                width: 0,
                height: 0,
                samples: r.len() as usize,
                summary: format!("{:.1} kHz, {} ch", spec.sample_rate as f64 / 1000.0, spec.channels),
            })
        }
        Some(ext) if is_jpeg_ext(ext) => {
            // Only the header is read here; the exact number of usable coefficients is known once the file is loaded.
            let (width, height) = crate::jpeg::probe(path)?;
            Ok(CarrierInfo { kind: CarrierKind::Jpeg, width, height, samples: 0, summary: format!("{width}x{height} JPEG") })
        }
        Some("flac") => {
            let r = claxon::FlacReader::open(path).map_err(|e| Error::Invalid(format!("FLAC: {e}")))?;
            let info = r.streaminfo();
            if info.bits_per_sample != 16 {
                return Err(Error::Unsupported("only 16-bit FLAC files can carry data".into()));
            }
            Ok(CarrierInfo {
                kind: CarrierKind::Flac,
                width: 0,
                height: 0,
                samples: info.samples.unwrap_or(0) as usize * info.channels as usize,
                summary: format!("{:.1} kHz, {} ch", info.sample_rate as f64 / 1000.0, info.channels),
            })
        }
        other => Err(Error::Unsupported(format!(
            "{} files cannot carry data (use PNG, BMP, TIFF, lossless WebP, PPM, baseline JPEG, 16-bit WAV or FLAC)",
            other.unwrap_or("this type of")
        ))),
    }
}

const LOSSY: &[&str] = &["heic", "heif", "mp3", "aac", "m4a", "ogg", "opus"];

pub fn classify(path: &Path) -> (FileKind, Option<CarrierInfo>, Option<String>) {
    if path.is_dir() {
        return (FileKind::Dir, None, None);
    }
    let ext = extension(path);
    match ext.as_deref() {
        Some(e) if is_image_ext(e) || is_audio_ext(e) || is_jpeg_ext(e) => match carrier_info(path) {
            Ok(info) => (FileKind::Carrier(info.kind), Some(info), None),
            Err(e) => (FileKind::Unusable, None, Some(crate::app::friendly::message(&e))),
        },
        Some(e) if LOSSY.contains(&e) => (FileKind::Unusable, None, Some("lossy format: hidden data would not survive".into())),
        _ => (FileKind::Other, None, None),
    }
}

/// Lists a folder: sub-folders first, then files, each group sorted by name ignoring case.
/// Dot-files are skipped.
pub fn scan_dir(dir: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = item.path();
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let (kind, info, note) = classify(&path);
        out.push(Entry { path, name, kind, size: meta.len(), info, note });
    }
    out.sort_by(|a, b| {
        let dir_rank = |e: &Entry| if e.kind == FileKind::Dir { 0 } else { 1 };
        dir_rank(a).cmp(&dir_rank(b)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Keeps a few decoded carriers in memory so repeated previews of the same image do not decode it again.
/// An entry is reused only while the file's size and modification time are unchanged.
pub struct CarrierCache {
    slots: Mutex<Vec<(PathBuf, SystemTime, u64, Arc<AnyCarrier>)>>,
    capacity: usize,
}

impl CarrierCache {
    pub fn new(capacity: usize) -> Self {
        Self { slots: Mutex::new(Vec::new()), capacity: capacity.max(1) }
    }

    pub fn load(&self, path: &Path) -> Result<Arc<AnyCarrier>> {
        let meta = std::fs::metadata(path)?;
        let (mtime, len) = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
        let mut slots = self.slots.lock().unwrap();
        if let Some(pos) = slots.iter().position(|(p, t, l, _)| p == path && *t == mtime && *l == len) {
            let hit = slots.remove(pos);
            let arc = hit.3.clone();
            slots.push(hit);
            return Ok(arc);
        }
        drop(slots);
        let loaded = Arc::new(AnyCarrier::load(path)?);
        let mut slots = self.slots.lock().unwrap();
        slots.retain(|(p, ..)| p != path);
        slots.push((path.to_path_buf(), mtime, len, loaded.clone()));
        while slots.len() > self.capacity {
            slots.remove(0);
        }
        Ok(loaded)
    }

    /// Like [`carrier_info`], but with the exact number of usable samples. A JPEG has to be decoded to count its
    /// usable coefficients, so that is done here (and cached) instead of while listing a folder.
    pub fn info_exact(&self, path: &Path) -> Result<CarrierInfo> {
        let mut info = carrier_info(path)?;
        if info.kind == CarrierKind::Jpeg {
            info.samples = self.load(path)?.len();
        }
        Ok(info)
    }

    pub fn len(&self) -> usize {
        self.slots.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
