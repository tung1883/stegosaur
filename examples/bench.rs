//! Developer benchmark: cargo run --release --example bench
use std::time::{Duration, Instant};
use stegosaur::analysis::analyze;
use stegosaur::container::*;
use stegosaur::crypto::{master_key, KdfParams};
use stegosaur::jpeg::JpegCarrier;
use stegosaur::ImageCarrier;

fn noise(w: u32, h: u32) -> ImageCarrier {
    let mut s = 0x1234_5678_9abc_def0u64;
    let mut v = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3u32 {
                s ^= s << 13; s ^= s >> 7; s ^= s << 17;
                let base = ((x * 255 / w) + (y * 255 / h) + c * 40) / 2;
                v.push((base as i32 + (s % 17) as i32 - 8).clamp(0, 255) as u8);
            }
        }
    }
    ImageCarrier::from_rgb(w, h, v).unwrap()
}

fn t<R>(f: impl FnOnce() -> R) -> (R, Duration) {
    let a = Instant::now();
    let r = f();
    (r, a.elapsed())
}
fn ms(d: Duration) -> String { format!("{:8.1} ms", d.as_secs_f64() * 1000.0) }

fn main() {
    let cfg = Config::default();
    let pass = b"bench passphrase";
    let (_, d) = t(|| master_key(pass, &[7u8; 16], KdfParams::default()).unwrap());
    println!("scrypt key derivation (default params, once per file): {}", ms(d));
    let (_, d) = t(|| master_key(pass, &[7u8; 16], KdfParams::fast()).unwrap());
    println!("scrypt key derivation (fast params):                  {}\n", ms(d));

    println!("{:>8} {:>10} {:>10} | {:>11} {:>11} {:>11} {:>11} | {:>9}", "MP", "capacity", "payload", "embed", "extract", "analyze", "png save", "embed MB/s");
    for (w, h) in [(1000u32, 1000u32), (2000, 2000), (3264, 2448), (6000, 4000)] {
        let mut img = noise(w, h);
        let cap = single_capacity(&img, &cfg);
        let payload: Vec<u8> = (0..cap / 2).map(|i| (i * 31 % 251) as u8).collect();
        let (_, de) = t(|| embed_single(&mut img, pass, &payload, &cfg).unwrap());
        let (back, dx) = t(|| extract_single(&img, pass, &cfg).unwrap());
        assert_eq!(back, payload);
        let (_, da) = t(|| analyze(&img));
        let tmp = std::env::temp_dir().join("stegosaur_bench.png");
        let (_, dp) = t(|| img.save(&tmp).unwrap());
        let _ = std::fs::remove_file(&tmp);
        let mbs = payload.len() as f64 / 1e6 / de.as_secs_f64();
        println!("{:>8.1} {:>8} KB {:>8} KB | {} {} {} {} | {:>9.2}", w as f64 * h as f64 / 1e6, cap / 1024, payload.len() / 1024, ms(de), ms(dx), ms(da), ms(dp), mbs);
    }

    println!("\nTwo-slot (hidden) layout, 8 MP:");
    let mut img = noise(3264, 2448);
    let cap = deniable_capacity(&img, &cfg);
    let a: Vec<u8> = vec![1; cap / 2];
    let b: Vec<u8> = vec![2; cap / 2];
    let (_, de) = t(|| embed_deniable(&mut img, (pass, &a[..]), Some((b"second pass", &b[..])), &cfg).unwrap());
    let (_, dx) = t(|| extract_deniable(&img, pass, &cfg).unwrap());
    println!("  slot {} KB: embed {} extract {}", cap / 1024, ms(de), ms(dx));

    println!("\nJPEG (coefficient domain):");
    for (w, h) in [(1600u32, 1200u32), (3264, 2448), (6000, 4000)] {
        let src = noise(w, h);
        let rgb = image::RgbImage::from_raw(w, h, src.samples.clone()).unwrap();
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90).encode_image(&rgb).unwrap();
        let (mut c, dl) = t(|| JpegCarrier::from_bytes(&bytes).unwrap());
        let cap = single_capacity(&c, &cfg);
        let payload: Vec<u8> = (0..cap / 2).map(|i| (i * 7 % 253) as u8).collect();
        let (_, de) = t(|| embed_single(&mut c, pass, &payload, &cfg).unwrap());
        let (out, ds) = t(|| c.to_bytes().unwrap());
        let c2 = JpegCarrier::from_bytes(&out).unwrap();
        let (back, dx) = t(|| extract_single(&c2, pass, &cfg).unwrap());
        assert_eq!(back, payload);
        let (_, dv) = t(|| c.pixels().unwrap());
        println!("  {:>5.1} MP file {:>5} KB cap {:>5} KB: parse {} embed {} write {} extract {} preview-decode {}", w as f64 * h as f64 / 1e6, bytes.len() / 1024, cap / 1024, ms(dl), ms(de), ms(ds), ms(dx), ms(dv));
    }
}
