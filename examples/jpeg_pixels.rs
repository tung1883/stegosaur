//! Developer check: our own JPEG pixel decoder against an independent one (zune-jpeg, via the `image` crate).
//! Usage: cargo run --release --example jpeg_pixels -- <list.txt> <base-dir>

use std::time::{Duration, Instant};
use stegosaur::jpeg::JpegCarrier;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let list = std::fs::read_to_string(&args[1]).unwrap();
    let base = std::path::PathBuf::from(&args[2]);
    let (mut files, mut mp_total) = (0usize, 0f64);
    let (mut t_ours, mut t_old, mut t_zune) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut worst: Vec<(f64, String)> = Vec::new();
    let (mut sum_mean, mut max_any) = (0f64, 0i32);
    for line in list.lines().filter(|l| !l.trim().is_empty()) {
        let bytes = match std::fs::read(base.join(line.trim())) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Ok(c) = JpegCarrier::from_bytes(&bytes) else { continue };
        let Some(theirs) = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).ok().map(|i| i.to_rgb8()) else { continue };

        let t = Instant::now();
        let ours = c.pixels().unwrap();
        t_ours += t.elapsed();
        let t = Instant::now();
        let re = c.to_bytes().unwrap();
        let _ = image::load_from_memory_with_format(&re, image::ImageFormat::Jpeg).unwrap();
        t_old += t.elapsed();
        let t = Instant::now();
        let _ = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap();
        t_zune += t.elapsed();

        assert_eq!((ours.width, ours.height), (theirs.width(), theirs.height()), "{line}");
        let (mut sum, mut mx) = (0u64, 0i32);
        for (a, b) in ours.samples.iter().zip(theirs.as_raw()) {
            let d = (*a as i32 - *b as i32).abs();
            sum += d as u64;
            mx = mx.max(d);
        }
        let mean = sum as f64 / ours.samples.len() as f64;
        files += 1;
        mp_total += ours.width as f64 * ours.height as f64 / 1e6;
        sum_mean += mean;
        max_any = max_any.max(mx);
        worst.push((mean, format!("{line} (max diff {mx})")));
    }
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("{files} files, {mp_total:.0} MP total");
    println!("mean absolute difference per sample: average {:.3}, worst file {:.3}, largest single difference {max_any}", sum_mean / files as f64, worst[0].0);
    for (m, n) in worst.iter().take(4) {
        println!("  worst: {m:.3}  {n}");
    }
    println!("time to get pixels: ours {t_ours:?} | old path (re-encode + decode) {t_old:?} | decoding the original file directly with zune {t_zune:?}");
}
