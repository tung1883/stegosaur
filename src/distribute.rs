//! Threshold embedding: split a secret into n shares, hide one share per carrier,
//! and recover it from any k of them.

use crate::carrier::Carrier;
use crate::container::{embed_single, extract_single, Config};
use crate::crypto::sha256;
use crate::error::{Error, Result};
use crate::shamir::{self, Share};
use rand::RngCore;

const VERSION: u8 = 1;
/// Bytes of metadata stored with every share, on top of the secret itself.
pub const SHARE_OVERHEAD: usize = 1 + 1 + 1 + 1 + 8 + 4 + 32;
const HEADER: usize = SHARE_OVERHEAD;

/// One decoded share together with the metadata that says which split it belongs to.
#[derive(Clone)]
pub struct ShareBlob {
    pub k: u8,
    pub n: u8,
    pub id: [u8; 8],
    secret_len: u32,
    digest: [u8; 32],
    share: Share,
}

impl ShareBlob {
    fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(HEADER + self.share.y.len());
        v.extend_from_slice(&[VERSION, self.k, self.n, self.share.x]);
        v.extend_from_slice(&self.id);
        v.extend_from_slice(&self.secret_len.to_le_bytes());
        v.extend_from_slice(&self.digest);
        v.extend_from_slice(&self.share.y);
        v
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() <= HEADER || b[0] != VERSION {
            return None;
        }
        let secret_len = u32::from_le_bytes(b[12..16].try_into().ok()?);
        if secret_len as usize != b.len() - HEADER || b[3] == 0 || b[1] == 0 || b[1] > b[2] {
            return None;
        }
        Some(Self {
            k: b[1],
            n: b[2],
            id: b[4..12].try_into().ok()?,
            secret_len,
            digest: b[16..48].try_into().ok()?,
            share: Share { x: b[3], y: b[HEADER..].to_vec() },
        })
    }
}

/// Hides `secret` across `carriers` (one share each); any `k` of them recover it.
pub fn distribute<C: Carrier>(secret: &[u8], pass: &[u8], k: u8, carriers: &mut [C], cfg: &Config) -> Result<()> {
    let n = u8::try_from(carriers.len()).map_err(|_| Error::Invalid("at most 255 carriers".into()))?;
    let mut rng = rand::thread_rng();
    let shares = shamir::split(secret, k, n, &mut rng)?;
    let mut id = [0u8; 8];
    rng.fill_bytes(&mut id);
    let digest = sha256(&[secret]);
    for (carrier, share) in carriers.iter_mut().zip(shares) {
        let blob = ShareBlob { k, n, id, secret_len: secret.len() as u32, digest, share }.encode();
        embed_single(carrier, pass, &blob, cfg)?;
    }
    Ok(())
}

/// Recovers the secret from whichever carriers still yield a valid share.
pub fn recover<C: Carrier>(pass: &[u8], carriers: &[C], cfg: &Config) -> Result<Vec<u8>> {
    let mut blobs: Vec<ShareBlob> = Vec::new();
    for c in carriers {
        if let Ok(raw) = extract_single(c, pass, cfg) {
            if let Some(b) = ShareBlob::decode(&raw) {
                blobs.push(b);
            }
        }
    }
    for i in 0..blobs.len() {
        let group: Vec<&ShareBlob> = blobs.iter().filter(|b| b.id == blobs[i].id).collect();
        if let Some(secret) = combine_group(&group) {
            return Ok(secret);
        }
    }
    Err(Error::NoPayload)
}

/// Rebuilds the secret from shares of one split; `None` when there are too few or they do not check out.
pub fn combine_group(group: &[&ShareBlob]) -> Option<Vec<u8>> {
    let head = group.first()?;
    let mut seen = Vec::new();
    let mut shares = Vec::new();
    for b in group {
        if b.id == head.id && !seen.contains(&b.share.x) && b.secret_len == head.secret_len {
            seen.push(b.share.x);
            shares.push(b.share.clone());
        }
    }
    if shares.len() < head.k as usize {
        return None;
    }
    shares.truncate(head.k as usize);
    let secret = shamir::combine(&shares).ok()?;
    (sha256(&[&secret]) == head.digest).then_some(secret)
}
