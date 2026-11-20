mod common;
use common::natural_image;
use std::path::PathBuf;
use stegosaur::app::bulk::{list_files, run, BulkOptions, Found};
use stegosaur::app::{AppConfig, Secret};
use stegosaur::container::{embed_deniable, embed_single};
use stegosaur::crypto::KdfParams;
use stegosaur::distribute::distribute;

fn app_cfg() -> AppConfig {
    AppConfig { kdf: KdfParams::fast(), ..AppConfig::default() }
}

fn opts(dir: &std::path::Path, passes: &[&str], names_only: bool) -> BulkOptions {
    BulkOptions { passes: passes.iter().map(|p| Secret::new(p)).collect(), out_dir: dir.join("recovered"), cfg: app_cfg(), names_only }
}

fn bytes(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

/// A folder holding one file of every kind plus two plain images.
fn folder() -> (tempfile::TempDir, [Vec<u8>; 4]) {
    let d = tempfile::tempdir().unwrap();
    let cfg = app_cfg().to_config();
    let (single, first, second, split) = (bytes(800, 1), bytes(300, 2), bytes(200, 3), bytes(250, 4));

    let mut a = natural_image(120, 90, 1);
    embed_single(&mut a, b"one", &single, &cfg).unwrap();
    a.save(&d.path().join("a.png")).unwrap();

    let mut b = natural_image(200, 150, 2);
    embed_deniable(&mut b, (b"two-a", &first), Some((b"two-b", &second)), &cfg).unwrap();
    b.save(&d.path().join("b.png")).unwrap();

    let mut shares = [natural_image(120, 90, 3), natural_image(120, 90, 4), natural_image(120, 90, 5)];
    distribute(&split, b"sharepass", 2, &mut shares, &cfg).unwrap();
    for (i, s) in shares.iter().enumerate() {
        s.save(&d.path().join(format!("share{i}.png"))).unwrap();
    }
    natural_image(120, 90, 6).save(&d.path().join("plain1.png")).unwrap();
    natural_image(120, 90, 7).save(&d.path().join("plain2.png")).unwrap();
    std::fs::write(d.path().join("notes.txt"), b"not an image").unwrap();
    (d, [single, first, second, split])
}

#[test]
fn finds_every_layout_without_being_told_which_file_is_which() {
    let (d, [single, first, second, split]) = folder();
    let files = list_files(d.path(), false, None).unwrap();
    let report = run(&files, &opts(d.path(), &["wrong", "one", "two-a", "two-b", "sharepass"], false), &mut |_, _, _| {}).unwrap();

    let read = |p: &PathBuf| std::fs::read(p).unwrap();
    let mut recovered: Vec<Vec<u8>> = Vec::new();
    for e in &report.entries {
        for f in &e.found {
            if let Found::File { path, .. } = f {
                recovered.push(read(path));
            }
        }
    }
    assert!(recovered.contains(&single), "one-file payload");
    assert!(recovered.contains(&first) && recovered.contains(&second), "both slots of the two-passphrase file");
    assert_eq!(report.sets.len(), 1);
    let set = &report.sets[0];
    assert_eq!((set.k, set.n, set.found), (2, 3, 3));
    assert_eq!(read(set.output.as_ref().unwrap()), split, "the split secret was rebuilt");

    // plain images and the text file report nothing / skipped, never a false hit
    for name in ["plain1.png", "plain2.png"] {
        let e = report.entries.iter().find(|e| e.file.file_name().unwrap() == name).unwrap();
        assert!(e.found.is_empty() && e.skipped.is_none(), "{name}");
    }
    let notes = report.entries.iter().find(|e| e.file.file_name().unwrap() == "notes.txt").unwrap();
    assert!(notes.skipped.is_some());
}

#[test]
fn a_split_set_with_too_few_shares_says_how_many_more_it_needs() {
    let (d, _) = folder();
    std::fs::remove_file(d.path().join("share0.png")).unwrap();
    std::fs::remove_file(d.path().join("share1.png")).unwrap();
    let files = list_files(d.path(), false, None).unwrap();
    let report = run(&files, &opts(d.path(), &["sharepass"], false), &mut |_, _, _| {}).unwrap();
    let set = &report.sets[0];
    assert!(set.output.is_none());
    assert_eq!((set.found, set.k, set.missing), (1, 2, 1));
}

#[test]
fn nothing_is_overwritten_and_rerunning_adds_new_files() {
    let (d, _) = folder();
    let files = list_files(d.path(), false, None).unwrap();
    let o = opts(d.path(), &["one"], false);
    let r1 = run(&files, &o, &mut |_, _, _| {}).unwrap();
    let r2 = run(&files, &o, &mut |_, _, _| {}).unwrap();
    let p1 = r1.entries.iter().flat_map(|e| &e.found).find_map(|f| if let Found::File { path, .. } = f { Some(path.clone()) } else { None }).unwrap();
    let p2 = r2.entries.iter().flat_map(|e| &e.found).find_map(|f| if let Found::File { path, .. } = f { Some(path.clone()) } else { None }).unwrap();
    assert_ne!(p1, p2);
    assert!(p1.exists() && p2.exists());
    // inputs are untouched
    assert!(std::fs::read(d.path().join("a.png")).unwrap().len() > 100);
}

#[test]
fn needs_a_passphrase() {
    let (d, _) = folder();
    let files = list_files(d.path(), false, None).unwrap();
    assert!(run(&files, &opts(d.path(), &[], false), &mut |_, _, _| {}).is_err());
}

#[test]
fn names_only_mode_uses_the_tag_and_skips_untagged_files() {
    let (d, _) = folder();
    let files = list_files(d.path(), false, None).unwrap();
    // These files carry no tag in their names, so nothing is even tried.
    let report = run(&files, &opts(d.path(), &["one", "sharepass"], true), &mut |_, _, _| {}).unwrap();
    assert_eq!(report.recovered_files(), 0);
}
