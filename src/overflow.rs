//! Extra room: when a cover is too small, the rest of the payload goes into an encrypted block
//! attached to the end of the output file, outside the picture or sound.
//!
//! The block is written and read in 1 MiB pieces, so memory use stays flat however large it is.
//! Every piece is sealed on its own, and a short sealed footer records the total length. All of it
//! looks like random bytes, and the footer only opens with the right passphrase, so a file without
//! overflow cannot be told apart from one with it by anyone who does not have the passphrase.
//!
//! File layout after the normal image or audio bytes: `piece 1 | piece 2 | ... | footer`.

use crate::crypto::{self, derive, NONCE_LEN, TAG_LEN};
use crate::error::{Error, Result};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Plaintext bytes per sealed piece.
pub const CHUNK: usize = 1 << 20;
const IDX: usize = 8;
const PIECE_OVERHEAD: usize = NONCE_LEN + IDX + TAG_LEN;
const FOOT_PLAIN: usize = 16;
/// Size of the sealed footer on disk.
pub const FOOT_LEN: usize = NONCE_LEN + FOOT_PLAIN + TAG_LEN;
const MAX_CHUNK: u64 = 16 << 20;

#[derive(Clone, Copy)]
pub struct TailKeys {
    chunk: [u8; 32],
    foot: [u8; 32],
}

/// Keys for the overflow block, split off the same master key as the pixel slot.
pub fn tail_keys(master: &[u8; 32]) -> TailKeys {
    TailKeys { chunk: derive(master, "tail/chunk"), foot: derive(master, "tail/foot") }
}

/// Only file types that ignore bytes after their end can carry the block unharmed.
pub fn supports(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "png" | "bmp" | "wav")
}

fn fill(r: &mut dyn Read, buf: &mut [u8]) -> Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = r.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// Appends everything `rest` yields to `out`. Returns the number of plaintext bytes (0 means nothing was added).
pub fn append(out: &Path, keys: &TailKeys, rest: &mut dyn Read) -> Result<u64> {
    let mut f = std::io::BufWriter::new(std::fs::OpenOptions::new().append(true).open(out)?);
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    let mut idx = 0u64;
    loop {
        crate::cancel::check()?;
        let n = fill(rest, &mut buf)?;
        if n == 0 {
            break;
        }
        let mut plain = Vec::with_capacity(IDX + n);
        plain.extend_from_slice(&idx.to_le_bytes());
        plain.extend_from_slice(&buf[..n]);
        f.write_all(&crypto::seal(&keys.chunk, &plain)?)?;
        total += n as u64;
        idx += 1;
    }
    if total == 0 {
        return Ok(0);
    }
    let mut foot = [0u8; FOOT_PLAIN];
    foot[..8].copy_from_slice(&total.to_le_bytes());
    foot[8..12].copy_from_slice(&(CHUNK as u32).to_le_bytes());
    f.write_all(&crypto::seal(&keys.foot, &foot)?)?;
    f.flush()?;
    Ok(total)
}

/// Looks for an overflow block at the end of `path` and writes it to `sink`.
/// Returns `None` when the file has no block that opens with these keys.
pub fn read(path: &Path, keys: &TailKeys, sink: &mut dyn Write) -> Result<Option<u64>> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len < FOOT_LEN as u64 {
        return Ok(None);
    }
    f.seek(SeekFrom::Start(len - FOOT_LEN as u64))?;
    let mut sealed = [0u8; FOOT_LEN];
    f.read_exact(&mut sealed)?;
    let Ok(foot) = crypto::open(&keys.foot, &sealed) else { return Ok(None) };
    if foot.len() != FOOT_PLAIN {
        return Ok(None);
    }
    let total = u64::from_le_bytes(foot[..8].try_into().expect("8 bytes"));
    let chunk = u32::from_le_bytes(foot[8..12].try_into().expect("4 bytes")) as u64;
    if total == 0 || chunk == 0 || chunk > MAX_CHUNK {
        return Ok(None);
    }
    let pieces = total.div_ceil(chunk);
    let body = total + pieces * PIECE_OVERHEAD as u64;
    if body + FOOT_LEN as u64 > len {
        return Err(Error::Invalid("the extra block at the end of this file is cut short".into()));
    }
    f.seek(SeekFrom::Start(len - FOOT_LEN as u64 - body))?;
    let mut f = std::io::BufReader::with_capacity(CHUNK, f);
    let mut left = total;
    for i in 0..pieces {
        let plain_len = left.min(chunk) as usize;
        let mut raw = vec![0u8; plain_len + PIECE_OVERHEAD];
        f.read_exact(&mut raw)?;
        let plain = crypto::open(&keys.chunk, &raw).map_err(|_| Error::Invalid("the extra block is damaged".into()))?;
        if plain.len() != IDX + plain_len || u64::from_le_bytes(plain[..IDX].try_into().expect("8 bytes")) != i {
            return Err(Error::Invalid("the extra block is damaged".into()));
        }
        sink.write_all(&plain[IDX..])?;
        left -= plain_len as u64;
    }
    Ok(Some(total))
}
