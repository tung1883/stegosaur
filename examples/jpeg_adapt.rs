//! Developer check: plain keyed matching vs the cost-aware trellis embedding on a JPEG.
//! Usage: jpeg_adapt <file.jpg>
use stegosaur::container::*;
use stegosaur::jpeg::JpegCarrier;
use stegosaur::Carrier;

struct Plain(JpegCarrier);
impl Carrier for Plain {
    fn len(&self) -> usize { self.0.len() }
    fn get(&self, i: usize) -> i32 { self.0.get(i) }
    fn set(&mut self, i: usize, v: i32) { self.0.set(i, v) }
    fn bounds(&self) -> (i32, i32) { self.0.bounds() }
    fn geometry(&self) -> Vec<u8> { self.0.geometry() }
}

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let orig = JpegCarrier::load(std::path::Path::new(&path)).unwrap();
    let cfg = Config::default();
    let cap = single_capacity(&orig, &cfg);
    let base = orig.pixels().unwrap();
    let costs = orig.costs().unwrap();
    println!("{} usable coefficients, capacity {} bytes", orig.len(), cap);
    println!("{:>10} | {:>22} | {:>22}", "payload", "plain: changes / cost / px err", "trellis: changes / cost / px err");
    for frac in [0.02f64, 0.1, 0.3, 0.7, 1.0] {
        let n = ((cap as f64) * frac) as usize;
        let payload: Vec<u8> = (0..n).map(|i| (i * 131 % 251) as u8).collect();
        let mut row = Vec::new();
        for adaptive in [false, true] {
            let (changes, cost, err) = if adaptive {
                let mut c = orig.clone();
                embed_single(&mut c, b"pw", &payload, &cfg).unwrap();
                assert_eq!(extract_single(&c, b"pw", &cfg).unwrap(), payload);
                measure(&orig, &c, &costs, &base)
            } else {
                let mut c = Plain(orig.clone());
                embed_single(&mut c, b"pw", &payload, &cfg).unwrap();
                assert_eq!(extract_single(&c, b"pw", &cfg).unwrap(), payload);
                measure(&orig, &c.0, &costs, &base)
            };
            row.push(format!("{:>8} / {:>7.1} / {:.4}", changes, cost, err));
        }
        println!("{:>8} B | {} | {}", n, row[0], row[1]);
    }
}

fn measure(orig: &JpegCarrier, now: &JpegCarrier, costs: &[f32], base: &stegosaur::ImageCarrier) -> (usize, f64, f64) {
    let mut ch = 0;
    let mut cost = 0f64;
    for i in 0..orig.len() {
        if orig.get(i) != now.get(i) {
            ch += 1;
            cost += costs[i] as f64;
        }
    }
    let px = now.pixels().unwrap();
    let err = px.samples.iter().zip(&base.samples).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / base.samples.len() as f64;
    (ch, cost, err)
}
