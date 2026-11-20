//! Writes a folder of sample images and files for trying the desktop app by hand or in automated checks.
//! Usage: cargo run -p stegosaur-desktop-core --example make_fixtures -- <folder>

use stegosaur::ImageCarrier;

fn image(w: u32, h: u32, seed: u32) -> ImageCarrier {
    let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
    let mut samples = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3u32 {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let sky = (y as f64) < h as f64 * 0.62;
                let base = if sky { 150.0 + y as f64 * 0.25 + c as f64 * 12.0 } else { 120.0 - (y as f64 - h as f64 * 0.62) * 0.3 + (c == 1) as u32 as f64 * 40.0 };
                let dx = x as f64 - w as f64 * 0.45;
                let dy = y as f64 - h as f64 * 0.62;
                let dino = if dx * dx / 3600.0 + dy * dy / 1500.0 < 1.0 { 40.0 } else { 0.0 };
                let noise = ((s >> 24) as f64 / 255.0 - 0.5) * 5.0;
                samples.push((base + dino + noise).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    ImageCarrier::from_rgb(w, h, samples).unwrap()
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: make_fixtures <folder>"));
    std::fs::create_dir_all(&dir).unwrap();
    image(400, 300, 1).save(&dir.join("beach.png")).unwrap();
    image(320, 240, 2).save(&dir.join("city.png")).unwrap();
    for i in 0..3 {
        image(160, 120, 10 + i).save(&dir.join(format!("share{i}.png"))).unwrap();
    }
    // One file per additional cover format.
    let pic = stegosaur::AnyCarrier::Image(image(300, 200, 7));
    pic.save(&dir.join("scan.tif")).unwrap();
    pic.save(&dir.join("web.webp")).unwrap();
    let samples: Vec<i16> = (0..44_100u32 * 2).map(|i| ((i as f64 * 0.05).sin() * 8000.0 + ((i.wrapping_mul(2654435761) >> 24) as f64 - 128.0) * 4.0) as i16).collect();
    let spec = hound::WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    stegosaur::AnyCarrier::Wav(stegosaur::WavCarrier { spec, samples }).save(&dir.join("song.flac")).unwrap();
    std::fs::write(dir.join("tower.jpg"), include_bytes!("../../../../tests/data/tower.jpg")).unwrap();
    std::fs::write(dir.join("tower_progressive.jpg"), include_bytes!("../../../../tests/data/tower_progressive.jpg")).unwrap();
    let bytes = |n: usize, seed: u8| (0..n).map(|i| (i as u8).wrapping_mul(29).wrapping_add(seed)).collect::<Vec<u8>>();
    std::fs::write(dir.join("report.bin"), bytes(30000, 1)).unwrap();
    std::fs::write(dir.join("secret.bin"), bytes(200, 2)).unwrap();
    std::fs::write(dir.join("hidden.bin"), bytes(300, 3)).unwrap();
    std::fs::write(dir.join("notes.txt"), b"holiday plans: nothing to see here").unwrap();
    std::fs::write(dir.join("photo.jpg"), b"not really a jpeg").unwrap();
    std::fs::create_dir_all(dir.join("old")).unwrap();
    println!("fixtures written to {}", dir.display());
}
