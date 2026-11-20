mod common;
use common::*;
use proptest::prelude::*;
use stegosaur::container::{embed_single, extract_single, single_capacity};
use stegosaur::{Carrier, Error, ImageCarrier, Mode, WavCarrier};

#[test]
fn roundtrip_both_modes() {
    for mode in [Mode::Replace, Mode::Match] {
        let mut img = natural_image(64, 64, 1);
        let data = random_bytes(500, 2);
        embed_single(&mut img, b"pw", &data, &cfg(mode)).unwrap();
        assert_eq!(extract_single(&img, b"pw", &cfg(mode)).unwrap(), data);
    }
}

#[test]
fn wrong_passphrase_fails() {
    let mut img = natural_image(64, 64, 1);
    embed_single(&mut img, b"right", b"secret", &cfg(Mode::Match)).unwrap();
    assert!(matches!(extract_single(&img, b"wrong", &cfg(Mode::Match)), Err(Error::NoPayload)));
}

#[test]
fn clean_carrier_yields_no_payload() {
    let img = natural_image(64, 64, 1);
    assert!(matches!(extract_single(&img, b"pw", &cfg(Mode::Match)), Err(Error::NoPayload)));
}

#[test]
fn empty_passphrase_rejected() {
    let mut img = natural_image(32, 32, 1);
    assert!(matches!(embed_single(&mut img, b"", b"x", &cfg(Mode::Match)), Err(Error::Invalid(_))));
}

#[test]
fn empty_payload_roundtrips() {
    let mut img = natural_image(32, 32, 1);
    embed_single(&mut img, b"pw", b"", &cfg(Mode::Match)).unwrap();
    assert_eq!(extract_single(&img, b"pw", &cfg(Mode::Match)).unwrap(), Vec::<u8>::new());
}

#[test]
fn capacity_is_exact() {
    let mut img = natural_image(40, 40, 3);
    let cap = single_capacity(&img, &cfg(Mode::Match));
    assert_eq!(cap, 40 * 40 * 3 / 8 - 32);
    let data = random_bytes(cap, 4);
    embed_single(&mut img, b"pw", &data, &cfg(Mode::Match)).unwrap();
    assert_eq!(extract_single(&img, b"pw", &cfg(Mode::Match)).unwrap(), data);

    let mut img2 = natural_image(40, 40, 3);
    let too_big = random_bytes(cap + 1, 5);
    assert!(matches!(
        embed_single(&mut img2, b"pw", &too_big, &cfg(Mode::Match)),
        Err(Error::Capacity { .. })
    ));
}

#[test]
fn tampering_with_payload_samples_is_detected() {
    let mut img = natural_image(64, 64, 1);
    embed_single(&mut img, b"pw", &random_bytes(300, 9), &cfg(Mode::Replace)).unwrap();
    let clean = natural_image(64, 64, 1);
    let touched: Vec<usize> = (0..img.samples.len()).filter(|&i| img.samples[i] != clean.samples[i]).collect();
    assert!(!touched.is_empty());
    for &i in touched.iter().step_by(37) {
        let mut broken = img.clone();
        broken.samples[i] ^= 1;
        let r = extract_single(&broken, b"pw", &cfg(Mode::Replace));
        assert!(r.is_err(), "flipping payload bit at sample {i} must break authentication");
    }
}

#[test]
fn unmodified_samples_stay_unmodified() {
    let clean = natural_image(64, 64, 1);
    let mut img = clean.clone();
    let data = random_bytes(100, 11);
    embed_single(&mut img, b"pw", &data, &cfg(Mode::Match)).unwrap();
    let frame_bits = (4 + 12 + 100 + 16) * 8;
    let changed = (0..img.samples.len()).filter(|&i| img.samples[i] != clean.samples[i]).count();
    assert!(changed <= frame_bits, "changed {changed} samples for {frame_bits} bits");
    assert!(changed as f64 > frame_bits as f64 * 0.35, "about half of the bits should flip");
    for i in 0..img.samples.len() {
        assert!((img.samples[i] as i32 - clean.samples[i] as i32).abs() <= 1, "+-1 only");
    }
}

#[test]
fn match_mode_respects_sample_range() {
    let mut img = ImageCarrier::from_rgb(32, 32, (0..32 * 32 * 3).map(|i| if i % 2 == 0 { 0 } else { 255 }).collect()).unwrap();
    let data = random_bytes(200, 3);
    embed_single(&mut img, b"pw", &data, &cfg(Mode::Match)).unwrap();
    assert_eq!(extract_single(&img, b"pw", &cfg(Mode::Match)).unwrap(), data);
}

#[test]
fn survives_png_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.png");
    let mut img = natural_image(64, 64, 8);
    let data = random_bytes(400, 6);
    embed_single(&mut img, b"pw", &data, &cfg(Mode::Match)).unwrap();
    img.save(&path).unwrap();
    let loaded = ImageCarrier::load(&path).unwrap();
    assert_eq!(extract_single(&loaded, b"pw", &cfg(Mode::Match)).unwrap(), data);
}

#[test]
fn wav_roundtrip_including_file() {
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let samples: Vec<i16> = (0..20000).map(|i| ((i as f64 / 20.0).sin() * 12000.0) as i16).collect();
    let mut wav = WavCarrier { spec, samples };
    let data = random_bytes(300, 12);
    embed_single(&mut wav, b"pw", &data, &cfg(Mode::Match)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.wav");
    wav.save(&path).unwrap();
    let loaded = WavCarrier::load(&path).unwrap();
    assert_eq!(extract_single(&loaded, b"pw", &cfg(Mode::Match)).unwrap(), data);
}

#[test]
fn wav_extremes_do_not_overflow() {
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let samples: Vec<i16> = (0..8000).map(|i| if i % 2 == 0 { i16::MIN } else { i16::MAX }).collect();
    let mut wav = WavCarrier { spec, samples };
    let data = random_bytes(100, 1);
    embed_single(&mut wav, b"pw", &data, &cfg(Mode::Match)).unwrap();
    assert_eq!(extract_single(&wav, b"pw", &cfg(Mode::Match)).unwrap(), data);
}

#[test]
fn default_kdf_parameters_work() {
    let mut img = natural_image(32, 32, 1);
    let c = stegosaur::Config { seed: Some(1), ..Default::default() };
    embed_single(&mut img, b"pw", b"hello", &c).unwrap();
    assert_eq!(extract_single(&img, b"pw", &c).unwrap(), b"hello");
}

#[test]
fn different_carriers_use_different_positions() {
    let mut a = natural_image(32, 32, 1);
    let mut b = natural_image(48, 48, 1);
    let clean_a = a.clone();
    let clean_b = b.clone();
    embed_single(&mut a, b"pw", b"same data", &cfg(Mode::Replace)).unwrap();
    embed_single(&mut b, b"pw", b"same data", &cfg(Mode::Replace)).unwrap();
    let pa: Vec<usize> = (0..a.samples.len()).filter(|&i| a.samples[i] != clean_a.samples[i]).collect();
    let pb: Vec<usize> = (0..b.samples.len()).filter(|&i| b.samples[i] != clean_b.samples[i]).collect();
    assert_ne!(pa, pb);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn prop_roundtrip(len in 0usize..=832, seed in any::<u64>(), pw in proptest::collection::vec(any::<u8>(), 1..24), replace in any::<bool>()) {
        let mode = if replace { Mode::Replace } else { Mode::Match };
        let mut img = natural_image(48, 48, seed % 5);
        let data = random_bytes(len, seed);
        embed_single(&mut img, &pw, &data, &cfg(mode)).unwrap();
        prop_assert_eq!(extract_single(&img, &pw, &cfg(mode)).unwrap(), data);
    }

    #[test]
    fn prop_positions_are_distinct_and_in_range(seed in any::<u8>(), start in 0usize..50, len in 1usize..400, take in 0usize..400) {
        let take = take.min(len);
        let key = [seed; 32];
        let mut s = stegosaur::embed::PositionStream::new(&key, start, len);
        let p = s.take(take);
        let mut seen = std::collections::HashSet::new();
        for &x in &p {
            prop_assert!((x as usize) >= start && (x as usize) < start + len);
            prop_assert!(seen.insert(x));
        }
        let mut s2 = stegosaur::embed::PositionStream::new(&key, start, len);
        let half = take / 2;
        let mut a = s2.take(half);
        a.extend(s2.take(take - half));
        prop_assert_eq!(p, a, "stream must be prefix-stable");
    }
}

#[test]
fn carrier_trait_is_object_safe_enough() {
    let mut img = natural_image(32, 32, 1);
    let dynref: &mut dyn Carrier = &mut img;
    embed_single(dynref, b"pw", b"dyn", &cfg(Mode::Match)).unwrap();
    assert_eq!(extract_single(dynref, b"pw", &cfg(Mode::Match)).unwrap(), b"dyn");
}

#[test]
fn saved_png_is_lossless_valid_and_compressed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fast.png");
    let img = natural_image(200, 150, 4);
    img.save(&path).unwrap();
    let back = ImageCarrier::load(&path).unwrap();
    assert_eq!((back.width, back.height), (200, 150));
    assert_eq!(back.samples, img.samples, "PNG must round-trip every sample exactly");
    let size = std::fs::metadata(&path).unwrap().len() as usize;
    assert!(size < img.samples.len(), "expected some compression, got {size} of {}", img.samples.len());

    let bmp = dir.path().join("x.bmp");
    img.save(&bmp).unwrap();
    assert_eq!(ImageCarrier::load(&bmp).unwrap().samples, img.samples);
}
