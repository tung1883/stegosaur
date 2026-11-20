use crate::error::{Error, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct KdfParams {
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self { log_n: 15, r: 8, p: 1 }
    }
}

impl KdfParams {
    /// Cheap parameters for tests and benchmarks only.
    pub fn fast() -> Self {
        Self { log_n: 6, r: 8, p: 1 }
    }
}

pub fn master_key(pass: &[u8], salt: &[u8], kdf: KdfParams) -> Result<[u8; 32]> {
    let params = scrypt::Params::new(kdf.log_n, kdf.r, kdf.p).map_err(|_| Error::Crypto)?;
    let mut out = [0u8; 32];
    scrypt::scrypt(pass, salt, &params, &mut out).map_err(|_| Error::Crypto)?;
    Ok(out)
}

pub fn derive(master: &[u8; 32], info: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, master);
    let mut out = [0u8; 32];
    hk.expand(info.as_bytes(), &mut out).expect("32 bytes is a valid HKDF length");
    out
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// Returns `nonce || ciphertext || tag`, which is indistinguishable from random bytes.
pub fn seal(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(&Nonce::from(nonce), plaintext)
        .map_err(|_| Error::Crypto)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return Err(Error::NoPayload);
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().expect("split length");
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    cipher
        .decrypt(&Nonce::from(nonce), ct)
        .map_err(|_| Error::NoPayload)
}
