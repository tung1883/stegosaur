mod common;
use common::*;
use stegosaur::analysis::analyze;
use stegosaur::container::{deniable_capacity, embed_deniable, extract_deniable};
use stegosaur::{Error, Mode};

#[test]
fn decoy_and_hidden_are_independent() {
    let mut img = natural_image(128, 128, 1);
    let c = cfg(Mode::Match);
    let decoy = b"grocery list: eggs, milk";
    let hidden = random_bytes(200, 3);
    embed_deniable(&mut img, (b"decoy-pass", decoy), Some((b"real-pass", &hidden)), &c).unwrap();
    assert_eq!(extract_deniable(&img, b"decoy-pass", &c).unwrap(), decoy);
    assert_eq!(extract_deniable(&img, b"real-pass", &c).unwrap(), hidden);
    assert!(matches!(extract_deniable(&img, b"other", &c), Err(Error::NoPayload)));
}

#[test]
fn decoy_only_still_works_and_hidden_pass_finds_nothing() {
    let mut img = natural_image(128, 128, 1);
    let c = cfg(Mode::Match);
    embed_deniable(&mut img, (b"decoy-pass", b"innocent"), None, &c).unwrap();
    assert_eq!(extract_deniable(&img, b"decoy-pass", &c).unwrap(), b"innocent");
    assert!(matches!(extract_deniable(&img, b"real-pass", &c), Err(Error::NoPayload)));
}

#[test]
fn identical_passphrases_are_rejected() {
    let mut img = natural_image(64, 64, 1);
    let r = embed_deniable(&mut img, (b"same", b"a"), Some((b"same", b"b")), &cfg(Mode::Match));
    assert!(matches!(r, Err(Error::Invalid(_))));
}

#[test]
fn oversize_payload_is_rejected_for_either_slot() {
    let mut img = natural_image(64, 64, 1);
    let c = cfg(Mode::Match);
    let cap = deniable_capacity(&img, &c);
    assert!(cap > 0);
    let big = random_bytes(cap + 1, 1);
    assert!(matches!(embed_deniable(&mut img.clone(), (b"a", &big), None, &c), Err(Error::Capacity { .. })));
    assert!(matches!(embed_deniable(&mut img, (b"a", b"x"), Some((b"b", &big)), &c), Err(Error::Capacity { .. })));
}

#[test]
fn max_capacity_payloads_roundtrip() {
    let mut img = natural_image(96, 96, 2);
    let c = cfg(Mode::Match);
    let cap = deniable_capacity(&img, &c);
    let (d, h) = (random_bytes(cap, 1), random_bytes(cap, 2));
    embed_deniable(&mut img, (b"a", &d), Some((b"b", &h)), &c).unwrap();
    assert_eq!(extract_deniable(&img, b"a", &c).unwrap(), d);
    assert_eq!(extract_deniable(&img, b"b", &c).unwrap(), h);
}

#[test]
fn empty_payloads_roundtrip() {
    let mut img = natural_image(64, 64, 2);
    let c = cfg(Mode::Match);
    embed_deniable(&mut img, (b"a", b""), Some((b"b", b"")), &c).unwrap();
    assert_eq!(extract_deniable(&img, b"a", &c).unwrap(), Vec::<u8>::new());
    assert_eq!(extract_deniable(&img, b"b", &c).unwrap(), Vec::<u8>::new());
}

#[test]
fn hidden_payload_size_is_not_visible_in_footprint() {
    let clean = natural_image(160, 160, 4);
    let c = cfg(Mode::Replace);
    let n = clean.samples.len();
    let measure = |hidden: Option<&[u8]>| {
        let mut img = clean.clone();
        embed_deniable(&mut img, (b"decoy", b"hello"), hidden.map(|h| (&b"real"[..], h)), &c).unwrap();
        (changed_fraction(&clean, &img, 0..n / 2), changed_fraction(&clean, &img, n / 2..n))
    };
    let (a0, b0) = measure(None);
    let (a1, b1) = measure(Some(&random_bytes(10, 1)));
    let (a2, b2) = measure(Some(&random_bytes(deniable_capacity(&clean, &c), 2)));
    // With replacement about half of the targeted bits flip; the slot is fully written in every case.
    for f in [a0, b0, a1, b1, a2, b2] {
        assert!((f - 0.1 / 2.0).abs() < 0.008, "changed fraction {f} should be near rate/2");
    }
}

#[test]
fn analysis_cannot_separate_hidden_from_no_hidden() {
    // A single image is too noisy for this (a clean 192x192 image can already read rs=0.04), so compare
    // the mean shift against the clean original over several images.
    let c = cfg(Mode::Match);
    let trials = 10;
    let (mut d_hidden, mut d_decoy) = ([0.0f64; 2], [0.0f64; 2]);
    for seed in 0..trials {
        let clean = natural_image(192, 192, 50 + seed);
        let base = analyze(&clean);
        let mut with_hidden = clean.clone();
        let mut without = clean.clone();
        embed_deniable(&mut with_hidden, (b"decoy", b"hello"), Some((b"real", &random_bytes(500, seed))), &c).unwrap();
        embed_deniable(&mut without, (b"decoy", b"hello"), None, &c).unwrap();
        let (h, d) = (analyze(&with_hidden), analyze(&without));
        d_hidden[0] += (h.rs - base.rs) / trials as f64;
        d_hidden[1] += (h.spa - base.spa) / trials as f64;
        d_decoy[0] += (d.rs - base.rs) / trials as f64;
        d_decoy[1] += (d.spa - base.spa) / trials as f64;
    }
    println!("mean shift vs clean: with hidden rs={:+.4} spa={:+.4} | decoy only rs={:+.4} spa={:+.4}",
        d_hidden[0], d_hidden[1], d_decoy[0], d_decoy[1]);
    assert!((d_hidden[0] - d_decoy[0]).abs() < 0.015, "rs shift differs");
    assert!((d_hidden[1] - d_decoy[1]).abs() < 0.015, "spa shift differs");
}

#[test]
fn second_half_bit_statistics_match_across_hidden_and_filler() {
    use stegosaur::Carrier;
    let clean = natural_image(160, 160, 6);
    let c = cfg(Mode::Match);
    let n = clean.len();
    let lsb_ones = |img: &stegosaur::ImageCarrier| {
        (n / 2..n).filter(|&i| img.samples[i] & 1 == 1).count() as f64 / (n - n / 2) as f64
    };
    let mut a = clean.clone();
    let mut b = clean.clone();
    let cap = deniable_capacity(&clean, &c);
    embed_deniable(&mut a, (b"d", b"x"), Some((b"h", &random_bytes(cap, 3))), &c).unwrap();
    embed_deniable(&mut b, (b"d", b"x"), None, &c).unwrap();
    assert!((lsb_ones(&a) - lsb_ones(&b)).abs() < 0.01);
}

#[test]
fn works_in_replace_mode_too() {
    let mut img = natural_image(96, 96, 7);
    let c = cfg(Mode::Replace);
    embed_deniable(&mut img, (b"d", b"decoy"), Some((b"h", b"hidden")), &c).unwrap();
    assert_eq!(extract_deniable(&img, b"h", &c).unwrap(), b"hidden");
}

#[test]
fn tiny_carrier_is_rejected_cleanly() {
    let mut img = natural_image(8, 8, 1);
    assert!(embed_deniable(&mut img, (b"a", b"x"), None, &cfg(Mode::Match)).is_err());
    assert!(extract_deniable(&img, b"a", &cfg(Mode::Match)).is_err());
}
