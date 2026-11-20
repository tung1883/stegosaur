//! Developer timing: where does the JPEG preview path spend its time? Usage: jpeg_time <file.jpg>
use std::time::Instant;
use stegosaur::jpeg::JpegCarrier;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let t = Instant::now();
    let c = JpegCarrier::from_bytes(&bytes).unwrap();
    let parse = t.elapsed();
    let t = Instant::now();
    let re = c.to_bytes().unwrap();
    let encode = t.elapsed();
    let t = Instant::now();
    let img = image::load_from_memory_with_format(&re, image::ImageFormat::Jpeg).unwrap().to_rgb8();
    let decode = t.elapsed();
    let t = Instant::now();
    let _ = c.pixels().unwrap();
    let total = t.elapsed();
    println!("{}x{} ({:.1} MP, {} KB file)", img.width(), img.height(), img.width() as f64 * img.height() as f64 / 1e6, bytes.len() / 1024);
    let t = Instant::now();
    let _ = c.pixels().unwrap();
    let ours = t.elapsed();
    println!("pixels with our own decoder: {ours:?}");
    println!("read coefficients (Huffman decode): {parse:?}");
    println!("preview path today = re-encode {encode:?} + zune decode {decode:?} = {total:?}");
}
