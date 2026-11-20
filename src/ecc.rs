//! Reed-Solomon error correction over GF(2^8).
//!
//! Data is split into blocks of at most `255 - parity` bytes, each followed by `parity`
//! check bytes; the last block is shortened. Each block corrects up to `parity / 2`
//! corrupted bytes at unknown positions (errors, not erasures). Roots are alpha^0..alpha^(parity-1)
//! with alpha = 3, the primitive element of the AES field.
//!
//! Speed: encoding multiplies the whole parity register by one constant per input byte, done with
//! SSSE3 byte shuffles on x86-64 (scalar table lookups elsewhere). A block is checked for damage by
//! re-encoding its data and comparing parity, so undamaged data never pays for syndrome computation.

use crate::error::{Error, Result};
use crate::gf::{alpha_pow, div, mul, MUL};
use std::sync::OnceLock;
#[cfg(target_arch = "x86_64")]
use crate::gf::NIBBLES;

fn check(parity: usize) -> Result<()> {
    if (2..=254).contains(&parity) {
        Ok(())
    } else {
        Err(Error::Invalid("parity must be between 2 and 254 bytes".into()))
    }
}

/// Bytes on the wire for `n` data bytes.
pub fn encoded_len(n: usize, parity: usize) -> usize {
    if n == 0 {
        0
    } else {
        n + parity * n.div_ceil(255 - parity)
    }
}

/// Largest data length whose encoding fits in `budget` bytes.
pub fn max_payload(budget: usize, parity: usize) -> usize {
    let (full, rem) = (budget / 255, budget % 255);
    full * (255 - parity) + rem.saturating_sub(parity)
}

/// Coefficients of the monic generator polynomial below its leading term (length `nsym`), cached per parity.
fn generator_tail(nsym: usize) -> &'static [u8] {
    static CACHE: [OnceLock<Vec<u8>>; 255] = [const { OnceLock::new() }; 255];
    CACHE[nsym].get_or_init(|| compute_generator_tail(nsym))
}

fn compute_generator_tail(nsym: usize) -> Vec<u8> {
    let mut g = vec![1u8];
    for i in 0..nsym {
        let root = alpha_pow(i as i64);
        let mut next = vec![0u8; g.len() + 1];
        for (j, &c) in g.iter().enumerate() {
            next[j] ^= c;
            next[j + 1] ^= mul(c, root);
        }
        g = next;
    }
    g.split_off(1)
}

fn parity_scalar(data: &[u8], nsym: usize, tail: &[u8]) -> Vec<u8> {
    let rows: Vec<&[u8; 256]> = tail.iter().map(|&g| &MUL[g as usize]).collect();
    let mut rem = vec![0u8; nsym];
    for &d in data {
        let f = (d ^ rem[0]) as usize;
        rem.copy_within(1.., 0);
        rem[nsym - 1] = 0;
        for j in 0..nsym {
            rem[j] ^= rows[j][f];
        }
    }
    rem
}

/// Parity register held in `C` SSE registers (16 bytes each); the per-byte shift uses `palignr`,
/// so nothing round-trips through memory inside the loop.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn parity_ssse3_n<const C: usize>(data: &[u8], nsym: usize, tail: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::*;
    // SAFETY: the caller verified SSSE3. Loads and stores are bounded by the fixed 256-byte local
    // buffers (C <= 16 chunks of 16 bytes) and the 32-byte NIBBLES rows.
    unsafe {
        let mut g = [0u8; 256];
        g[..nsym].copy_from_slice(tail);
        let mask = _mm_set1_epi8(0x0f);
        let zero = _mm_setzero_si128();
        let mut g_lo = [zero; C];
        let mut g_hi = [zero; C];
        for c in 0..C {
            let v = _mm_loadu_si128(g.as_ptr().add(16 * c) as *const __m128i);
            g_lo[c] = _mm_and_si128(v, mask);
            g_hi[c] = _mm_and_si128(_mm_srli_epi16(v, 4), mask);
        }

        let mut r = [zero; C];
        for &d in data {
            let f = ((_mm_cvtsi128_si32(r[0]) as u8) ^ d) as usize;
            let lo = _mm_loadu_si128(NIBBLES[f].as_ptr() as *const __m128i);
            let hi = _mm_loadu_si128(NIBBLES[f].as_ptr().add(16) as *const __m128i);
            let mut next = [zero; C];
            for c in 0..C {
                let upper = if c + 1 < C { r[c + 1] } else { zero };
                let shifted = _mm_alignr_epi8::<1>(upper, r[c]);
                let prod = _mm_xor_si128(_mm_shuffle_epi8(lo, g_lo[c]), _mm_shuffle_epi8(hi, g_hi[c]));
                next[c] = _mm_xor_si128(shifted, prod);
            }
            r = next;
        }

        let mut out = [0u8; 256];
        for c in 0..C {
            _mm_storeu_si128(out.as_mut_ptr().add(16 * c) as *mut __m128i, r[c]);
        }
        out[..nsym].to_vec()
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn parity_ssse3(data: &[u8], nsym: usize, tail: &[u8]) -> Vec<u8> {
    macro_rules! dispatch {
        ($($n:literal),*) => {
            match nsym.div_ceil(16) {
                // SAFETY: forwarded from this function's own contract (SSSE3 present).
                $($n => unsafe { parity_ssse3_n::<$n>(data, nsym, tail) },)*
                _ => unreachable!("parity is validated to 2..=254"),
            }
        };
    }
    dispatch!(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)
}

fn parity_bytes(data: &[u8], nsym: usize, tail: &[u8]) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("ssse3") {
        // SAFETY: the CPU feature was just verified.
        return unsafe { parity_ssse3(data, nsym, tail) };
    }
    parity_scalar(data, nsym, tail)
}

pub fn encode(data: &[u8], parity: usize) -> Result<Vec<u8>> {
    check(parity)?;
    let tail = generator_tail(parity);
    let mut out = Vec::with_capacity(encoded_len(data.len(), parity));
    for block in data.chunks(255 - parity) {
        out.extend_from_slice(block);
        out.extend_from_slice(&parity_bytes(block, parity, &tail));
    }
    Ok(out)
}

/// Same output as [`encode`] using only table lookups; exists so tests can cross-check the SIMD path.
#[doc(hidden)]
pub fn encode_scalar(data: &[u8], parity: usize) -> Result<Vec<u8>> {
    check(parity)?;
    let tail = generator_tail(parity);
    let mut out = Vec::with_capacity(encoded_len(data.len(), parity));
    for block in data.chunks(255 - parity) {
        out.extend_from_slice(block);
        out.extend_from_slice(&parity_scalar(block, parity, &tail));
    }
    Ok(out)
}

fn syndromes(cw: &[u8], nsym: usize) -> Vec<u8> {
    let rows: Vec<&[u8; 256]> = (0..nsym).map(|j| &MUL[alpha_pow(j as i64) as usize]).collect();
    let mut s = vec![0u8; nsym];
    for &c in cw {
        for j in 0..nsym {
            s[j] = rows[j][s[j] as usize] ^ c;
        }
    }
    s
}

fn eval_asc(poly: &[u8], x: u8) -> u8 {
    poly.iter().rev().fold(0u8, |acc, &c| mul(acc, x) ^ c)
}

fn correct_block(cw: &mut [u8], nsym: usize, tail: &[u8]) -> Result<()> {
    let n = cw.len();
    let k = n - nsym;
    // c(x) mod g(x) equals (recomputed parity) xor (received parity), a polynomial of nsym terms, and
    // c(alpha^j) = (c mod g)(alpha^j) because g vanishes there. So the syndromes cost nsym^2 lookups
    // on that difference instead of nsym * n on the whole block.
    let diff: Vec<u8> = parity_bytes(&cw[..k], nsym, tail).iter().zip(&cw[k..]).map(|(a, b)| a ^ b).collect();
    if diff.iter().all(|&d| d == 0) {
        return Ok(());
    }
    let synd = syndromes(&diff, nsym);

    // Berlekamp-Massey: error locator polynomial, ascending coefficients.
    let mut c = vec![1u8];
    let mut b = vec![1u8];
    let (mut l, mut m, mut bb) = (0usize, 1usize, 1u8);
    for i in 0..nsym {
        let mut d = synd[i];
        for j in 1..=l {
            if j < c.len() {
                d ^= mul(c[j], synd[i - j]);
            }
        }
        if d == 0 {
            m += 1;
            continue;
        }
        let coef = div(d, bb);
        let prev = c.clone();
        if c.len() < b.len() + m {
            c.resize(b.len() + m, 0);
        }
        for (k, &bk) in b.iter().enumerate() {
            c[k + m] ^= mul(coef, bk);
        }
        if 2 * l <= i {
            l = i + 1 - l;
            b = prev;
            bb = d;
            m = 1;
        } else {
            m += 1;
        }
    }
    if l == 0 || 2 * l > nsym {
        return Err(Error::NoPayload);
    }
    c.resize(l + 1, 0);

    // Chien search, incremental: position `pos` has locator X = alpha^(n-1-pos) and is an error iff
    // Lambda(X^-1) = 0. Moving one position right multiplies X^-1 by alpha, so term i gains a factor alpha^i.
    let mut terms: Vec<u8> = (0..=l)
        .map(|i| mul(c[i], alpha_pow(-((n - 1) as i64) * i as i64)))
        .collect();
    let step: Vec<&[u8; 256]> = (0..=l).map(|i| &MUL[alpha_pow(i as i64) as usize]).collect();
    let mut errors = Vec::with_capacity(l);
    for pos in 0..n {
        if terms.iter().fold(0u8, |a, &t| a ^ t) == 0 {
            errors.push(pos);
            if errors.len() > l {
                break;
            }
        }
        for i in 1..=l {
            terms[i] = step[i][terms[i] as usize];
        }
    }
    if errors.len() != l {
        return Err(Error::NoPayload);
    }

    // Omega = S * Lambda mod x^nsym
    let omega: Vec<u8> = (0..nsym)
        .map(|k| (0..=k.min(c.len() - 1)).fold(0u8, |acc, i| acc ^ mul(c[i], synd[k - i])))
        .collect();

    for &pos in &errors {
        let p = (n - 1 - pos) as i64;
        let (x, xinv) = (alpha_pow(p), alpha_pow(-p));
        let num = eval_asc(&omega, xinv);
        let den = (1..c.len())
            .step_by(2)
            .fold(0u8, |acc, i| acc ^ mul(c[i], alpha_pow(-(p * (i as i64 - 1)))));
        if den == 0 {
            return Err(Error::NoPayload);
        }
        cw[pos] ^= mul(x, div(num, den));
    }

    if parity_bytes(&cw[..k], nsym, tail) == cw[k..] {
        Ok(())
    } else {
        Err(Error::NoPayload)
    }
}

/// The data bytes of `encoded` with parity dropped and no correction applied (the code is systematic).
pub fn strip(encoded: &[u8], data_len: usize, parity: usize) -> Result<Vec<u8>> {
    check(parity)?;
    if encoded.len() < encoded_len(data_len, parity) {
        return Err(Error::NoPayload);
    }
    let mut out = Vec::with_capacity(data_len);
    let (mut pos, mut left) = (0usize, data_len);
    while left > 0 {
        let k = left.min(255 - parity);
        out.extend_from_slice(&encoded[pos..pos + k]);
        pos += k + parity;
        left -= k;
    }
    Ok(out)
}

/// Decodes `data_len` bytes from `encoded`, repairing up to `parity / 2` bad bytes per block.
pub fn decode(encoded: &[u8], data_len: usize, parity: usize) -> Result<Vec<u8>> {
    check(parity)?;
    if encoded.len() < encoded_len(data_len, parity) {
        return Err(Error::NoPayload);
    }
    let tail = generator_tail(parity);
    let mut out = Vec::with_capacity(data_len);
    let (mut pos, mut left) = (0usize, data_len);
    while left > 0 {
        let k = left.min(255 - parity);
        let mut block = encoded[pos..pos + k + parity].to_vec();
        correct_block(&mut block, parity, &tail)?;
        out.extend_from_slice(&block[..k]);
        pos += k + parity;
        left -= k;
    }
    Ok(out)
}
