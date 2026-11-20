//! JPEG carrier: hides data in the quantized DCT coefficients, so it survives as a real JPEG (no pixel re-encoding).
//!
//! * Reads baseline, extended-sequential and progressive 8-bit Huffman-coded JPEGs, with restart intervals and any
//!   subsampling. Progressive files are written back as baseline JPEGs with identical pixels. Arithmetic-coded, lossless
//!   and 12-bit files are rejected with a clear error.
//! * The usable positions are exactly the non-zero AC coefficients. Embedding never turns one into zero (a coefficient of
//!   magnitude 1 can only grow to 2, never shrink to 0), so the embedder and the extractor see the same position list.
//! * The bit carried is the parity of the coefficient magnitude, which is `v & 1` for negative numbers too.
//! * The file is rewritten losslessly: the original Huffman tables are reused when every symbol still exists, and optimal
//!   tables are generated only when a changed coefficient needs a symbol the original tables lack.

use crate::carrier::{Carrier, ImageCarrier};
use crate::error::{Error, Result};
use std::path::Path;

/// Zigzag index -> natural (row-major) position.
pub(crate) const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58,
    59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const fn f2f(x: f32) -> i32 {
    (x * 4096.0 + 0.5) as i32
}

/// One-dimensional 8-point inverse DCT in 12-bit fixed point (the classic "islow" algorithm).
/// Returns x0..x3 and t0..t3; outputs are x0+t3, x1+t2, x2+t1, x3+t0, x3-t0, x2-t1, x1-t2, x0-t3.
#[inline(always)]
#[allow(clippy::excessive_precision)] // the textbook constants, kept as published
fn idct_1d(s: [i32; 8]) -> [i32; 8] {
    let (p2, p3) = (s[2], s[6]);
    let p1 = (p2 + p3) * f2f(0.5411961);
    let mut t2 = p1 + p3 * f2f(-1.847759065);
    let mut t3 = p1 + p2 * f2f(0.765366865);
    let (p2, p3) = (s[0], s[4]);
    let mut t0 = (p2 + p3) * 4096;
    let mut t1 = (p2 - p3) * 4096;
    let x0 = t0 + t3;
    let x3 = t0 - t3;
    let x1 = t1 + t2;
    let x2 = t1 - t2;
    t0 = s[7];
    t1 = s[5];
    t2 = s[3];
    t3 = s[1];
    let mut p3 = t0 + t2;
    let mut p4 = t1 + t3;
    let mut p1 = t0 + t3;
    let mut p2 = t1 + t2;
    let p5 = (p3 + p4) * f2f(1.175875602);
    t0 *= f2f(0.298631336);
    t1 *= f2f(2.053119869);
    t2 *= f2f(3.072711026);
    t3 *= f2f(1.501321110);
    p1 = p5 + p1 * f2f(-0.899976223);
    p2 = p5 + p2 * f2f(-2.562915447);
    p3 *= f2f(-1.961570560);
    p4 *= f2f(-0.390180644);
    t3 += p1 + p4;
    t2 += p2 + p3;
    t1 += p2 + p4;
    t0 += p1 + p3;
    [x0, x1, x2, x3, t0, t1, t2, t3]
}

/// Inverse DCT of one dequantised block (natural order) into 8 rows of an 8-bit plane.
fn idct_block(data: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut v = [0i32; 64];
    for i in 0..8 {
        if (1..8).all(|r| data[r * 8 + i] == 0) {
            let dc = data[i] * 4;
            for r in 0..8 {
                v[r * 8 + i] = dc;
            }
        } else {
            let [mut x0, mut x1, mut x2, mut x3, t0, t1, t2, t3] = idct_1d([data[i], data[8 + i], data[16 + i], data[24 + i], data[32 + i], data[40 + i], data[48 + i], data[56 + i]]);
            x0 += 512;
            x1 += 512;
            x2 += 512;
            x3 += 512;
            v[i] = (x0 + t3) >> 10;
            v[56 + i] = (x0 - t3) >> 10;
            v[8 + i] = (x1 + t2) >> 10;
            v[48 + i] = (x1 - t2) >> 10;
            v[16 + i] = (x2 + t1) >> 10;
            v[40 + i] = (x2 - t1) >> 10;
            v[24 + i] = (x3 + t0) >> 10;
            v[32 + i] = (x3 - t0) >> 10;
        }
    }
    const ROUND: i32 = 65536 + (128 << 17);
    for r in 0..8 {
        let row = &v[r * 8..r * 8 + 8];
        let [mut x0, mut x1, mut x2, mut x3, t0, t1, t2, t3] = idct_1d([row[0], row[1], row[2], row[3], row[4], row[5], row[6], row[7]]);
        x0 += ROUND;
        x1 += ROUND;
        x2 += ROUND;
        x3 += ROUND;
        let o = &mut out[r * stride..r * stride + 8];
        let c = |x: i32| (x >> 17).clamp(0, 255) as u8;
        o[0] = c(x0 + t3);
        o[7] = c(x0 - t3);
        o[1] = c(x1 + t2);
        o[6] = c(x1 - t2);
        o[2] = c(x2 + t1);
        o[5] = c(x2 - t1);
        o[3] = c(x3 + t0);
        o[4] = c(x3 - t0);
    }
}

fn bad(msg: &str) -> Error {
    Error::Invalid(format!("JPEG: {msg}"))
}

fn unsupported(msg: &str) -> Error {
    Error::Unsupported(format!("JPEG: {msg}"))
}

#[derive(Clone, Default)]
struct Spec {
    counts: [u8; 16],
    vals: Vec<u8>,
}

struct Dec {
    mincode: [i32; 17],
    maxcode: [i32; 18],
    valptr: [usize; 17],
    vals: Vec<u8>,
}

fn build_dec(s: &Spec) -> Result<Dec> {
    let total: usize = s.counts.iter().map(|&c| c as usize).sum();
    if total != s.vals.len() || total == 0 || total > 256 {
        return Err(bad("corrupt Huffman table"));
    }
    let mut d = Dec { mincode: [0; 17], maxcode: [-1; 18], valptr: [0; 17], vals: s.vals.clone() };
    let (mut code, mut k) = (0i32, 0usize);
    for len in 1..=16usize {
        let n = s.counts[len - 1] as usize;
        if n > 0 {
            d.valptr[len] = k;
            d.mincode[len] = code;
            code += n as i32;
            if code > (1 << len) {
                return Err(bad("corrupt Huffman table"));
            }
            d.maxcode[len] = code - 1;
            k += n;
        }
        code <<= 1;
    }
    Ok(d)
}

#[derive(Clone)]
struct Enc {
    code: [u16; 256],
    size: [u8; 256],
}

fn build_enc(s: &Spec) -> Enc {
    let mut e = Enc { code: [0; 256], size: [0; 256] };
    let (mut code, mut k) = (0u32, 0usize);
    for len in 1..=16usize {
        for _ in 0..s.counts[len - 1] {
            let sym = s.vals[k] as usize;
            e.code[sym] = code as u16;
            e.size[sym] = len as u8;
            code += 1;
            k += 1;
        }
        code <<= 1;
    }
    e
}

/// Optimal length-limited Huffman table for the given symbol frequencies (JPEG Annex K.2).
fn gen_optimal(freq_in: &[u64; 256]) -> Spec {
    let mut freq = [0i64; 257];
    for i in 0..256 {
        freq[i] = freq_in[i] as i64;
    }
    freq[256] = 1; // reserved symbol keeps the all-ones code unused
    let mut codesize = [0usize; 257];
    let mut others = [-1i32; 257];
    loop {
        let mut c1 = -1i32;
        let mut v = i64::MAX;
        for i in 0..=256usize {
            if freq[i] != 0 && freq[i] <= v {
                v = freq[i];
                c1 = i as i32;
            }
        }
        let mut c2 = -1i32;
        v = i64::MAX;
        for i in 0..=256usize {
            if freq[i] != 0 && freq[i] <= v && i as i32 != c1 {
                v = freq[i];
                c2 = i as i32;
            }
        }
        if c2 < 0 {
            break;
        }
        freq[c1 as usize] += freq[c2 as usize];
        freq[c2 as usize] = 0;
        let mut a = c1 as usize;
        codesize[a] += 1;
        while others[a] >= 0 {
            a = others[a] as usize;
            codesize[a] += 1;
        }
        others[a] = c2;
        let mut b = c2 as usize;
        codesize[b] += 1;
        while others[b] >= 0 {
            b = others[b] as usize;
            codesize[b] += 1;
        }
    }
    let mut bits = [0i32; 33];
    for &cs in codesize.iter() {
        if cs > 0 {
            bits[cs.min(32)] += 1;
        }
    }
    for i in (17..=32).rev() {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
    }
    let mut i = 16;
    while i > 0 && bits[i] == 0 {
        i -= 1;
    }
    if i > 0 {
        bits[i] -= 1; // remove the reserved symbol
    }
    let mut spec = Spec::default();
    for l in 1..=16 {
        spec.counts[l - 1] = bits[l] as u8;
    }
    for len in 1..=32usize {
        for sym in 0..256usize {
            if codesize[sym] == len {
                spec.vals.push(sym as u8);
            }
        }
    }
    let wanted: usize = spec.counts.iter().map(|&c| c as usize).sum();
    spec.vals.truncate(wanted);
    if spec.vals.is_empty() {
        spec.counts[0] = 1;
        spec.vals.push(0);
    }
    spec
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
    marker: bool,
    real: u64,
    used: u64,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, pos: 0, acc: 0, n: 0, marker: false, real: 0, used: 0 }
    }

    fn fill(&mut self) {
        while self.n <= 48 {
            let mut byte = 0u8;
            if !self.marker && self.pos < self.d.len() {
                let b = self.d[self.pos];
                if b == 0xFF {
                    match self.d.get(self.pos + 1) {
                        Some(0) => {
                            self.pos += 2;
                            byte = 0xFF;
                            self.real += 8;
                        }
                        _ => self.marker = true,
                    }
                } else {
                    self.pos += 1;
                    byte = b;
                    self.real += 8;
                }
            }
            self.acc = (self.acc << 8) | byte as u64;
            self.n += 8;
        }
    }

    #[inline]
    fn take(&mut self, nbits: u32) -> Result<u32> {
        if nbits == 0 {
            return Ok(0);
        }
        if self.n < nbits {
            self.fill();
        }
        self.n -= nbits;
        self.used += nbits as u64;
        if self.used > self.real {
            return Err(bad("the image data is truncated"));
        }
        Ok(((self.acc >> self.n) & ((1u64 << nbits) - 1)) as u32)
    }

    fn huff(&mut self, t: &Dec) -> Result<u8> {
        let mut code = 0i32;
        for len in 1..=16usize {
            code = (code << 1) | self.take(1)? as i32;
            if code <= t.maxcode[len] {
                return Ok(t.vals[t.valptr[len] + (code - t.mincode[len]) as usize]);
            }
        }
        Err(bad("invalid Huffman code"))
    }

    /// Discards padding bits and consumes the RSTn marker at an interval boundary.
    fn restart(&mut self) -> Result<()> {
        self.acc = 0;
        self.n = 0;
        self.real = 0;
        self.used = 0;
        let mut p = self.pos;
        for _ in 0..16 {
            if p + 1 < self.d.len() && self.d[p] == 0xFF && (0xD0..=0xD7).contains(&self.d[p + 1]) {
                self.pos = p + 2;
                self.marker = false;
                return Ok(());
            }
            if p + 1 >= self.d.len() || (self.d[p] == 0xFF && self.d[p + 1] != 0) {
                break;
            }
            p += 1;
        }
        Err(bad("missing restart marker"))
    }
}

fn extend(v: u32, s: u32) -> i32 {
    let vt = 1i32 << (s - 1);
    if (v as i32) < vt {
        v as i32 - (1 << s) + 1
    } else {
        v as i32
    }
}

fn cat_bits(v: i32) -> (u8, u32) {
    if v == 0 {
        return (0, 0);
    }
    let a = v.unsigned_abs();
    let cat = 32 - a.leading_zeros();
    let bits = if v < 0 { ((v - 1) as u32) & ((1u32 << cat) - 1) } else { v as u32 };
    (cat as u8, bits)
}

#[derive(Clone)]
struct Comp {
    id: u8,
    tq: u8,
    h: u8,
    v: u8,
    dc: u8,
    ac: u8,
}

trait Sink {
    fn dc(&mut self, table: usize, cat: u8, bits: u32);
    fn ac(&mut self, table: usize, sym: u8, cat: u8, bits: u32);
    fn restart(&mut self);
}

struct Counter {
    dc: [[u64; 256]; 4],
    ac: [[u64; 256]; 4],
}

impl Sink for Counter {
    fn dc(&mut self, table: usize, cat: u8, _: u32) {
        self.dc[table][cat as usize] += 1;
    }
    fn ac(&mut self, table: usize, sym: u8, _: u8, _: u32) {
        self.ac[table][sym as usize] += 1;
    }
    fn restart(&mut self) {}
}

struct Writer<'a> {
    dc: &'a [Option<Enc>; 4],
    ac: &'a [Option<Enc>; 4],
    out: Vec<u8>,
    acc: u64,
    n: u32,
    rst: u8,
}

impl Writer<'_> {
    fn put(&mut self, code: u32, size: u32) {
        if size == 0 {
            return;
        }
        self.acc = (self.acc << size) | code as u64;
        self.n += size;
        while self.n >= 8 {
            let byte = (self.acc >> (self.n - 8)) as u8;
            self.out.push(byte);
            if byte == 0xFF {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1u64 << self.n) - 1;
    }

    fn flush(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1u32 << pad) - 1, pad);
        }
    }
}

impl Sink for Writer<'_> {
    fn dc(&mut self, table: usize, cat: u8, bits: u32) {
        let (code, size) = {
            let t = self.dc[table].as_ref().expect("table checked before encoding");
            (t.code[cat as usize] as u32, t.size[cat as usize] as u32)
        };
        self.put(code, size);
        self.put(bits, cat as u32);
    }
    fn ac(&mut self, table: usize, sym: u8, cat: u8, bits: u32) {
        let (code, size) = {
            let t = self.ac[table].as_ref().expect("table checked before encoding");
            (t.code[sym as usize] as u32, t.size[sym as usize] as u32)
        };
        self.put(code, size);
        self.put(bits, cat as u32);
    }
    fn restart(&mut self) {
        self.flush();
        self.out.push(0xFF);
        self.out.push(0xD0 + (self.rst & 7));
        self.rst = self.rst.wrapping_add(1);
    }
}

fn be16(d: &[u8]) -> usize {
    ((d[0] as usize) << 8) | d[1] as usize
}

/// Result of reading the marker segments up to the start of the scan.
struct Header {
    width: u32,
    height: u32,
    comps: Vec<Comp>,
    restart: usize,
    dc_specs: [Option<Spec>; 4],
    ac_specs: [Option<Spec>; 4],
    dht_ranges: Vec<(usize, usize)>,
    dri_ranges: Vec<(usize, usize)>,
    sos_pos: usize,
    scan_start: usize,
    progressive: bool,
    sof_pos: usize,
    frame: Vec<(u8, u8, u8)>,
    tqs: Vec<u8>,
    qt: [Option<[u16; 64]>; 4],
    adobe: Option<u8>,
}

/// Walks the segments before the scan. `d` may be only the start of the file when probing.
fn read_header(d: &[u8]) -> Result<Header> {
    if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
        return Err(bad("not a JPEG file"));
    }
    let mut h = Header {
        width: 0,
        height: 0,
        comps: Vec::new(),
        restart: 0,
        dc_specs: Default::default(),
        ac_specs: Default::default(),
        dht_ranges: Vec::new(),
        dri_ranges: Vec::new(),
        sos_pos: 0,
        scan_start: 0,
        progressive: false,
        sof_pos: 0,
        frame: Vec::new(),
        tqs: Vec::new(),
        qt: [None; 4],
        adobe: None,
    };
    let mut sof: Vec<(u8, u8, u8, u8)> = Vec::new(); // id, h, v, tq
    let mut pos = 2usize;
    loop {
        if pos + 1 >= d.len() {
            return Err(bad("no image data found"));
        }
        if d[pos] != 0xFF {
            return Err(bad("damaged marker"));
        }
        while pos + 2 < d.len() && d[pos + 1] == 0xFF {
            pos += 1;
        }
        let m = d[pos + 1];
        let seg_start = pos;
        pos += 2;
        if m == 0x01 || (0xD0..=0xD8).contains(&m) {
            continue;
        }
        if m == 0xD9 {
            return Err(bad("no image data found"));
        }
        if pos + 2 > d.len() {
            return Err(bad("truncated file"));
        }
        let l = be16(&d[pos..]);
        if l < 2 || pos + l > d.len() {
            return Err(bad("truncated file"));
        }
        let body = &d[pos + 2..pos + l];
        let seg_end = pos + l;
        match m {
            0xC0 | 0xC1 | 0xC2 => {
                h.progressive = m == 0xC2;
                h.sof_pos = seg_start;
                if body.len() < 6 || body[0] != 8 {
                    return Err(unsupported("only 8-bit JPEG files are supported"));
                }
                h.height = be16(&body[1..3]) as u32;
                h.width = be16(&body[3..5]) as u32;
                let n = body[5] as usize;
                if h.width == 0 || h.height == 0 || !(1..=4).contains(&n) || body.len() < 6 + 3 * n {
                    return Err(bad("invalid frame header"));
                }
                sof.clear();
                for i in 0..n {
                    let c = &body[6 + 3 * i..];
                    let (hh, vv) = (c[1] >> 4, c[1] & 15);
                    if !(1..=4).contains(&hh) || !(1..=4).contains(&vv) {
                        return Err(bad("invalid sampling factors"));
                    }
                    sof.push((c[0], hh, vv, c[2]));
                }
                h.frame = sof.iter().map(|c| (c.0, c.1, c.2)).collect::<Vec<_>>();
                h.tqs = sof.iter().map(|c| c.3).collect();
                if n == 1 {
                    h.frame[0].1 = 1;
                    h.frame[0].2 = 1;
                }
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(unsupported("this JPEG type (lossless, hierarchical or arithmetic coded) is not supported")),
            0xC4 => {
                parse_dht(body, &mut h.dc_specs, &mut h.ac_specs)?;
                h.dht_ranges.push((seg_start, seg_end));
            }
            0xDB => {
                let mut p = 0;
                while p < body.len() {
                    let (pq, tq) = (body[p] >> 4, (body[p] & 15) as usize);
                    p += 1;
                    let n = if pq == 0 { 64 } else { 128 };
                    if tq > 3 || p + n > body.len() {
                        return Err(bad("corrupt quantization table"));
                    }
                    let mut t = [0u16; 64];
                    for (i, v) in t.iter_mut().enumerate() {
                        *v = if pq == 0 { body[p + i] as u16 } else { be16(&body[p + 2 * i..]) as u16 };
                    }
                    h.qt[tq] = Some(t);
                    p += n;
                }
            }
            0xEE => {
                if body.len() >= 12 && &body[..5] == b"Adobe" {
                    h.adobe = Some(body[11]);
                }
            }
            0xDD => {
                if body.len() < 2 {
                    return Err(bad("invalid restart interval"));
                }
                h.restart = be16(body);
                h.dri_ranges.push((seg_start, seg_end));
            }
            0xDA => {
                if sof.is_empty() {
                    return Err(bad("scan before frame header"));
                }
                if h.progressive {
                    h.sos_pos = seg_start;
                    h.scan_start = seg_end;
                    return Ok(h);
                }
                let ns = *body.first().ok_or_else(|| bad("invalid scan header"))? as usize;
                if body.len() < 1 + 2 * ns + 3 || ns != sof.len() {
                    return Err(unsupported("only single-scan JPEG files are supported"));
                }
                let (ss, se, ahal) = (body[1 + 2 * ns], body[2 + 2 * ns], body[3 + 2 * ns]);
                if ss != 0 || se != 63 || ahal != 0 {
                    return Err(unsupported("progressive JPEG files are not supported yet (baseline only)"));
                }
                h.comps.clear();
                for i in 0..ns {
                    let cs = body[1 + 2 * i];
                    let tt = body[2 + 2 * i];
                    let (_, hh, vv, tq) = *sof.iter().find(|c| c.0 == cs).ok_or_else(|| bad("scan refers to an unknown component"))?;
                    h.comps.push(Comp { id: cs, tq, h: hh, v: vv, dc: tt >> 4, ac: tt & 15 });
                }
                if h.comps.iter().any(|c| c.dc > 3 || c.ac > 3) {
                    return Err(bad("invalid table selector"));
                }
                h.sos_pos = seg_start;
                h.scan_start = seg_end;
                return Ok(h);
            }
            _ => {}
        }
        pos = seg_end;
    }
}

fn parse_dht(body: &[u8], dc: &mut [Option<Spec>; 4], ac: &mut [Option<Spec>; 4]) -> Result<()> {
    let mut p = 0;
    while p < body.len() {
        let tc = body[p] >> 4;
        let th = (body[p] & 15) as usize;
        if p + 17 > body.len() || tc > 1 || th > 3 {
            return Err(bad("corrupt Huffman table"));
        }
        let mut spec = Spec::default();
        spec.counts.copy_from_slice(&body[p + 1..p + 17]);
        let n: usize = spec.counts.iter().map(|&c| c as usize).sum();
        if p + 17 + n > body.len() {
            return Err(bad("corrupt Huffman table"));
        }
        spec.vals = body[p + 17..p + 17 + n].to_vec();
        if tc == 0 {
            dc[th] = Some(spec);
        } else {
            ac[th] = Some(spec);
        }
        p += 17 + n;
    }
    Ok(())
}

fn mcu_layout(h: &Header) -> (usize, usize) {
    if h.comps.len() == 1 {
        let bx = (h.width as usize).div_ceil(8);
        let by = (h.height as usize).div_ceil(8);
        return (bx * by, 1);
    }
    let hmax = h.comps.iter().map(|c| c.h as usize).max().unwrap();
    let vmax = h.comps.iter().map(|c| c.v as usize).max().unwrap();
    let mx = (h.width as usize).div_ceil(8 * hmax);
    let my = (h.height as usize).div_ceil(8 * vmax);
    (mx * my, h.comps.iter().map(|c| c.h as usize * c.v as usize).sum())
}

/// Cheap check used when listing folders: reads only the start of the file.
pub fn probe(path: &Path) -> Result<(u32, u32)> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(1 << 20).read_to_end(&mut buf)?;
    let h = read_header(&buf)?;
    Ok((h.width, h.height))
}

#[derive(Clone)]
pub struct Jpeg {
    pub width: u32,
    pub height: u32,
    head: Vec<u8>,
    tail: Vec<u8>,
    sos_pos: usize,
    dht_ranges: Vec<(usize, usize)>,
    dri_ranges: Vec<(usize, usize)>,
    comps: Vec<Comp>,
    restart: usize,
    dc_specs: [Option<Spec>; 4],
    ac_specs: [Option<Spec>; 4],
    mcus: usize,
    blocks: Vec<[i16; 64]>,
    progressive: bool,
    sof_pos: usize,
    qt: [Option<[u16; 64]>; 4],
    adobe: Option<u8>,
}

impl Jpeg {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let h = read_header(d)?;
        if h.progressive {
            return parse_progressive(d, h);
        }
        let (mcus, bpm) = mcu_layout(&h);
        let single = h.comps.len() == 1;
        let mut dec_dc: [Option<Dec>; 4] = Default::default();
        let mut dec_ac: [Option<Dec>; 4] = Default::default();
        for c in &h.comps {
            let (dc, ac) = (c.dc as usize, c.ac as usize);
            if dec_dc[dc].is_none() {
                dec_dc[dc] = Some(build_dec(h.dc_specs[dc].as_ref().ok_or_else(|| bad("missing DC Huffman table"))?)?);
            }
            if dec_ac[ac].is_none() {
                dec_ac[ac] = Some(build_dec(h.ac_specs[ac].as_ref().ok_or_else(|| bad("missing AC Huffman table"))?)?);
            }
        }
        if mcus.saturating_mul(bpm) > 64 * 1024 * 1024 {
            return Err(unsupported("image is too large"));
        }
        let mut br = Bits::new(&d[h.scan_start..]);
        let mut preds = [0i32; 4];
        let mut blocks: Vec<[i16; 64]> = Vec::with_capacity(mcus * bpm);
        for mcu in 0..mcus {
            if h.restart > 0 && mcu > 0 && mcu % h.restart == 0 {
                br.restart()?;
                preds = [0; 4];
            }
            for (ci, c) in h.comps.iter().enumerate() {
                let nb = if single { 1 } else { c.h as usize * c.v as usize };
                let (dct, act) = (dec_dc[c.dc as usize].as_ref().unwrap(), dec_ac[c.ac as usize].as_ref().unwrap());
                for _ in 0..nb {
                    let mut blk = [0i16; 64];
                    let t = br.huff(dct)? as u32;
                    if t > 15 {
                        return Err(bad("invalid DC category"));
                    }
                    let diff = if t == 0 { 0 } else { extend(br.take(t)?, t) };
                    preds[ci] += diff;
                    blk[0] = i16::try_from(preds[ci]).map_err(|_| bad("DC value out of range"))?;
                    let mut k = 1usize;
                    while k < 64 {
                        let rs = br.huff(act)?;
                        let (r, s) = ((rs >> 4) as usize, (rs & 15) as u32);
                        if s == 0 {
                            if r == 15 {
                                k += 16;
                                continue;
                            }
                            break;
                        }
                        k += r;
                        if k > 63 {
                            return Err(bad("coefficient index out of range"));
                        }
                        blk[k] = extend(br.take(s)?, s) as i16;
                        k += 1;
                    }
                    blocks.push(blk);
                }
            }
        }
        // The entropy-coded data ends at the next real marker.
        let mut j = h.scan_start + br.pos;
        while j + 1 < d.len() && !(d[j] == 0xFF && d[j + 1] != 0 && !(0xD0..=0xD7).contains(&d[j + 1])) {
            j += 1;
        }
        let j = j.min(d.len());
        Ok(Self {
            width: h.width,
            height: h.height,
            head: d[..h.scan_start].to_vec(),
            tail: d[j..].to_vec(),
            sos_pos: h.sos_pos,
            dht_ranges: h.dht_ranges,
            dri_ranges: h.dri_ranges,
            comps: h.comps,
            restart: h.restart,
            dc_specs: h.dc_specs,
            ac_specs: h.ac_specs,
            mcus,
            blocks,
            progressive: false,
            sof_pos: h.sof_pos,
            qt: h.qt,
            adobe: h.adobe,
        })
    }

    fn walk<S: Sink>(&self, restart: usize, s: &mut S) {
        let mut preds = [0i32; 4];
        let single = self.comps.len() == 1;
        let mut bi = 0usize;
        for mcu in 0..self.mcus {
            if restart > 0 && mcu > 0 && mcu % restart == 0 {
                s.restart();
                preds = [0; 4];
            }
            for (ci, c) in self.comps.iter().enumerate() {
                let nb = if single { 1 } else { c.h as usize * c.v as usize };
                for _ in 0..nb {
                    let blk = &self.blocks[bi];
                    bi += 1;
                    let diff = blk[0] as i32 - preds[ci];
                    preds[ci] = blk[0] as i32;
                    let (cat, bits) = cat_bits(diff);
                    s.dc(c.dc as usize, cat, bits);
                    let mut run = 0u8;
                    for &v in &blk[1..] {
                        if v == 0 {
                            run += 1;
                            continue;
                        }
                        while run > 15 {
                            s.ac(c.ac as usize, 0xF0, 0, 0);
                            run -= 16;
                        }
                        let (cat, bits) = cat_bits(v as i32);
                        s.ac(c.ac as usize, (run << 4) | cat, cat, bits);
                        run = 0;
                    }
                    if run > 0 {
                        s.ac(c.ac as usize, 0, 0, 0);
                    }
                }
            }
        }
    }

    /// Writes the file with the original Huffman tables when they still cover every symbol.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.encode(false, self.restart)
    }

    /// Writes the file with freshly optimised Huffman tables (and optionally another restart interval).
    pub fn to_bytes_optimized(&self, restart: usize) -> Result<Vec<u8>> {
        self.encode(true, restart)
    }


    /// Writes a progressive file (spectral selection only, optimal tables per scan), so a
    /// progressive cover stays progressive after embedding.
    fn encode_progressive(&self) -> Result<Vec<u8>> {
        struct Tok {
            table: u8,
            sym: u8,
            nb: u8,
            bits: u32,
        }
        let n = self.comps.len();
        let single = n == 1;
        let eff = |c: &Comp| if single { (1usize, 1usize) } else { (c.h as usize, c.v as usize) };
        let hmax = self.comps.iter().map(|c| eff(c).0).max().unwrap_or(1);
        let vmax = self.comps.iter().map(|c| eff(c).1).max().unwrap_or(1);
        let (w, h) = (self.width as usize, self.height as usize);
        let mcux = w.div_ceil(8 * hmax);
        let mcuy = h.div_ceil(8 * vmax);
        let bpm: usize = self.comps.iter().map(|c| eff(c).0 * eff(c).1).sum();
        if self.blocks.len() != mcux * mcuy * bpm {
            return Err(bad("the image blocks do not match the frame size"));
        }
        let mut offs = Vec::new();
        let mut acc = 0usize;
        for c in &self.comps {
            offs.push(acc);
            acc += eff(c).0 * eff(c).1;
        }

        let mut out = Vec::with_capacity(self.head.len() + self.blocks.len() * 30 + self.tail.len() + 2048);
        let mut cursor = 0usize;
        let mut strip: Vec<(usize, usize)> = self.dht_ranges.iter().chain(&self.dri_ranges).copied().collect();
        strip.sort();
        for (s, e) in strip {
            out.extend_from_slice(&self.head[cursor..s]);
            cursor = e;
        }
        out.extend_from_slice(&self.head[cursor..self.sos_pos]);

        // Emits one scan: tables, scan header, entropy-coded data.
        let emit = |out: &mut Vec<u8>, toks: &[Tok], class_of: &dyn Fn(&Tok) -> u8, comps: &[&Comp], ss: u8, se: u8| {
            let mut freq = [[[0u64; 256]; 4]; 2];
            for t in toks {
                freq[class_of(t) as usize][t.table as usize & 3][t.sym as usize] += 1;
            }
            let mut encs: [[Option<Enc>; 4]; 2] = Default::default();
            let mut seg = Vec::new();
            for class in 0..2usize {
                for id in 0..4usize {
                    if freq[class][id].iter().all(|&f| f == 0) {
                        continue;
                    }
                    let spec = gen_optimal(&freq[class][id]);
                    seg.push(((class as u8) << 4) | id as u8);
                    seg.extend_from_slice(&spec.counts);
                    seg.extend_from_slice(&spec.vals);
                    encs[class][id] = Some(build_enc(&spec));
                }
            }
            if !seg.is_empty() {
                out.extend_from_slice(&[0xFF, 0xC4]);
                out.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
                out.extend_from_slice(&seg);
            }
            out.extend_from_slice(&[0xFF, 0xDA]);
            out.extend_from_slice(&((6 + 2 * comps.len()) as u16).to_be_bytes());
            out.push(comps.len() as u8);
            for c in comps {
                out.push(c.id);
                out.push((c.dc << 4) | c.ac);
            }
            out.extend_from_slice(&[ss, se, 0]);
            let none: [Option<Enc>; 4] = Default::default();
            let mut wr = Writer { dc: &none, ac: &none, out: Vec::new(), acc: 0, n: 0, rst: 0 };
            for t in toks {
                let e = encs[class_of(t) as usize][t.table as usize & 3].as_ref().expect("table built from the same tokens");
                wr.put(e.code[t.sym as usize] as u32, e.size[t.sym as usize] as u32);
                wr.put(t.bits, t.nb as u32);
            }
            wr.flush();
            out.extend_from_slice(&wr.out);
        };

        // ---- DC scan, all components interleaved ----
        let mut toks: Vec<Tok> = Vec::with_capacity(self.blocks.len());
        let mut pred = vec![0i32; n];
        for m in 0..mcux * mcuy {
            for (ci, c) in self.comps.iter().enumerate() {
                let (bh, bv) = eff(c);
                for k in 0..bh * bv {
                    let dc = self.blocks[m * bpm + offs[ci] + k][0] as i32;
                    let (cat, bits) = cat_bits(dc - pred[ci]);
                    pred[ci] = dc;
                    toks.push(Tok { table: c.dc, sym: cat, nb: cat, bits });
                }
            }
        }
        let all: Vec<&Comp> = self.comps.iter().collect();
        emit(&mut out, &toks, &|_| 0, &all, 0, 0);

        // ---- AC scans, one component at a time, two spectral bands ----
        for (ci, c) in self.comps.iter().enumerate() {
            let (bh, bv) = eff(c);
            let cw = (w * bh).div_ceil(hmax).div_ceil(8);
            let ch = (h * bv).div_ceil(vmax).div_ceil(8);
            for (ss, se) in [(1usize, 5usize), (6, 63)] {
                let mut toks: Vec<Tok> = Vec::new();
                let mut eobrun = 0u32;
                let flush = |toks: &mut Vec<Tok>, eobrun: &mut u32| {
                    if *eobrun > 0 {
                        let nbits = 31 - eobrun.leading_zeros();
                        toks.push(Tok { table: c.ac, sym: (nbits << 4) as u8, nb: nbits as u8, bits: *eobrun - (1 << nbits) });
                        *eobrun = 0;
                    }
                };
                for y in 0..ch {
                    for x in 0..cw {
                        let (mx, hi, my, vi) = (x / bh, x % bh, y / bv, y % bv);
                        let blk = &self.blocks[(my * mcux + mx) * bpm + offs[ci] + vi * bh + hi];
                        let mut run = 0u32;
                        for k in ss..=se {
                            let v = blk[k] as i32;
                            if v == 0 {
                                run += 1;
                                continue;
                            }
                            flush(&mut toks, &mut eobrun);
                            while run > 15 {
                                toks.push(Tok { table: c.ac, sym: 0xF0, nb: 0, bits: 0 });
                                run -= 16;
                            }
                            let (cat, bits) = cat_bits(v);
                            toks.push(Tok { table: c.ac, sym: ((run << 4) as u8) | cat, nb: cat, bits });
                            run = 0;
                        }
                        if run > 0 {
                            eobrun += 1;
                            if eobrun == 0x7FFF {
                                flush(&mut toks, &mut eobrun);
                            }
                        }
                    }
                }
                flush(&mut toks, &mut eobrun);
                emit(&mut out, &toks, &|_| 1, &[c], ss as u8, se as u8);
            }
        }
        out.extend_from_slice(&self.tail);
        Ok(out)
    }

    fn encode(&self, force_optimal: bool, restart: usize) -> Result<Vec<u8>> {
        if self.progressive {
            return self.encode_progressive();
        }
        let mut counter = Counter { dc: [[0; 256]; 4], ac: [[0; 256]; 4] };
        self.walk(restart, &mut counter);

        let mut enc_dc: [Option<Enc>; 4] = Default::default();
        let mut enc_ac: [Option<Enc>; 4] = Default::default();
        let force_optimal = force_optimal || self.progressive;
        let mut covered = !force_optimal;
        if covered {
            for c in &self.comps {
                let (dc, ac) = (c.dc as usize, c.ac as usize);
                let (Some(sd), Some(sa)) = (&self.dc_specs[dc], &self.ac_specs[ac]) else {
                    covered = false;
                    break;
                };
                enc_dc[dc] = Some(build_enc(sd));
                enc_ac[ac] = Some(build_enc(sa));
            }
        }
        if covered {
            let ok = |freq: &[u64; 256], t: &Option<Enc>| t.as_ref().is_some_and(|t| (0..256).all(|s| freq[s] == 0 || t.size[s] > 0));
            for c in &self.comps {
                if !ok(&counter.dc[c.dc as usize], &enc_dc[c.dc as usize]) || !ok(&counter.ac[c.ac as usize], &enc_ac[c.ac as usize]) {
                    covered = false;
                }
            }
        }

        let mut new_dht: Option<Vec<u8>> = None;
        if !covered {
            enc_dc = Default::default();
            enc_ac = Default::default();
            let mut seg = Vec::new();
            let mut put_spec = |class: u8, id: usize, spec: &Spec| {
                seg.push((class << 4) | id as u8);
                seg.extend_from_slice(&spec.counts);
                seg.extend_from_slice(&spec.vals);
            };
            let mut done_dc = [false; 4];
            let mut done_ac = [false; 4];
            for c in &self.comps {
                let (dc, ac) = (c.dc as usize, c.ac as usize);
                if !done_dc[dc] {
                    let spec = gen_optimal(&counter.dc[dc]);
                    enc_dc[dc] = Some(build_enc(&spec));
                    put_spec(0, dc, &spec);
                    done_dc[dc] = true;
                }
                if !done_ac[ac] {
                    let spec = gen_optimal(&counter.ac[ac]);
                    enc_ac[ac] = Some(build_enc(&spec));
                    put_spec(1, ac, &spec);
                    done_ac[ac] = true;
                }
            }
            let mut out = vec![0xFF, 0xC4];
            out.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(&seg);
            new_dht = Some(out);
        }

        let mut w = Writer { dc: &enc_dc, ac: &enc_ac, out: Vec::with_capacity(self.blocks.len() * 40), acc: 0, n: 0, rst: 0 };
        self.walk(restart, &mut w);
        w.flush();
        let scan = w.out;

        // Assemble: original header, minus replaced segments, plus new ones just before the scan header.
        let mut strip: Vec<(usize, usize)> = Vec::new();
        if new_dht.is_some() {
            strip.extend(&self.dht_ranges);
        }
        let new_dri = restart != self.restart;
        if new_dri || self.progressive {
            strip.extend(&self.dri_ranges);
        }
        strip.sort();
        let mut out = Vec::with_capacity(self.head.len() + scan.len() + self.tail.len() + 600);
        let mut cursor = 0usize;
        for (s, e) in strip {
            out.extend_from_slice(&self.head[cursor..s]);
            cursor = e;
        }
        out.extend_from_slice(&self.head[cursor..self.sos_pos]);
        if self.progressive {
            // The frame header says "progressive"; the single scan written below is a baseline one.
            let p = self.sof_pos + 1;
            if let Some(b) = out.get_mut(p) {
                if *b == 0xC2 {
                    *b = 0xC0;
                }
            }
        }
        if let Some(dht) = &new_dht {
            out.extend_from_slice(dht);
        }
        if (new_dri || self.progressive) && restart > 0 {
            out.extend_from_slice(&[0xFF, 0xDD, 0, 4]);
            out.extend_from_slice(&(restart as u16).to_be_bytes());
        }
        if self.progressive {
            let n = self.comps.len();
            out.extend_from_slice(&[0xFF, 0xDA]);
            out.extend_from_slice(&((6 + 2 * n) as u16).to_be_bytes());
            out.push(n as u8);
            for c in &self.comps {
                out.push(c.id);
                out.push((c.dc << 4) | c.ac);
            }
            out.extend_from_slice(&[0, 63, 0]);
        } else {
            out.extend_from_slice(&self.head[self.sos_pos..]);
        }
        out.extend_from_slice(&scan);
        out.extend_from_slice(&self.tail);
        Ok(out)
    }

    /// Decodes the coefficients straight to RGB pixels (no re-encoding, no external decoder).
    /// Chroma is upsampled by replication, so colours at sharp edges can differ slightly from other viewers.
    /// Large images are split across the available cores.
    pub fn to_rgb(&self) -> Result<ImageCarrier> {
        let n = self.comps.len();
        if n != 1 && n != 3 {
            return Err(unsupported("a pixel preview needs a grayscale or 3-component JPEG"));
        }
        let single = n == 1;
        let eff = |c: &Comp| if single { (1usize, 1usize) } else { (c.h as usize, c.v as usize) };
        let hmax = self.comps.iter().map(|c| eff(c).0).max().unwrap();
        let vmax = self.comps.iter().map(|c| eff(c).1).max().unwrap();
        let (w, h) = (self.width as usize, self.height as usize);
        let mcux = w.div_ceil(8 * hmax);
        let mcuy = h.div_ceil(8 * vmax);
        let bpm: usize = self.comps.iter().map(|c| eff(c).0 * eff(c).1).sum();
        if self.blocks.len() != mcux * mcuy * bpm {
            return Err(bad("the image blocks do not match the frame size"));
        }
        let strides: Vec<usize> = self.comps.iter().map(|c| mcux * eff(c).0 * 8).collect();
        let band: Vec<usize> = self.comps.iter().map(|c| eff(c).1 * 8).collect(); // plane rows per MCU row
        let mut offs = Vec::with_capacity(n);
        let mut acc = 0usize;
        for c in &self.comps {
            offs.push(acc);
            acc += eff(c).0 * eff(c).1;
        }
        let mut qts: Vec<[i32; 64]> = Vec::with_capacity(n);
        for c in &self.comps {
            let q = self.qt[c.tq as usize & 3].as_ref().ok_or_else(|| bad("missing quantization table"))?;
            let mut t = [0i32; 64];
            for k in 0..64 {
                t[ZIGZAG[k]] = q[k] as i32; // natural order, to multiply coefficients as they are scattered
            }
            qts.push(t);
        }

        let threads = if self.blocks.len() < 6000 { 1 } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(16) };

        // ---- 1. dequantise + inverse DCT into one plane per component, one MCU row per work item ----
        let mut planes: Vec<Vec<u8>> = (0..n).map(|i| vec![0u8; strides[i] * band[i] * mcuy]).collect();
        let mut per_row: Vec<Vec<&mut [u8]>> = (0..mcuy).map(|_| Vec::with_capacity(n)).collect();
        for (ci, p) in planes.iter_mut().enumerate() {
            for (my, chunk) in p.chunks_mut(strides[ci] * band[ci]).enumerate() {
                per_row[my].push(chunk);
            }
        }
        let rows_per_thread = mcuy.div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            for (ti, group) in per_row.chunks_mut(rows_per_thread).enumerate() {
                let (comps, blocks, strides, offs, qts, eff) = (&self.comps, &self.blocks, &strides, &offs, &qts, &eff);
                scope.spawn(move || {
                    let mut deq = [0i32; 64];
                    for (gi, bands) in group.iter_mut().enumerate() {
                        let my = ti * rows_per_thread + gi;
                        for mx in 0..mcux {
                            for (ci, c) in comps.iter().enumerate() {
                                let (bh, bv) = eff(c);
                                for vi in 0..bv {
                                    for hi in 0..bh {
                                        let blk = &blocks[(my * mcux + mx) * bpm + offs[ci] + vi * bh + hi];
                                        let (x0, y0) = ((mx * bh + hi) * 8, vi * 8);
                                        let out = &mut bands[ci][y0 * strides[ci] + x0..];
                                        if blk[1..].iter().all(|&v| v == 0) {
                                            // Flat block: same arithmetic as the full transform, without the work.
                                            let dc = blk[0] as i32 * qts[ci][0];
                                            let v = (((dc * 16384 + 65536) >> 17) + 128).clamp(0, 255) as u8;
                                            for r in 0..8 {
                                                out[r * strides[ci]..r * strides[ci] + 8].fill(v);
                                            }
                                        } else {
                                            deq.fill(0);
                                            for k in 0..64 {
                                                if blk[k] != 0 {
                                                    deq[ZIGZAG[k]] = blk[k] as i32 * qts[ci][ZIGZAG[k]];
                                                }
                                            }
                                            idct_block(&deq, out, strides[ci]);
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });

        // ---- 2. upsample + colour convert, a band of output rows per thread ----
        let xs: Vec<Vec<usize>> = self.comps.iter().map(|c| (0..w).map(|x| x * eff(c).0 / hmax).collect()).collect();
        let rgb_direct = self.adobe == Some(0);
        let mut samples = vec![0u8; w * h * 3];
        let rows_per_chunk = h.div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            for (ti, chunk) in samples.chunks_mut(rows_per_chunk * w * 3).enumerate() {
                let (planes, strides, xs, comps, eff) = (&planes, &strides, &xs, &self.comps, &eff);
                scope.spawn(move || {
                    for (ri, row) in chunk.chunks_exact_mut(w * 3).enumerate() {
                        let y = ti * rows_per_chunk + ri;
                        let line = |ci: usize| &planes[ci][(y * eff(&comps[ci]).1 / vmax) * strides[ci]..];
                        if single {
                            let r = line(0);
                            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                                px.fill(r[xs[0][x]]);
                            }
                        } else {
                            let (r0, r1, r2) = (line(0), line(1), line(2));
                            for (x, px) in row.chunks_exact_mut(3).enumerate() {
                                let (a, b, c) = (r0[xs[0][x]] as i32, r1[xs[1][x]] as i32, r2[xs[2][x]] as i32);
                                if rgb_direct {
                                    px.copy_from_slice(&[a as u8, b as u8, c as u8]);
                                } else {
                                    let (cb, cr) = (b - 128, c - 128);
                                    px[0] = (a + ((91881 * cr + 32768) >> 16)).clamp(0, 255) as u8;
                                    px[1] = (a - ((22554 * cb + 46802 * cr + 32768) >> 16)).clamp(0, 255) as u8;
                                    px[2] = (a + ((116130 * cb + 32768) >> 16)).clamp(0, 255) as u8;
                                }
                            }
                        }
                    }
                });
            }
        });
        ImageCarrier::from_rgb(self.width, self.height, samples)
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The luminance blocks in raster order (zigzag coefficient order) with their quantization table.
    pub fn luma_coefs(&self) -> Result<LumaCoefs> {
        let n = self.comps.len();
        if n != 1 && n != 3 {
            return Err(unsupported("needs a grayscale or 3-component JPEG"));
        }
        let single = n == 1;
        let eff = |c: &Comp| if single { (1usize, 1usize) } else { (c.h as usize, c.v as usize) };
        let hmax = self.comps.iter().map(|c| eff(c).0).max().unwrap_or(1);
        let vmax = self.comps.iter().map(|c| eff(c).1).max().unwrap_or(1);
        let mcux = (self.width as usize).div_ceil(8 * hmax);
        let mcuy = (self.height as usize).div_ceil(8 * vmax);
        let bpm: usize = self.comps.iter().map(|c| eff(c).0 * eff(c).1).sum();
        if self.blocks.len() != mcux * mcuy * bpm {
            return Err(bad("the image blocks do not match the frame size"));
        }
        let (bh, bv) = eff(&self.comps[0]);
        let (gw, gh) = (mcux * bh, mcuy * bv);
        let q = self.qt[self.comps[0].tq as usize & 3].ok_or_else(|| bad("missing quantization table"))?;
        let mut blocks = vec![[0i16; 64]; gw * gh];
        for my in 0..mcuy {
            for mx in 0..mcux {
                for vi in 0..bv {
                    for hi in 0..bh {
                        blocks[(my * bv + vi) * gw + mx * bh + hi] = self.blocks[(my * mcux + mx) * bpm + vi * bh + hi];
                    }
                }
            }
        }
        Ok(LumaCoefs { gw, gh, q, blocks })
    }


    /// Per block: its component index and the local texture energy (UERD-style): the block's own
    /// summed dequantised AC magnitude plus a quarter of each neighbouring block's. Changes in
    /// busy areas are cheap, changes in flat areas are expensive.
    fn block_context(&self) -> Result<(Vec<u8>, Vec<f32>)> {
        let n = self.comps.len();
        let single = n == 1;
        let eff = |c: &Comp| if single { (1usize, 1usize) } else { (c.h as usize, c.v as usize) };
        let hmax = self.comps.iter().map(|c| eff(c).0).max().unwrap_or(1);
        let vmax = self.comps.iter().map(|c| eff(c).1).max().unwrap_or(1);
        let mcux = (self.width as usize).div_ceil(8 * hmax);
        let mcuy = (self.height as usize).div_ceil(8 * vmax);
        let bpm: usize = self.comps.iter().map(|c| eff(c).0 * eff(c).1).sum();
        if self.blocks.len() != mcux * mcuy * bpm {
            return Err(bad("the image blocks do not match the frame size"));
        }
        let mut comp_of = vec![0u8; self.blocks.len()];
        let mut denom = vec![0f32; self.blocks.len()];
        let mut off = 0usize;
        for (ci, c) in self.comps.iter().enumerate() {
            let (bh, bv) = eff(c);
            let q = self.qt[c.tq as usize & 3].as_ref().ok_or_else(|| bad("missing quantization table"))?;
            let (gw, gh) = (mcux * bh, mcuy * bv);
            let mut e = vec![0f32; gw * gh];
            let mut at = vec![0usize; gw * gh];
            for my in 0..mcuy {
                for mx in 0..mcux {
                    for vi in 0..bv {
                        for hi in 0..bh {
                            let bi = (my * mcux + mx) * bpm + off + vi * bh + hi;
                            let g = (my * bv + vi) * gw + mx * bh + hi;
                            let blk = &self.blocks[bi];
                            e[g] = (1..64).map(|k| blk[k].unsigned_abs() as f32 * q[k] as f32).sum();
                            at[g] = bi;
                        }
                    }
                }
            }
            for y in 0..gh {
                for x in 0..gw {
                    let mut d = e[y * gw + x];
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            if (dx, dy) == (0, 0) {
                                continue;
                            }
                            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                            if nx >= 0 && ny >= 0 && (nx as usize) < gw && (ny as usize) < gh {
                                d += 0.25 * e[ny as usize * gw + nx as usize];
                            }
                        }
                    }
                    let bi = at[y * gw + x];
                    comp_of[bi] = ci as u8;
                    denom[bi] = d;
                }
            }
            off += bh * bv;
        }
        Ok((comp_of, denom))
    }

    /// Cost of changing each listed coefficient (`block * 64 + zigzag index`) by one.
    pub fn change_costs(&self, positions: &[u32]) -> Result<Vec<f32>> {
        let (comp_of, denom) = self.block_context()?;
        let mut qs: Vec<[u16; 64]> = Vec::new();
        for c in &self.comps {
            qs.push(self.qt[c.tq as usize & 3].ok_or_else(|| bad("missing quantization table"))?);
        }
        Ok(positions
            .iter()
            .map(|&p| {
                let (b, k) = (p as usize / 64, p as usize % 64);
                qs[comp_of[b] as usize][k] as f32 / (denom[b] + 1.0)
            })
            .collect())
    }

}

struct ScanHeader {
    comps: Vec<usize>,
    td: Vec<u8>,
    ta: Vec<u8>,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
}

fn parse_scan_header(body: &[u8], frame: &[(u8, u8, u8)]) -> Result<ScanHeader> {
    let ns = *body.first().ok_or_else(|| bad("invalid scan header"))? as usize;
    if ns == 0 || ns > frame.len() || body.len() < 1 + 2 * ns + 3 {
        return Err(bad("invalid scan header"));
    }
    let mut sh = ScanHeader { comps: Vec::new(), td: Vec::new(), ta: Vec::new(), ss: body[1 + 2 * ns] as usize, se: body[2 + 2 * ns] as usize, ah: (body[3 + 2 * ns] >> 4) as u32, al: (body[3 + 2 * ns] & 15) as u32 };
    for i in 0..ns {
        let ci = frame.iter().position(|c| c.0 == body[1 + 2 * i]).ok_or_else(|| bad("scan refers to an unknown component"))?;
        if sh.comps.contains(&ci) {
            return Err(bad("component repeated in a scan"));
        }
        sh.comps.push(ci);
        sh.td.push(body[2 + 2 * i] >> 4);
        sh.ta.push(body[2 + 2 * i] & 15);
    }
    if sh.td.iter().chain(sh.ta.iter()).any(|&t| t > 3) || sh.al > 13 || sh.ah > 13 {
        return Err(bad("invalid scan parameters"));
    }
    let dc_scan = sh.ss == 0;
    if (dc_scan && sh.se != 0) || (!dc_scan && (sh.se < sh.ss || sh.se > 63 || ns != 1)) {
        return Err(bad("invalid spectral selection"));
    }
    Ok(sh)
}

fn refine_existing(br: &mut Bits, c: &mut i16, p1: i32, m1: i32) -> Result<()> {
    if br.take(1)? == 1 && (*c as i32 & p1) == 0 {
        *c = if *c >= 0 { *c as i32 + p1 } else { *c as i32 + m1 } as i16;
    }
    Ok(())
}

fn ac_first(br: &mut Bits, dec: &Dec, blk: &mut [i16; 64], ss: usize, se: usize, al: u32, eobrun: &mut u32) -> Result<()> {
    if *eobrun > 0 {
        *eobrun -= 1;
        return Ok(());
    }
    let mut k = ss;
    while k <= se {
        let rs = br.huff(dec)?;
        let (r, s) = ((rs >> 4) as usize, (rs & 15) as u32);
        if s != 0 {
            k += r;
            if k > se {
                return Err(bad("coefficient index out of range"));
            }
            blk[k] = (extend(br.take(s)?, s) << al) as i16;
        } else if r == 15 {
            k += 15;
        } else {
            *eobrun = 1 << r;
            if r > 0 {
                *eobrun += br.take(r as u32)?;
            }
            *eobrun -= 1;
            break;
        }
        k += 1;
    }
    Ok(())
}

fn ac_refine(br: &mut Bits, dec: &Dec, blk: &mut [i16; 64], ss: usize, se: usize, al: u32, eobrun: &mut u32) -> Result<()> {
    let p1 = 1i32 << al;
    let m1 = -(1i32 << al);
    let mut k = ss;
    if *eobrun == 0 {
        while k <= se {
            let rs = br.huff(dec)?;
            let mut r = (rs >> 4) as i32;
            let s = rs & 15;
            let mut val = 0i32;
            if s != 0 {
                if s != 1 {
                    return Err(bad("invalid refinement symbol"));
                }
                val = if br.take(1)? == 1 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = 1 << r;
                if r > 0 {
                    *eobrun += br.take(r as u32)?;
                }
                break;
            }
            // Skip over already-nonzero coefficients (refining them) and r still-zero ones.
            loop {
                if blk[k] != 0 {
                    refine_existing(br, &mut blk[k], p1, m1)?;
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
                if k > se {
                    break;
                }
            }
            if s != 0 {
                if k > se {
                    return Err(bad("coefficient index out of range"));
                }
                blk[k] = val as i16;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= se {
            if blk[k] != 0 {
                refine_existing(br, &mut blk[k], p1, m1)?;
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

fn parse_progressive(d: &[u8], h: Header) -> Result<Jpeg> {
    let frame = h.frame.clone();
    let n = frame.len();
    let hmax = frame.iter().map(|c| c.1 as usize).max().unwrap();
    let vmax = frame.iter().map(|c| c.2 as usize).max().unwrap();
    let (w, ht) = (h.width as usize, h.height as usize);
    let mcux = w.div_ceil(8 * hmax);
    let mcuy = ht.div_ceil(8 * vmax);
    let bpm: usize = frame.iter().map(|c| c.1 as usize * c.2 as usize).sum();
    if mcux.saturating_mul(mcuy).saturating_mul(bpm) > 64 * 1024 * 1024 {
        return Err(unsupported("image is too large"));
    }
    // Per component: padded storage grid and the real block grid used by single-component scans.
    let pbw: Vec<usize> = frame.iter().map(|c| mcux * c.1 as usize).collect();
    let pbh: Vec<usize> = frame.iter().map(|c| mcuy * c.2 as usize).collect();
    let rbw: Vec<usize> = frame.iter().map(|c| (w * c.1 as usize).div_ceil(hmax).div_ceil(8)).collect();
    let rbh: Vec<usize> = frame.iter().map(|c| (ht * c.2 as usize).div_ceil(vmax).div_ceil(8)).collect();
    let mut coefs: Vec<Vec<[i16; 64]>> = (0..n).map(|i| vec![[0i16; 64]; pbw[i] * pbh[i]]).collect();

    let mut dc_specs = h.dc_specs.clone();
    let mut ac_specs = h.ac_specs.clone();
    let mut restart = h.restart;
    let mut pos = h.sos_pos;
    let mut scans = 0usize;
    let tail_start;
    'scans: loop {
        // pos is at an SOS marker.
        if pos + 4 > d.len() || d[pos] != 0xFF || d[pos + 1] != 0xDA {
            return Err(bad("damaged scan header"));
        }
        let l = be16(&d[pos + 2..]);
        if l < 2 || pos + 2 + l > d.len() {
            return Err(bad("truncated file"));
        }
        let sh = parse_scan_header(&d[pos + 4..pos + 2 + l], &frame)?;
        let scan_start = pos + 2 + l;

        let mut dec_dc: [Option<Dec>; 4] = Default::default();
        let mut dec_ac: [Option<Dec>; 4] = Default::default();
        let dc_scan = sh.ss == 0;
        for i in 0..sh.comps.len() {
            if dc_scan && sh.ah == 0 {
                let t = sh.td[i] as usize;
                if dec_dc[t].is_none() {
                    dec_dc[t] = Some(build_dec(dc_specs[t].as_ref().ok_or_else(|| bad("missing DC Huffman table"))?)?);
                }
            }
            if !dc_scan {
                let t = sh.ta[i] as usize;
                if dec_ac[t].is_none() {
                    dec_ac[t] = Some(build_dec(ac_specs[t].as_ref().ok_or_else(|| bad("missing AC Huffman table"))?)?);
                }
            }
        }

        let mut br = Bits::new(&d[scan_start..]);
        let mut preds = [0i32; 4];
        let mut eobrun = 0u32;
        let single = sh.comps.len() == 1;
        let nmcu = if single { rbw[sh.comps[0]] * rbh[sh.comps[0]] } else { mcux * mcuy };
        for mcu in 0..nmcu {
            if restart > 0 && mcu > 0 && mcu % restart == 0 {
                br.restart()?;
                preds = [0; 4];
                eobrun = 0;
            }
            for (si, &ci) in sh.comps.iter().enumerate() {
                let (bh, bv) = if single { (1, 1) } else { (frame[ci].1 as usize, frame[ci].2 as usize) };
                for vi in 0..bv {
                    for hi in 0..bh {
                        let idx = if single {
                            (mcu / rbw[ci]) * pbw[ci] + (mcu % rbw[ci])
                        } else {
                            ((mcu / mcux) * bv + vi) * pbw[ci] + (mcu % mcux) * bh + hi
                        };
                        let blk = &mut coefs[ci][idx];
                        if dc_scan {
                            if sh.ah == 0 {
                                let t = br.huff(dec_dc[sh.td[si] as usize].as_ref().unwrap())? as u32;
                                if t > 15 {
                                    return Err(bad("invalid DC category"));
                                }
                                let diff = if t == 0 { 0 } else { extend(br.take(t)?, t) };
                                preds[ci] += diff;
                                blk[0] = i16::try_from(preds[ci] << sh.al).map_err(|_| bad("DC value out of range"))?;
                            } else if br.take(1)? == 1 {
                                blk[0] |= 1 << sh.al;
                            }
                        } else {
                            let dec = dec_ac[sh.ta[si] as usize].as_ref().unwrap();
                            if sh.ah == 0 {
                                ac_first(&mut br, dec, blk, sh.ss, sh.se, sh.al, &mut eobrun)?;
                            } else {
                                ac_refine(&mut br, dec, blk, sh.ss, sh.se, sh.al, &mut eobrun)?;
                            }
                        }
                    }
                }
            }
        }
        scans += 1;
        if scans > 200 {
            return Err(bad("too many scans"));
        }

        // Skip to the next marker, then read any table updates until the next scan or the end.
        let mut j = scan_start + br.pos;
        while j + 1 < d.len() && !(d[j] == 0xFF && d[j + 1] != 0 && !(0xD0..=0xD7).contains(&d[j + 1])) {
            j += 1;
        }
        loop {
            if j + 1 >= d.len() {
                tail_start = d.len();
                break 'scans;
            }
            if d[j] != 0xFF {
                return Err(bad("damaged marker"));
            }
            match d[j + 1] {
                0xFF => j += 1,
                0xD9 => {
                    tail_start = j;
                    break 'scans;
                }
                0xDA => {
                    pos = j;
                    continue 'scans;
                }
                0x01 | 0xD0..=0xD8 => j += 2,
                m => {
                    if j + 4 > d.len() {
                        return Err(bad("truncated file"));
                    }
                    let l = be16(&d[j + 2..]);
                    if l < 2 || j + 2 + l > d.len() {
                        return Err(bad("truncated file"));
                    }
                    let body = &d[j + 4..j + 2 + l];
                    match m {
                        0xC4 => parse_dht(body, &mut dc_specs, &mut ac_specs)?,
                        0xDD if body.len() >= 2 => restart = be16(body),
                        _ => {}
                    }
                    j += 2 + l;
                }
            }
        }
    }

    // Re-order into the interleaved block sequence of a baseline scan.
    let comps: Vec<Comp> = frame
        .iter()
        .enumerate()
        .map(|(i, c)| Comp { id: c.0, tq: h.tqs[i], h: c.1, v: c.2, dc: (i > 0) as u8, ac: (i > 0) as u8 })
        .collect();
    let single = n == 1;
    let mut blocks: Vec<[i16; 64]> = Vec::with_capacity(mcux * mcuy * bpm);
    for my in 0..mcuy {
        for mx in 0..mcux {
            for (ci, c) in comps.iter().enumerate() {
                let (bh, bv) = if single { (1, 1) } else { (c.h as usize, c.v as usize) };
                for vi in 0..bv {
                    for hi in 0..bh {
                        blocks.push(coefs[ci][(my * bv + vi) * pbw[ci] + mx * bh + hi]);
                    }
                }
            }
        }
    }
    Ok(Jpeg {
        width: h.width,
        height: h.height,
        head: d[..h.sos_pos].to_vec(),
        tail: d[tail_start.min(d.len())..].to_vec(),
        sos_pos: h.sos_pos,
        dht_ranges: h.dht_ranges,
        dri_ranges: h.dri_ranges,
        comps,
        restart: 0,
        dc_specs: Default::default(),
        ac_specs: Default::default(),
        mcus: mcux * mcuy,
        blocks,
        progressive: true,
        sof_pos: h.sof_pos,
        qt: h.qt,
        adobe: h.adobe,
    })
}

/// Luminance coefficients laid out as a block grid.
pub struct LumaCoefs {
    pub gw: usize,
    pub gh: usize,
    pub q: [u16; 64],
    pub blocks: Vec<[i16; 64]>,
}

/// A JPEG whose non-zero AC coefficients are the carrier samples.
#[derive(Clone)]
pub struct JpegCarrier {
    jpeg: Jpeg,
    idx: Vec<u32>,
}

impl JpegCarrier {
    pub fn from_bytes(d: &[u8]) -> Result<Self> {
        let jpeg = Jpeg::parse(d)?;
        let mut idx = Vec::new();
        for (b, blk) in jpeg.blocks.iter().enumerate() {
            for (k, &v) in blk.iter().enumerate().skip(1) {
                if v != 0 {
                    idx.push((b * 64 + k) as u32);
                }
            }
        }
        Ok(Self { jpeg, idx })
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.jpeg.to_bytes()
    }

    pub fn jpeg(&self) -> &Jpeg {
        &self.jpeg
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_bytes()?)?;
        Ok(())
    }

    pub fn width(&self) -> u32 {
        self.jpeg.width
    }

    pub fn height(&self) -> u32 {
        self.jpeg.height
    }

    /// Decoded pixels of the current coefficients, for previews and comparisons.
    pub fn pixels(&self) -> Result<ImageCarrier> {
        self.jpeg.to_rgb()
    }
}

impl Carrier for JpegCarrier {
    fn len(&self) -> usize {
        self.idx.len()
    }
    #[inline]
    fn get(&self, i: usize) -> i32 {
        let p = self.idx[i] as usize;
        self.jpeg.blocks[p / 64][p % 64] as i32
    }
    #[inline]
    fn set(&mut self, i: usize, v: i32) {
        let p = self.idx[i] as usize;
        let slot = &mut self.jpeg.blocks[p / 64][p % 64];
        // A coefficient must stay non-zero or the position list would change. Zero has the same parity as 2.
        let v = if v == 0 { if *slot < 0 { -2 } else { 2 } } else { v };
        *slot = v as i16;
    }
    fn bounds(&self) -> (i32, i32) {
        (-1023, 1023)
    }
    fn adaptive(&self) -> bool {
        true
    }
    fn costs(&self) -> Option<Vec<f32>> {
        self.jpeg.change_costs(&self.idx).ok()
    }
    fn geometry(&self) -> Vec<u8> {
        let mut g = b"jpg".to_vec();
        g.extend_from_slice(&self.jpeg.width.to_le_bytes());
        g.extend_from_slice(&self.jpeg.height.to_le_bytes());
        g.extend_from_slice(&(self.idx.len() as u64).to_le_bytes());
        g
    }
}
