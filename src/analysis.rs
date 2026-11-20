//! Classical LSB steganalysis for 8-bit image carriers.
//!
//! * chi-square (Westfeld & Pfitzmann 2000): histogram pair equalisation
//! * RS analysis (Fridrich, Goljan, Du 2001): regular/singular group statistics
//! * SPA (Dumitrescu, Wu, Wang 2003): sample pair analysis
//!
//! These estimate how visible an embedding is to *classical* detectors. They do not
//! model modern feature-based classifiers (SRM + ensemble), so a low score is not proof of safety.

use crate::carrier::ImageCarrier;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// Westfeld p-value of embedding; unreliable on images with smooth histograms.
    pub chi_p: f64,
    /// RS-estimated fraction of samples carrying a message (about 1.0 = full-rate LSB replacement).
    pub rs: f64,
    /// SPA-estimated fraction.
    pub spa: f64,
    /// 0..100, derived from max(rs, spa).
    pub score: f64,
    pub risk: Risk,
}

pub fn analyze(img: &ImageCarrier) -> Report {
    let chi_p = chi_square(&img.samples);
    let planes = img.planes();
    let w = img.width as usize;
    let rs = planes.iter().map(|p| rs_estimate(p, w)).sum::<f64>() / 3.0;
    let spa = planes.iter().map(|p| spa_estimate(p, w)).sum::<f64>() / 3.0;
    let score = 100.0 * rs.max(spa).clamp(0.0, 1.0);
    let risk = if score < 5.0 {
        Risk::Low
    } else if score < 20.0 {
        Risk::Medium
    } else {
        Risk::High
    };
    Report { chi_p, rs, spa, score, risk }
}

pub fn chi_square(samples: &[u8]) -> f64 {
    let mut h = [0u64; 256];
    for &s in samples {
        h[s as usize] += 1;
    }
    let mut chi = 0.0;
    let mut bins = 0usize;
    for k in 0..128 {
        let (a, b) = (h[2 * k] as f64, h[2 * k + 1] as f64);
        let e = (a + b) / 2.0;
        if e >= 5.0 {
            chi += (a - e) * (a - e) / e;
            bins += 1;
        }
    }
    if bins < 2 {
        return 0.0;
    }
    gamma_q((bins - 1) as f64 / 2.0, chi / 2.0)
}

fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 6] = [
        76.18009172947146,
        -86.50532032941677,
        24.01409824083091,
        -1.231739572450155,
        0.1208650973866179e-2,
        -0.5395239384953e-5,
    ];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015;
    for g in G {
        y += 1.0;
        ser += g / y;
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}

/// Regularised upper incomplete gamma function Q(a, x).
pub fn gamma_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    if x < a + 1.0 {
        let mut ap = a;
        let mut sum = 1.0 / a;
        let mut del = sum;
        for _ in 0..1000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-14 {
                break;
            }
        }
        1.0 - sum * (-x + a * x.ln() - ln_gamma(a)).exp()
    } else {
        let tiny = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / tiny;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..1000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < tiny {
                d = tiny;
            }
            c = b + an / c;
            if c.abs() < tiny {
                c = tiny;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-14 {
                break;
            }
        }
        (-x + a * x.ln() - ln_gamma(a)).exp() * h
    }
}

#[inline]
fn flip_pos(v: i32) -> i32 {
    v ^ 1
}

#[inline]
fn flip_neg(v: i32) -> i32 {
    ((v + 1) ^ 1) - 1
}

#[inline]
fn disc(g: &[i32; 4]) -> i32 {
    (g[1] - g[0]).abs() + (g[2] - g[1]).abs() + (g[3] - g[2]).abs()
}

/// Returns (R_M - S_M, R_-M - S_-M) as fractions of the group count, for mask [0,1,1,0].
fn rs_counts(plane: &[u8], flipped: bool) -> (f64, f64, usize) {
    let (mut rm, mut sm, mut rn, mut sn, mut total) = (0i64, 0i64, 0i64, 0i64, 0usize);
    for ch in plane.chunks_exact(4) {
        let mut g = [ch[0] as i32, ch[1] as i32, ch[2] as i32, ch[3] as i32];
        if flipped {
            for v in &mut g {
                *v = flip_pos(*v);
            }
        }
        let f0 = disc(&g);
        let pm = [g[0], flip_pos(g[1]), flip_pos(g[2]), g[3]];
        let nm = [g[0], flip_neg(g[1]), flip_neg(g[2]), g[3]];
        let (fp, fneg) = (disc(&pm), disc(&nm));
        if fp > f0 {
            rm += 1;
        } else if fp < f0 {
            sm += 1;
        }
        if fneg > f0 {
            rn += 1;
        } else if fneg < f0 {
            sn += 1;
        }
        total += 1;
    }
    let t = total.max(1) as f64;
    ((rm - sm) as f64 / t, (rn - sn) as f64 / t, total)
}

/// RS analysis on one plane. Groups are 4 consecutive samples; `width` is unused
/// except to document that rows are contiguous (groups may span a row boundary).
pub fn rs_estimate(plane: &[u8], _width: usize) -> f64 {
    let (d0, dm0, total) = rs_counts(plane, false);
    if total == 0 {
        return 0.0;
    }
    let (d1, dm1, _) = rs_counts(plane, true);
    let a = 2.0 * (d1 + d0);
    let b = dm0 - dm1 - d1 - 3.0 * d0;
    let c = d0 - dm0;
    let p_of = |x: f64| {
        let d = x - 0.5;
        if d.abs() < 1e-12 { 1.0 } else { x / d }
    };
    let mut complex = false;
    let roots: Vec<f64> = if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            return 0.0;
        }
        vec![-c / b]
    } else {
        // Near full embedding sampling noise can push the discriminant slightly negative;
        // treat that as a double root instead of discarding the estimate.
        let disc = b * b - 4.0 * a * c;
        complex = disc < 0.0;
        let s = disc.max(0.0).sqrt();
        vec![(-b + s) / (2.0 * a), (-b - s) / (2.0 * a)]
    };
    // Fridrich picks the root of smaller magnitude. Roots in (0, 0.5) map to a negative
    // message length, which is impossible, so only feasible roots compete.
    let feasible = |x: &&f64| p_of(**x) >= -0.1;
    let mut sorted = roots.clone();
    sorted.sort_by(|u, v| u.abs().total_cmp(&v.abs()));
    match sorted.iter().find(feasible) {
        Some(&x) => p_of(x).min(1.0),
        // No real, feasible root: the fitted model is inconsistent with any message length
        // below one, which is what saturation (about 100% embedding) looks like.
        None if complex => 1.0,
        None => p_of(sorted[0]).clamp(0.0, 1.0),
    }
}

/// Sample pair analysis on one plane using horizontal neighbours.
pub fn spa_estimate(plane: &[u8], width: usize) -> f64 {
    if width < 2 {
        return 0.0;
    }
    let (mut x, mut y, mut k, mut n) = (0f64, 0f64, 0f64, 0f64);
    for row in plane.chunks_exact(width) {
        for w in row.windows(2) {
            let (u, v) = (w[0] as i32, w[1] as i32);
            n += 1.0;
            if (v % 2 == 0 && u < v) || (v % 2 == 1 && u > v) {
                x += 1.0;
            }
            if (v % 2 == 0 && u > v) || (v % 2 == 1 && u < v) {
                y += 1.0;
            }
            if (u >> 1) == (v >> 1) {
                k += 1.0;
            }
        }
    }
    if k == 0.0 {
        return 0.0;
    }
    let a = 2.0 * k;
    let b = 2.0 * (2.0 * x - n);
    let c = y - x;
    let disc = (b * b - 4.0 * a * c).max(0.0);
    let s = disc.sqrt();
    let (r1, r2) = ((-b + s) / (2.0 * a), (-b - s) / (2.0 * a));
    // The root is the fraction of samples whose LSB was flipped; random message bits flip
    // half of the carrying samples, so the relative message length is twice that.
    2.0 * if r1.abs() < r2.abs() { r1 } else { r2 }
}

// ---------------------------------------------------------------------------------------------
// JPEG: calibrated coefficient-histogram detector (Fridrich et al. 2003, adapted)
// ---------------------------------------------------------------------------------------------

/// Forward 8x8 DCT of a level-shifted block (JPEG scaling), natural order output.
fn fdct8(px: &[f32; 64], out: &mut [f32; 64]) {
    use std::sync::OnceLock;
    static T: OnceLock<[[f32; 8]; 8]> = OnceLock::new();
    let t = T.get_or_init(|| {
        let mut t = [[0f32; 8]; 8];
        for (u, row) in t.iter_mut().enumerate() {
            let cu = if u == 0 { (0.5f64).sqrt() } else { 1.0 };
            for (x, v) in row.iter_mut().enumerate() {
                *v = (0.5 * cu * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos()) as f32;
            }
        }
        t
    });
    let mut tmp = [0f32; 64];
    for y in 0..8 {
        for u in 0..8 {
            tmp[y * 8 + u] = (0..8).map(|x| px[y * 8 + x] * t[u][x]).sum();
        }
    }
    for u in 0..8 {
        for v in 0..8 {
            out[v * 8 + u] = (0..8).map(|y| tmp[y * 8 + u] * t[v][y]).sum();
        }
    }
}

/// Estimated fraction of non-zero AC coefficients that were changed by +-1 embedding.
///
/// The image is decoded, cropped by four pixels (which shifts it off the 8x8 grid), and
/// re-quantised with the same table. That gives a prediction of the unmodified coefficient
/// histogram, and the difference between it and the file's own histogram is fitted to the
/// pattern an embedder that nudges magnitudes by one leaves behind.
pub fn jpeg_change_estimate(j: &crate::jpeg::JpegCarrier) -> Option<f64> {
    let jpeg = j.jpeg();
    let (w, h) = (jpeg.width as usize, jpeg.height as usize);
    if w < 40 || h < 40 {
        return None;
    }
    let luma = jpeg.luma_coefs().ok()?;
    let img = jpeg.to_rgb().ok()?;
    let y: Vec<f32> = img.samples.chunks_exact(3).map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).collect();
    let (nbx, nby) = ((w - 4) / 8, (h - 4) / 8);
    const MODES: usize = 15; // low-frequency zigzag positions 1..=15 carry most of the signal
    const MAXD: usize = 6;
    let mut hs = [[0f64; MAXD + 2]; MODES + 1];
    let mut hc = [[0f64; MAXD + 2]; MODES + 1];
    let mut count = 0f64;
    let mut px = [0f32; 64];
    let mut co = [0f32; 64];
    for by in 0..nby.min(luma.gh) {
        for bx in 0..nbx.min(luma.gw) {
            let blk = &luma.blocks[by * luma.gw + bx];
            for k in 1..=MODES {
                let d = (blk[k].unsigned_abs() as usize).min(MAXD + 1);
                hs[k][d] += 1.0;
            }
            for r in 0..8 {
                for c in 0..8 {
                    px[r * 8 + c] = y[(by * 8 + 4 + r) * w + bx * 8 + 4 + c] - 128.0;
                }
            }
            fdct8(&px, &mut co);
            for k in 1..=MODES {
                let v = co[crate::jpeg::ZIGZAG[k]] / luma.q[k] as f32;
                let d = (v.abs().round() as usize).min(MAXD + 1);
                hc[k][d] += 1.0;
            }
            count += 1.0;
        }
    }
    if count < 200.0 {
        return None;
    }
    let (mut num, mut den) = (0f64, 0f64);
    for k in 1..=MODES {
        let p: Vec<f64> = hc[k].iter().map(|v| v / count).collect();
        let s: Vec<f64> = hs[k].iter().map(|v| v / count).collect();
        for d in 1..=MAXD - 1 {
            let delta = match d {
                1 => 0.5 * p[2] - p[1],
                2 => p[1] + 0.5 * p[3] - p[2],
                _ => 0.5 * (p[d - 1] + p[d + 1]) - p[d],
            };
            num += (s[d] - p[d]) * delta;
            den += delta * delta;
        }
    }
    if den <= 0.0 {
        return None;
    }
    Some((num / den).clamp(-1.0, 1.0))
}

/// Report for a JPEG using the calibrated-histogram estimate. `rs` and `spa` both carry that
/// estimate (the fraction of altered coefficients); `chi_p` is not computed.
pub fn analyze_jpeg(j: &crate::jpeg::JpegCarrier) -> Option<Report> {
    let est = jpeg_change_estimate(j)?;
    // Clean files read within about +-0.03 of zero (calibration error); filling a file to capacity reads about 0.5.
    let score = (100.0 * est * 2.0).clamp(0.0, 100.0);
    let risk = if est < 0.06 {
        Risk::Low
    } else if est < 0.15 {
        Risk::Medium
    } else {
        Risk::High
    };
    Some(Report { chi_p: 1.0, rs: est, spa: est, score, risk })
}
