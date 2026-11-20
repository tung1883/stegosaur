mod common;
use common::*;
use std::path::{Path, PathBuf};
use stegosaur::analysis::Risk;
use stegosaur::app::files::{carrier_info, scan_dir, CarrierCache, CarrierKind, FileKind};
use stegosaur::app::jobs::{self, default_output, JobEvent, JobResult, JobSpec, RevealLayout};
use stegosaur::app::preview::{preview_embed, preview_hidden, preview_split};
use stegosaur::app::{AppConfig, Secret};
use stegosaur::container::{deniable_capacity, single_capacity};
use stegosaur::crypto::KdfParams;
use stegosaur::distribute::SHARE_OVERHEAD;
use stegosaur::{AnyCarrier, Carrier, Mode};

fn acfg(mode: Mode) -> AppConfig {
    AppConfig { mode, parity: 0, rate: 0.1, kdf: KdfParams::fast() }
}

fn write_png(dir: &Path, name: &str, w: u32, h: u32, seed: u64) -> PathBuf {
    let p = dir.join(name);
    natural_image(w, h, seed).save(&p).unwrap();
    p
}

fn write_wav(dir: &Path, name: &str, bits: u16) -> PathBuf {
    let p = dir.join(name);
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: bits, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&p, spec).unwrap();
    for i in 0..20000 {
        let v = ((i as f64 / 20.0).sin() * 9000.0) as i32;
        if bits == 16 { w.write_sample(v as i16).unwrap() } else { w.write_sample((v / 256) as i8).unwrap() }
    }
    w.finalize().unwrap();
    p
}

fn run_ok(spec: JobSpec) -> JobResult {
    jobs::run(spec, &mut |_| {}).unwrap_or_else(|e| panic!("job failed: {e}"))
}

fn run_err(spec: JobSpec) -> stegosaur::Error {
    jobs::run(spec, &mut |_| {}).expect_err("job should have failed")
}

// ---------- files ----------

#[test]
fn scan_sorts_and_classifies() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("Zeta")).unwrap();
    std::fs::create_dir(d.path().join("alpha")).unwrap();
    write_png(d.path(), "b.png", 40, 30, 1);
    write_png(d.path(), "A.png", 20, 20, 2);
    std::fs::write(d.path().join("notes.txt"), b"hi").unwrap();
    std::fs::write(d.path().join("photo.JPG"), b"not really a jpeg").unwrap();
    std::fs::write(d.path().join("phone.heic"), b"x").unwrap();
    std::fs::write(d.path().join("broken.png"), b"definitely not a png").unwrap();
    std::fs::write(d.path().join(".hidden.png"), b"x").unwrap();
    write_wav(d.path(), "ok.wav", 16);
    write_wav(d.path(), "eight.wav", 8);

    let entries = scan_dir(d.path()).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names[..2], ["alpha", "Zeta"], "folders first, ignoring case: {names:?}");
    assert!(!names.contains(&".hidden.png"), "dot-files are skipped");
    let kind = |n: &str| entries.iter().find(|e| e.name == n).unwrap().kind;
    assert_eq!(kind("b.png"), FileKind::Carrier(CarrierKind::Png));
    assert_eq!(kind("ok.wav"), FileKind::Carrier(CarrierKind::Wav));
    assert_eq!(kind("notes.txt"), FileKind::Other);
    assert_eq!(kind("photo.JPG"), FileKind::Unusable);
    assert_eq!(kind("broken.png"), FileKind::Unusable);
    assert_eq!(kind("eight.wav"), FileKind::Unusable);
    let note = |n: &str| entries.iter().find(|e| e.name == n).unwrap().note.clone().unwrap();
    assert!(note("photo.JPG").to_lowercase().contains("jpeg"), "a damaged JPEG says why it cannot be used");
    assert_eq!(kind("phone.heic"), FileKind::Unusable);
    assert!(note("phone.heic").contains("lossy"));
    assert!(note("eight.wav").contains("16-bit"));
    let b = entries.iter().find(|e| e.name == "b.png").unwrap();
    assert_eq!(b.info.as_ref().unwrap().samples, 40 * 30 * 3);
    assert_eq!(b.info.as_ref().unwrap().summary, "40x30");
    let ai = entries.iter().position(|e| e.name == "A.png").unwrap();
    let bi = entries.iter().position(|e| e.name == "b.png").unwrap();
    assert!(ai < bi, "files sorted ignoring case");
}

#[test]
fn carrier_info_matches_loaded_carrier() {
    let d = tempfile::tempdir().unwrap();
    let png = write_png(d.path(), "a.png", 64, 48, 3);
    let wav = write_wav(d.path(), "a.wav", 16);
    for p in [&png, &wav] {
        let info = carrier_info(p).unwrap();
        assert_eq!(info.samples, AnyCarrier::load(p).unwrap().len(), "{p:?}");
    }
    assert!(carrier_info(&d.path().join("x.txt")).is_err());
}

#[test]
fn cache_reuses_and_invalidates() {
    let d = tempfile::tempdir().unwrap();
    let p = write_png(d.path(), "a.png", 32, 32, 1);
    let cache = CarrierCache::new(2);
    let (a, b) = (cache.load(&p).unwrap(), cache.load(&p).unwrap());
    assert!(std::sync::Arc::ptr_eq(&a, &b), "second load must hit the cache");
    std::thread::sleep(std::time::Duration::from_millis(30));
    natural_image(48, 32, 9).save(&p).unwrap();
    let c = cache.load(&p).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&a, &c), "changed file must be reloaded");
    assert_eq!(c.len(), 48 * 32 * 3);
    for i in 0..4 {
        cache.load(&write_png(d.path(), &format!("n{i}.png"), 16, 16, i)).unwrap();
    }
    assert_eq!(cache.len(), 2, "cache holds at most its capacity");
}

// ---------- previews ----------

#[test]
fn preview_capacity_matches_library() {
    let d = tempfile::tempdir().unwrap();
    let p = write_png(d.path(), "a.png", 128, 96, 4);
    let c = AnyCarrier::load(&p).unwrap();
    for parity in [0usize, 16, 64] {
        let cfg = AppConfig { parity, ..acfg(Mode::Match) };
        let pv = preview_embed(&c, 100, &cfg, false, None);
        assert_eq!(pv.capacity, single_capacity(&c, &cfg.to_config()), "single, parity {parity}");
        let hv = preview_hidden(&c, 10, Some(10), &cfg, false, None);
        assert_eq!(hv.slot_capacity, deniable_capacity(&c, &cfg.to_config()), "hidden, parity {parity}");
    }
}

#[test]
fn preview_fit_boundaries_and_messages() {
    let d = tempfile::tempdir().unwrap();
    let c = AnyCarrier::load(&write_png(d.path(), "a.png", 64, 64, 5)).unwrap();
    let cfg = acfg(Mode::Match);
    let cap = single_capacity(&c, &cfg.to_config());
    assert!(preview_embed(&c, cap, &cfg, false, None).fits);
    let over = preview_embed(&c, cap + 1, &cfg, true, None);
    assert!(!over.fits && over.measured.is_none());
    assert!(over.error.unwrap().contains("bigger image"), "friendly capacity message");
}

#[test]
fn preview_detectability_is_measured_and_mode_sensitive() {
    let d = tempfile::tempdir().unwrap();
    let c = AnyCarrier::load(&write_png(d.path(), "a.png", 256, 256, 6)).unwrap();
    let payload = single_capacity(&c, &acfg(Mode::Match).to_config()) * 3 / 4;
    let replace = preview_embed(&c, payload, &acfg(Mode::Replace), true, None).measured.expect("image carriers are measured");
    let matching = preview_embed(&c, payload, &acfg(Mode::Match), true, None).measured.unwrap();
    println!("75% full: replace {:.0}/100, match {:.0}/100, untouched {:.1}", replace.after.score, matching.after.score, replace.before.score);
    assert_eq!(replace.after.risk, Risk::High);
    assert_eq!(matching.after.risk, Risk::Low);
    assert_eq!(replace.before.risk, Risk::Low, "the untouched image reads clean");
    let cached = preview_embed(&c, payload, &acfg(Mode::Replace), true, Some(replace.before)).measured.unwrap();
    assert_eq!(cached.before.score, replace.before.score, "a supplied baseline is reused");
}

#[test]
fn preview_for_audio_reports_a_note_instead_of_a_number() {
    let d = tempfile::tempdir().unwrap();
    let c = AnyCarrier::load(&write_wav(d.path(), "a.wav", 16)).unwrap();
    let pv = preview_embed(&c, 100, &acfg(Mode::Match), true, None);
    assert!(pv.fits && pv.measured.is_none());
    assert!(pv.detect_note.unwrap().contains("images"));
}

#[test]
fn hidden_preview_checks_both_slots() {
    let d = tempfile::tempdir().unwrap();
    let c = AnyCarrier::load(&write_png(d.path(), "a.png", 192, 192, 7)).unwrap();
    let cfg = acfg(Mode::Match);
    let cap = deniable_capacity(&c, &cfg.to_config());
    let ok = preview_hidden(&c, 100, Some(100), &cfg, true, None);
    assert!(ok.error.is_none() && ok.measured.is_some());
    let big_hidden = preview_hidden(&c, 100, Some(cap + 1), &cfg, true, None);
    assert_eq!(big_hidden.hidden_fits, Some(false));
    assert!(big_hidden.error.unwrap().contains("hidden file"));
    assert!(preview_hidden(&c, cap + 1, None, &cfg, false, None).error.unwrap().contains("public file"));
    assert_eq!(preview_hidden(&c, 10, None, &cfg, false, None).hidden_fits, None);
}

#[test]
fn split_preview_share_size_is_exact() {
    let cfg = acfg(Mode::Match);
    // The share overhead must be the real one: a secret that fits by this arithmetic really embeds, one byte more does not.
    let samples = 96 * 96 * 3;
    let cap = single_capacity_for(samples, &cfg);
    let secret_ok = cap - SHARE_OVERHEAD;
    let pv = preview_split(&[samples], secret_ok, 1, &cfg);
    assert!(pv.error.is_none() && pv.carriers[0].fits);
    assert!(preview_split(&[samples], secret_ok + 1, 1, &cfg).error.is_some());

    let mut imgs = vec![natural_image(96, 96, 1)];
    let conf = cfg.to_config();
    assert!(stegosaur::distribute::distribute(&random_bytes(secret_ok, 1), b"pw", 1, &mut imgs, &conf).is_ok());
    let mut imgs = vec![natural_image(96, 96, 1)];
    assert!(stegosaur::distribute::distribute(&random_bytes(secret_ok + 1, 1), b"pw", 1, &mut imgs, &conf).is_err());

    assert!(preview_split(&[], 10, 1, &cfg).error.unwrap().contains("at least one"));
    assert!(preview_split(&[samples, samples], 10, 3, &cfg).error.unwrap().contains("between 1 and 2"));
    assert!(preview_split(&[samples, 100], 10, 1, &cfg).error.is_some());
}

fn single_capacity_for(samples: usize, cfg: &AppConfig) -> usize {
    stegosaur::container::single_capacity_for_len(samples, &cfg.to_config())
}

// ---------- jobs ----------

#[test]
fn embed_then_reveal_roundtrips_and_leaves_inputs_alone() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "cover.png", 128, 128, 1);
    let payload = d.path().join("secret.bin");
    let data = random_bytes(700, 2);
    std::fs::write(&payload, &data).unwrap();
    let before = std::fs::read(&cover).unwrap();
    let out = default_output(&cover);
    assert_eq!(out.file_name().unwrap(), "cover_1.png");

    let r = run_ok(JobSpec::Embed { carrier: cover.clone(), payload: payload.clone(), output: out.clone(), pass: Secret::new("pw"), cfg: acfg(Mode::Match), overwrite: false });
    let JobResult::Embedded { bytes, report, .. } = r else { panic!("wrong result") };
    assert_eq!(bytes, 700);
    assert!(report.is_some(), "image output is analysed");
    assert_eq!(std::fs::read(&cover).unwrap(), before, "the cover file is untouched");

    let rev = d.path().join("back.bin");
    let r = run_ok(JobSpec::Reveal { carrier: out.clone(), pass: Secret::new("pw"), output: rev.clone(), cfg: acfg(Mode::Match), overwrite: false });
    let JobResult::Revealed { layout, bytes, .. } = r else { panic!("wrong result") };
    assert_eq!((layout, bytes), (RevealLayout::Single, 700));
    assert_eq!(std::fs::read(&rev).unwrap(), data);
}

#[test]
fn hidden_job_roundtrips_both_slots_through_reveal() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "cover.png", 192, 192, 2);
    let (public, hidden) = (d.path().join("public.txt"), d.path().join("hidden.bin"));
    std::fs::write(&public, b"holiday plans").unwrap();
    let secret = random_bytes(300, 3);
    std::fs::write(&hidden, &secret).unwrap();
    let out = d.path().join("out.png");
    run_ok(JobSpec::Hidden { carrier: cover, public_file: public, public_pass: Secret::new("first"), hidden: Some((hidden, Secret::new("second"))), output: out.clone(), cfg: acfg(Mode::Match), overwrite: false });

    let reveal = |pass: &str, name: &str| {
        let o = d.path().join(name);
        let r = jobs::run(JobSpec::Reveal { carrier: out.clone(), pass: Secret::new(pass), output: o.clone(), cfg: acfg(Mode::Match), overwrite: false }, &mut |_| {});
        (r, o)
    };
    let (r, o) = reveal("first", "a.bin");
    assert!(matches!(r.unwrap(), JobResult::Revealed { layout: RevealLayout::Hidden, .. }));
    assert_eq!(std::fs::read(o).unwrap(), b"holiday plans");
    let (r, o) = reveal("second", "b.bin");
    assert!(r.is_ok());
    assert_eq!(std::fs::read(o).unwrap(), secret);
    let (r, o) = reveal("third", "c.bin");
    assert!(matches!(r, Err(stegosaur::Error::NoPayload)));
    assert!(!o.exists(), "nothing is written when recovery fails");
}

#[test]
fn split_then_join_with_a_quorum() {
    let d = tempfile::tempdir().unwrap();
    let covers: Vec<PathBuf> = (0..4).map(|i| write_png(d.path(), &format!("c{i}.png"), 96, 96, 10 + i)).collect();
    let secret_path = d.path().join("secret.bin");
    let secret = random_bytes(200, 4);
    std::fs::write(&secret_path, &secret).unwrap();
    let out_dir = d.path().join("shares");
    std::fs::create_dir(&out_dir).unwrap();
    let r = run_ok(JobSpec::Split { secret: secret_path, k: 2, pass: Secret::new("pw"), carriers: covers.clone(), out_dir: out_dir.clone(), cfg: acfg(Mode::Match), overwrite: false });
    let JobResult::Split { outputs } = r else { panic!() };
    assert_eq!(outputs.len(), 4);
    let joined = d.path().join("joined.bin");
    let r = run_ok(JobSpec::Join { carriers: vec![outputs[1].clone(), covers[0].clone(), outputs[3].clone()], pass: Secret::new("pw"), output: joined.clone(), cfg: acfg(Mode::Match), overwrite: false });
    assert!(matches!(r, JobResult::Joined { used: 3, bytes: 200, .. }));
    assert_eq!(std::fs::read(&joined).unwrap(), secret);
    let one = d.path().join("one.bin");
    assert!(matches!(
        run_err(JobSpec::Join { carriers: vec![outputs[0].clone()], pass: Secret::new("pw"), output: one.clone(), cfg: acfg(Mode::Match), overwrite: false }),
        stegosaur::Error::NoPayload
    ));
    assert!(!one.exists());
}

#[test]
fn output_safety_rules() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "cover.png", 64, 64, 1);
    let payload = d.path().join("p.bin");
    std::fs::write(&payload, b"data").unwrap();
    let embed = |out: PathBuf, overwrite: bool| JobSpec::Embed { carrier: cover.clone(), payload: payload.clone(), output: out, pass: Secret::new("pw"), cfg: acfg(Mode::Match), overwrite };

    let e = run_err(embed(cover.clone(), true));
    assert!(e.to_string().contains("overwrite an input"), "never overwrite the cover even with overwrite=true: {e}");
    let e = run_err(embed(payload.clone(), true));
    assert!(e.to_string().contains("overwrite an input"), "never overwrite the payload: {e}");
    assert_eq!(std::fs::read(&payload).unwrap(), b"data");

    let out = d.path().join("out.png");
    std::fs::write(&out, b"precious").unwrap();
    let e = run_err(embed(out.clone(), false));
    assert!(e.to_string().contains("already exists"), "{e}");
    assert_eq!(std::fs::read(&out).unwrap(), b"precious", "existing output untouched on refusal");
    run_ok(embed(out.clone(), true));
    assert_ne!(std::fs::read(&out).unwrap(), b"precious");

    assert!(run_err(embed(d.path().join("nope").join("x.png"), false)).to_string().contains("folder does not exist"));
    assert!(matches!(run_err(embed(d.path().join("out.jpg"), false)), stegosaur::Error::Unsupported(_)));
    assert!(run_err(JobSpec::Embed { carrier: cover.clone(), payload: payload.clone(), output: d.path().join("e.png"), pass: Secret::new(""), cfg: acfg(Mode::Match), overwrite: false }).to_string().contains("passphrase"));
}

#[test]
fn oversize_payload_gives_a_friendly_error_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 32, 32, 1);
    let payload = d.path().join("big.bin");
    std::fs::write(&payload, random_bytes(5000, 1)).unwrap();
    let out = d.path().join("o.png");
    let h = jobs::spawn(JobSpec::Embed { carrier: cover, payload, output: out.clone(), pass: Secret::new("pw"), cfg: acfg(Mode::Match), overwrite: false });
    let events: Vec<JobEvent> = h.events.iter().collect();
    let Some(JobEvent::Error(msg)) = events.last() else { panic!("expected a final error: {events:?}") };
    assert!(msg.contains("bigger image"), "{msg}");
    assert!(!out.exists());
}

#[test]
fn spawned_jobs_report_steps_in_order_then_one_final_event() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 96, 96, 1);
    let payload = d.path().join("p.bin");
    std::fs::write(&payload, b"hello").unwrap();
    let h = jobs::spawn(JobSpec::Embed { carrier: cover, payload, output: d.path().join("o.png"), pass: Secret::new("pw"), cfg: acfg(Mode::Match), overwrite: false });
    let events: Vec<JobEvent> = h.events.iter().collect();
    let steps: Vec<&String> = events.iter().filter_map(|e| if let JobEvent::Step(s) = e { Some(s) } else { None }).collect();
    assert_eq!(steps.len(), 3, "{events:?}");
    assert!(steps[0].contains("Reading") && steps[2].contains("Writing"));
    assert!(matches!(events.last(), Some(JobEvent::Done(JobResult::Embedded { .. }))));
    assert_eq!(events.iter().filter(|e| !matches!(e, JobEvent::Step(_))).count(), 1, "exactly one terminal event");
}

#[test]
fn analyze_job_and_audio_limits() {
    let d = tempfile::tempdir().unwrap();
    let img = write_png(d.path(), "a.png", 128, 128, 1);
    let JobResult::Analyzed { report } = run_ok(JobSpec::Analyze { image: img }) else { panic!() };
    assert_eq!(report.risk, Risk::Low);
    let wav = write_wav(d.path(), "a.wav", 16);
    assert!(matches!(run_err(JobSpec::Analyze { image: wav.clone() }), stegosaur::Error::Unsupported(_)));
    // audio can still carry data
    let payload = d.path().join("p.bin");
    std::fs::write(&payload, b"audio secret").unwrap();
    let out = d.path().join("a_1.wav");
    run_ok(JobSpec::Embed { carrier: wav, payload, output: out.clone(), pass: Secret::new("pw"), cfg: acfg(Mode::Match), overwrite: false });
    let back = d.path().join("back.bin");
    run_ok(JobSpec::Reveal { carrier: out, pass: Secret::new("pw"), output: back.clone(), cfg: acfg(Mode::Match), overwrite: false });
    assert_eq!(std::fs::read(back).unwrap(), b"audio secret");
}

#[test]
fn secrets_never_print() {
    let s = Secret::new("hunter2-hunter2");
    assert!(!format!("{s:?}").contains("hunter2"));
    let spec = JobSpec::Reveal { carrier: "a.png".into(), pass: s, output: "b".into(), cfg: acfg(Mode::Match), overwrite: false };
    assert!(!format!("{spec:?}").contains("hunter2"), "debug output of a job must not leak the passphrase");
}

#[test]
fn defaults_agree_with_the_library() {
    let a = AppConfig::default();
    let c = stegosaur::Config::default();
    assert_eq!((a.parity, a.rate, a.mode), (c.parity, c.rate, c.mode));
    let p = a.preview_config();
    assert!(p.seed.is_some() && p.kdf.log_n < a.kdf.log_n, "previews use a fixed seed and cheap key derivation");
    assert!(a.to_config().seed.is_none());
}

// ---------- cover text ----------

mod cover_text {
    use super::*;
    use stegosaur::app::text;
    use stegosaur::corpora::Style;
    use stegosaur::linguistic::bits_per_word;

    #[test]
    fn model_cache_returns_the_same_model() {
        let a = text::model(Style::Syslog, None, None);
        let b = text::model(Style::Syslog, None, None);
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert!(!std::sync::Arc::ptr_eq(&a, &text::model(Style::Syslog, Some(2), None)), "different order is a different model");
    }

    #[test]
    fn jobs_roundtrip_every_style_through_a_file() {
        let d = tempfile::tempdir().unwrap();
        for (i, style) in text::all_styles().into_iter().enumerate() {
            let out = d.path().join(format!("cover{i}.txt"));
            let r = run_ok(JobSpec::TextHide { message: b"meet at the usual place".to_vec(), pass: Secret::new("pw"), style, order: None, cfg: acfg(Mode::Match), save: Some(out.clone()), overwrite: false });
            let JobResult::TextHidden { text, words, saved } = r else { panic!() };
            assert!(words > 20 && saved.as_deref() == Some(out.as_path()), "{style:?}");
            assert_eq!(std::fs::read_to_string(&out).unwrap(), text);

            let back = d.path().join(format!("back{i}.bin"));
            let r = run_ok(JobSpec::TextReveal { text_file: out.clone(), pass: Secret::new("pw"), style, order: None, cfg: acfg(Mode::Match), save: Some(back.clone()), overwrite: false });
            let JobResult::TextRevealed { bytes, .. } = r else { panic!() };
            assert_eq!(bytes, b"meet at the usual place", "{style:?}");
            assert_eq!(std::fs::read(&back).unwrap(), bytes);
        }
    }

    #[test]
    fn wrong_style_or_passphrase_recovers_nothing_and_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("c.txt");
        run_ok(JobSpec::TextHide { message: b"secret".to_vec(), pass: Secret::new("pw"), style: Style::Syslog, order: None, cfg: acfg(Mode::Match), save: Some(out.clone()), overwrite: false });
        let target = d.path().join("x.bin");
        for (style, pass) in [(Style::Nginx, "pw"), (Style::Syslog, "other")] {
            let e = run_err(JobSpec::TextReveal { text_file: out.clone(), pass: Secret::new(pass), style, order: None, cfg: acfg(Mode::Match), save: Some(target.clone()), overwrite: false });
            assert!(matches!(e, stegosaur::Error::NoPayload), "{style:?}/{pass}");
            assert!(!target.exists());
        }
        let e = run_err(JobSpec::TextHide { message: b"x".to_vec(), pass: Secret::new("pw"), style: Style::Syslog, order: None, cfg: acfg(Mode::Match), save: Some(out.clone()), overwrite: false });
        assert!(e.to_string().contains("already exists"));
        assert!(run_err(JobSpec::TextHide { message: vec![], pass: Secret::new("pw"), style: Style::Syslog, order: None, cfg: acfg(Mode::Match), save: None, overwrite: false }).to_string().contains("message"));
    }

    #[test]
    fn shipped_capacity_table_matches_real_generation() {
        // The table in app::text was measured offline; this keeps it honest if the corpora or models change.
        for (style, order) in [(Style::Syslog, 4usize), (Style::Postgres, 2), (Style::Nginx, 2), (Style::Prose, 2), (Style::Env, 2)] {
            let data = random_bytes(200, 5);
            let t = text::hide(style, Some(order), &Secret::new("pw"), &data, KdfParams::fast()).unwrap();
            let words = t.split_whitespace().count();
            let real_bpw = bits_per_word(&*text::model(style, Some(order), None), &t);
            let table = text::stats(style, order).bits_per_word;
            let (est_words, est_lines) = text::estimate(style, order, 200);
            let lines = t.lines().count();
            println!("{style:?} order {order}: table {table:.2} bits/word, measured {real_bpw:.2}; estimate {est_words} words, actual {words}");
            assert!((real_bpw - table).abs() / table < 0.30, "{style:?}: table {table} vs measured {real_bpw}");
            assert!((est_words as f64 - words as f64).abs() / (words as f64) < 0.35, "{style:?}: estimate {est_words} vs actual {words}");
            if style != Style::Prose { assert!((est_lines as f64 - lines as f64).abs() / (lines as f64) < 0.40, "{style:?}: lines estimate {est_lines} vs actual {lines}"); }
        }
    }

    #[test]
    fn style_metadata() {
        assert!(text::adjustable_order(Style::Syslog) && !text::adjustable_order(Style::Env));
        assert_eq!(text::style_name(Style::Postgres), "postgres");
        assert_eq!(text::stats(Style::Syslog, 4).well_formed_percent, Some(95));
        assert_eq!(text::stats(Style::Prose, 2).well_formed_percent, None);
    }
}

// ---------- pixel views ----------

mod pixel_views {
    use super::*;
    use stegosaur::app::preview::{simulate_hidden, simulate_single};
    use stegosaur::app::views::render_views;

    #[test]
    fn identical_images_show_no_change() {
        let img = natural_image(64, 48, 1);
        let v = render_views(&img, &img, 1000).unwrap();
        assert_eq!((v.width, v.height), (64, 48), "small images are not upscaled");
        assert_eq!(v.changed, 0.0);
        assert!(v.diff.chunks(4).all(|p| p == [248, 243, 228, 255]), "no orange anywhere");
        assert_eq!(v.original, v.stego);
        assert!(v.original.chunks(4).all(|p| p[3] == 255), "fully opaque RGBA");
    }

    #[test]
    fn simulated_embed_changes_a_small_measurable_share() {
        let img = natural_image(160, 120, 2);
        let cfg = acfg(Mode::Match);
        let copy = simulate_single(&img, 500, &cfg).unwrap();
        let v = render_views(&img, &copy, 1000).unwrap();
        // 532 bytes of frame = 4256 bits over 57600 samples: about half the bits flip
        let expect = (500.0 + 32.0 + 4.0 + 16.0) * 8.0 / 2.0 / (160.0 * 120.0 * 3.0);
        assert!((v.changed - expect).abs() < expect * 0.25, "changed {} vs expected about {expect}", v.changed);
        assert!(v.diff.chunks(4).any(|p| p == [238, 138, 63, 255]), "changed areas are orange");
        let oranges = v.diff.chunks(4).filter(|p| p == &[238, 138, 63, 255]).count() as f64;
        assert!(oranges / (160.0 * 120.0) < 0.2, "a 500-byte payload touches a small part of the image");
        // +-1 matching only
        for (a, b) in img.samples.iter().zip(&copy.samples) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
        assert_eq!(simulate_single(&img, 10_000_000, &cfg).is_none(), true, "an oversize payload cannot be simulated");
    }

    #[test]
    fn downscaling_keeps_aspect_and_still_reports_sparse_changes() {
        let img = natural_image(400, 200, 3);
        let copy = simulate_single(&img, 300, &acfg(Mode::Match)).unwrap();
        let v = render_views(&img, &copy, 100).unwrap();
        assert_eq!((v.width, v.height), (100, 50));
        assert!(v.diff.chunks(4).any(|p| p[0] == 238), "block-based diff does not lose isolated changes when shrinking");
        assert_eq!(v.diff.len(), 100 * 50 * 4);
        assert_eq!(v.lsb.len(), v.diff.len());
        assert!(v.lsb.chunks(4).any(|p| p[0] == 46) && v.lsb.chunks(4).any(|p| p[0] == 244), "LSB plane has both bit values");
    }

    #[test]
    fn mismatched_dimensions_are_rejected() {
        assert!(render_views(&natural_image(10, 10, 1), &natural_image(10, 11, 1), 100).is_none());
    }

    #[test]
    fn hidden_simulation_changes_both_halves_equally() {
        let img = natural_image(192, 192, 4);
        let copy = simulate_hidden(&img, 100, Some(100), &acfg(Mode::Match)).unwrap();
        let n = img.samples.len();
        let changed = |range: std::ops::Range<usize>| range.clone().filter(|&i| img.samples[i] != copy.samples[i]).count() as f64 / range.len() as f64;
        let (top, bottom) = (changed(0..n / 2), changed(n / 2..n));
        assert!((top - bottom).abs() < 0.01, "slots are the same size so both halves change alike: {top} vs {bottom}");
        assert!(top > 0.02, "each slot is written in full");
    }
}
