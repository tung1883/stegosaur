//! Developer check of the JPEG detector. Usage: jpeg_detect <file.jpg>...
use stegosaur::analysis::jpeg_change_estimate;
use stegosaur::container::*;
use stegosaur::jpeg::JpegCarrier;
use stegosaur::Carrier;
fn main() {
    let cfg = Config::default();
    for path in std::env::args().skip(1) {
        let orig = JpegCarrier::load(std::path::Path::new(&path)).unwrap();
        let cap = single_capacity(&orig, &cfg);
        print!("{:<28} {:>6} coefs | clean {:+.4}", path.rsplit('/').next().unwrap(), orig.len(), jpeg_change_estimate(&orig).unwrap_or(f64::NAN));
        for frac in [0.05f64, 0.2, 0.5, 1.0] {
            let n = ((cap as f64) * frac) as usize;
            let payload: Vec<u8> = (0..n).map(|i| (i * 131 % 251) as u8).collect();
            let mut c = orig.clone();
            embed_single(&mut c, b"pw", &payload, &cfg).unwrap();
            let moved = (0..orig.len()).filter(|&i| orig.get(i) != c.get(i)).count() as f64 / orig.len() as f64;
            print!(" | {:.0}%: {:+.4} (true {:.3})", frac * 100.0, jpeg_change_estimate(&c).unwrap_or(f64::NAN), moved);
        }
        println!();
    }
}
