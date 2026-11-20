//! GF(2^8) arithmetic with the AES polynomial 0x11b and primitive element 3.
//! All tables are built at compile time, so lookups are plain array reads with no init check.

const fn build_exp_log() -> ([u8; 510], [u8; 256]) {
    let mut exp = [0u8; 510];
    let mut log = [0u8; 256];
    let mut v: u8 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = v;
        log[v as usize] = i as u8;
        let hi = v & 0x80 != 0;
        v = v ^ (v << 1) ^ if hi { 0x1b } else { 0 };
        i += 1;
    }
    while i < 510 {
        exp[i] = exp[i - 255];
        i += 1;
    }
    (exp, log)
}

#[allow(long_running_const_eval)]
const fn build_mul(exp: &[u8; 510], log: &[u8; 256]) -> [[u8; 256]; 256] {
    let mut mul = [[0u8; 256]; 256];
    let mut a = 1;
    while a < 256 {
        let mut b = 1;
        while b < 256 {
            mul[a][b] = exp[log[a] as usize + log[b] as usize];
            b += 1;
        }
        a += 1;
    }
    mul
}

/// For each scalar f: f * x for the 16 low nibbles, then f * (x << 4) for the 16 high nibbles.
/// Feeds byte shuffles that multiply a whole vector by one constant.
const fn build_nibbles(mul: &[[u8; 256]; 256]) -> [[u8; 32]; 256] {
    let mut t = [[0u8; 32]; 256];
    let mut f = 0;
    while f < 256 {
        let mut x = 0;
        while x < 16 {
            t[f][x] = mul[f][x];
            t[f][16 + x] = mul[f][x << 4];
            x += 1;
        }
        f += 1;
    }
    t
}

const EXP_LOG: ([u8; 510], [u8; 256]) = build_exp_log();
pub static EXP: [u8; 510] = EXP_LOG.0;
pub static LOG: [u8; 256] = EXP_LOG.1;
pub static MUL: [[u8; 256]; 256] = build_mul(&EXP_LOG.0, &EXP_LOG.1);
pub static NIBBLES: [[u8; 32]; 256] = build_nibbles(&MUL);

/// Borrowed view of the tables, kept so callers can write `tables().mul[a][b]`.
pub struct Tables {
    pub exp: &'static [u8; 510],
    pub log: &'static [u8; 256],
    pub mul: &'static [[u8; 256]; 256],
}

#[inline]
pub fn tables() -> Tables {
    Tables { exp: &EXP, log: &LOG, mul: &MUL }
}

#[inline]
pub fn mul(a: u8, b: u8) -> u8 {
    MUL[a as usize][b as usize]
}

pub fn inv(a: u8) -> u8 {
    EXP[255 - LOG[a as usize] as usize]
}

pub fn div(a: u8, b: u8) -> u8 {
    mul(a, inv(b))
}

/// alpha^e for any integer exponent, alpha = 3.
#[inline]
pub fn alpha_pow(e: i64) -> u8 {
    EXP[e.rem_euclid(255) as usize]
}
