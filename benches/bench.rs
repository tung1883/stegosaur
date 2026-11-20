#[path = "../tests/common/mod.rs"]
mod common;

use common::{cfg, natural_image, random_bytes};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::hint::black_box;
use std::time::Duration;
use stegosaur::analysis::analyze;
use stegosaur::carrier::write_bit;
use stegosaur::container::{embed_deniable, embed_single, extract_deniable, extract_single};
use stegosaur::crypto::KdfParams;
use stegosaur::embed::{read_bytes, write_bytes, PositionStream};
use stegosaur::linguistic::{decode, encode, NgramModel};
use stegosaur::shamir::{combine, split};
use stegosaur::{Carrier, Config, ImageCarrier, Mode};

const SIDE: u32 = 512;
const PAYLOAD: usize = 8 * 1024;

/// Plain sequential LSB replacement: no key, no encryption, no permutation. The speed ceiling.
fn naive_embed(img: &mut ImageCarrier, payload: &[u8]) {
    for (bi, &b) in payload.iter().enumerate() {
        for k in 0..8 {
            let i = bi * 8 + k;
            img.samples[i] = (img.samples[i] & !1) | ((b >> (7 - k)) & 1);
        }
    }
}

fn naive_extract(img: &ImageCarrier, n: usize) -> Vec<u8> {
    (0..n)
        .map(|bi| (0..8).fold(0u8, |acc, k| (acc << 1) | (img.samples[bi * 8 + k] & 1)))
        .collect()
}

fn bench_embed(c: &mut Criterion) {
    let clean = natural_image(SIDE, SIDE, 1);
    let payload = random_bytes(PAYLOAD, 2);
    let key = [9u8; 32];
    let n = clean.len();
    let mut g = c.benchmark_group("embed 512x512 RGB, 8 KiB payload");
    g.sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(3));
    g.throughput(Throughput::Bytes(PAYLOAD as u64));

    g.bench_function("naive sequential LSB (baseline, no crypto)", |b| {
        b.iter_batched_ref(|| clean.clone(), |img| naive_embed(img, black_box(&payload)), criterion::BatchSize::LargeInput)
    });
    for (name, mode) in [("replace", Mode::Replace), ("match", Mode::Match)] {
        g.bench_function(format!("stegosaur core keyed {name} (no KDF/AEAD)"), |b| {
            let mut rng = ChaCha8Rng::seed_from_u64(1);
            b.iter_batched_ref(
                || clean.clone(),
                |img| {
                    let pos = PositionStream::new(&key, 0, n).take(payload.len() * 8);
                    write_bytes(img, &pos, black_box(&payload), mode, &mut rng);
                },
                criterion::BatchSize::LargeInput,
            )
        });
    }
    g.bench_function("stegosaur embed_single (AEAD + fast KDF)", |b| {
        let c = cfg(Mode::Match);
        b.iter_batched_ref(|| clean.clone(), |img| embed_single(img, b"pw", black_box(&payload), &c).unwrap(), criterion::BatchSize::LargeInput)
    });
    g.bench_function("stegosaur embed_single (AEAD + default scrypt N=2^15)", |b| {
        let c = Config { kdf: KdfParams::default(), ..cfg(Mode::Match) };
        b.iter_batched_ref(|| clean.clone(), |img| embed_single(img, b"pw", black_box(&payload), &c).unwrap(), criterion::BatchSize::LargeInput)
    });
    g.bench_function("stegosaur embed_deniable (2 slots, fast KDF)", |b| {
        let c = cfg(Mode::Match);
        let small = random_bytes(2000, 3);
        b.iter_batched_ref(
            || clean.clone(),
            |img| embed_deniable(img, (b"a", black_box(&small)), Some((b"b", &small)), &c).unwrap(),
            criterion::BatchSize::LargeInput,
        )
    });
    g.bench_function("steganography 1.0.2 encode_alpha (overwrites whole alpha bytes)", |b| {
        use image021::{DynamicImage, RgbImage};
        let dynimg = DynamicImage::ImageRgb8(RgbImage::from_raw(SIDE, SIDE, clean.samples.clone()).unwrap());
        b.iter(|| {
            let enc = steganography::encoder::Encoder::new(black_box(&payload), dynimg.clone());
            black_box(enc.encode_alpha())
        })
    });
    g.finish();
}

fn bench_extract(c: &mut Criterion) {
    let clean = natural_image(SIDE, SIDE, 1);
    let payload = random_bytes(PAYLOAD, 2);
    let key = [9u8; 32];
    let n = clean.len();
    let mut g = c.benchmark_group("extract 512x512 RGB, 8 KiB payload");
    g.sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(3));
    g.throughput(Throughput::Bytes(PAYLOAD as u64));

    let mut naive_img = clean.clone();
    naive_embed(&mut naive_img, &payload);
    g.bench_function("naive sequential LSB (baseline)", |b| b.iter(|| black_box(naive_extract(&naive_img, PAYLOAD))));

    let mut keyed = clean.clone();
    let pos = PositionStream::new(&key, 0, n).take(PAYLOAD * 8);
    write_bytes(&mut keyed, &pos, &payload, Mode::Match, &mut ChaCha8Rng::seed_from_u64(1));
    g.bench_function("stegosaur core keyed read (no KDF/AEAD)", |b| {
        b.iter(|| {
            let pos = PositionStream::new(&key, 0, n).take(PAYLOAD * 8);
            black_box(read_bytes(&keyed, &pos))
        })
    });

    let c1 = cfg(Mode::Match);
    let mut full = clean.clone();
    embed_single(&mut full, b"pw", &payload, &c1).unwrap();
    g.bench_function("stegosaur extract_single (AEAD + fast KDF)", |b| b.iter(|| black_box(extract_single(&full, b"pw", &c1).unwrap())));

    let mut den = clean.clone();
    let small = random_bytes(2000, 3);
    embed_deniable(&mut den, (b"a", &small), Some((b"b", &small)), &c1).unwrap();
    g.bench_function("stegosaur extract_deniable (hidden slot, fast KDF)", |b| b.iter(|| black_box(extract_deniable(&den, b"b", &c1).unwrap())));

    g.bench_function("steganography 1.0.2 decode_alpha", |b| {
        use image021::{DynamicImage, RgbImage};
        let dynimg = DynamicImage::ImageRgb8(RgbImage::from_raw(SIDE, SIDE, clean.samples.clone()).unwrap());
        let enc = steganography::encoder::Encoder::new(&payload, dynimg).encode_alpha();
        b.iter(|| black_box(steganography::decoder::Decoder::new(enc.clone()).decode_alpha()))
    });
    g.finish();
}

fn bench_shamir(c: &mut Criterion) {
    let mut g = c.benchmark_group("shamir k=3 n=5");
    g.sample_size(15).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    for &size in &[1024usize, 16 * 1024, 256 * 1024] {
        let secret = random_bytes(size, 5);
        g.throughput(Throughput::Bytes(size as u64));
        g.bench_with_input(BenchmarkId::new("stegosaur split", size), &secret, |b, s| {
            let mut rng = ChaCha8Rng::seed_from_u64(1);
            b.iter(|| black_box(split(black_box(s), 3, 5, &mut rng).unwrap()))
        });
        g.bench_with_input(BenchmarkId::new("sharks split", size), &secret, |b, s| {
            let sharks = sharks::Sharks(3);
            b.iter(|| black_box(sharks.dealer(black_box(s)).take(5).collect::<Vec<_>>()))
        });
        let mine = split(&secret, 3, 5, &mut ChaCha8Rng::seed_from_u64(1)).unwrap();
        let theirs: Vec<sharks::Share> = sharks::Sharks(3).dealer(&secret).take(5).collect();
        g.bench_with_input(BenchmarkId::new("stegosaur combine", size), &mine, |b, sh| {
            b.iter(|| black_box(combine(black_box(&sh[1..4])).unwrap()))
        });
        g.bench_with_input(BenchmarkId::new("sharks recover", size), &theirs, |b, sh| {
            let sharks = sharks::Sharks(3);
            b.iter(|| black_box(sharks.recover(black_box(&sh[1..4])).unwrap()))
        });
    }
    g.finish();
}

fn bench_analysis(c: &mut Criterion) {
    let mut g = c.benchmark_group("steganalysis (chi-square + RS + SPA, 3 planes)");
    g.sample_size(15).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    for &side in &[256u32, 512, 1024] {
        let img = natural_image(side, side, 7);
        g.throughput(Throughput::Bytes(img.len() as u64));
        g.bench_with_input(BenchmarkId::new("analyze", format!("{side}x{side}")), &img, |b, i| b.iter(|| black_box(analyze(black_box(i)))));
    }
    g.finish();
}

fn bench_linguistic(c: &mut Criterion) {
    let mut g = c.benchmark_group("linguistic (n-gram model, fast KDF)");
    g.sample_size(20).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    let model = NgramModel::default_english();
    let kdf = KdfParams::fast();
    for &len in &[32usize, 256] {
        let data = random_bytes(len, 8);
        g.throughput(Throughput::Bytes(len as u64));
        g.bench_with_input(BenchmarkId::new("encode", len), &data, |b, d| b.iter(|| black_box(encode(&model, b"pw", black_box(d), kdf).unwrap())));
        let text = encode(&model, b"pw", &data, kdf).unwrap();
        g.bench_with_input(BenchmarkId::new("decode", len), &text, |b, t| b.iter(|| black_box(decode(&model, b"pw", black_box(t), kdf).unwrap())));
    }
    g.finish();
}

fn bench_write_bit(c: &mut Criterion) {
    // Micro: cost of the +-1 decision itself, for context against raw replacement.
    let mut g = c.benchmark_group("write_bit micro, 1M samples");
    g.sample_size(20).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    let clean = natural_image(577, 577, 3);
    let n = 1_000_000usize.min(clean.len());
    g.throughput(Throughput::Elements(n as u64));
    for (name, mode) in [("replace", Mode::Replace), ("match", Mode::Match)] {
        g.bench_function(name, |b| {
            let mut rng = ChaCha8Rng::seed_from_u64(1);
            b.iter_batched_ref(
                || clean.clone(),
                |img| {
                    for i in 0..n {
                        write_bit(img, i, (i as u8 >> 3) & 1, mode, &mut rng);
                    }
                },
                criterion::BatchSize::LargeInput,
            )
        });
    }
    g.finish();
}

/// The previous placement algorithm: full Fisher-Yates over an n-sized table. Kept only as a before/after baseline.
fn old_fisher_yates_positions(key: &[u8; 32], n: usize, count: usize) -> Vec<u32> {
    use rand::Rng;
    let mut rng = rand_chacha::ChaCha8Rng::from_seed(*key);
    let mut perm: Vec<u32> = (0..n as u32).collect();
    (0..count)
        .map(|i| {
            let j = rng.gen_range(i..n);
            perm.swap(i, j);
            perm[i]
        })
        .collect()
}

fn bench_positions(c: &mut Criterion) {
    let mut g = c.benchmark_group("keyed positions: 65,536 of 786,432 samples");
    g.sample_size(20).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    let (n, count, key) = (512 * 512 * 3, 65_536usize, [3u8; 32]);
    g.throughput(Throughput::Elements(count as u64));
    g.bench_function("previous: full Fisher-Yates table", |b| b.iter(|| black_box(old_fisher_yates_positions(&key, n, count))));
    g.bench_function("current: bitmap + rejection sampling", |b| {
        b.iter(|| black_box(PositionStream::new(&key, 0, n).take(count)))
    });
    g.finish();
}

fn bench_ecc(c: &mut Criterion) {
    use rand::seq::SliceRandom;
    let mut g = c.benchmark_group("reed-solomon, one 255-byte block (223 data + 32 parity)");
    g.sample_size(30).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    let data = random_bytes(223, 1);
    g.throughput(Throughput::Bytes(223));

    let mine = stegosaur::ecc::encode(&data, 32).unwrap();
    let theirs = reed_solomon::Encoder::new(32).encode(&data).to_vec();
    let mut rng = ChaCha8Rng::seed_from_u64(4);
    let damage = |clean: &[u8], errs: usize, rng: &mut ChaCha8Rng| {
        let mut v = clean.to_vec();
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.shuffle(rng);
        for &i in idx.iter().take(errs) {
            v[i] ^= 0x5a;
        }
        v
    };

    g.bench_function("stegosaur encode", |b| b.iter(|| black_box(stegosaur::ecc::encode(black_box(&data), 32).unwrap())));
    g.bench_function("stegosaur encode (scalar fallback)", |b| b.iter(|| black_box(stegosaur::ecc::encode_scalar(black_box(&data), 32).unwrap())));
    g.bench_function("reed-solomon 0.2 encode", |b| b.iter(|| black_box(reed_solomon::Encoder::new(32).encode(black_box(&data)))));
    for errs in [0usize, 8, 16] {
        let (m, t) = (damage(&mine, errs, &mut rng), damage(&theirs, errs, &mut rng));
        g.bench_function(format!("stegosaur decode, {errs} errors"), |b| b.iter(|| black_box(stegosaur::ecc::decode(black_box(&m), 223, 32).unwrap())));
        g.bench_function(format!("reed-solomon 0.2 decode, {errs} errors"), |b| {
            b.iter(|| {
                let mut buf = t.clone();
                black_box(reed_solomon::Decoder::new(32).correct(&mut buf, None).unwrap().len())
            })
        });
    }
    g.finish();

    let mut g = c.benchmark_group("reed-solomon, 8 KiB payload, parity 32 (stegosaur only)");
    g.sample_size(30).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    let big = random_bytes(8 * 1024, 2);
    g.throughput(Throughput::Bytes(big.len() as u64));
    let enc = stegosaur::ecc::encode(&big, 32).unwrap();
    g.bench_function("encode", |b| b.iter(|| black_box(stegosaur::ecc::encode(black_box(&big), 32).unwrap())));
    g.bench_function("decode, clean", |b| b.iter(|| black_box(stegosaur::ecc::decode(black_box(&enc), big.len(), 32).unwrap())));
    let mut dirty = enc.clone();
    for blk in 0..enc.len() / 255 {
        for e in 0..12 {
            dirty[blk * 255 + e * 20] ^= 0xa5;
        }
    }
    g.bench_function("decode, 12 errors per block", |b| b.iter(|| black_box(stegosaur::ecc::decode(black_box(&dirty), big.len(), 32).unwrap())));
    g.finish();
}

fn bench_embed_ecc(c: &mut Criterion) {
    let clean = natural_image(SIDE, SIDE, 1);
    let payload = random_bytes(PAYLOAD, 2);
    let mut g = c.benchmark_group("full embed/extract 512x512, 8 KiB, fast KDF, by parity");
    g.sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(3));
    g.throughput(Throughput::Bytes(PAYLOAD as u64));
    for parity in [0usize, 16, 64] {
        let cfg = Config { parity, ..cfg(Mode::Match) };
        g.bench_function(format!("embed parity {parity}"), |b| {
            b.iter_batched_ref(|| clean.clone(), |img| embed_single(img, b"pw", black_box(&payload), &cfg).unwrap(), criterion::BatchSize::LargeInput)
        });
        let mut stego = clean.clone();
        embed_single(&mut stego, b"pw", &payload, &cfg).unwrap();
        g.bench_function(format!("extract parity {parity}"), |b| b.iter(|| black_box(extract_single(&stego, b"pw", &cfg).unwrap())));
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_embed,
    bench_extract,
    bench_positions,
    bench_ecc,
    bench_embed_ecc,
    bench_shamir,
    bench_analysis,
    bench_linguistic,
    bench_write_bit,
    bench_png_and_kdf
);
criterion_main!(benches);

fn png_bytes(img: &ImageCarrier, compression: image::codecs::png::CompressionType, filter: image::codecs::png::FilterType) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(&mut out, compression, filter)
        .write_image(&img.samples, img.width, img.height, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn bench_png_and_kdf(c: &mut Criterion) {
    use image::codecs::png::{CompressionType, FilterType};
    let mut g = c.benchmark_group("end to end: PNG I/O and default-scrypt deniable");
    g.sample_size(15).warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(3));
    for &side in &[512u32, 1024] {
        let img = natural_image(side, side, 9);
        let configs = [
            ("default compression + adaptive filter", CompressionType::Default, FilterType::Adaptive),
            ("fast compression + sub filter", CompressionType::Fast, FilterType::Sub),
        ];
        for (name, comp, filt) in configs {
            let bytes = png_bytes(&img, comp, filt);
            println!("{side}x{side} {name}: {} bytes ({:.1}% of raw)", bytes.len(), 100.0 * bytes.len() as f64 / img.samples.len() as f64);
            g.bench_function(format!("{side}x{side} encode, {name}"), |b| b.iter(|| black_box(png_bytes(black_box(&img), comp, filt))));
        }
        let bytes = png_bytes(&img, CompressionType::Default, FilterType::Adaptive);
        g.bench_function(format!("{side}x{side} decode"), |b| {
            b.iter(|| black_box(image::load_from_memory_with_format(black_box(&bytes), image::ImageFormat::Png).unwrap().to_rgb8()))
        });
    }
    let clean = natural_image(512, 512, 1);
    let cfg = Config { kdf: KdfParams::default(), ..cfg(Mode::Match) };
    let small = random_bytes(2000, 3);
    let mut den = clean.clone();
    embed_deniable(&mut den, (b"a", &small), Some((b"b", &small)), &cfg).unwrap();
    g.bench_function("extract_deniable, default scrypt, hidden slot", |b| b.iter(|| black_box(extract_deniable(&den, b"b", &cfg).unwrap())));
    let mut single = clean.clone();
    embed_single(&mut single, b"pw", &small, &cfg).unwrap();
    g.bench_function("extract_single, default scrypt", |b| b.iter(|| black_box(extract_single(&single, b"pw", &cfg).unwrap())));
    g.finish();
}
