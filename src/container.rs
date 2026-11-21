//! Keyed, encrypted, error-corrected payload containers on top of a [`Carrier`].
//!
//! * `Layout::Single`: one variable-length slot, key-derived sample positions.
//! * `Layout::Deniable`: two fixed-size slots in disjoint halves of the carrier.
//!   Both slots are always filled (unused slots with random bytes), so a carrier
//!   holding only a public file is statistically the same as one holding public + hidden.
//!
//! With `Config::parity > 0` every sealed payload is Reed-Solomon coded, so corrupted
//! samples (a re-save that touched some pixels, noise, partial overwrites) can be repaired.

use crate::carrier::{Carrier, Mode};
use crate::crypto::{self, derive, master_key, KdfParams, NONCE_LEN, TAG_LEN};
use crate::ecc;
use crate::embed::{read_adaptive, read_bytes, spread, write_adaptive, write_bytes, PositionStream};
use crate::error::{Error, Result};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub kdf: KdfParams,
    pub mode: Mode,
    /// Deniable layout only: fraction of each half's samples used (bits per sample).
    pub rate: f64,
    /// Reed-Solomon check bytes per 255-byte block; each block repairs `parity / 2` bad bytes.
    /// 0 disables error correction. Extraction must use the same value.
    pub parity: usize,
    /// Fixes the +-1 choices of `Mode::Match`; for reproducible tests only.
    pub seed: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self { kdf: KdfParams::default(), mode: Mode::Match, rate: 0.1, parity: 16, seed: None }
    }
}

impl Config {
    fn match_rng(&self) -> ChaCha20Rng {
        match self.seed {
            Some(s) => ChaCha20Rng::seed_from_u64(s),
            None => ChaCha20Rng::from_entropy(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Single,
    Deniable,
}

struct SlotKeys {
    aead: [u8; 32],
    pos: [u8; 32],
    stc: [u8; 32],
    mask: [u8; 4],
}

/// nonce + tag + 4-byte plaintext length prefix
const SEALED_OVERHEAD: usize = NONCE_LEN + TAG_LEN;
const SLOT_OVERHEAD: usize = SEALED_OVERHEAD + 4;

fn region(layout: Layout, idx: usize, n: usize) -> (usize, usize) {
    match (layout, idx) {
        (Layout::Single, _) => (0, n),
        (Layout::Deniable, 0) => (0, n / 2),
        (Layout::Deniable, _) => (n / 2, n - n / 2),
    }
}

/// One scrypt run per passphrase and carrier; slot keys for each region are split off with HKDF.
pub(crate) fn master(pass: &[u8], geometry: &[u8], layout: Layout, kdf: KdfParams) -> Result<[u8; 32]> {
    if pass.is_empty() {
        return Err(Error::Invalid("passphrase must not be empty".into()));
    }
    let salt = crypto::sha256(&[b"stegosaur/v2", geometry, &[layout as u8]]);
    master_key(pass, &salt, kdf)
}

fn slot_keys(master: &[u8; 32], idx: usize) -> SlotKeys {
    let m = derive(master, &format!("len-mask/{idx}"));
    SlotKeys {
        aead: derive(master, &format!("aead/{idx}")),
        pos: derive(master, &format!("pos/{idx}")),
        stc: derive(master, &format!("stc/{idx}")),
        mask: [m[0], m[1], m[2], m[3]],
    }
}

fn wire(data: &[u8], parity: usize) -> Result<Vec<u8>> {
    if parity == 0 { Ok(data.to_vec()) } else { ecc::encode(data, parity) }
}

fn unwire(bytes: &[u8], len: usize, parity: usize) -> Result<Vec<u8>> {
    if parity == 0 { Ok(bytes[..len].to_vec()) } else { ecc::decode(bytes, len, parity) }
}

/// Opens an AEAD-sealed, optionally Reed-Solomon coded payload. Undamaged data is authenticated
/// straight from the systematic bytes; error correction only runs when that fails.
fn open_coded(key: &[u8; 32], raw: &[u8], sealed_len: usize, parity: usize) -> Result<Vec<u8>> {
    if parity == 0 {
        return crypto::open(key, &raw[..sealed_len]);
    }
    if let Ok(plain) = crypto::open(key, &ecc::strip(raw, sealed_len, parity)?) {
        return Ok(plain);
    }
    crypto::open(key, &ecc::decode(raw, sealed_len, parity)?)
}

fn wire_len(len: usize, parity: usize) -> usize {
    if parity == 0 { len } else { ecc::encoded_len(len, parity) }
}

fn max_data(budget: usize, parity: usize) -> usize {
    if parity == 0 { budget } else { ecc::max_payload(budget, parity) }
}

fn deniable_slot_bytes(region_len: usize, rate: f64) -> usize {
    (((region_len as f64) * rate / 8.0).floor() as usize).min(region_len / 8)
}

/// Largest payload that fits one hidden-layout slot, from the carrier's sample count alone.
pub fn deniable_capacity_for_len(samples: usize, cfg: &Config) -> usize {
    let (_, len) = region(Layout::Deniable, 0, samples);
    max_data(deniable_slot_bytes(len, cfg.rate), cfg.parity).saturating_sub(SLOT_OVERHEAD)
}

/// Largest payload that fits the single layout (1 bit per sample), from the sample count alone.
pub fn single_capacity_for_len(samples: usize, cfg: &Config) -> usize {
    let header = 4 + if cfg.parity > 0 { cfg.parity } else { 0 };
    let avail = (samples / 8).saturating_sub(header);
    max_data(avail, cfg.parity).saturating_sub(SEALED_OVERHEAD)
}

/// Largest payload that fits one deniable slot.
pub fn deniable_capacity<C: Carrier + ?Sized>(c: &C, cfg: &Config) -> usize {
    deniable_capacity_for_len(c.len(), cfg)
}

/// Largest payload that fits the single layout (1 bit per sample).
pub fn single_capacity<C: Carrier + ?Sized>(c: &C, cfg: &Config) -> usize {
    single_capacity_for_len(c.len(), cfg)
}

pub fn embed_single<C: Carrier + ?Sized>(c: &mut C, pass: &[u8], data: &[u8], cfg: &Config) -> Result<()> {
    embed_single_keys(c, pass, data, cfg).map(|_| ())
}

/// Like [`embed_single`], and also returns the keys for an overflow block (see [`crate::overflow`]).
pub fn embed_single_keys<C: Carrier + ?Sized>(c: &mut C, pass: &[u8], data: &[u8], cfg: &Config) -> Result<crate::overflow::TailKeys> {
    let m = master(pass, &c.geometry(), Layout::Single, cfg.kdf)?;
    let keys = slot_keys(&m, 0);
    let sealed = crypto::seal(&keys.aead, data)?;
    let header = ((sealed.len() as u32) ^ u32::from_le_bytes(keys.mask)).to_le_bytes();
    let mut frame = wire(&header, cfg.parity)?;
    frame.extend_from_slice(&wire(&sealed, cfg.parity)?);

    if frame.len() > c.len() / 8 {
        return Err(Error::Capacity { needed: data.len(), available: single_capacity(c, cfg) });
    }
    let (start, len) = region(Layout::Single, 0, c.len());
    let mut rng = cfg.match_rng();
    let mut stream = PositionStream::new(&keys.pos, start, len);
    if c.adaptive() {
        let costs = c.costs().ok_or_else(|| Error::Invalid("could not measure this image's coefficient costs".into()))?;
        let head_bits = wire_len(4, cfg.parity) * 8;
        let (head, body) = frame.split_at(head_bits / 8);
        write_bytes(c, &stream.take(head_bits), head, cfg.mode, &mut rng);
        let w = spread(len - head_bits, body.len() * 8);
        let positions = stream.take(body.len() * 8 * w);
        write_adaptive(c, &positions, w, &keys.stc, &costs, body, cfg.mode, &mut rng);
    } else {
        write_bytes(c, &stream.take(frame.len() * 8), &frame, cfg.mode, &mut rng);
    }
    Ok(crate::overflow::tail_keys(&m))
}

pub fn extract_single<C: Carrier + ?Sized>(c: &C, pass: &[u8], cfg: &Config) -> Result<Vec<u8>> {
    extract_single_keys(c, pass, cfg).map(|(d, _)| d)
}

/// Like [`extract_single`], and also returns the keys for an overflow block.
pub fn extract_single_keys<C: Carrier + ?Sized>(c: &C, pass: &[u8], cfg: &Config) -> Result<(Vec<u8>, crate::overflow::TailKeys)> {
    let m = master(pass, &c.geometry(), Layout::Single, cfg.kdf)?;
    let keys = slot_keys(&m, 0);
    let (start, len) = region(Layout::Single, 0, c.len());
    let head_wire = wire_len(4, cfg.parity);
    if len / 8 < head_wire + wire_len(SEALED_OVERHEAD, cfg.parity) {
        return Err(Error::NoPayload);
    }
    let mut stream = PositionStream::new(&keys.pos, start, len);
    let head = unwire(&read_bytes(c, &stream.take(head_wire * 8)), 4, cfg.parity)?;
    let sealed_len = (u32::from_le_bytes([head[0], head[1], head[2], head[3]]) ^ u32::from_le_bytes(keys.mask)) as usize;
    if sealed_len < SEALED_OVERHEAD || head_wire + wire_len(sealed_len, cfg.parity) > len / 8 {
        return Err(Error::NoPayload);
    }
    let body_wire = wire_len(sealed_len, cfg.parity);
    let raw = if c.adaptive() {
        let w = spread(len - head_wire * 8, body_wire * 8);
        read_adaptive(c, &stream.take(body_wire * 8 * w), w, &keys.stc)
    } else {
        read_bytes(c, &stream.take(body_wire * 8))
    };
    Ok((open_coded(&keys.aead, &raw, sealed_len, cfg.parity)?, crate::overflow::tail_keys(&m)))
}

/// Sealed length that fits a slot of `slot_bytes` on the wire.
fn sealed_len_for(slot_bytes: usize, parity: usize) -> usize {
    max_data(slot_bytes, parity)
}

fn build_slot(keys: &SlotKeys, data: &[u8], slot_bytes: usize, parity: usize) -> Result<Vec<u8>> {
    let sealed_len = sealed_len_for(slot_bytes, parity);
    if sealed_len < SLOT_OVERHEAD {
        return Err(Error::Capacity { needed: data.len(), available: 0 });
    }
    let max = sealed_len - SLOT_OVERHEAD;
    if data.len() > max {
        return Err(Error::Capacity { needed: data.len(), available: max });
    }
    let mut plain = vec![0u8; sealed_len - SEALED_OVERHEAD];
    plain[..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    plain[4..4 + data.len()].copy_from_slice(data);
    let sealed = crypto::seal(&keys.aead, &plain)?;
    debug_assert_eq!(sealed.len(), sealed_len);
    let mut out = wire(&sealed, parity)?;
    let used = out.len();
    out.resize(slot_bytes, 0);
    rand::thread_rng().fill_bytes(&mut out[used..]);
    Ok(out)
}

/// Embeds a public payload and an optional hidden payload.
/// `public` goes to the first half, `hidden` (or random filler) to the second.
pub fn embed_deniable<C: Carrier + ?Sized>(
    c: &mut C,
    public: (&[u8], &[u8]),
    hidden: Option<(&[u8], &[u8])>,
    cfg: &Config,
) -> Result<()> {
    if let Some((hp, _)) = hidden {
        if hp == public.0 {
            return Err(Error::Invalid("the two passphrases must differ".into()));
        }
    }
    let geometry = c.geometry();
    let slot_bytes = deniable_slot_bytes(region(Layout::Deniable, 0, c.len()).1, cfg.rate);
    if sealed_len_for(slot_bytes, cfg.parity) <= SLOT_OVERHEAD {
        return Err(Error::Capacity { needed: 1, available: 0 });
    }
    let mut rng = cfg.match_rng();

    let mut slots: Vec<(Vec<u8>, [u8; 32], [u8; 32])> = Vec::with_capacity(2);
    let costs = if c.adaptive() {
        Some(c.costs().ok_or_else(|| Error::Invalid("could not measure this image's coefficient costs".into()))?)
    } else {
        None
    };
    let k0 = slot_keys(&master(public.0, &geometry, Layout::Deniable, cfg.kdf)?, 0);
    slots.push((build_slot(&k0, public.1, slot_bytes, cfg.parity)?, k0.pos, k0.stc));
    match hidden {
        Some((hp, hd)) => {
            let k1 = slot_keys(&master(hp, &geometry, Layout::Deniable, cfg.kdf)?, 1);
            slots.push((build_slot(&k1, hd, slot_bytes, cfg.parity)?, k1.pos, k1.stc));
        }
        None => {
            let mut filler = vec![0u8; slot_bytes];
            let mut key = [0u8; 32];
            let mut stc = [0u8; 32];
            let mut os = rand::thread_rng();
            os.fill_bytes(&mut filler);
            os.fill_bytes(&mut key);
            os.fill_bytes(&mut stc);
            slots.push((filler, key, stc));
        }
    }
    for (idx, (bytes, pos_key, stc_key)) in slots.iter().enumerate() {
        let (start, len) = region(Layout::Deniable, idx, c.len());
        if let Some(costs) = &costs {
            let w = spread(len, bytes.len() * 8);
            let positions = PositionStream::new(pos_key, start, len).take(bytes.len() * 8 * w);
            write_adaptive(c, &positions, w, stc_key, costs, bytes, cfg.mode, &mut rng);
        } else {
            let positions = PositionStream::new(pos_key, start, len).take(bytes.len() * 8);
            write_bytes(c, &positions, bytes, cfg.mode, &mut rng);
        }
    }
    Ok(())
}

/// Tries both slots with the given passphrase; the AEAD tag decides which one (if any) is real.
pub fn extract_deniable<C: Carrier + ?Sized>(c: &C, pass: &[u8], cfg: &Config) -> Result<Vec<u8>> {
    let geometry = c.geometry();
    let slot_bytes = deniable_slot_bytes(region(Layout::Deniable, 0, c.len()).1, cfg.rate);
    let sealed_len = sealed_len_for(slot_bytes, cfg.parity);
    if sealed_len <= SLOT_OVERHEAD {
        return Err(Error::NoPayload);
    }
    let master = master(pass, &geometry, Layout::Deniable, cfg.kdf)?;
    for idx in 0..2 {
        let keys = slot_keys(&master, idx);
        let (start, len) = region(Layout::Deniable, idx, c.len());
        let raw = if c.adaptive() {
            let w = spread(len, slot_bytes * 8);
            let positions = PositionStream::new(&keys.pos, start, len).take(slot_bytes * 8 * w);
            read_adaptive(c, &positions, w, &keys.stc)
        } else {
            read_bytes(c, &PositionStream::new(&keys.pos, start, len).take(slot_bytes * 8))
        };
        if let Ok(plain) = open_coded(&keys.aead, &raw, sealed_len, cfg.parity) {
            let n = u32::from_le_bytes([plain[0], plain[1], plain[2], plain[3]]) as usize;
            if n <= plain.len() - 4 {
                return Ok(plain[4..4 + n].to_vec());
            }
        }
    }
    Err(Error::NoPayload)
}
