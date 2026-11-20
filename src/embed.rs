use crate::carrier::{write_bit, Carrier, Mode};
use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Key-derived stream of distinct sample indices.
///
/// While fewer than half of the region has been handed out it draws uniformly and rejects
/// repeats using a bitmap (n/8 bytes, so it stays in cache and needs no n-sized table).
/// After that it shuffles the remaining indices. The switch point depends only on the region
/// size, so the stream is prefix-stable: the first k outputs never depend on how many are drawn.
pub struct PositionStream {
    n: u32,
    start: u32,
    rng: ChaCha8Rng,
    seen: Vec<u64>,
    distinct: usize,
    switch_at: usize,
    tail: Vec<u32>,
    tail_built: bool,
    tail_next: usize,
}

impl PositionStream {
    pub fn new(key: &[u8; 32], start: usize, len: usize) -> Self {
        assert!(start + len <= u32::MAX as usize, "carrier too large");
        Self {
            n: len as u32,
            start: start as u32,
            rng: ChaCha8Rng::from_seed(*key),
            seen: vec![0u64; len.div_ceil(64)],
            distinct: 0,
            switch_at: len / 2,
            tail: Vec::new(),
            tail_built: false,
            tail_next: 0,
        }
    }

    #[inline]
    fn next_one(&mut self) -> u32 {
        if self.distinct < self.switch_at {
            loop {
                let x = self.rng.gen_range(0..self.n);
                let (w, bit) = ((x >> 6) as usize, 1u64 << (x & 63));
                if self.seen[w] & bit == 0 {
                    self.seen[w] |= bit;
                    self.distinct += 1;
                    return x;
                }
            }
        }
        if !self.tail_built {
            self.tail = (0..self.n).filter(|&x| self.seen[(x >> 6) as usize] & (1u64 << (x & 63)) == 0).collect();
            self.tail_built = true;
        }
        let i = self.tail_next;
        assert!(i < self.tail.len(), "more positions requested than region size");
        let j = self.rng.gen_range(i..self.tail.len());
        self.tail.swap(i, j);
        self.tail_next += 1;
        self.tail[i]
    }

    pub fn take(&mut self, count: usize) -> Vec<u32> {
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.next_one() + self.start);
        }
        out
    }
}

pub fn write_bytes<C: Carrier + ?Sized, R: RngCore>(
    c: &mut C,
    positions: &[u32],
    bytes: &[u8],
    mode: Mode,
    rng: &mut R,
) {
    debug_assert_eq!(positions.len(), bytes.len() * 8);
    for (bi, &b) in bytes.iter().enumerate() {
        for k in 0..8 {
            let bit = (b >> (7 - k)) & 1;
            write_bit(c, positions[bi * 8 + k] as usize, bit, mode, rng);
        }
    }
}

pub fn read_bytes<C: Carrier + ?Sized>(c: &C, positions: &[u32]) -> Vec<u8> {
    debug_assert_eq!(positions.len() % 8, 0);
    positions
        .chunks_exact(8)
        .map(|ch| ch.iter().fold(0u8, |acc, &p| (acc << 1) | (c.get(p as usize) & 1) as u8))
        .collect()
}

/// Most elements one message bit may be spread over.
pub const MAX_SPREAD: usize = 100;

/// How many cover elements each message bit gets when `avail` elements carry `bits` message bits.
pub fn spread(avail: usize, bits: usize) -> usize {
    if bits == 0 { 1 } else { (avail / bits).clamp(1, MAX_SPREAD) }
}

fn bit_vec(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().flat_map(|&b| (0..8).map(move |k| (b >> (7 - k)) & 1)).collect()
}

fn bit_bytes(bits: &[u8]) -> Vec<u8> {
    bits.chunks_exact(8).map(|ch| ch.iter().fold(0u8, |a, &b| (a << 1) | (b & 1))).collect()
}

/// Writes `bytes` with a syndrome-trellis code over `bytes.len()*8*w` cover elements, changing
/// the cheapest ones. `positions` must hold exactly that many sample indices.
pub fn write_adaptive<C: Carrier + ?Sized, R: RngCore>(
    c: &mut C,
    positions: &[u32],
    w: usize,
    key: &[u8; 32],
    costs: &[f32],
    bytes: &[u8],
    mode: Mode,
    rng: &mut R,
) {
    let msg = bit_vec(bytes);
    debug_assert_eq!(positions.len(), msg.len() * w);
    let cover: Vec<u8> = positions.iter().map(|&p| (c.get(p as usize) & 1) as u8).collect();
    let cost: Vec<f32> = positions.iter().map(|&p| costs[p as usize]).collect();
    let stego = crate::stc::encode(key, w, &cover, &cost, &msg);
    for ((&p, &x), &y) in positions.iter().zip(&cover).zip(&stego) {
        if x != y {
            write_bit(c, p as usize, y, mode, rng);
        }
    }
}

pub fn read_adaptive<C: Carrier + ?Sized>(c: &C, positions: &[u32], w: usize, key: &[u8; 32]) -> Vec<u8> {
    let cover: Vec<u8> = positions.iter().map(|&p| (c.get(p as usize) & 1) as u8).collect();
    bit_bytes(&crate::stc::decode(key, w, cover.len() / w, &cover))
}
