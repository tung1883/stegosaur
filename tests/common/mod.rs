#![allow(dead_code)]
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use stegosaur::container::Config;
use stegosaur::crypto::KdfParams;
use stegosaur::{ImageCarrier, Mode};

/// Smooth gradients plus mild sensor-like noise: statistically close enough to a
/// photograph for LSB steganalysis to behave realistically.
pub fn natural_image(w: u32, h: u32, seed: u64) -> ImageCarrier {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let phase: Vec<f64> = (0..9).map(|_| rng.gen_range(0.0..std::f64::consts::TAU)).collect();
    let mut samples = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let (xf, yf) = (x as f64, y as f64);
                let base = 128.0
                    + 55.0 * (xf / 41.0 + phase[c]).sin() * (yf / 57.0 + phase[3 + c]).cos()
                    + 28.0 * ((xf + yf) / 23.0 + phase[6 + c]).sin();
                let noise: f64 = (0..4).map(|_| rng.gen_range(-1.0..1.0)).sum::<f64>() * 1.3;
                samples.push((base + noise).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    ImageCarrier::from_rgb(w, h, samples).unwrap()
}

pub fn cfg(mode: Mode) -> Config {
    Config { kdf: KdfParams::fast(), mode, rate: 0.1, parity: 0, seed: Some(7) }
}

pub fn random_bytes(n: usize, seed: u64) -> Vec<u8> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    (0..n).map(|_| rng.r#gen()).collect()
}

/// Fraction of samples that differ between two equally sized carriers.
pub fn changed_fraction(a: &ImageCarrier, b: &ImageCarrier, range: std::ops::Range<usize>) -> f64 {
    let diff = range.clone().filter(|&i| a.samples[i] != b.samples[i]).count();
    diff as f64 / range.len() as f64
}
