use crate::error::{Error, Result};
use rand::RngCore;
use std::path::Path;

/// Lossless image formats that can carry data. Output is written in whatever format the file name asks for.
pub const IMAGE_EXTS: &[&str] = &["png", "bmp", "tif", "tiff", "webp", "ppm"];
/// Lossless 16-bit audio formats that can carry data.
pub const AUDIO_EXTS: &[&str] = &["wav", "flac"];
/// JPEG is embedded in the DCT coefficients, not the pixels.
pub const JPEG_EXTS: &[&str] = &["jpg", "jpeg"];

pub fn ext_of(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

pub fn is_image_ext(ext: &str) -> bool {
    IMAGE_EXTS.contains(&ext)
}

pub fn is_audio_ext(ext: &str) -> bool {
    AUDIO_EXTS.contains(&ext)
}

pub fn is_jpeg_ext(ext: &str) -> bool {
    JPEG_EXTS.contains(&ext)
}

/// True when a WebP file stores its pixels losslessly (VP8L). Lossy WebP re-quantises data and cannot carry a payload.
pub fn webp_is_lossless(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]) as usize;
        match id {
            b"VP8L" => return true,
            b"VP8 " => return false,
            _ => pos += 8 + size + (size & 1),
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Overwrite the LSB. Fast but trivially detected by chi-square / RS / SPA.
    Replace,
    /// Randomly add or subtract one when the LSB differs (LSB matching, +-1).
    Match,
}

/// An integer sample array whose least significant bits can carry data.
pub trait Carrier {
    fn len(&self) -> usize;
    fn get(&self, i: usize) -> i32;
    fn set(&mut self, i: usize, v: i32);
    fn bounds(&self) -> (i32, i32);
    /// Bytes that stay constant under embedding; used as KDF salt material.
    fn geometry(&self) -> Vec<u8>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Per-sample cost of changing the sample by one (higher is easier to detect). Carriers that
    /// provide it are embedded with a syndrome-trellis code that spends the changes where they
    /// cost least; others use plain keyed matching.
    /// True when embedding uses `costs()`; fixed per carrier type so the reader can tell.
    fn adaptive(&self) -> bool {
        false
    }

    fn costs(&self) -> Option<Vec<f32>> {
        None
    }
}

#[inline]
pub fn write_bit<C: Carrier + ?Sized, R: RngCore>(c: &mut C, i: usize, bit: u8, mode: Mode, rng: &mut R) {
    let v = c.get(i);
    if (v & 1) as u8 == bit {
        return;
    }
    let nv = match mode {
        Mode::Replace => v ^ 1,
        Mode::Match => {
            let (lo, hi) = c.bounds();
            if v <= lo {
                v + 1
            } else if v >= hi {
                v - 1
            } else if rng.next_u32() & 1 == 1 {
                v + 1
            } else {
                v - 1
            }
        }
    };
    c.set(i, nv);
}

#[derive(Clone)]
pub struct ImageCarrier {
    pub width: u32,
    pub height: u32,
    pub samples: Vec<u8>,
}

impl ImageCarrier {
    pub fn from_rgb(width: u32, height: u32, samples: Vec<u8>) -> Result<Self> {
        if samples.len() != (width as usize) * (height as usize) * 3 {
            return Err(Error::Invalid("sample count does not match dimensions".into()));
        }
        Ok(Self { width, height, samples })
    }

    pub fn load(path: &Path) -> Result<Self> {
        if ext_of(path).as_deref() == Some("webp") {
            let mut head = vec![0u8; 4096];
            let n = std::io::Read::read(&mut std::fs::File::open(path)?, &mut head)?;
            if !webp_is_lossless(&head[..n]) {
                return Err(Error::Unsupported("lossy WebP cannot carry data (only lossless WebP can)".into()));
            }
        }
        let img = image::open(path)?.to_rgb8();
        Ok(Self { width: img.width(), height: img.height(), samples: img.into_raw() })
    }

    /// PNG output uses fast compression with the Sub filter: still lossless, about 4% larger than the
    /// default, and roughly 80x faster to write (1 ms vs 93 ms for 512x512).
    pub fn save(&self, path: &Path) -> Result<()> {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::ImageEncoder;
        let is_png = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("png"));
        if is_png {
            let file = std::io::BufWriter::new(std::fs::File::create(path)?);
            PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::Sub).write_image(
                &self.samples,
                self.width,
                self.height,
                image::ExtendedColorType::Rgb8,
            )?;
        } else {
            let img = image::RgbImage::from_raw(self.width, self.height, self.samples.clone())
                .ok_or_else(|| Error::Invalid("bad image buffer".into()))?;
            img.save(path)?;
        }
        Ok(())
    }

    /// Splits interleaved RGB into three row-major planes.
    pub fn planes(&self) -> [Vec<u8>; 3] {
        let n = self.samples.len() / 3;
        let mut p = [Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
        for px in self.samples.chunks_exact(3) {
            p[0].push(px[0]);
            p[1].push(px[1]);
            p[2].push(px[2]);
        }
        p
    }
}

impl Carrier for ImageCarrier {
    fn len(&self) -> usize {
        self.samples.len()
    }
    #[inline]
    fn get(&self, i: usize) -> i32 {
        self.samples[i] as i32
    }
    #[inline]
    fn set(&mut self, i: usize, v: i32) {
        self.samples[i] = v as u8;
    }
    fn bounds(&self) -> (i32, i32) {
        (0, 255)
    }
    fn geometry(&self) -> Vec<u8> {
        let mut g = b"img".to_vec();
        g.extend_from_slice(&self.width.to_le_bytes());
        g.extend_from_slice(&self.height.to_le_bytes());
        g
    }
}

#[derive(Clone)]
pub struct WavCarrier {
    pub spec: hound::WavSpec,
    pub samples: Vec<i16>,
}

impl WavCarrier {
    pub fn load(path: &Path) -> Result<Self> {
        if ext_of(path).as_deref() == Some("flac") {
            return Self::load_flac(path);
        }
        let mut r = hound::WavReader::open(path)?;
        let spec = r.spec();
        if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
            return Err(Error::Unsupported("only 16-bit PCM WAV is supported".into()));
        }
        let samples = r.samples::<i16>().collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Self { spec, samples })
    }

    fn load_flac(path: &Path) -> Result<Self> {
        let mut r = claxon::FlacReader::open(path).map_err(|e| Error::Invalid(format!("FLAC: {e}")))?;
        let info = r.streaminfo();
        if info.bits_per_sample != 16 {
            return Err(Error::Unsupported("only 16-bit FLAC is supported".into()));
        }
        let spec = hound::WavSpec {
            channels: info.channels as u16,
            sample_rate: info.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let samples = r
            .samples()
            .map(|s| s.map(|v| v as i16))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Invalid(format!("FLAC: {e}")))?;
        Ok(Self { spec, samples })
    }

    fn save_flac(&self, path: &Path) -> Result<()> {
        use flacenc::component::BitRepr;
        use flacenc::error::Verify;
        let bad = |what: &str, e: String| Error::Invalid(format!("FLAC {what}: {e}"));
        let config = flacenc::config::Encoder::default().into_verified().map_err(|e| bad("settings", format!("{e:?}")))?;
        let samples: Vec<i32> = self.samples.iter().map(|&s| s as i32).collect();
        let source = flacenc::source::MemSource::from_samples(&samples, self.spec.channels as usize, 16, self.spec.sample_rate as usize);
        let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|e| bad("encoding", format!("{e:?}")))?;
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).map_err(|e| bad("writing", format!("{e:?}")))?;
        std::fs::write(path, sink.as_slice())?;
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if ext_of(path).as_deref() == Some("flac") {
            return self.save_flac(path);
        }
        let mut w = hound::WavWriter::create(path, self.spec)?;
        for &s in &self.samples {
            w.write_sample(s)?;
        }
        w.finalize()?;
        Ok(())
    }
}

impl Carrier for WavCarrier {
    fn len(&self) -> usize {
        self.samples.len()
    }
    #[inline]
    fn get(&self, i: usize) -> i32 {
        self.samples[i] as i32
    }
    #[inline]
    fn set(&mut self, i: usize, v: i32) {
        self.samples[i] = v as i16;
    }
    fn bounds(&self) -> (i32, i32) {
        (i16::MIN as i32, i16::MAX as i32)
    }
    fn geometry(&self) -> Vec<u8> {
        let mut g = b"wav".to_vec();
        g.extend_from_slice(&self.spec.channels.to_le_bytes());
        g.extend_from_slice(&self.spec.sample_rate.to_le_bytes());
        g.extend_from_slice(&(self.samples.len() as u64).to_le_bytes());
        g
    }
}

pub enum AnyCarrier {
    Image(ImageCarrier),
    Wav(WavCarrier),
    Jpeg(crate::jpeg::JpegCarrier),
}

impl Carrier for AnyCarrier {
    fn len(&self) -> usize {
        match self {
            Self::Image(c) => c.len(),
            Self::Wav(c) => c.len(),
            Self::Jpeg(c) => c.len(),
        }
    }
    #[inline]
    fn get(&self, i: usize) -> i32 {
        match self {
            Self::Image(c) => c.get(i),
            Self::Wav(c) => c.get(i),
            Self::Jpeg(c) => c.get(i),
        }
    }
    #[inline]
    fn set(&mut self, i: usize, v: i32) {
        match self {
            Self::Image(c) => c.set(i, v),
            Self::Wav(c) => c.set(i, v),
            Self::Jpeg(c) => c.set(i, v),
        }
    }
    fn bounds(&self) -> (i32, i32) {
        match self {
            Self::Image(c) => c.bounds(),
            Self::Wav(c) => c.bounds(),
            Self::Jpeg(c) => c.bounds(),
        }
    }
    fn geometry(&self) -> Vec<u8> {
        match self {
            Self::Image(c) => c.geometry(),
            Self::Wav(c) => c.geometry(),
            Self::Jpeg(c) => c.geometry(),
        }
    }
    fn adaptive(&self) -> bool {
        matches!(self, Self::Jpeg(_))
    }
    fn costs(&self) -> Option<Vec<f32>> {
        match self {
            Self::Image(c) => c.costs(),
            Self::Wav(c) => c.costs(),
            Self::Jpeg(c) => c.costs(),
        }
    }
}

impl AnyCarrier {
    pub fn load(path: &Path) -> Result<Self> {
        let ext = ext_of(path);
        match ext.as_deref() {
            Some(e) if is_image_ext(e) => Ok(Self::Image(ImageCarrier::load(path)?)),
            Some(e) if is_audio_ext(e) => Ok(Self::Wav(WavCarrier::load(path)?)),
            Some(e) if is_jpeg_ext(e) => Ok(Self::Jpeg(crate::jpeg::JpegCarrier::load(path)?)),
            other => Err(Error::Unsupported(format!(
                "carrier type {:?}: lossless images (PNG, BMP, TIFF, WebP, PPM), baseline JPEG and 16-bit audio (WAV, FLAC) only",
                other
            ))),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        match self {
            Self::Image(c) => c.save(path),
            Self::Wav(c) => c.save(path),
            Self::Jpeg(c) => c.save(path),
        }
    }
}
