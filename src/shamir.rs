//! Shamir secret sharing over GF(2^8) (AES polynomial 0x11b), applied bytewise.

use crate::error::{Error, Result};
use rand::RngCore;
use crate::gf::{inv, tables};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    pub x: u8,
    pub y: Vec<u8>,
}

pub fn split<R: RngCore>(secret: &[u8], k: u8, n: u8, rng: &mut R) -> Result<Vec<Share>> {
    if secret.is_empty() {
        return Err(Error::Invalid("secret must not be empty".into()));
    }
    if k == 0 || k > n {
        return Err(Error::Invalid("need 1 <= k <= n".into()));
    }
    let t = tables();
    let len = secret.len();
    let mut coeffs: Vec<Vec<u8>> = Vec::with_capacity(k as usize);
    coeffs.push(secret.to_vec());
    for _ in 1..k {
        let mut c = vec![0u8; len];
        rng.fill_bytes(&mut c);
        coeffs.push(c);
    }
    let mut shares = Vec::with_capacity(n as usize);
    for x in 1..=n {
        let row = &t.mul[x as usize];
        let mut y = coeffs[k as usize - 1].clone();
        for j in (0..k as usize - 1).rev() {
            for (yi, ci) in y.iter_mut().zip(&coeffs[j]) {
                *yi = row[*yi as usize] ^ ci;
            }
        }
        shares.push(Share { x, y });
    }
    Ok(shares)
}

/// Interpolates at zero. Fewer than `k` shares silently yield a wrong value;
/// callers must verify the result (see `distribute`).
pub fn combine(shares: &[Share]) -> Result<Vec<u8>> {
    let first = shares.first().ok_or_else(|| Error::Invalid("no shares".into()))?;
    let len = first.y.len();
    for (i, s) in shares.iter().enumerate() {
        if s.x == 0 || s.y.len() != len {
            return Err(Error::Invalid("malformed share".into()));
        }
        if shares[..i].iter().any(|o| o.x == s.x) {
            return Err(Error::Invalid("duplicate share index".into()));
        }
    }
    let t = tables();
    let mut out = vec![0u8; len];
    for (i, si) in shares.iter().enumerate() {
        let mut num = 1u8;
        let mut den = 1u8;
        for (j, sj) in shares.iter().enumerate() {
            if i != j {
                num = t.mul[num as usize][sj.x as usize];
                den = t.mul[den as usize][(si.x ^ sj.x) as usize];
            }
        }
        let row = &t.mul[t.mul[num as usize][inv(den) as usize] as usize];
        for (o, y) in out.iter_mut().zip(&si.y) {
            *o ^= row[*y as usize];
        }
    }
    Ok(out)
}
