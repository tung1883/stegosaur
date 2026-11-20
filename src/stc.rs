//! Syndrome-trellis codes (Filler, Judas, Fridrich 2011): embed a message in the parities of a
//! cover while changing as little cost as possible. The receiver needs only the parities and the
//! key-derived parity-check matrix; the per-element costs are used by the sender alone.
//!
//! Each message bit covers `w` cover elements. Larger `w` (a shorter message relative to the
//! cover) lets the encoder pick cheaper elements and make fewer changes.
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

const H: usize = 7;
const STATES: usize = 1 << H;
/// Message bits per independent trellis segment (bounds memory and allows threads).
const SEG: usize = 1024;
const INF: f32 = 1e30;

/// Key-derived sub-matrix columns; every column has its first and last row set.
fn columns(key: &[u8; 32], w: usize) -> Vec<u8> {
    let mut rng = ChaCha8Rng::from_seed(*key);
    (0..w).map(|_| ((rng.next_u32() as u8) & 0x7f) | 1 | (1 << (H - 1))).collect()
}

fn decode_segment(cols: &[u8], w: usize, cover: &[u8], out: &mut Vec<u8>) {
    let mut s = 0u8;
    for block in cover.chunks(w) {
        for (j, &x) in block.iter().enumerate() {
            if x & 1 == 1 {
                s ^= cols[j];
            }
        }
        out.push(s & 1);
        s >>= 1;
    }
}

/// The message carried by `cover` (one 0/1 per element), `m` bits, `w` elements per bit.
pub fn decode(key: &[u8; 32], w: usize, m: usize, cover: &[u8]) -> Vec<u8> {
    assert_eq!(cover.len(), w * m);
    let cols = columns(key, w);
    let mut out = Vec::with_capacity(m);
    for seg in cover.chunks(w * SEG) {
        decode_segment(&cols, w, seg, &mut out);
    }
    out
}

fn encode_segment(cols: &[u8], w: usize, cover: &[u8], cost: &[f32], msg: &[u8]) -> Vec<u8> {
    let n = cover.len();
    let mut cur = [INF; STATES];
    cur[0] = 0.0;
    let mut back: Vec<u128> = vec![0; n];
    for (b, &mb) in msg.iter().enumerate() {
        for j in 0..w {
            let i = b * w + j;
            let col = cols[j] as usize;
            let (c0, c1) = if cover[i] & 1 == 0 { (0.0, cost[i]) } else { (cost[i], 0.0) };
            let mut next = [INF; STATES];
            let mut bits = 0u128;
            for s in 0..STATES {
                let keep = cur[s] + c0;
                let flip = cur[s ^ col] + c1;
                if flip < keep {
                    next[s] = flip;
                    bits |= 1u128 << s;
                } else {
                    next[s] = keep;
                }
            }
            back[i] = bits;
            cur = next;
        }
        // Row b is complete: its parity must equal the message bit, then the window slides.
        let mut shifted = [INF; STATES];
        for s in 0..STATES {
            if (s & 1) as u8 == mb & 1 {
                shifted[s >> 1] = cur[s];
            }
        }
        cur = shifted;
    }
    let mut state = (0..STATES).min_by(|&a, &b| cur[a].partial_cmp(&cur[b]).unwrap()).unwrap();
    let mut stego = vec![0u8; n];
    for b in (0..msg.len()).rev() {
        state = (state << 1) | (msg[b] & 1) as usize;
        for j in (0..w).rev() {
            let i = b * w + j;
            let y = ((back[i] >> state) & 1) as u8;
            stego[i] = y;
            if y == 1 {
                state ^= cols[j] as usize;
            }
        }
    }
    stego
}

/// Stego parities (0/1 per element) that decode to `msg` at minimum total change cost.
pub fn encode(key: &[u8; 32], w: usize, cover: &[u8], cost: &[f32], msg: &[u8]) -> Vec<u8> {
    let m = msg.len();
    assert_eq!(cover.len(), w * m);
    assert_eq!(cost.len(), cover.len());
    let cols = columns(key, w);
    let jobs: Vec<(&[u8], &[f32], &[u8])> = cover
        .chunks(w * SEG)
        .zip(cost.chunks(w * SEG))
        .zip(msg.chunks(SEG))
        .map(|((a, b), c)| (a, b, c))
        .collect();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(jobs.len()).max(1);
    let mut results: Vec<Vec<u8>> = vec![Vec::new(); jobs.len()];
    let per = jobs.len().div_ceil(threads);
    std::thread::scope(|sc| {
        for (jc, rc) in jobs.chunks(per).zip(results.chunks_mut(per)) {
            let cols = &cols;
            sc.spawn(move || {
                for (j, r) in jc.iter().zip(rc.iter_mut()) {
                    *r = encode_segment(cols, w, j.0, j.1, j.2);
                }
            });
        }
    });
    results.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rnd(n: usize, seed: u64) -> Vec<u8> {
        let mut r = ChaCha8Rng::seed_from_u64(seed);
        (0..n).map(|_| (r.next_u32() & 1) as u8).collect()
    }

    #[test]
    fn decode_returns_the_message_for_many_shapes() {
        for (m, w) in [(1usize, 1usize), (7, 1), (100, 3), (1024, 5), (2500, 4), (3000, 20)] {
            let key = [9u8; 32];
            let cover = rnd(m * w, 1);
            let cost: Vec<f32> = (0..m * w).map(|i| 1.0 + (i % 7) as f32).collect();
            let msg = rnd(m, 2);
            let stego = encode(&key, w, &cover, &cost, &msg);
            assert_eq!(decode(&key, w, m, &stego), msg, "m={m} w={w}");
        }
    }

    #[test]
    fn spreading_the_message_makes_fewer_changes() {
        let key = [3u8; 32];
        let m = 4000;
        let msg = rnd(m, 5);
        let mut last = usize::MAX;
        for w in [1usize, 2, 4, 8] {
            let cover = rnd(m * w, 11);
            let cost = vec![1.0f32; m * w];
            let stego = encode(&key, w, &cover, &cost, &msg);
            let changes = cover.iter().zip(&stego).filter(|(a, b)| a != b).count();
            if w == 1 {
                // fully constrained: about half the bits must change
                assert!(changes > m / 3);
            }
            assert!(changes <= last, "w={w}: {changes} > {last}");
            last = changes;
        }
        // At 1/8 relative payload plain matching needs about m/2 changes; the trellis needs far fewer.
        assert!(last < m / 4, "{last}");
    }

    #[test]
    fn it_prefers_cheap_elements() {
        let key = [4u8; 32];
        let (m, w) = (2000usize, 8usize);
        let cover = rnd(m * w, 21);
        let msg = rnd(m, 22);
        let cost: Vec<f32> = (0..m * w).map(|i| if i % 2 == 0 { 1.0 } else { 100.0 }).collect();
        let stego = encode(&key, w, &cover, &cost, &msg);
        let (mut cheap, mut dear) = (0, 0);
        for i in 0..m * w {
            if cover[i] != stego[i] {
                if i % 2 == 0 { cheap += 1 } else { dear += 1 }
            }
        }
        assert!(dear * 10 < cheap, "cheap {cheap} dear {dear}");
    }
}
