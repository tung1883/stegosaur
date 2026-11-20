//! Developer check: runs the JPEG carrier over a list of real files (one path per line). Files are only read.
//! Usage: cargo run --release --example jpeg_check -- <list.txt> [base-dir]

use stegosaur::container::{embed_single, extract_single, single_capacity, Config};
use stegosaur::crypto::KdfParams;
use stegosaur::jpeg::JpegCarrier;
use stegosaur::{Carrier, Error, Mode};

fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).ok().map(|i| i.to_rgb8().into_raw())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let list = std::fs::read_to_string(&args[1]).unwrap();
    let base = args.get(2).cloned().unwrap_or_default();
    let cfg = Config { kdf: KdfParams::fast(), mode: Mode::Match, rate: 0.1, parity: 0, seed: Some(7) };

    let (mut total, mut ok, mut progressive, mut other_unsupported, mut fail) = (0, 0, 0, 0, 0);
    let (mut too_small, mut optimal_used, mut restart_files) = (0, 0, 0);
    let mut bytes_total = 0u64;
    let mut cap_total = 0usize;
    for line in list.lines().filter(|l| !l.trim().is_empty()) {
        total += 1;
        let path = std::path::Path::new(&base).join(line.trim());
        let orig = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("READ    {line}: {e}");
                fail += 1;
                continue;
            }
        };
        bytes_total += orig.len() as u64;
        let mut c = match JpegCarrier::from_bytes(&orig) {
            Ok(c) => c,
            Err(Error::Unsupported(m)) => {
                if m.contains("progressive") {
                    progressive += 1;
                } else {
                    other_unsupported += 1;
                    println!("UNSUPP  {line}: {m}");
                }
                continue;
            }
            Err(e) => {
                println!("PARSE   {line}: {e}");
                fail += 1;
                continue;
            }
        };
        // 1. lossless rewrite, pixel for pixel
        let want = decode(&orig);
        let same = c.to_bytes().ok().and_then(|b| decode(&b)) == want && want.is_some();
        let same_opt = c.jpeg().to_bytes_optimized(0).ok().and_then(|b| decode(&b)) == want;
        if orig.windows(2).any(|w| w == [0xFF, 0xDD]) {
            restart_files += 1;
        }
        if !same || !same_opt {
            println!("REWRITE {line}: original-tables ok={same} optimal-tables ok={same_opt}");
            fail += 1;
            continue;
        }
        // 2. hide and recover
        let cap = single_capacity(&c, &cfg);
        cap_total += cap;
        if cap < 32 {
            too_small += 1;
            ok += 1;
            continue;
        }
        let data: Vec<u8> = (0..cap.min(2000)).map(|i| (i * 31 + 7) as u8).collect();
        let usable = c.len();
        if embed_single(&mut c, b"check phrase", &data, &cfg).is_err() {
            println!("EMBED   {line}");
            fail += 1;
            continue;
        }
        let stego = match c.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                println!("WRITE   {line}: {e}");
                fail += 1;
                continue;
            }
        };
        if stego.len() > orig.len() + orig.len() / 2 + 2000 {
            optimal_used += 1;
        }
        let good = decode(&stego).is_some()
            && JpegCarrier::from_bytes(&stego).map(|b| b.len() == usable && extract_single(&b, b"check phrase", &cfg).ok().as_deref() == Some(&data[..])).unwrap_or(false);
        if good {
            ok += 1;
        } else {
            println!("ROUNDTRIP {line}");
            fail += 1;
        }
    }
    println!(
        "files {total}: ok {ok} (of which {too_small} too small to hide in), progressive-skipped {progressive}, other-unsupported {other_unsupported}, FAILED {fail}; {:.1} MB read, total capacity {:.1} MB, {restart_files} had restart markers, {optimal_used} grew a lot",
        bytes_total as f64 / 1e6,
        cap_total as f64 / 1e6
    );
}
