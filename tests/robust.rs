mod common;
use common::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use stegosaur::container::{
    deniable_capacity, embed_deniable, embed_single, extract_deniable, extract_single, single_capacity,
};
use stegosaur::distribute::{distribute, recover};
use stegosaur::{Config, Error, ImageCarrier, Mode};

fn ecc_cfg(parity: usize) -> Config {
    Config { parity, ..cfg(Mode::Match) }
}

/// Flips the LSB of each sample independently with probability `p` (noise, partial overwrite, lossy re-save).
fn noise_lsbs(img: &mut ImageCarrier, p: f64, seed: u64) -> usize {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut flipped = 0;
    for s in img.samples.iter_mut() {
        if rng.gen_bool(p) {
            *s ^= 1;
            flipped += 1;
        }
    }
    flipped
}

#[test]
fn single_survives_noise_that_breaks_the_unprotected_payload() {
    let data = random_bytes(1000, 1);
    let mut ok_with = 0;
    let mut ok_without = 0;
    let trials = 12;
    for seed in 0..trials {
        let mut protected = natural_image(128, 128, seed);
        let mut bare = protected.clone();
        embed_single(&mut protected, b"pw", &data, &ecc_cfg(32)).unwrap();
        embed_single(&mut bare, b"pw", &data, &ecc_cfg(0)).unwrap();
        noise_lsbs(&mut protected, 0.003, 100 + seed);
        noise_lsbs(&mut bare, 0.003, 100 + seed);
        if extract_single(&protected, b"pw", &ecc_cfg(32)).ok().as_deref() == Some(&data[..]) {
            ok_with += 1;
        }
        if extract_single(&bare, b"pw", &ecc_cfg(0)).ok().as_deref() == Some(&data[..]) {
            ok_without += 1;
        }
    }
    println!("0.3% LSB noise: with RS(parity 32) {ok_with}/{trials} recovered, without {ok_without}/{trials}");
    assert_eq!(ok_with, trials, "parity 32 should repair 0.3% sample noise");
    assert_eq!(ok_without, 0, "without ECC the AEAD tag must reject corrupted data");
}

#[test]
fn single_fails_cleanly_when_damage_exceeds_correction() {
    let data = random_bytes(1000, 2);
    let mut img = natural_image(128, 128, 3);
    embed_single(&mut img, b"pw", &data, &ecc_cfg(16)).unwrap();
    noise_lsbs(&mut img, 0.08, 5);
    assert!(matches!(extract_single(&img, b"pw", &ecc_cfg(16)), Err(Error::NoPayload)));
}

#[test]
fn correction_limit_scales_with_parity() {
    println!("parity  max sample-noise rate recovered (of 6 trials each)");
    let data = random_bytes(600, 4);
    let mut last = 0.0;
    for parity in [8usize, 32, 64] {
        let mut best = 0.0;
        for &p in &[0.001, 0.003, 0.006, 0.012, 0.024] {
            let ok = (0..6)
                .filter(|&seed| {
                    let mut img = natural_image(128, 128, 20 + seed);
                    embed_single(&mut img, b"pw", &data, &ecc_cfg(parity)).unwrap();
                    noise_lsbs(&mut img, p, 300 + seed);
                    extract_single(&img, b"pw", &ecc_cfg(parity)).ok().as_deref() == Some(&data[..])
                })
                .count();
            if ok == 6 {
                best = p;
            }
        }
        println!("{parity:<7} {best}");
        assert!(best >= last, "more parity must not tolerate less noise");
        last = best;
    }
    assert!(last >= 0.006, "parity 64 should survive at least 0.6% sample noise");
}

#[test]
fn deniable_slots_are_both_protected() {
    let c = ecc_cfg(32);
    let mut img = natural_image(192, 192, 7);
    let (decoy, hidden) = (random_bytes(300, 1), random_bytes(300, 2));
    embed_deniable(&mut img, (b"a", &decoy), Some((b"b", &hidden)), &c).unwrap();
    noise_lsbs(&mut img, 0.003, 9);
    assert_eq!(extract_deniable(&img, b"a", &c).unwrap(), decoy);
    assert_eq!(extract_deniable(&img, b"b", &c).unwrap(), hidden);
    assert!(extract_deniable(&img, b"wrong", &c).is_err());
}

#[test]
fn capacity_accounting_is_exact_for_every_parity() {
    for parity in [0usize, 2, 16, 32, 100, 200] {
        let c = ecc_cfg(parity);
        // single
        let mut img = natural_image(64, 64, 1);
        let cap = single_capacity(&img, &c);
        assert!(cap > 0, "parity {parity}");
        let data = random_bytes(cap, parity as u64);
        embed_single(&mut img, b"pw", &data, &c).unwrap();
        assert_eq!(extract_single(&img, b"pw", &c).unwrap(), data, "single parity {parity}");
        let mut img = natural_image(64, 64, 1);
        assert!(matches!(
            embed_single(&mut img, b"pw", &random_bytes(cap + 1, 1), &c),
            Err(Error::Capacity { .. })
        ), "single parity {parity} accepted cap+1");

        // deniable
        let mut big = natural_image(160, 160, 2);
        let dcap = deniable_capacity(&big, &c);
        if dcap == 0 {
            continue;
        }
        let (d, h) = (random_bytes(dcap, 1), random_bytes(dcap, 2));
        embed_deniable(&mut big, (b"a", &d), Some((b"b", &h)), &c).unwrap();
        assert_eq!(extract_deniable(&big, b"a", &c).unwrap(), d, "deniable parity {parity}");
        assert_eq!(extract_deniable(&big, b"b", &c).unwrap(), h);
        let mut big = natural_image(160, 160, 2);
        assert!(matches!(
            embed_deniable(&mut big, (b"a", &random_bytes(dcap + 1, 1)), None, &c),
            Err(Error::Capacity { .. })
        ), "deniable parity {parity} accepted cap+1");
    }
}

#[test]
fn mismatched_parity_does_not_decode() {
    let mut img = natural_image(96, 96, 5);
    embed_single(&mut img, b"pw", b"hello", &ecc_cfg(16)).unwrap();
    assert!(extract_single(&img, b"pw", &ecc_cfg(32)).is_err());
    assert!(extract_single(&img, b"pw", &ecc_cfg(0)).is_err());
}

#[test]
fn invalid_parity_is_rejected() {
    let mut img = natural_image(64, 64, 1);
    assert!(embed_single(&mut img, b"pw", b"x", &ecc_cfg(1)).is_err());
    assert!(embed_single(&mut img, b"pw", b"x", &ecc_cfg(255)).is_err());
}

#[test]
fn threshold_recovery_tolerates_damaged_carriers() {
    let c = ecc_cfg(32);
    let secret = random_bytes(200, 3);
    let mut imgs: Vec<ImageCarrier> = (0..5).map(|i| natural_image(96, 96, 40 + i)).collect();
    distribute(&secret, b"pw", 3, &mut imgs, &c).unwrap();
    for (i, img) in imgs.iter_mut().enumerate() {
        noise_lsbs(img, 0.003, 70 + i as u64);
    }
    assert_eq!(recover(b"pw", &imgs[..3], &c).unwrap(), secret);
}

#[test]
fn clean_carrier_still_has_no_payload_with_ecc() {
    let img = natural_image(96, 96, 1);
    assert!(matches!(extract_single(&img, b"pw", &ecc_cfg(16)), Err(Error::NoPayload)));
    assert!(matches!(extract_deniable(&img, b"pw", &ecc_cfg(16)), Err(Error::NoPayload)));
}
