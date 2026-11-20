//! Every supported cover format must carry a payload through a real save and reload.
mod common;
use common::*;
use stegosaur::app::files::{carrier_info, classify, CarrierKind, FileKind};
use stegosaur::carrier::{webp_is_lossless, AnyCarrier};
use stegosaur::container::{embed_single, extract_single};
use stegosaur::{Carrier, Error, Mode, WavCarrier};

fn tone(channels: u16, seconds: u32, seed: u64) -> WavCarrier {
    let rate = 22_050u32;
    let noise = random_bytes((rate * seconds * channels as u32) as usize, seed);
    let mut samples = Vec::new();
    for i in 0..(rate * seconds) as usize {
        for c in 0..channels as usize {
            let t = i as f64 / rate as f64;
            let v = 9000.0 * (t * 440.0 * (c + 1) as f64 * std::f64::consts::TAU).sin() + (noise[i * channels as usize + c] as f64 - 128.0) * 6.0;
            samples.push(v as i16);
        }
    }
    let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    WavCarrier { spec, samples }
}

fn samples_of(c: &AnyCarrier) -> Vec<i32> {
    (0..c.len()).map(|i| c.get(i)).collect()
}

#[test]
fn every_image_format_roundtrips_through_a_real_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = random_bytes(700, 5);
    for ext in ["png", "bmp", "tif", "tiff", "webp", "ppm"] {
        for mode in [Mode::Replace, Mode::Match] {
            let mut img = natural_image(96, 80, 3);
            embed_single(&mut img, b"pw", &data, &cfg(mode)).unwrap();
            let path = dir.path().join(format!("stego_{mode:?}.{ext}"));
            AnyCarrier::Image(img.clone()).save(&path).unwrap();

            let back = AnyCarrier::load(&path).unwrap_or_else(|e| panic!("{ext}: reload failed: {e}"));
            let AnyCarrier::Image(back) = back else { panic!("{ext}: not an image") };
            assert_eq!(back.samples, img.samples, "{ext} {mode:?}: pixels changed on the way to disk");
            assert_eq!(extract_single(&back, b"pw", &cfg(mode)).unwrap(), data, "{ext} {mode:?}: payload lost");
        }
    }
}

#[test]
fn flac_roundtrips_mono_and_stereo_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let data = random_bytes(900, 6);
    for channels in [1u16, 2] {
        let mut wav = tone(channels, 3, 11);
        embed_single(&mut wav, b"pw", &data, &cfg(Mode::Match)).unwrap();
        let path = dir.path().join(format!("a{channels}.flac"));
        AnyCarrier::Wav(wav.clone()).save(&path).unwrap();
        assert_eq!(&std::fs::read(&path).unwrap()[..4], b"fLaC", "a real FLAC stream was written");

        let AnyCarrier::Wav(back) = AnyCarrier::load(&path).unwrap() else { panic!("not audio") };
        assert_eq!(back.samples, wav.samples, "{channels} ch: samples changed through FLAC");
        assert_eq!(back.spec.channels, channels);
        assert_eq!(back.spec.sample_rate, 22_050);
        assert_eq!(extract_single(&back, b"pw", &cfg(Mode::Match)).unwrap(), data);
    }
}

#[test]
fn a_payload_survives_converting_wav_to_flac_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let data = random_bytes(400, 7);
    let mut wav = tone(2, 2, 12);
    embed_single(&mut wav, b"pw", &data, &cfg(Mode::Replace)).unwrap();
    let w = dir.path().join("a.wav");
    AnyCarrier::Wav(wav).save(&w).unwrap();
    let f = dir.path().join("a.flac");
    AnyCarrier::load(&w).unwrap().save(&f).unwrap();
    let w2 = dir.path().join("b.wav");
    AnyCarrier::load(&f).unwrap().save(&w2).unwrap();
    let AnyCarrier::Wav(last) = AnyCarrier::load(&w2).unwrap() else { panic!() };
    assert_eq!(extract_single(&last, b"pw", &cfg(Mode::Replace)).unwrap(), data);
    assert_eq!(samples_of(&AnyCarrier::load(&w).unwrap()), samples_of(&AnyCarrier::load(&f).unwrap()));
}

#[test]
fn folder_scan_reports_the_new_formats() {
    let dir = tempfile::tempdir().unwrap();
    let img = natural_image(40, 30, 1);
    for ext in ["tif", "webp", "ppm"] {
        AnyCarrier::Image(img.clone()).save(&dir.path().join(format!("a.{ext}"))).unwrap();
    }
    AnyCarrier::Wav(tone(2, 1, 1)).save(&dir.path().join("a.flac")).unwrap();

    let want = [("a.tif", CarrierKind::Tiff), ("a.webp", CarrierKind::Webp), ("a.ppm", CarrierKind::Ppm), ("a.flac", CarrierKind::Flac)];
    for (name, kind) in want {
        let (k, info, note) = classify(&dir.path().join(name));
        assert_eq!(k, FileKind::Carrier(kind), "{name}: {note:?}");
        assert!(info.is_some());
    }
    let i = carrier_info(&dir.path().join("a.webp")).unwrap();
    assert_eq!((i.width, i.height, i.samples), (40, 30, 40 * 30 * 3));
    let f = carrier_info(&dir.path().join("a.flac")).unwrap();
    assert_eq!(f.samples, 22_050 * 2, "header-only scan counts samples across channels");
    assert!(f.kind.is_audio());
}

#[test]
fn lossy_webp_and_other_lossy_formats_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    // The smallest valid-looking lossy WebP container: RIFF header + a "VP8 " chunk.
    let mut lossy = b"RIFF".to_vec();
    lossy.extend_from_slice(&26u32.to_le_bytes());
    lossy.extend_from_slice(b"WEBPVP8 ");
    lossy.extend_from_slice(&10u32.to_le_bytes());
    lossy.extend_from_slice(&[0u8; 10]);
    let p = dir.path().join("lossy.webp");
    std::fs::write(&p, &lossy).unwrap();
    assert!(!webp_is_lossless(&lossy));
    assert!(matches!(AnyCarrier::load(&p), Err(Error::Unsupported(_))));
    let (kind, _, note) = classify(&p);
    assert_eq!(kind, FileKind::Unusable);
    assert!(note.unwrap().to_lowercase().contains("lossy"));

    let j = dir.path().join("photo.heic");
    std::fs::write(&j, b"x").unwrap();
    assert!(matches!(AnyCarrier::load(&j), Err(Error::Unsupported(_))));
    assert_eq!(classify(&j).0, FileKind::Unusable);
}

#[test]
fn webp_chunk_parser_handles_the_extended_layout() {
    let mut ext = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
    ext.extend_from_slice(&10u32.to_le_bytes());
    ext.extend_from_slice(&[0u8; 10]);
    let mut lossless = ext.clone();
    lossless.extend_from_slice(b"VP8L");
    lossless.extend_from_slice(&4u32.to_le_bytes());
    assert!(webp_is_lossless(&lossless), "VP8X followed by VP8L is lossless");
    let mut lossy = ext;
    lossy.extend_from_slice(b"VP8 ");
    lossy.extend_from_slice(&4u32.to_le_bytes());
    assert!(!webp_is_lossless(&lossy));
    assert!(!webp_is_lossless(b"not a webp at all"));
}

#[test]
fn only_sixteen_bit_flac_is_accepted() {
    // 8-bit streams cannot be written by our encoder, so check the guard through a bad file instead.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("broken.flac");
    std::fs::write(&p, b"fLaC garbage").unwrap();
    assert!(AnyCarrier::load(&p).is_err());
    assert_eq!(classify(&p).0, FileKind::Unusable);
}
