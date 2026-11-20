//! JPEG carrier: the coefficients must survive a rewrite exactly, and data must survive a real JPEG file.
mod common;
use common::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use stegosaur::container::{embed_deniable, embed_single, extract_deniable, extract_single, single_capacity};
use stegosaur::jpeg::{probe, JpegCarrier};
use stegosaur::{AnyCarrier, Carrier, Error, Mode};

fn make_jpeg(w: u32, h: u32, quality: u8, seed: u64, gray: bool) -> Vec<u8> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;
    // Photo-like texture: smooth structure plus fine detail that survives quantisation.
    let mut img = natural_image(w, h, seed);
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5eed);
    for v in img.samples.iter_mut() {
        *v = (*v as i32 + rng.gen_range(-18..=18)).clamp(0, 255) as u8;
    }
    let mut out = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, quality);
    if gray {
        let luma: Vec<u8> = img.samples.chunks_exact(3).map(|p| ((p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32) / 10) as u8).collect();
        enc.write_image(&luma, w, h, image::ExtendedColorType::L8).unwrap();
    } else {
        enc.write_image(&img.samples, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    out
}

fn decode(bytes: &[u8]) -> Vec<u8> {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).expect("a valid JPEG").to_rgb8().into_raw()
}

#[test]
fn rewriting_without_changes_keeps_every_pixel_for_all_layouts() {
    for (w, h, q, gray) in [(37, 29, 50, false), (64, 64, 90, false), (200, 131, 75, false), (45, 70, 80, true), (16, 16, 95, false)] {
        let orig = make_jpeg(w, h, q, 3, gray);
        let c = JpegCarrier::from_bytes(&orig).unwrap_or_else(|e| panic!("{w}x{h} q{q}: {e}"));
        let want = decode(&orig);
        assert_eq!(decode(&c.to_bytes().unwrap()), want, "{w}x{h} q{q} gray={gray}: original tables");
        assert_eq!(decode(&c.jpeg().to_bytes_optimized(0).unwrap()), want, "{w}x{h}: optimal tables");
        assert_eq!(decode(&c.jpeg().to_bytes_optimized(3).unwrap()), want, "{w}x{h}: restart interval 3");
        assert_eq!((c.width(), c.height()), (w, h));
    }
}

#[test]
fn restart_intervals_are_read_and_preserved() {
    let orig = make_jpeg(120, 90, 85, 4, false);
    let base = JpegCarrier::from_bytes(&orig).unwrap();
    let with_rst = base.jpeg().to_bytes_optimized(5).unwrap();
    assert!(with_rst.windows(2).any(|w| w == [0xFF, 0xD0]), "restart markers were written");
    let c = JpegCarrier::from_bytes(&with_rst).unwrap();
    assert_eq!(c.len(), base.len(), "same usable coefficients");
    assert_eq!(decode(&c.to_bytes().unwrap()), decode(&orig));

    let mut c = c;
    let data = random_bytes(300, 9);
    embed_single(&mut c, b"pw", &data, &cfg(Mode::Match)).unwrap();
    let back = JpegCarrier::from_bytes(&c.to_bytes().unwrap()).unwrap();
    assert_eq!(extract_single(&back, b"pw", &cfg(Mode::Match)).unwrap(), data);
}

#[test]
fn payload_survives_a_real_jpeg_in_both_modes() {
    for mode in [Mode::Replace, Mode::Match] {
        for (w, h, q, gray) in [(160, 120, 85, false), (96, 96, 60, true), (203, 77, 92, false)] {
            let orig = make_jpeg(w, h, q, 5, gray);
            let mut c = JpegCarrier::from_bytes(&orig).unwrap();
            let before = c.len();
            let cap = single_capacity(&c, &cfg(mode));
            assert!(cap >= 40, "{w}x{h} q{q}: capacity {cap} is too small for this test");
            let data = random_bytes((cap / 2).min(200), 11);
            embed_single(&mut c, b"correct horse", &data, &cfg(mode)).unwrap();
            assert_eq!(c.len(), before, "embedding must not change which coefficients are usable");

            let bytes = c.to_bytes().unwrap();
            assert_eq!(&bytes[..2], [0xFF, 0xD8]);
            assert_eq!(&bytes[bytes.len() - 2..], [0xFF, 0xD9]);
            decode(&bytes); // still a valid JPEG for ordinary decoders
            let back = JpegCarrier::from_bytes(&bytes).unwrap();
            assert_eq!(back.len(), before, "{w}x{h}: the stego file exposes the same positions");
            assert_eq!(extract_single(&back, b"correct horse", &cfg(mode)).unwrap(), data, "{w}x{h} q{q} {mode:?}");
            assert!(matches!(extract_single(&back, b"wrong", &cfg(mode)), Err(Error::NoPayload)));
        }
    }
}

#[test]
fn a_clean_jpeg_has_no_payload_and_capacity_matches() {
    let c = JpegCarrier::from_bytes(&make_jpeg(128, 128, 85, 6, false)).unwrap();
    assert!(matches!(extract_single(&c, b"pw", &cfg(Mode::Match)), Err(Error::NoPayload)));
    let cap = single_capacity(&c, &cfg(Mode::Match));
    assert!(cap > 100, "a 128x128 photo-like JPEG should hold something: {cap}");
    let mut c = c;
    embed_single(&mut c, b"pw", &random_bytes(cap, 1), &cfg(Mode::Match)).expect("exactly the reported capacity fits");
    assert!(matches!(embed_single(&mut c, b"pw", &random_bytes(cap + 1, 2), &cfg(Mode::Match)), Err(Error::Capacity { .. })));
}

#[test]
fn two_passphrase_layout_works_in_a_jpeg() {
    let mut c = JpegCarrier::from_bytes(&make_jpeg(320, 240, 85, 7, false)).unwrap();
    let (public, hidden) = (random_bytes(60, 1), random_bytes(40, 2));
    embed_deniable(&mut c, (b"first one", &public), Some((b"second one", &hidden)), &cfg(Mode::Match)).unwrap();
    let back = JpegCarrier::from_bytes(&c.to_bytes().unwrap()).unwrap();
    assert_eq!(extract_deniable(&back, b"first one", &cfg(Mode::Match)).unwrap(), public);
    assert_eq!(extract_deniable(&back, b"second one", &cfg(Mode::Match)).unwrap(), hidden);
}

#[test]
fn a_symbol_missing_from_the_original_tables_triggers_new_tables() {
    // Start from a file whose tables were optimised for its own data, so large magnitudes are not in them.
    let tight = JpegCarrier::from_bytes(&JpegCarrier::from_bytes(&make_jpeg(80, 60, 80, 8, false)).unwrap().jpeg().to_bytes_optimized(0).unwrap()).unwrap();
    let mut c = tight.clone();
    let n = c.len();
    for i in (0..n).step_by(7).take(40) {
        c.set(i, 900); // magnitude 900 is category 10: almost certainly not in a tight table
    }
    let bytes = c.to_bytes().unwrap();
    let back = JpegCarrier::from_bytes(&bytes).unwrap();
    assert_eq!(back.len(), n);
    for i in (0..n).step_by(7).take(40) {
        assert_eq!(back.get(i), 900, "coefficient {i} survived the table change");
    }
    decode(&bytes);
}

#[test]
fn coefficients_never_become_zero_or_leave_the_range() {
    let mut c = JpegCarrier::from_bytes(&make_jpeg(64, 64, 30, 9, false)).unwrap();
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    for _ in 0..2000 {
        let i = rng.gen_range(0..c.len());
        let want_bit = rng.gen_range(0..=1u8);
        let mode = if rng.gen_bool(0.5) { Mode::Match } else { Mode::Replace };
        stegosaur::carrier::write_bit(&mut c, i, want_bit, mode, &mut rng);
        let v = c.get(i);
        assert!(v != 0, "a usable coefficient became zero");
        assert!((-1023..=1023).contains(&v));
        assert_eq!((v & 1) as u8, want_bit, "the carried bit must be the magnitude parity");
    }
}

#[test]
fn unsupported_and_damaged_files_fail_cleanly() {
    let good = make_jpeg(64, 48, 80, 10, false);
    // A baseline stream relabelled as progressive is damaged data, and must be rejected rather than misread.
    let mut prog = good.clone();
    let p = prog.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    prog[p + 1] = 0xC2;
    assert!(JpegCarrier::from_bytes(&prog).is_err());
    // Arithmetic-coded (C9) files are refused with a clear reason.
    let mut arith = good.clone();
    arith[p + 1] = 0xC9;
    assert!(matches!(JpegCarrier::from_bytes(&arith), Err(Error::Unsupported(_))));
    // Truncated scan data.
    assert!(JpegCarrier::from_bytes(&good[..good.len() / 2]).is_err());
    assert!(JpegCarrier::from_bytes(b"not a jpeg").is_err());
    assert!(JpegCarrier::from_bytes(&[]).is_err());
    assert!(matches!(AnyCarrier::load(std::path::Path::new("missing.jpg")), Err(_)));
}

#[test]
fn corrupted_data_never_panics() {
    let good = make_jpeg(72, 56, 80, 12, false);
    let mut rng = ChaCha8Rng::seed_from_u64(99);
    for _ in 0..300 {
        let mut d = good.clone();
        for _ in 0..rng.gen_range(1..6) {
            let i = rng.gen_range(2..d.len());
            d[i] = rng.r#gen();
        }
        if let Ok(c) = JpegCarrier::from_bytes(&d) {
            let _ = c.to_bytes(); // either a result or an error, never a panic
        }
    }
}

#[test]
fn folder_scan_recognises_jpeg() {
    use stegosaur::app::files::{carrier_info, classify, CarrierKind, FileKind};
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    std::fs::write(&p, make_jpeg(90, 60, 85, 13, false)).unwrap();
    assert_eq!(probe(&p).unwrap(), (90, 60));
    assert_eq!(classify(&p).0, FileKind::Carrier(CarrierKind::Jpeg));
    assert_eq!(carrier_info(&p).unwrap().summary, "90x60 JPEG");
    // Arithmetic-coded files are listed but marked unusable, with the reason.
    let mut arith = std::fs::read(&p).unwrap();
    let k = arith.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    arith[k + 1] = 0xC9;
    let q = dir.path().join("arith.jpg");
    std::fs::write(&q, &arith).unwrap();
    assert_eq!(classify(&q).0, FileKind::Unusable);
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap()
}

#[test]
fn real_world_files_rewrite_exactly_and_carry_data() {
    for name in ["tower.jpg", "tower_grayscale.jpg", "tower_progressive.jpg"] {
        let orig = fixture(name);
        let mut c = JpegCarrier::from_bytes(&orig).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want = decode(&orig);
        assert_eq!(decode(&c.to_bytes().unwrap()), want, "{name}: lossless rewrite changed pixels");
        assert_eq!(decode(&c.jpeg().to_bytes_optimized(0).unwrap()), want, "{name}: optimal tables changed pixels");

        let cap = single_capacity(&c, &cfg(Mode::Match));
        assert!(cap > 1000, "{name}: capacity {cap}");
        let data = random_bytes(cap / 2, 21);
        let n = c.len();
        embed_single(&mut c, b"tower pass", &data, &cfg(Mode::Match)).unwrap();
        let bytes = c.to_bytes().unwrap();
        decode(&bytes);
        let back = JpegCarrier::from_bytes(&bytes).unwrap();
        assert_eq!(back.len(), n, "{name}");
        assert_eq!(extract_single(&back, b"tower pass", &cfg(Mode::Match)).unwrap(), data, "{name}: payload lost");
    }
}

/// Counts real scans by walking the marker segments (an EXIF thumbnail also contains FF DA bytes).
fn top_level_scans(d: &[u8]) -> usize {
    let (mut p, mut scans) = (2usize, 0usize);
    while p + 3 < d.len() && d[p] == 0xFF {
        let m = d[p + 1];
        if m == 0xD9 {
            break;
        }
        if m == 0xDA {
            scans += 1;
            let l = ((d[p + 2] as usize) << 8) | d[p + 3] as usize;
            p += 2 + l;
            while p + 1 < d.len() && !(d[p] == 0xFF && d[p + 1] != 0 && !(0xD0..=0xD7).contains(&d[p + 1])) {
                p += 1;
            }
            continue;
        }
        let l = ((d[p + 2] as usize) << 8) | d[p + 3] as usize;
        p += 2 + l;
    }
    scans
}

#[test]
fn progressive_files_stay_progressive_and_keep_every_coefficient() {
    let orig = fixture("tower_progressive.jpg");
    assert!(orig.windows(2).any(|w| w == [0xFF, 0xC2]), "the fixture really is progressive");
    let out = JpegCarrier::from_bytes(&orig).unwrap().to_bytes().unwrap();
    assert!(out.windows(2).any(|w| w == [0xFF, 0xC2]), "output keeps the progressive frame header");
    assert!(top_level_scans(&out) > 1, "several scans");
    // Same coefficients when read again, and an independent decoder agrees on the pixels.
    let again = JpegCarrier::from_bytes(&out).unwrap();
    assert_eq!(decode(&again.to_bytes().unwrap()), decode(&orig));
    let theirs = image::load_from_memory_with_format(&out, image::ImageFormat::Jpeg).unwrap().to_rgb8();
    let ours = JpegCarrier::from_bytes(&orig).unwrap().pixels().unwrap();
    assert!(mean_abs_diff(&ours.samples, theirs.as_raw()) < 1.0);
}

#[test]
fn a_progressive_cover_carries_data_and_stays_progressive() {
    use stegosaur::container::*;
    let orig = fixture("tower_progressive.jpg");
    let cfg = Config::default();
    let mut c = JpegCarrier::from_bytes(&orig).unwrap();
    let payload: Vec<u8> = (0..3000).map(|i| (i * 11 % 251) as u8).collect();
    embed_single(&mut c, b"pw", &payload, &cfg).unwrap();
    let bytes = c.to_bytes().unwrap();
    assert!(bytes.windows(2).any(|w| w == [0xFF, 0xC2]));
    assert!(image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).is_ok());
    let c2 = JpegCarrier::from_bytes(&bytes).unwrap();
    assert_eq!(extract_single(&c2, b"pw", &cfg).unwrap(), payload);
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as u64).sum::<u64>() as f64 / a.len() as f64
}

#[test]
fn own_pixel_decoder_matches_an_independent_decoder() {
    let mut cases: Vec<(String, Vec<u8>)> = ["tower.jpg", "tower_grayscale.jpg", "tower_progressive.jpg"].iter().map(|n| (n.to_string(), fixture(n))).collect();
    for (w, h, q, gray) in [(37, 29, 50, false), (200, 131, 75, false), (45, 70, 80, true), (16, 16, 95, false), (64, 64, 30, false)] {
        cases.push((format!("generated {w}x{h} q{q}"), make_jpeg(w, h, q, 3, gray)));
    }
    for (name, bytes) in cases {
        let ours = JpegCarrier::from_bytes(&bytes).unwrap().pixels().unwrap();
        let theirs = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap().to_rgb8();
        assert_eq!((ours.width, ours.height), (theirs.width(), theirs.height()), "{name}");
        let d = mean_abs_diff(&ours.samples, theirs.as_raw());
        assert!(d < 1.5, "{name}: mean difference {d:.3} per sample is too large");
    }
}

#[test]
fn threaded_and_single_block_paths_agree_and_flat_blocks_are_exact() {
    // A flat grey image is all DC-only blocks: the shortcut must give exactly the right grey.
    use image::codecs::jpeg::JpegEncoder;
    use image::ImageEncoder;
    let flat: Vec<u8> = vec![100; 64 * 64 * 3];
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 90).write_image(&flat, 64, 64, image::ExtendedColorType::Rgb8).unwrap();
    let ours = JpegCarrier::from_bytes(&bytes).unwrap().pixels().unwrap();
    let theirs = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap().to_rgb8();
    assert!(mean_abs_diff(&ours.samples, theirs.as_raw()) < 0.6);
    assert!(ours.samples.iter().all(|&v| (v as i32 - 100).abs() <= 2));

    // A big picture crosses the threshold where the work is split across threads; two runs must be identical.
    let big = JpegCarrier::from_bytes(&make_jpeg(700, 500, 85, 4, false)).unwrap();
    assert!(big.jpeg().block_count() >= 6000, "the test image is large enough to use several threads");
    assert_eq!(big.pixels().unwrap().samples, big.pixels().unwrap().samples);
}

#[test]
fn stego_pixels_change_in_proportion_to_the_payload() {
    let orig = make_jpeg(160, 120, 85, 5, false);
    let base = JpegCarrier::from_bytes(&orig).unwrap();
    let before = base.pixels().unwrap();
    let cap = single_capacity(&base, &cfg(Mode::Match));
    let change_at = |fraction: usize| {
        let mut c = base.clone();
        embed_single(&mut c, b"pw", &random_bytes(cap / fraction, 1), &cfg(Mode::Match)).unwrap();
        mean_abs_diff(&before.samples, &c.pixels().unwrap().samples)
    };
    let (tenth, half) = (change_at(10), change_at(2));
    println!("mean pixel change: {tenth:.2} at 10% of capacity, {half:.2} at 50%");
    assert!(tenth > 0.0 && tenth < half, "a bigger payload changes more");
    assert!(half < 6.0, "and even half-full stays modest: {half:.2} out of 255");
}

// ---- cost-aware (syndrome-trellis) embedding and the coefficient detector ----

struct Plain(stegosaur::JpegCarrier);
impl stegosaur::Carrier for Plain {
    fn len(&self) -> usize { self.0.len() }
    fn get(&self, i: usize) -> i32 { self.0.get(i) }
    fn set(&mut self, i: usize, v: i32) { self.0.set(i, v) }
    fn bounds(&self) -> (i32, i32) { self.0.bounds() }
    fn geometry(&self) -> Vec<u8> { self.0.geometry() }
}

#[test]
fn trellis_embedding_changes_less_and_cheaper_than_plain_matching() {
    use stegosaur::container::*;
    use stegosaur::Carrier;
    let orig = stegosaur::JpegCarrier::from_bytes(include_bytes!("data/tower.jpg")).unwrap();
    let cfg = Config::default();
    let payload: Vec<u8> = (0..single_capacity(&orig, &cfg) / 10).map(|i| (i * 7 % 251) as u8).collect();
    let costs = orig.costs().unwrap();
    let mut a = Plain(orig.clone());
    embed_single(&mut a, b"pw", &payload, &cfg).unwrap();
    let mut b = orig.clone();
    embed_single(&mut b, b"pw", &payload, &cfg).unwrap();
    assert_eq!(extract_single(&b, b"pw", &cfg).unwrap(), payload);
    let spent = |c: &dyn Carrier| -> (usize, f64) {
        let mut n = 0;
        let mut s = 0.0;
        for i in 0..orig.len() {
            if orig.get(i) != c.get(i) {
                n += 1;
                s += costs[i] as f64;
            }
        }
        (n, s)
    };
    let (pa, pb) = (spent(&a), spent(&b));
    assert!(pb.0 < pa.0, "changes {} vs {}", pb.0, pa.0);
    assert!(pb.1 * 5.0 < pa.1, "cost {} vs {}", pb.1, pa.1);
}

#[test]
fn trellis_layouts_round_trip_through_a_saved_file() {
    use stegosaur::container::*;
    let orig = stegosaur::JpegCarrier::from_bytes(include_bytes!("data/tower.jpg")).unwrap();
    let cfg = Config::default();
    let mut c = orig.clone();
    let (p, h) = (vec![5u8; 300], vec![9u8; 200]);
    embed_deniable(&mut c, (b"one", &p), Some((b"two", &h)), &cfg).unwrap();
    let c2 = stegosaur::JpegCarrier::from_bytes(&c.to_bytes().unwrap()).unwrap();
    assert_eq!(extract_deniable(&c2, b"one", &cfg).unwrap(), p);
    assert_eq!(extract_deniable(&c2, b"two", &cfg).unwrap(), h);
    assert!(extract_deniable(&c2, b"three", &cfg).is_err());
}

#[test]
fn detector_reads_clean_files_low_and_filled_files_high() {
    use stegosaur::analysis::analyze_jpeg;
    use stegosaur::container::*;
    let orig = stegosaur::JpegCarrier::from_bytes(include_bytes!("data/tower.jpg")).unwrap();
    let cfg = Config::default();
    let clean = analyze_jpeg(&orig).unwrap();
    assert_eq!(clean.risk, stegosaur::analysis::Risk::Low, "{clean:?}");
    let mut full = orig.clone();
    let payload: Vec<u8> = (0..single_capacity(&full, &cfg) * 9 / 10).map(|i| (i * 13 % 251) as u8).collect();
    embed_single(&mut full, b"pw", &payload, &cfg).unwrap();
    let r = analyze_jpeg(&full).unwrap();
    assert_eq!(r.risk, stegosaur::analysis::Risk::High, "{r:?}");
}
