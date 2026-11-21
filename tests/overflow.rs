mod common;
use common::natural_image;
use std::path::{Path, PathBuf};
use stegosaur::app::bulk::{run as bulk_run, BulkOptions};
use stegosaur::app::hide::{run_hide, HideSpec, Layout, Naming, Payload};
use stegosaur::app::jobs::{run as run_job, JobResult, JobSpec};
use stegosaur::app::{AppConfig, Secret};
use stegosaur::carrier::AnyCarrier;
use stegosaur::container::{extract_single, single_capacity};
use stegosaur::crypto::KdfParams;
use stegosaur::overflow;

fn cfg() -> AppConfig {
    AppConfig { kdf: KdfParams::fast(), ..AppConfig::default() }
}

fn cover(d: &Path, name: &str) -> PathBuf {
    let p = d.join(name);
    natural_image(160, 120, 7).save(&p).unwrap();
    p
}

fn blob(len: usize) -> Vec<u8> {
    let mut x = 0x2545F491u32;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

fn hide(cover: &Path, payload: &Path, pass: &str, extra: bool) -> stegosaur::Result<PathBuf> {
    let spec = HideSpec {
        layout: Layout::Single { payload: Payload::File(payload.to_path_buf()), cover: cover.to_path_buf() },
        pass: Secret::new(pass),
        cfg: cfg(),
        naming: Naming::Suffix,
        out_dir: None,
        overwrite: false,
        extra,
    };
    let done = run_hide(spec, &mut |_| {})?;
    Ok(done.items[0].output.clone().unwrap())
}

fn reveal(carrier: &Path, pass: &str, out: &Path) -> stegosaur::Result<usize> {
    let r = run_job(JobSpec::Reveal { carrier: carrier.to_path_buf(), pass: Secret::new(pass), output: out.to_path_buf(), cfg: cfg(), overwrite: false }, &mut |_| {})?;
    match r {
        JobResult::Revealed { bytes, .. } => Ok(bytes),
        _ => panic!("unexpected result"),
    }
}

#[test]
fn a_file_larger_than_the_cover_holds_comes_back_whole() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let cap = single_capacity(&AnyCarrier::load(&c).unwrap(), &cfg().to_config());
    let data = blob(cap * 4 + 123);
    let p = d.path().join("big.bin");
    std::fs::write(&p, &data).unwrap();
    assert!(hide(&c, &p, "pw", false).is_err(), "without extra room it is refused");
    let out = hide(&c, &p, "pw", true).unwrap();
    let back = d.path().join("back.bin");
    assert_eq!(reveal(&out, "pw", &back).unwrap(), data.len());
    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn the_original_cover_is_not_touched() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let before = std::fs::read(&c).unwrap();
    let p = d.path().join("big.bin");
    std::fs::write(&p, blob(200_000)).unwrap();
    hide(&c, &p, "pw", true).unwrap();
    assert_eq!(std::fs::read(&c).unwrap(), before);
}

#[test]
fn the_picture_still_opens_and_holds_the_first_part() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let p = d.path().join("big.bin");
    std::fs::write(&p, blob(150_000)).unwrap();
    let out = hide(&c, &p, "pw", true).unwrap();
    let img = AnyCarrier::load(&out).unwrap();
    let head = extract_single(&img, b"pw", &cfg().to_config()).unwrap();
    assert!(head.len() < 150_000 && !head.is_empty());
    assert_eq!(head, std::fs::read(&p).unwrap()[..head.len()]);
    // the file grew by about the overflow, not by the whole payload twice
    let grown = std::fs::metadata(&out).unwrap().len() as usize;
    assert!(grown > 150_000 - head.len() && grown < 150_000 + 100_000);
}

#[test]
fn a_payload_of_many_pieces_streams_back_exactly() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let data = blob(overflow::CHUNK * 2 + 4321);
    let p = d.path().join("huge.bin");
    std::fs::write(&p, &data).unwrap();
    let out = hide(&c, &p, "pw", true).unwrap();
    let back = d.path().join("back.bin");
    assert_eq!(reveal(&out, "pw", &back).unwrap(), data.len());
    assert!(std::fs::read(&back).unwrap() == data);
}

#[test]
fn a_payload_that_fits_adds_no_block() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let p = d.path().join("tiny.bin");
    std::fs::write(&p, b"fits").unwrap();
    let with = hide(&c, &p, "pw", true).unwrap();
    let keys = {
        let img = AnyCarrier::load(&with).unwrap();
        stegosaur::container::extract_single_keys(&img, b"pw", &cfg().to_config()).unwrap().1
    };
    let mut sink = Vec::new();
    assert_eq!(overflow::read(&with, &keys, &mut sink).unwrap(), None);
}

#[test]
fn a_wrong_passphrase_finds_nothing() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let p = d.path().join("big.bin");
    std::fs::write(&p, blob(150_000)).unwrap();
    let out = hide(&c, &p, "right", true).unwrap();
    assert!(reveal(&out, "wrong", &d.path().join("x.bin")).is_err());
    assert!(!d.path().join("x.bin").exists());
}

#[test]
fn types_that_do_not_ignore_trailing_bytes_are_refused() {
    let d = tempfile::tempdir().unwrap();
    let c = d.path().join("small.tiff");
    natural_image(160, 120, 7).save(&c).unwrap();
    let p = d.path().join("big.bin");
    std::fs::write(&p, blob(150_000)).unwrap();
    let e = hide(&c, &p, "pw", true).unwrap_err();
    assert!(e.to_string().contains("extra room"), "{e}");
}

#[test]
fn a_cut_short_block_is_reported_and_leaves_no_output() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let p = d.path().join("big.bin");
    std::fs::write(&p, blob(150_000)).unwrap();
    let out = hide(&c, &p, "pw", true).unwrap();
    // flip a byte inside the block (not in the footer)
    let mut bytes = std::fs::read(&out).unwrap();
    let at = bytes.len() - overflow::FOOT_LEN - 500;
    bytes[at] ^= 0xFF;
    std::fs::write(&out, &bytes).unwrap();
    let back = d.path().join("back.bin");
    assert!(reveal(&out, "pw", &back).is_err());
    assert!(!back.exists());
}

#[test]
fn bulk_reveal_finds_overflow_too() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "small.png");
    let data = blob(120_000);
    let p = d.path().join("big.bin");
    std::fs::write(&p, &data).unwrap();
    let out = hide(&c, &p, "pw", true).unwrap();
    let out_dir = d.path().join("found");
    let opts = BulkOptions { passes: vec![Secret::new("pw")], out_dir: out_dir.clone(), cfg: cfg(), names_only: false, text: None };
    let rep = bulk_run(&[out], &opts, &mut |_, _, _| {}).unwrap();
    let found = &rep.entries[0].found;
    assert_eq!(found.len(), 1);
    let path = match &found[0] {
        stegosaur::app::bulk::Found::File { path, bytes, .. } => {
            assert_eq!(*bytes, data.len());
            path.clone()
        }
        _ => panic!("expected a file"),
    };
    assert_eq!(std::fs::read(path).unwrap(), data);
}

#[test]
fn audio_covers_take_extra_room_and_still_play() {
    let d = tempfile::tempdir().unwrap();
    let c = d.path().join("clip.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&c, spec).unwrap();
    for i in 0..16000 {
        w.write_sample(((i as f32 * 0.05).sin() * 6000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    let data = blob(60_000);
    let p = d.path().join("big.bin");
    std::fs::write(&p, &data).unwrap();
    let out = hide(&c, &p, "pw", true).unwrap();
    assert_eq!(hound::WavReader::open(&out).unwrap().len(), 16000, "the audio length is unchanged");
    let back = d.path().join("back.bin");
    reveal(&out, "pw", &back).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), data);
}
