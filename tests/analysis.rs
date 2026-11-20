mod common;
use common::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use stegosaur::analysis::{analyze, chi_square, gamma_q, Risk};
use stegosaur::container::embed_single;
use stegosaur::{ImageCarrier, Mode};

/// Classic reference embedding: random message bits written by LSB replacement into the
/// first `rate * N` samples. A detector reporting "rate" is correctly calibrated.
fn sequential_replace(img: &ImageCarrier, rate: f64, seed: u64) -> ImageCarrier {
    let mut out = img.clone();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let n = (out.samples.len() as f64 * rate) as usize;
    for s in out.samples.iter_mut().take(n) {
        *s = (*s & !1) | rng.gen_range(0..2u8);
    }
    out
}

/// Random message bits written by +-1 matching into the first `rate * N` samples.
fn sequential_match(img: &ImageCarrier, rate: f64, seed: u64) -> ImageCarrier {
    let mut out = img.clone();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let n = (out.samples.len() as f64 * rate) as usize;
    for s in out.samples.iter_mut().take(n) {
        let bit = rng.gen_range(0..2u8);
        if *s & 1 != bit {
            *s = match *s {
                0 => 1,
                255 => 254,
                v if rng.gen_bool(0.5) => v + 1,
                v => v - 1,
            };
        }
    }
    out
}

#[test]
fn incomplete_gamma_matches_known_values() {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(close(gamma_q(1.0, 1.0), (-1.0f64).exp()));
    assert!(close(gamma_q(0.5, 0.5), 0.3173105078629141)); // erfc(sqrt(0.5))
    // chi-square upper-tail critical values at p = 0.05: df=1 -> 3.841459, df=5 -> 11.070498, df=10 -> 18.307038
    assert!(close(gamma_q(0.5, 3.841458820694124 / 2.0), 0.05));
    assert!(close(gamma_q(2.5, 11.070497693516351 / 2.0), 0.05));
    assert!(close(gamma_q(5.0, 18.307038053275146 / 2.0), 0.05));
    assert!(close(gamma_q(3.0, 0.0), 1.0));
    assert!(gamma_q(100.0, 1e4) < 1e-12);
}

#[test]
fn clean_images_score_low() {
    for seed in 0..6 {
        let img = natural_image(256, 256, seed);
        let r = analyze(&img);
        println!("clean seed={seed}: rs={:.3} spa={:.3} chi_p={:.3}", r.rs, r.spa, r.chi_p);
        assert!(r.rs.abs() < 0.08, "rs false positive {}", r.rs);
        assert!(r.spa.abs() < 0.08, "spa false positive {}", r.spa);
        assert_eq!(r.risk, Risk::Low);
    }
}

#[test]
fn replacement_rate_is_estimated() {
    println!("rate    rs      spa     score  risk");
    for &rate in &[0.1, 0.25, 0.5, 0.75, 1.0] {
        let mut rs_err = 0.0;
        let mut spa_err = 0.0;
        for seed in 0..4 {
            let img = natural_image(256, 256, 10 + seed);
            let r = analyze(&sequential_replace(&img, rate, seed));
            if rate >= 0.99 {
                let planes = sequential_replace(&img, rate, seed).planes();
                let per: Vec<String> = planes.iter().map(|p| format!("{:.2}", stegosaur::analysis::rs_estimate(p, 256))).collect();
                println!("  full-rate seed {seed}: rs per plane {per:?}");
            }
            rs_err += (r.rs - rate).abs() / 4.0;
            spa_err += (r.spa - rate).abs() / 4.0;
            if seed == 0 {
                println!("{rate:<7} {:<7.3} {:<7.3} {:<6.1} {:?}", r.rs, r.spa, r.score, r.risk);
            }
        }
        assert!(rs_err < 0.12, "rs mean abs error {rs_err} at rate {rate}");
        assert!(spa_err < 0.12, "spa mean abs error {spa_err} at rate {rate}");
    }
}

#[test]
fn high_rate_replacement_is_flagged_high_risk() {
    let img = natural_image(256, 256, 3);
    assert_eq!(analyze(&sequential_replace(&img, 1.0, 1)).risk, Risk::High);
    assert_eq!(analyze(&sequential_replace(&img, 0.5, 1)).risk, Risk::High);
}

#[test]
fn chi_square_flags_full_replacement_on_bumpy_histograms() {
    // Chi-square needs a histogram with real pair imbalance, as in decompressed or contrast-stretched photos.
    let mut img = natural_image(256, 256, 4);
    for s in img.samples.iter_mut() {
        let v = (*s as f64 - 128.0) * 0.8 + 128.0; // stretch-and-quantise-like unevenness
        *s = (v / 0.8f64.max(1e-9)).round().clamp(0.0, 255.0) as u8;
    }
    let before = chi_square(&img.samples);
    let after = chi_square(&sequential_replace(&img, 1.0, 5).samples);
    println!("chi p before={before:.4} after={after:.4}");
    assert!(after > 0.9, "full replacement equalises pairs: p={after}");
    assert!(after > before);
}

#[test]
fn keyed_embedding_modes_compare() {
    // The headline result: same payload, ~1 bit per 8 samples (12.5% of samples carry data).
    let clean = natural_image(256, 256, 21);
    let payload = random_bytes(256 * 256 * 3 / 8 - 40, 9);
    let mut rep = clean.clone();
    let mut mat = clean.clone();
    embed_single(&mut rep, b"pw", &payload, &cfg(Mode::Replace)).unwrap();
    embed_single(&mut mat, b"pw", &payload, &cfg(Mode::Match)).unwrap();
    let (rr, rm) = (analyze(&rep), analyze(&mat));
    println!("near-full capacity: replace rs={:.3} spa={:.3} score={:.1} | match rs={:.3} spa={:.3} score={:.1}",
        rr.rs, rr.spa, rr.score, rm.rs, rm.spa, rm.score);
    assert_eq!(rr.risk, Risk::High);
    assert!(rm.score < rr.score, "matching must be less detectable than replacement");
}

#[test]
fn matching_is_less_detectable_than_replacement_at_every_rate() {
    println!("rate   replace(rs/spa)   match(rs/spa)");
    for &rate in &[0.25, 0.5, 1.0] {
        let (mut sr, mut sm) = (0.0, 0.0);
        for seed in 0..4 {
            let img = natural_image(256, 256, 40 + seed);
            let r = analyze(&sequential_replace(&img, rate, seed));
            let m = analyze(&sequential_match(&img, rate, seed));
            sr += r.rs.max(r.spa) / 4.0;
            sm += m.rs.max(m.spa) / 4.0;
        }
        println!("{rate:<6} {sr:<17.3} {sm:.3}");
        assert!(sm < sr, "rate {rate}: match {sm} !< replace {sr}");
    }
}

#[test]
fn small_payload_stays_below_detection_floor() {
    let clean = natural_image(256, 256, 30);
    let mut img = clean.clone();
    embed_single(&mut img, b"pw", &random_bytes(200, 1), &cfg(Mode::Replace)).unwrap();
    let r = analyze(&img);
    println!("200 B in 24 KiB carrier, replace: rs={:.4} spa={:.4}", r.rs, r.spa);
    assert_eq!(r.risk, Risk::Low);
}

#[test]
fn analysis_handles_degenerate_images() {
    let flat = ImageCarrier::from_rgb(32, 32, vec![128; 32 * 32 * 3]).unwrap();
    let r = analyze(&flat);
    assert!(r.rs.is_finite() && r.spa.is_finite() && r.chi_p.is_finite());
    let tiny = ImageCarrier::from_rgb(2, 2, vec![0; 12]).unwrap();
    let r = analyze(&tiny);
    assert!(r.rs.is_finite() && r.spa.is_finite());
    let noise = ImageCarrier::from_rgb(64, 64, random_bytes(64 * 64 * 3, 1)).unwrap();
    let r = analyze(&noise);
    assert!(r.rs.is_finite() && r.spa.is_finite());
}
