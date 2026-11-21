mod common;
use common::natural_image;
use std::path::{Path, PathBuf};
use stegosaur::app::hide::{run_hide, HideSpec, Layout, Naming, Payload};
use stegosaur::app::{AppConfig, Secret};
use stegosaur::carrier::AnyCarrier;
use stegosaur::container::{extract_deniable, extract_single};
use stegosaur::crypto::KdfParams;
use stegosaur::distribute::recover;
use stegosaur::tag::{check_name, Kind};

fn cfg() -> AppConfig {
    AppConfig { kdf: KdfParams::fast(), ..AppConfig::default() }
}

fn cover(d: &Path, name: &str, seed: u64) -> PathBuf {
    let p = d.join(name);
    natural_image(160, 120, seed).save(&p).unwrap();
    p
}

fn file(d: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = d.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn spec(layout: Layout, pass: &str, naming: Naming) -> HideSpec {
    HideSpec { layout, pass: Secret::new(pass), cfg: cfg(), naming, out_dir: None, overwrite: false, extra: false }
}

fn go(s: HideSpec) -> stegosaur::Result<stegosaur::app::hide::HideDone> {
    run_hide(s, &mut |_| {})
}

#[test]
fn typed_text_is_hidden_without_a_file_and_comes_back() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "beach.png", 1);
    let done = go(spec(Layout::Single { payload: Payload::Text(Secret::new("meet at noon")), cover: c }, "pw one", Naming::Ordinary)).unwrap();
    let out = done.items[0].output.clone().unwrap();
    let loaded = AnyCarrier::load(&out).unwrap();
    assert_eq!(extract_single(&loaded, b"pw one", &cfg().to_config()).unwrap(), b"meet at noon");
    assert_eq!(done.items[0].what, "typed message");
    // nothing but the cover and the new picture is on disk
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
}

#[test]
fn an_empty_typed_message_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "a.png", 2);
    assert!(go(spec(Layout::Single { payload: Payload::Text(Secret::new("")), cover: c }, "pw", Naming::Ordinary)).is_err());
}

#[test]
fn ordinary_names_carry_a_tag_only_the_passphrase_can_check() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "a.png", 3);
    let s = file(d.path(), "s.bin", b"secret data");
    let done = go(spec(Layout::Single { payload: Payload::File(s), cover: c }, "right phrase", Naming::Ordinary)).unwrap();
    let name = done.items[0].output.as_ref().unwrap().file_name().unwrap().to_str().unwrap().to_string();
    assert!(!name.to_lowercase().contains("stego"), "{name}");
    assert!(check_name(b"right phrase", &name, KdfParams::fast()).unwrap().contains(&Kind::Single));
}

#[test]
fn suffix_and_pattern_naming() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "beach.png", 4);
    let s = file(d.path(), "s.bin", b"x");
    let l = || Layout::Single { payload: Payload::File(s.clone()), cover: c.clone() };
    let a = go(spec(l(), "pw", Naming::Suffix)).unwrap();
    assert_eq!(a.items[0].output.as_ref().unwrap().file_name().unwrap(), "beach_1.png");
    let b = go(spec(l(), "pw", Naming::Suffix)).unwrap();
    assert_eq!(b.items[0].output.as_ref().unwrap().file_name().unwrap(), "beach_2.png");
    let p = go(spec(l(), "pw", Naming::Pattern("{name}-{payload}-{n}".into()))).unwrap();
    assert_eq!(p.items[0].output.as_ref().unwrap().file_name().unwrap(), "beach-s-01.png");
    // a pattern that would reuse an existing name is refused, never overwritten
    assert!(go(spec(l(), "pw", Naming::Pattern("{name}-{payload}-{n}".into()))).is_err());
}

#[test]
fn two_secrets_in_one_cover_open_with_their_own_passphrases() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "c.png", 5);
    let (a, b) = (file(d.path(), "a.bin", b"first secret"), file(d.path(), "b.bin", b"second secret"));
    let done = go(spec(Layout::Two { payload: Payload::File(a), second: Payload::Text(Secret::new("second secret")), second_pass: Secret::new("pw two"), cover: c.clone() }, "pw one", Naming::Suffix)).unwrap();
    let _ = b;
    let out = AnyCarrier::load(done.items[0].output.as_ref().unwrap()).unwrap();
    assert_eq!(extract_deniable(&out, b"pw one", &cfg().to_config()).unwrap(), b"first secret");
    assert_eq!(extract_deniable(&out, b"pw two", &cfg().to_config()).unwrap(), b"second secret");
    assert!(extract_deniable(&out, b"pw three", &cfg().to_config()).is_err());
    // identical passphrases are refused
    let again = go(spec(Layout::Two { payload: Payload::Text(Secret::new("x")), second: Payload::Text(Secret::new("y")), second_pass: Secret::new("same"), cover: c }, "same", Naming::Suffix));
    assert!(again.is_err());
}

#[test]
fn the_same_look_layout_holds_one_file() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "c.png", 6);
    let done = go(spec(Layout::SameLook { payload: Payload::Text(Secret::new("quiet")), cover: c }, "pw", Naming::Ordinary)).unwrap();
    let out = AnyCarrier::load(done.items[0].output.as_ref().unwrap()).unwrap();
    assert_eq!(extract_deniable(&out, b"pw", &cfg().to_config()).unwrap(), b"quiet");
    let name = done.items[0].output.as_ref().unwrap().file_name().unwrap().to_str().unwrap().to_string();
    assert!(check_name(b"pw", &name, KdfParams::fast()).unwrap().contains(&Kind::Hidden));
}

#[test]
fn split_spreads_one_file_over_several_covers() {
    let d = tempfile::tempdir().unwrap();
    let covers: Vec<PathBuf> = (0..3).map(|i| cover(d.path(), &format!("c{i}.png"), 10 + i)).collect();
    let s = file(d.path(), "s.bin", b"split me please");
    let done = go(spec(Layout::Split { payload: Payload::File(s), covers, k: 2 }, "pw", Naming::Ordinary)).unwrap();
    assert_eq!(done.items.len(), 3);
    let loaded: Vec<AnyCarrier> = done.items.iter().take(2).map(|i| AnyCarrier::load(i.output.as_ref().unwrap()).unwrap()).collect();
    assert_eq!(recover(b"pw", &loaded, &cfg().to_config()).unwrap(), b"split me please");
    let one: Vec<AnyCarrier> = done.items.iter().take(1).map(|i| AnyCarrier::load(i.output.as_ref().unwrap()).unwrap()).collect();
    assert!(recover(b"pw", &one, &cfg().to_config()).is_err());
}

#[test]
fn bulk_gives_every_file_its_own_cover_and_reports_failures_per_item() {
    let d = tempfile::tempdir().unwrap();
    let (c1, c2) = (cover(d.path(), "one.png", 20), cover(d.path(), "two.png", 21));
    let small = file(d.path(), "small.bin", b"ok");
    let huge = file(d.path(), "huge.bin", &vec![7u8; 20_000]); // far more than a 160x120 picture holds
    let done = go(spec(Layout::Bulk { pairs: vec![(Payload::File(small), c1), (Payload::File(huge), c2)] }, "pw", Naming::Ordinary)).unwrap();
    assert_eq!(done.items.len(), 2);
    assert!(done.items[0].output.is_some() && done.items[0].error.is_none());
    assert!(done.items[1].output.is_none() && done.items[1].error.as_ref().is_some_and(|e| e.contains("holds")), "{:?}", done.items[1].error);
}

#[test]
fn nothing_is_ever_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "a.png", 30);
    file(d.path(), "a_1.png", b"precious");
    let r = go(spec(Layout::Single { payload: Payload::Text(Secret::new("x")), cover: c.clone() }, "pw", Naming::Pattern("{name}_1".into())));
    assert!(r.is_err());
    assert_eq!(std::fs::read(d.path().join("a_1.png")).unwrap(), b"precious");
    // the cover is never used as its own output
    let r2 = go(spec(Layout::Single { payload: Payload::Text(Secret::new("x")), cover: c }, "pw", Naming::Pattern("{name}".into())));
    assert!(r2.is_err());
}

#[test]
fn one_cover_can_be_used_for_every_file() {
    let d = tempfile::tempdir().unwrap();
    let c = cover(d.path(), "beach.png", 11);
    let a = file(d.path(), "a.bin", b"first secret");
    let b = file(d.path(), "b.bin", b"second secret, a bit longer");
    let before = std::fs::read(&c).unwrap();
    let done = go(spec(Layout::Bulk { pairs: vec![(Payload::File(a), c.clone()), (Payload::File(b), c.clone())] }, "pw", Naming::Suffix)).unwrap();
    assert_eq!(done.items.len(), 2);
    let outs: Vec<PathBuf> = done.items.iter().map(|i| i.output.clone().unwrap()).collect();
    assert_ne!(outs[0], outs[1]);
    for (out, want) in outs.iter().zip([&b"first secret"[..], &b"second secret, a bit longer"[..]]) {
        let img = AnyCarrier::load(out).unwrap();
        assert_eq!(extract_single(&img, b"pw", &cfg().to_config()).unwrap(), want);
    }
    assert_eq!(std::fs::read(&c).unwrap(), before, "the cover itself is not changed");
}
