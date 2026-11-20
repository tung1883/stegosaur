mod common;
use common::random_bytes;
use proptest::prelude::*;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use stegosaur::ecc::{decode, encode, encoded_len, max_payload};

/// Corrupts `per_block` distinct bytes in every codeword block of `enc`.
fn corrupt(enc: &mut [u8], data_len: usize, parity: usize, per_block: usize, rng: &mut ChaCha8Rng) {
    let k = 255 - parity;
    let (mut pos, mut left) = (0usize, data_len);
    while left > 0 {
        let n = left.min(k) + parity;
        let mut idx: Vec<usize> = (0..n).collect();
        idx.shuffle(rng);
        for &i in idx.iter().take(per_block) {
            enc[pos + i] ^= rng.gen_range(1..=255u8);
        }
        pos += n;
        left -= left.min(k);
    }
}

#[test]
fn clean_roundtrip_for_many_lengths_and_parities() {
    for parity in [2usize, 4, 16, 32, 64, 128, 254] {
        for len in [0usize, 1, 2, 10, 100, 253, 254, 255, 256, 500, 1000] {
            let data = random_bytes(len, (parity * 1000 + len) as u64);
            let enc = encode(&data, parity).unwrap();
            assert_eq!(enc.len(), encoded_len(len, parity), "parity {parity} len {len}");
            assert_eq!(decode(&enc, len, parity).unwrap(), data);
        }
    }
}

#[test]
fn encoding_is_systematic() {
    let data = random_bytes(100, 1);
    let enc = encode(&data, 16).unwrap();
    assert_eq!(&enc[..100], &data[..]);
}

#[test]
fn corrects_exactly_up_to_half_the_parity_per_block() {
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    for parity in [2usize, 8, 16, 32, 64] {
        for len in [1usize, 50, 255 - parity, 255 - parity + 1, 700] {
            let data = random_bytes(len, len as u64);
            let enc = encode(&data, parity).unwrap();
            for t in 0..=parity / 2 {
                let mut bad = enc.clone();
                corrupt(&mut bad, len, parity, t, &mut rng);
                assert_eq!(decode(&bad, len, parity).unwrap(), data, "parity {parity} len {len} errors/block {t}");
            }
        }
    }
}

#[test]
fn errors_in_parity_bytes_are_corrected_too() {
    let data = random_bytes(60, 3);
    let mut enc = encode(&data, 16).unwrap();
    for i in 60..60 + 8 {
        enc[i] ^= 0xA5;
    }
    assert_eq!(decode(&enc, 60, 16).unwrap(), data);
}

#[test]
fn beyond_capacity_never_returns_wrong_data_silently_in_practice() {
    // t+1 errors usually fail; if the decoder miscorrects it must be to a different valid codeword,
    // which the AEAD layer above rejects. Here we only require: it never claims the original.
    let mut rng = ChaCha8Rng::seed_from_u64(5);
    let (mut failed, mut miscorrected) = (0, 0);
    for trial in 0..300 {
        let data = random_bytes(100, trial);
        let enc = encode(&data, 16).unwrap();
        let mut bad = enc.clone();
        corrupt(&mut bad, 100, 16, 9, &mut rng);
        match decode(&bad, 100, 16) {
            Err(_) => failed += 1,
            Ok(d) => {
                assert_ne!(d, data, "9 errors cannot be corrected by parity 16");
                miscorrected += 1;
            }
        }
    }
    println!("t+1 errors: {failed} detected, {miscorrected} miscorrected of 300");
    assert!(failed > 250, "most over-capacity blocks should be detected, got {failed}");
}

#[test]
fn random_data_is_almost_never_a_valid_codeword() {
    let mut rng = ChaCha8Rng::seed_from_u64(8);
    let mut accepted = 0;
    for _ in 0..2000 {
        let junk: Vec<u8> = (0..116).map(|_| rng.r#gen()).collect();
        if decode(&junk, 100, 16).is_ok() {
            accepted += 1;
        }
    }
    println!("random 116-byte blocks accepted by RS(116,100): {accepted}/2000");
    assert!(accepted < 5);
}

#[test]
fn invalid_parity_and_short_input_are_rejected() {
    assert!(encode(b"x", 0).is_err());
    assert!(encode(b"x", 1).is_err());
    assert!(encode(b"x", 255).is_err());
    assert!(decode(&[0u8; 5], 100, 16).is_err());
}

#[test]
fn max_payload_is_the_exact_inverse_of_encoded_len() {
    for parity in [2usize, 16, 100, 254] {
        for budget in 0..1200usize {
            let m = max_payload(budget, parity);
            assert!(encoded_len(m, parity) <= budget, "parity {parity} budget {budget}");
            assert!(encoded_len(m + 1, parity) > budget, "not maximal: parity {parity} budget {budget}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(80))]
    #[test]
    fn prop_recovers_from_random_correctable_damage(
        data in proptest::collection::vec(any::<u8>(), 0..700),
        parity in 2usize..=60,
        seed in any::<u64>(),
    ) {
        let enc = encode(&data, parity).unwrap();
        let mut bad = enc.clone();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        corrupt(&mut bad, data.len(), parity, parity / 2, &mut rng);
        prop_assert_eq!(decode(&bad, data.len(), parity).unwrap(), data);
    }
}

#[test]
fn simd_and_scalar_encoders_agree_everywhere() {
    use stegosaur::ecc::encode_scalar;
    for parity in [2usize, 3, 8, 15, 16, 17, 31, 32, 33, 64, 100, 128, 200, 254] {
        for len in [0usize, 1, 5, 100, 255 - parity, 255 - parity + 1, 600, 2000] {
            let data = random_bytes(len, (parity * 7919 + len) as u64);
            assert_eq!(
                encode(&data, parity).unwrap(),
                encode_scalar(&data, parity).unwrap(),
                "parity {parity} len {len}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(120))]
    #[test]
    fn prop_simd_equals_scalar(data in proptest::collection::vec(any::<u8>(), 0..800), parity in 2usize..=254) {
        prop_assert_eq!(
            encode(&data, parity).unwrap(),
            stegosaur::ecc::encode_scalar(&data, parity).unwrap()
        );
    }
}
