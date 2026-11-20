mod common;
use common::{natural_image, random_bytes};
use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_stegosaur"));
    c.args(args).env_remove("STEGOSAUR_PASS");
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn ok(o: &Output) -> String {
    assert!(o.status.success(), "stderr: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn embed_extract_and_failure_modes() {
    let d = tempfile::tempdir().unwrap();
    let (cover, stego, data, back) = (d.path().join("c.png"), d.path().join("s.png"), d.path().join("d.bin"), d.path().join("b.bin"));
    natural_image(128, 128, 1).save(&cover).unwrap();
    let payload = random_bytes(1000, 2);
    std::fs::write(&data, &payload).unwrap();

    ok(&run(&["embed", s(&cover), s(&stego), "--data", s(&data), "--pass", "pw"], &[]));
    ok(&run(&["extract", s(&stego), "--out", s(&back), "--pass", "pw"], &[]));
    assert_eq!(std::fs::read(&back).unwrap(), payload);

    // passphrase via environment variable
    ok(&run(&["extract", s(&stego), "--out", s(&back)], &[("STEGOSAUR_PASS", "pw")]));

    let bad = run(&["extract", s(&stego), "--out", s(&back), "--pass", "nope"], &[]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no payload"));

    let lossy = run(&["embed", "x.jpg", s(&stego), "--data", s(&data), "--pass", "pw"], &[]);
    assert!(!lossy.status.success());
    let missing_pass = run(&["extract", s(&stego), "--out", s(&back)], &[]);
    assert!(!missing_pass.status.success());
}

#[test]
fn hidden_roundtrip() {
    let d = tempfile::tempdir().unwrap();
    let cover = d.path().join("c.png");
    let stego = d.path().join("s.png");
    let (decoy, real, out) = (d.path().join("decoy.txt"), d.path().join("real.bin"), d.path().join("out.bin"));
    natural_image(160, 160, 3).save(&cover).unwrap();
    std::fs::write(&decoy, b"nothing to see").unwrap();
    let secret = random_bytes(300, 4);
    std::fs::write(&real, &secret).unwrap();

    ok(&run(&[
        "hidden-embed", s(&cover), s(&stego), "--public-data", s(&decoy), "--public-pass", "a",
        "--hidden-data", s(&real), "--hidden-pass", "b",
    ], &[]));
    ok(&run(&["hidden-extract", s(&stego), "--out", s(&out), "--pass", "a"], &[]));
    assert_eq!(std::fs::read(&out).unwrap(), b"nothing to see");
    ok(&run(&["hidden-extract", s(&stego), "--out", s(&out), "--pass", "b"], &[]));
    assert_eq!(std::fs::read(&out).unwrap(), secret);
    assert!(!run(&["hidden-extract", s(&stego), "--out", s(&out), "--pass", "c"], &[]).status.success());
}

#[test]
fn split_and_join_with_files_on_disk() {
    let d = tempfile::tempdir().unwrap();
    let mut covers = Vec::new();
    for i in 0..4 {
        let p = d.path().join(format!("c{i}.png"));
        natural_image(96, 96, 10 + i).save(&p).unwrap();
        covers.push(p);
    }
    let secret_path = d.path().join("secret.bin");
    let secret = random_bytes(200, 7);
    std::fs::write(&secret_path, &secret).unwrap();
    let outdir = d.path().join("shares");
    let mut args = vec!["split", "--secret", s(&secret_path), "--k", "2", "--pass", "pw", "--out-dir", s(&outdir)];
    args.extend(covers.iter().map(|p| s(p)));
    ok(&run(&args, &[]));

    let out = d.path().join("joined.bin");
    // any two of the modified carriers, plus an untouched original, suffice
    let (a, b) = (outdir.join("c1.png"), outdir.join("c3.png"));
    ok(&run(&["join", "--out", s(&out), "--pass", "pw", s(&a), s(&b), s(&covers[0])], &[]));
    assert_eq!(std::fs::read(&out).unwrap(), secret);
    assert!(!run(&["join", "--out", s(&out), "--pass", "pw", s(&a), s(&covers[0])], &[]).status.success());
}

#[test]
fn text_roundtrip_and_analyze() {
    let d = tempfile::tempdir().unwrap();
    let (data, text, out) = (d.path().join("d.bin"), d.path().join("t.txt"), d.path().join("o.bin"));
    let payload = random_bytes(48, 9);
    std::fs::write(&data, &payload).unwrap();
    ok(&run(&["text-embed", "--data", s(&data), "--pass", "pw", "--out", s(&text)], &[]));
    ok(&run(&["text-extract", s(&text), "--out", s(&out), "--pass", "pw"], &[]));
    assert_eq!(std::fs::read(&out).unwrap(), payload);

    let img = d.path().join("i.png");
    natural_image(128, 128, 5).save(&img).unwrap();
    let report = ok(&run(&["analyze", s(&img)], &[]));
    assert!(report.contains("detectability") && report.contains("Low"), "{report}");
}

#[test]
fn log_style_cover_text_roundtrip() {
    let d = tempfile::tempdir().unwrap();
    let (data, text, out) = (d.path().join("d.bin"), d.path().join("t.log"), d.path().join("o.bin"));
    let payload = random_bytes(40, 11);
    std::fs::write(&data, &payload).unwrap();
    for style in ["syslog", "postgres", "nginx", "env"] {
        ok(&run(&["text-embed", "--data", s(&data), "--pass", "pw", "--style", style, "--out", s(&text)], &[]));
        let cover = std::fs::read_to_string(&text).unwrap();
        assert!(cover.lines().count() > 2, "{style}: {cover}");
        ok(&run(&["text-extract", s(&text), "--out", s(&out), "--pass", "pw", "--style", style], &[]));
        assert_eq!(std::fs::read(&out).unwrap(), payload, "{style}");
        // wrong style must not decode
        let wrong = if style == "syslog" { "nginx" } else { "syslog" };
        assert!(!run(&["text-extract", s(&text), "--out", s(&out), "--pass", "pw", "--style", wrong], &[]).status.success());
    }
}

#[test]
fn parity_flag_must_match_between_embed_and_extract() {
    let d = tempfile::tempdir().unwrap();
    let (cover, stego, data, back) = (d.path().join("c.png"), d.path().join("s.png"), d.path().join("d.bin"), d.path().join("b.bin"));
    natural_image(128, 128, 1).save(&cover).unwrap();
    std::fs::write(&data, random_bytes(500, 2)).unwrap();
    ok(&run(&["embed", s(&cover), s(&stego), "--data", s(&data), "--pass", "pw", "--parity", "32"], &[]));
    ok(&run(&["extract", s(&stego), "--out", s(&back), "--pass", "pw", "--parity", "32"], &[]));
    assert_eq!(std::fs::read(&back).unwrap(), std::fs::read(&data).unwrap());
    assert!(!run(&["extract", s(&stego), "--out", s(&back), "--pass", "pw"], &[]).status.success());
}

#[test]
fn old_deniable_command_names_still_work_as_aliases() {
    let d = tempfile::tempdir().unwrap();
    let (cover, stego, decoy, out) = (d.path().join("c.png"), d.path().join("s.png"), d.path().join("d.txt"), d.path().join("o.txt"));
    natural_image(160, 160, 3).save(&cover).unwrap();
    std::fs::write(&decoy, b"alias check").unwrap();
    ok(&run(&["deniable-embed", s(&cover), s(&stego), "--decoy-data", s(&decoy), "--decoy-pass", "a"], &[]));
    ok(&run(&["deniable-extract", s(&stego), "--out", s(&out), "--pass", "a"], &[]));
    assert_eq!(std::fs::read(&out).unwrap(), b"alias check");
    let help = String::from_utf8_lossy(&run(&["--help"], &[]).stdout).to_string();
    assert!(help.contains("hidden-embed") && !help.to_lowercase().contains("deniab"), "help should show only the new names:\n{help}");
}

#[test]
fn jpeg_round_trip_and_honest_analyze() {
    let d = tempfile::tempdir().unwrap();
    let jpeg = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tower_progressive.jpg");
    let (stego, data, back) = (d.path().join("s.jpg"), d.path().join("d.bin"), d.path().join("b.bin"));
    let payload = random_bytes(2000, 3);
    std::fs::write(&data, &payload).unwrap();

    let out = ok(&run(&["embed", s(&jpeg), s(&stego), "--data", s(&data), "--pass", "pw"], &[]));
    assert!(out.contains("hid 2000 bytes"), "{out}");
    ok(&run(&["extract", s(&stego), "--out", s(&back), "--pass", "pw"], &[]));
    assert_eq!(std::fs::read(&back).unwrap(), payload);

    // JPEG data is read with the calibrated coefficient detector, not the pixel detectors.
    let a = ok(&run(&["analyze", s(&stego)], &[]));
    assert!(a.contains("altered-coefficient estimate"), "{a}");
}
