//! Pixel views for a graphical front end: the image as it was, as it is now, where it changed, and
//! the least-significant-bit plane. All four are RGBA, downscaled to fit a preview.

use crate::carrier::ImageCarrier;

const CHANGED: [u8; 3] = [238, 138, 63];
const UNCHANGED: [u8; 3] = [248, 243, 228];
const BIT_ONE: [u8; 3] = [46, 59, 44];
const BIT_ZERO: [u8; 3] = [244, 240, 226];

pub struct Views {
    pub width: u32,
    pub height: u32,
    /// Fraction of all samples (not pixels) that differ between the two images.
    pub changed: f64,
    pub original: Vec<u8>,
    pub stego: Vec<u8>,
    /// Orange where any sample in the source area changed, cream elsewhere.
    pub diff: Vec<u8>,
    /// Green-channel LSB of the stego image: dark for 1, light for 0.
    pub lsb: Vec<u8>,
}

fn put(buf: &mut [u8], i: usize, rgb: [u8; 3]) {
    buf[i * 4..i * 4 + 3].copy_from_slice(&rgb);
    buf[i * 4 + 3] = 255;
}

/// Returns `None` when the two images do not have the same dimensions.
pub fn render_views(orig: &ImageCarrier, stego: &ImageCarrier, max_dim: u32) -> Option<Views> {
    if (orig.width, orig.height) != (stego.width, stego.height) || orig.width == 0 || orig.height == 0 {
        return None;
    }
    let (w, h) = (orig.width as usize, orig.height as usize);
    let scale = (max_dim.max(1) as f64 / w.max(h) as f64).min(1.0);
    let (ow, oh) = (((w as f64 * scale).round() as usize).max(1), ((h as f64 * scale).round() as usize).max(1));

    let differing = orig.samples.iter().zip(&stego.samples).filter(|(a, b)| a != b).count();
    let changed = differing as f64 / orig.samples.len().max(1) as f64;

    let n = ow * oh;
    let (mut original, mut stego_px, mut diff, mut lsb) = (vec![0u8; n * 4], vec![0u8; n * 4], vec![0u8; n * 4], vec![0u8; n * 4]);
    for oy in 0..oh {
        let (y0, y1) = (oy * h / oh, ((oy + 1) * h / oh).max(oy * h / oh + 1).min(h));
        for ox in 0..ow {
            let (x0, x1) = (ox * w / ow, ((ox + 1) * w / ow).max(ox * w / ow + 1).min(w));
            let mut any = false;
            'scan: for y in y0..y1 {
                let row = y * w * 3;
                for x in x0..x1 {
                    let i = row + x * 3;
                    if orig.samples[i..i + 3] != stego.samples[i..i + 3] {
                        any = true;
                        break 'scan;
                    }
                }
            }
            let c = ((y0 + y1) / 2 * w + (x0 + x1) / 2) * 3;
            let i = oy * ow + ox;
            put(&mut original, i, [orig.samples[c], orig.samples[c + 1], orig.samples[c + 2]]);
            put(&mut stego_px, i, [stego.samples[c], stego.samples[c + 1], stego.samples[c + 2]]);
            put(&mut diff, i, if any { CHANGED } else { UNCHANGED });
            put(&mut lsb, i, if stego.samples[c + 1] & 1 == 1 { BIT_ONE } else { BIT_ZERO });
        }
    }
    Some(Views { width: ow as u32, height: oh as u32, changed, original, stego: stego_px, diff, lsb })
}
