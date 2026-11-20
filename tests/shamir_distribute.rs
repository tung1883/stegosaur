mod common;
use common::*;
use proptest::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use stegosaur::distribute::{distribute, recover};
use stegosaur::shamir::{combine, split, Share};
use stegosaur::{Error, ImageCarrier, Mode};

fn combos(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(i + 1, n, k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}

#[test]
fn every_k_subset_recovers() {
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let secret = random_bytes(64, 2);
    for (k, n) in [(1u8, 1u8), (1, 4), (2, 3), (3, 5), (4, 7), (5, 5), (6, 8)] {
        let shares = split(&secret, k, n, &mut rng).unwrap();
        for subset in combos(n as usize, k as usize) {
            let picked: Vec<Share> = subset.iter().map(|&i| shares[i].clone()).collect();
            assert_eq!(combine(&picked).unwrap(), secret, "k={k} n={n} subset={subset:?}");
        }
    }
}

#[test]
fn more_than_k_shares_also_work() {
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let secret = random_bytes(32, 2);
    let shares = split(&secret, 3, 6, &mut rng).unwrap();
    assert_eq!(combine(&shares).unwrap(), secret);
    assert_eq!(combine(&shares[1..5]).unwrap(), secret);
}

#[test]
fn fewer_than_k_shares_do_not_recover() {
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    let secret = random_bytes(48, 4);
    let shares = split(&secret, 4, 6, &mut rng).unwrap();
    for subset in combos(6, 3) {
        let picked: Vec<Share> = subset.iter().map(|&i| shares[i].clone()).collect();
        assert_ne!(combine(&picked).unwrap(), secret);
    }
}

#[test]
fn k_minus_one_shares_are_uniform_regardless_of_secret() {
    // Perfect secrecy check: for k=2, a single share's byte is uniform for any secret byte.
    let mut rng = ChaCha8Rng::seed_from_u64(9);
    for secret_byte in [0u8, 1, 255] {
        let mut counts = [0u32; 256];
        let trials = 51200;
        for _ in 0..trials {
            let shares = split(&[secret_byte], 2, 3, &mut rng).unwrap();
            counts[shares[0].y[0] as usize] += 1;
        }
        let expected = trials as f64 / 256.0;
        let chi: f64 = counts.iter().map(|&c| (c as f64 - expected).powi(2) / expected).sum();
        // chi-square with 255 dof: mean 255, sd ~22.6; 6 sigma bound
        assert!(chi < 255.0 + 6.0 * 22.6, "secret {secret_byte}: chi={chi}");
    }
}

#[test]
fn invalid_parameters_and_inputs_are_rejected() {
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    assert!(split(b"x", 0, 3, &mut rng).is_err());
    assert!(split(b"x", 4, 3, &mut rng).is_err());
    assert!(split(b"", 2, 3, &mut rng).is_err());
    let s = split(b"abc", 2, 3, &mut rng).unwrap();
    assert!(combine(&[s[0].clone(), s[0].clone()]).is_err(), "duplicate x");
    assert!(combine(&[]).is_err());
    let mut bad = s[1].clone();
    bad.y.pop();
    assert!(combine(&[s[0].clone(), bad]).is_err(), "length mismatch");
}

#[test]
fn max_share_count_255() {
    let mut rng = ChaCha8Rng::seed_from_u64(5);
    let secret = random_bytes(16, 6);
    let shares = split(&secret, 255, 255, &mut rng).unwrap();
    assert_eq!(combine(&shares).unwrap(), secret);
    let shares = split(&secret, 128, 255, &mut rng).unwrap();
    assert_eq!(combine(&shares[100..228]).unwrap(), secret);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn prop_split_combine(secret in proptest::collection::vec(any::<u8>(), 1..200), n in 1u8..=20, kfrac in 0.0f64..1.0, seed in any::<u64>()) {
        let k = 1 + ((n - 1) as f64 * kfrac) as u8;
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let shares = split(&secret, k, n, &mut rng).unwrap();
        let mut idx: Vec<usize> = (0..n as usize).collect();
        use rand::seq::SliceRandom;
        idx.shuffle(&mut rng);
        let picked: Vec<Share> = idx[..k as usize].iter().map(|&i| shares[i].clone()).collect();
        prop_assert_eq!(combine(&picked).unwrap(), secret);
    }
}

fn carriers(n: usize, size: u32) -> Vec<ImageCarrier> {
    (0..n).map(|i| natural_image(size, size, 100 + i as u64)).collect()
}

#[test]
fn distribute_and_recover_from_any_three_of_five() {
    let c = cfg(Mode::Match);
    let secret = random_bytes(300, 5);
    let mut imgs = carriers(5, 64);
    distribute(&secret, b"pw", 3, &mut imgs, &c).unwrap();
    for subset in combos(5, 3) {
        let picked: Vec<ImageCarrier> = subset.iter().map(|&i| imgs[i].clone()).collect();
        assert_eq!(recover(b"pw", &picked, &c).unwrap(), secret, "subset {subset:?}");
    }
}

#[test]
fn two_of_five_is_not_enough() {
    let c = cfg(Mode::Match);
    let mut imgs = carriers(5, 64);
    distribute(&random_bytes(100, 1), b"pw", 3, &mut imgs, &c).unwrap();
    for subset in combos(5, 2) {
        let picked: Vec<ImageCarrier> = subset.iter().map(|&i| imgs[i].clone()).collect();
        assert!(matches!(recover(b"pw", &picked, &c), Err(Error::NoPayload)));
    }
}

#[test]
fn clean_and_foreign_carriers_are_ignored() {
    let c = cfg(Mode::Match);
    let secret = random_bytes(80, 2);
    let mut imgs = carriers(4, 64);
    distribute(&secret, b"pw", 3, &mut imgs, &c).unwrap();
    let mut pool = vec![imgs[0].clone(), natural_image(64, 64, 999), imgs[2].clone(), natural_image(64, 64, 998), imgs[3].clone()];
    assert_eq!(recover(b"pw", &pool, &c).unwrap(), secret);
    pool.remove(4);
    assert!(recover(b"pw", &pool, &c).is_err());
}

#[test]
fn wrong_passphrase_recovers_nothing() {
    let c = cfg(Mode::Match);
    let mut imgs = carriers(3, 64);
    distribute(b"top secret", b"pw", 2, &mut imgs, &c).unwrap();
    assert!(recover(b"nope", &imgs, &c).is_err());
}

#[test]
fn shares_from_two_secrets_do_not_mix() {
    let c = cfg(Mode::Match);
    let (sa, sb) = (random_bytes(50, 1), random_bytes(50, 2));
    let mut a = carriers(3, 64);
    let mut b: Vec<ImageCarrier> = (0..3).map(|i| natural_image(64, 64, 200 + i)).collect();
    distribute(&sa, b"pw", 2, &mut a, &c).unwrap();
    distribute(&sb, b"pw", 2, &mut b, &c).unwrap();
    // one share from each secret is not a quorum for either
    let mixed = vec![a[0].clone(), b[1].clone()];
    assert!(recover(b"pw", &mixed, &c).is_err());
    // a full quorum of A alongside a single foreign B share still resolves to A
    let pool = vec![b[0].clone(), a[0].clone(), a[1].clone()];
    assert_eq!(recover(b"pw", &pool, &c).unwrap(), sa);
    // with a quorum of each, one valid secret is returned (which one is unspecified)
    let both = vec![b[0].clone(), a[0].clone(), a[1].clone(), b[2].clone()];
    let got = recover(b"pw", &both, &c).unwrap();
    assert!(got == sa || got == sb);
}

#[test]
fn k_equals_one_and_k_equals_n() {
    let c = cfg(Mode::Match);
    let secret = random_bytes(40, 8);
    let mut imgs = carriers(3, 64);
    distribute(&secret, b"pw", 1, &mut imgs, &c).unwrap();
    for i in 0..3 {
        assert_eq!(recover(b"pw", &imgs[i..i + 1], &c).unwrap(), secret);
    }
    let mut imgs = carriers(3, 64);
    distribute(&secret, b"pw", 3, &mut imgs, &c).unwrap();
    assert_eq!(recover(b"pw", &imgs, &c).unwrap(), secret);
    assert!(recover(b"pw", &imgs[..2], &c).is_err());
}

#[test]
fn undersized_carrier_is_reported() {
    let c = cfg(Mode::Match);
    let mut imgs = vec![natural_image(64, 64, 1), natural_image(8, 8, 2)];
    assert!(matches!(distribute(&random_bytes(200, 1), b"pw", 2, &mut imgs, &c), Err(Error::Capacity { .. })));
}
