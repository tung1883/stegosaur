use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use std::path::{Path, PathBuf};
use std::time::Duration;
use stegosaur::ImageCarrier;
use stegosaur_tui::app::{row_name, App, HideKind, HitKind, LogKind, Msg, Screen};
use stegosaur_tui::form::Id;
use stegosaur_tui::ui;

const WAIT: Duration = Duration::from_secs(20);

fn noise_image(w: u32, h: u32, seed: u32) -> ImageCarrier {
    let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
    let mut samples = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3u32 {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let base = 128.0 + 55.0 * ((x as f64 / 41.0) + c as f64).sin() * ((y as f64 / 57.0) + 1.0).cos();
                let n = ((s >> 24) as f64 / 255.0 - 0.5) * 5.0;
                samples.push((base + n).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    ImageCarrier::from_rgb(w, h, samples).unwrap()
}

fn write_png(dir: &Path, name: &str, w: u32, h: u32, seed: u32) -> PathBuf {
    let p = dir.join(name);
    noise_image(w, h, seed).save(&p).unwrap();
    p
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn payload(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

fn key(app: &mut App, code: KeyCode) {
    app.update(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn focus(app: &mut App, id: Id) {
    app.form_mut().focus_id(id);
}

fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    let buf = t.backend().buffer().clone();
    let mut s = String::new();
    for y in 0..h {
        for x in 0..w {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

fn last_ok(app: &App) -> Option<String> {
    app.log.iter().rev().find(|l| l.kind == LogKind::Ok).map(|l| l.text.clone())
}

fn last_err(app: &App) -> Option<String> {
    app.log.iter().rev().find(|l| l.kind == LogKind::Err).map(|l| l.text.clone())
}

fn fresh(dir: &Path) -> App {
    App::new(dir.to_path_buf())
}

// ---------- embed and reveal through the UI ----------

#[test]
fn hide_then_reveal_through_the_ui() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "cover.png", 160, 120, 1);
    let data = payload(500, 3);
    let secret = write_file(d.path(), "secret.bin", &data);
    let mut app = fresh(d.path());
    let before = snapshot(d.path());

    app.form_mut().set_path(Id::Carrier, Some(cover.clone()));
    app.form_mut().set_path(Id::Payload, Some(secret));
    app.after_change();
    assert!(app.derived.embed.as_ref().unwrap().fits);
    assert!(app.derived.problems.iter().any(|p| p.contains("passphrase")), "missing passphrase is reported: {:?}", app.derived.problems);

    focus(&mut app, Id::Pass);
    typ(&mut app, "correct horse");
    assert!(app.derived.problems.is_empty(), "{:?}", app.derived.problems);
    assert!(app.wait_idle(WAIT));
    let m = app.measured.expect("a measurement arrives for a fitting payload");
    assert!(m.after.score >= 0.0);
    assert!(!app.derived.command.contains("correct horse"), "the equivalent command never shows the passphrase");
    assert!(app.derived.command.contains("--pass ******"));

    focus(&mut app, Id::Run);
    key(&mut app, KeyCode::Enter);
    assert!(app.busy());
    assert!(app.wait_idle(WAIT));
    let ok = last_ok(&app).expect("success is logged");
    assert!(ok.contains("Saved") && ok.contains("500 bytes"), "{ok}");
    let out = new_png(d.path(), &before);
    assert!(out.exists());
    assert_eq!(app.form().text(Id::Pass), "", "the passphrase is wiped after a successful run");

    // reveal it
    app.set_screen(Screen::Reveal.index());
    app.form_mut().set_path(Id::Carrier, Some(out.clone()));
    app.after_change();
    focus(&mut app, Id::Pass);
    typ(&mut app, "correct horse");
    focus(&mut app, Id::Run);
    key(&mut app, KeyCode::Enter);
    assert!(app.wait_idle(WAIT));
    assert!(last_ok(&app).unwrap().contains("Recovered 500 bytes"));
    assert_eq!(std::fs::read(recovered_of(&out)).unwrap(), data);
}

#[test]
fn wrong_passphrase_reveals_nothing_and_keeps_the_field() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 128, 96, 2);
    let secret = write_file(d.path(), "s.bin", &payload(100, 1));
    let mut app = fresh(d.path());
    let before = snapshot(d.path());
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(secret));
    focus(&mut app, Id::Pass);
    typ(&mut app, "right");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));

    app.set_screen(Screen::Reveal.index());
    app.form_mut().set_path(Id::Carrier, Some(new_png(d.path(), &before)));
    focus(&mut app, Id::Pass);
    typ(&mut app, "wrong");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    let e = last_err(&app).expect("an error is logged");
    assert!(e.contains("Nothing could be recovered"), "{e}");
    assert_eq!(app.form().text(Id::Pass), "wrong", "a failed run keeps the passphrase so it can be corrected");
    assert!(!recovered_of(&new_png(d.path(), &before)).exists());
}

#[test]
fn validation_blocks_the_run_with_a_reason() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "tiny.png", 32, 32, 3);
    let big = write_file(d.path(), "big.bin", &payload(5000, 9));
    let mut app = fresh(d.path());

    focus(&mut app, Id::Run);
    key(&mut app, KeyCode::Enter);
    assert!(last_err(&app).unwrap().contains("Choose a cover image"), "{:?}", last_err(&app));
    assert!(!app.busy());

    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(big));
    focus(&mut app, Id::Pass);
    typ(&mut app, "pw");
    app.after_change();
    assert!(app.derived.problems.iter().any(|p| p.contains("bigger image")), "{:?}", app.derived.problems);
    assert!(!app.derived.embed.as_ref().unwrap().fits);
    focus(&mut app, Id::Run);
    key(&mut app, KeyCode::Enter);
    assert!(!app.busy(), "an oversize payload never starts a job");
    assert!(app.measured.is_none());
    assert!(app.wait_idle(WAIT), "no measurement is scheduled for a payload that does not fit");
}

#[test]
fn existing_output_is_refused_not_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 96, 96, 4);
    let secret = write_file(d.path(), "s.bin", b"hello");
    write_file(d.path(), "c_1.png", b"precious");
    let mut app = fresh(d.path());
    app.form_mut().set_text(Id::Output, d.path().join("c_1.png").to_str().unwrap());
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(secret));
    focus(&mut app, Id::Pass);
    typ(&mut app, "pw");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert!(last_err(&app).unwrap().contains("already exists"));
    assert_eq!(std::fs::read(d.path().join("c_1.png")).unwrap(), b"precious");
    assert_eq!(app.form().text(Id::Pass), "pw");
}

// ---------- hidden layout ----------

#[test]
fn second_file_switch_two_passphrases_two_files() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "cover.png", 256, 192, 5);
    let public = write_file(d.path(), "public.txt", b"holiday plans");
    let hidden_data = payload(300, 7);
    let hidden = write_file(d.path(), "hidden.bin", &hidden_data);
    let mut app = fresh(d.path());
    let before = snapshot(d.path());
    assert_eq!(app.screen, Screen::Hide);

    assert!(app.form().get(Id::HiddenPass).hidden, "the second passphrase appears only once the second-file switch is on");
    assert_eq!(app.hide_kind(), HideKind::Embed);
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(public));
    focus(&mut app, Id::Pass);
    typ(&mut app, "first");
    focus(&mut app, Id::Second);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.hide_kind(), HideKind::Hidden);
    assert_eq!(app.form().focused().label, "Add a second file");
    assert!(!app.form().get(Id::HiddenPass).hidden && !app.form().get(Id::HiddenFile).hidden && !app.form().get(Id::Slot).hidden);
    assert!(app.form().get(Id::TwoSlot).hidden, "the look-the-same option is implied once a second file is added");
    assert_eq!(app.form().get(Id::Pass).label, "First passphrase");
    assert_eq!(app.form().get(Id::Run).label, "Hide both");
    app.form_mut().set_path(Id::HiddenFile, Some(hidden));
    app.after_change();
    assert!(app.derived.problems.iter().any(|p| p.contains("second passphrase")), "{:?}", app.derived.problems);
    focus(&mut app, Id::HiddenPass);
    typ(&mut app, "first");
    assert!(app.derived.problems.iter().any(|p| p.contains("must differ")), "{:?}", app.derived.problems);
    app.form_mut().get_mut(Id::HiddenPass).clear();
    typ(&mut app, "second");
    assert!(app.derived.problems.is_empty(), "{:?}", app.derived.problems);
    assert!(app.wait_idle(WAIT));
    assert!(app.measured.is_some());

    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert!(last_ok(&app).unwrap().contains("Saved"));
    let out = new_png(d.path(), &before);

    for (pass, name, expect) in [("first", "a.bin", b"holiday plans".to_vec()), ("second", "b.bin", hidden_data)] {
        app.set_screen(Screen::Reveal.index());
        app.form_mut().set_path(Id::Carrier, Some(out.clone()));
        app.form_mut().set_text(Id::Output, d.path().join(name).to_str().unwrap());
        app.form_mut().get_mut(Id::Pass).clear();
        focus(&mut app, Id::Pass);
        typ(&mut app, pass);
        app.after_change();
        app.run_current();
        assert!(app.wait_idle(WAIT));
        assert!(last_ok(&app).unwrap().contains("Hidden layout"), "{pass}");
        assert_eq!(std::fs::read(d.path().join(name)).unwrap(), expect, "{pass}");
    }
}

#[test]
fn look_the_same_option_uses_the_two_slot_layout_for_one_file() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 256, 192, 3);
    let data = payload(400, 5);
    let file = write_file(d.path(), "f.bin", &data);
    let mut app = fresh(d.path());
    let before = snapshot(d.path());
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(file));
    assert!(app.form().get(Id::Slot).hidden && !app.form().get(Id::TwoSlot).hidden);
    focus(&mut app, Id::TwoSlot);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.hide_kind(), HideKind::Hidden);
    assert!(!app.form().get(Id::Slot).hidden && app.form().get(Id::HiddenFile).hidden, "slot size shows, but no second file is needed");
    assert_eq!(app.form().get(Id::Run).label, "Hide it", "still one file");
    focus(&mut app, Id::Pass);
    typ(&mut app, "only one");
    assert!(app.derived.problems.is_empty(), "{:?}", app.derived.problems);
    assert!(app.derived.command.contains("hidden-embed") && !app.derived.command.contains("--hidden-data"), "{}", app.derived.command);
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert!(last_ok(&app).unwrap().contains("Saved"), "{:?}", app.log.last());

    app.set_screen(Screen::Reveal.index());
    app.form_mut().set_path(Id::Carrier, Some(new_png(d.path(), &before)));
    focus(&mut app, Id::Pass);
    typ(&mut app, "only one");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert!(last_ok(&app).unwrap().contains("Hidden layout"), "{:?}", app.log.last());
    assert_eq!(std::fs::read(recovered_of(&new_png(d.path(), &before))).unwrap(), data);
}

#[test]
fn the_cover_switch_swaps_file_fields_for_the_message_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    for id in [Id::Carrier, Id::Payload, Id::Second, Id::Mode, Id::Parity, Id::Output] {
        assert!(!app.form().get(id).hidden, "{id:?} is shown for a file cover");
    }
    for id in [Id::Message, Id::Style, Id::TextOutput] {
        assert!(app.form().get(id).hidden, "{id:?} is hidden for a file cover");
    }
    focus(&mut app, Id::Cover);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.hide_kind(), HideKind::Text);
    for id in [Id::Carrier, Id::Payload, Id::Second, Id::HiddenFile, Id::TwoSlot, Id::Slot, Id::Mode, Id::Parity, Id::Output] {
        assert!(app.form().get(id).hidden, "{id:?} is hidden for text");
    }
    for id in [Id::Message, Id::Style, Id::TextOutput] {
        assert!(!app.form().get(id).hidden, "{id:?} is shown for text");
    }
    assert_eq!(app.form().get(Id::Run).label, "Generate text");
    assert!(app.derived.problems.iter().any(|p| p.contains("message")));
    key(&mut app, KeyCode::Left);
    assert_eq!(app.hide_kind(), HideKind::Embed);
    assert_eq!(app.form().get(Id::Run).label, "Hide it");
}

#[test]
fn reveal_tells_text_files_from_images_and_audio() {
    let d = tempfile::tempdir().unwrap();
    let img = write_png(d.path(), "a.png", 64, 64, 1);
    let log = write_file(d.path(), "cover.log", b"Nov 19 08:14:02 web01 sshd[1]: x\n");
    let mut app = fresh(d.path());
    app.set_screen(Screen::Reveal.index());
    app.form_mut().set_path(Id::Carrier, Some(img));
    app.after_change();
    assert!(!app.derived.reveal_text);
    assert!(!app.form().get(Id::Parity).hidden && app.form().get(Id::Style).hidden && !app.form().get(Id::Output).hidden);
    assert!(app.derived.command.starts_with("stegosaur extract"), "{}", app.derived.command);

    app.form_mut().set_path(Id::Carrier, Some(log));
    app.after_change();
    assert!(app.derived.reveal_text);
    assert!(app.form().get(Id::Parity).hidden && app.form().get(Id::Slot).hidden && app.form().get(Id::Output).hidden);
    assert!(!app.form().get(Id::Style).hidden && !app.form().get(Id::TextOutput).hidden);
    assert!(app.derived.command.starts_with("stegosaur text-extract cover.log"), "{}", app.derived.command);
    assert!(render(&mut app, 110, 34).contains("same style and realism"));
}

#[test]
fn hidden_slot_size_changes_capacity_live() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 192, 192, 6);
    let public = write_file(d.path(), "p.bin", &payload(2000, 1));
    let mut app = fresh(d.path());
    app.form_mut().set_choice(Id::TwoSlot, 1);
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(public));
    app.after_change();
    let before = app.derived.hidden.as_ref().unwrap().slot_capacity;
    focus(&mut app, Id::Slot);
    for _ in 0..3 {
        key(&mut app, KeyCode::Right);
    }
    let after = app.derived.hidden.as_ref().unwrap().slot_capacity;
    assert!(after > before, "{before} -> {after}");
}

// ---------- split / join, analyze, text ----------

#[test]
fn split_then_join_through_the_ui() {
    let d = tempfile::tempdir().unwrap();
    let secret_data = payload(200, 4);
    let secret = write_file(d.path(), "secret.bin", &secret_data);
    let covers: Vec<PathBuf> = (0..3).map(|i| write_png(d.path(), &format!("c{i}.png"), 96, 96, 10 + i)).collect();
    let mut app = fresh(d.path());
    let before = snapshot(d.path());
    app.set_screen(Screen::Split.index());
    app.form_mut().set_path(Id::Secret, Some(secret));
    focus(&mut app, Id::Carriers);
    // add carriers through the real browser
    for c in &covers {
        key(&mut app, KeyCode::Enter);
        assert!(app.browser.is_some());
        let idx = app.browser.as_ref().unwrap().rows.iter().position(|r| row_name(r) == c.file_name().unwrap().to_str().unwrap()).unwrap();
        app.browser.as_mut().unwrap().sel = idx;
        key(&mut app, KeyCode::Enter);
        assert!(app.browser.is_none());
    }
    assert_eq!(app.form().paths(Id::Carriers).len(), 3);
    focus(&mut app, Id::K);
    key(&mut app, KeyCode::Left); // 2 -> 1
    key(&mut app, KeyCode::Right); // back to 2
    focus(&mut app, Id::Pass);
    typ(&mut app, "shared");
    app.after_change();
    assert!(app.derived.split.as_ref().unwrap().error.is_none(), "{:?}", app.derived.split);
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert!(last_ok(&app).unwrap().contains("Wrote 3 images") || last_err(&app).is_some(), "{:?}", app.log.last());
    // the shares overwrite nothing: the outputs are new files
    let shares = new_files(d.path(), &before, "png");
    assert_eq!(shares.len(), 3);

    // join two of them
    app.form_mut().set_choice(Id::Direction, 1);
    app.after_change();
    assert!(app.form().get(Id::Secret).hidden && !app.form().get(Id::JoinCarriers).hidden);
    for p in [shares[0].clone(), shares[2].clone()] {
        if let stegosaur_tui::form::Kind::Paths { items, .. } = &mut app.form_mut().get_mut(Id::JoinCarriers).kind {
            items.push(p);
        }
    }
    app.form_mut().get_mut(Id::Pass).clear();
    focus(&mut app, Id::Pass);
    typ(&mut app, "shared");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert!(last_ok(&app).unwrap().contains("Recovered 200 bytes from 2 images"), "{:?}", app.log.last());
    assert_eq!(std::fs::read(d.path().join("recovered.bin")).unwrap(), secret_data);
}

#[test]
fn analyze_scans_and_compares() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "a.png", 192, 192, 8);
    let secret = write_file(d.path(), "s.bin", &payload(8000, 2));
    let mut app = fresh(d.path());
    let before = snapshot(d.path());
    app.form_mut().set_path(Id::Carrier, Some(cover.clone()));
    app.form_mut().set_path(Id::Payload, Some(secret));
    focus(&mut app, Id::Pass);
    typ(&mut app, "pw");
    app.after_change();
    app.form_mut().set_choice(Id::Mode, 1); // replace mode: clearly visible
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    let stego = new_png(d.path(), &before);

    app.set_screen(Screen::Analyze.index());
    app.form_mut().set_path(Id::Image, Some(stego));
    app.form_mut().set_path(Id::Compare, Some(cover));
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    let (a, b) = (app.analysis[0].expect("image scanned"), app.analysis[1].expect("original scanned"));
    assert!(a.score > b.score + 5.0, "replace-mode embedding must read higher than the original: {} vs {}", a.score, b.score);
    let screen = render(&mut app, 110, 34);
    assert!(screen.contains("Original") && screen.contains("change:"), "comparison is displayed");
}

#[test]
fn analyze_rejects_audio() {
    let d = tempfile::tempdir().unwrap();
    let wav = d.path().join("a.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    for i in 0..4000 {
        w.write_sample(((i as f64 / 9.0).sin() * 8000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    let mut app = fresh(d.path());
    app.set_screen(Screen::Analyze.index());
    app.form_mut().set_path(Id::Image, Some(wav));
    app.after_change();
    assert!(app.derived.problems.iter().any(|p| p.contains("only works on images stored as pixels")));
}

#[test]
fn cover_text_roundtrip_through_the_ui() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    app.form_mut().set_choice(Id::Cover, 1);
    app.after_change();
    focus(&mut app, Id::Pass);
    typ(&mut app, "pw");
    focus(&mut app, Id::Message);
    typ(&mut app, "meet at the usual place");
    app.form_mut().set_text(Id::TextOutput, d.path().join("cover.log").to_str().unwrap());
    app.after_change();
    let (words, lines) = app.derived.words_lines.expect("an estimate is shown");
    assert!(words > 50 && lines > 3);
    app.run_current();
    assert!(app.wait_idle(WAIT));
    let text = app.text_out.clone().expect("generated text is kept for display");
    assert!(text.lines().count() > 3);
    assert!(render(&mut app, 110, 34).contains("Generated cover text"));
    assert_eq!(std::fs::read_to_string(d.path().join("cover.log")).unwrap(), text);

    // read it back on the Reveal screen: a text file switches it to the text settings
    app.set_screen(Screen::Reveal.index());
    app.form_mut().set_path(Id::Carrier, Some(d.path().join("cover.log")));
    app.after_change();
    assert!(app.derived.reveal_text);
    assert!(!app.form().get(Id::Style).hidden && app.form().get(Id::Parity).hidden);
    focus(&mut app, Id::Pass);
    typ(&mut app, "pw");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert_eq!(app.recovered.as_deref(), Some("meet at the usual place"));
    assert!(render(&mut app, 110, 34).contains("Recovered message"));
}

#[test]
fn order_control_appears_only_for_styles_that_use_it() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    app.form_mut().set_choice(Id::Cover, 1);
    app.after_change();
    assert!(!app.form().get(Id::Order).hidden, "syslog (default) has an adjustable order");
    focus(&mut app, Id::Style);
    key(&mut app, KeyCode::Left); // syslog -> prose
    assert!(app.form().get(Id::Order).hidden);
    key(&mut app, KeyCode::Right);
    assert!(!app.form().get(Id::Order).hidden);
}

// ---------- browser ----------

#[test]
fn file_browser_navigation_and_rules() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("sub")).unwrap();
    write_png(&d.path().join("sub"), "inner.png", 32, 32, 1);
    write_png(d.path(), "a.png", 32, 32, 2);
    write_file(d.path(), "notes.txt", b"x");
    write_file(d.path(), "photo.heic", b"heic-ish");
    let mut app = fresh(d.path());

    focus(&mut app, Id::Carrier);
    key(&mut app, KeyCode::Enter);
    let names = |app: &App| app.browser.as_ref().unwrap().rows.iter().map(row_name).collect::<Vec<_>>();
    assert_eq!(names(&app)[..2], ["..", "sub"], "up-row, then folders first: {:?}", names(&app));
    let idx_of = |app: &App, n: &str| names(app).iter().position(|x| x == n).unwrap();

    let i = idx_of(&app, "photo.heic");
    app.browser.as_mut().unwrap().sel = i;
    key(&mut app, KeyCode::Enter);
    assert!(app.browser.is_some() && app.browser.as_ref().unwrap().note.as_ref().unwrap().contains("lossy"), "lossy files cannot be picked");
    let i = idx_of(&app, "notes.txt");
    app.browser.as_mut().unwrap().sel = i;
    key(&mut app, KeyCode::Enter);
    assert!(app.browser.is_some(), "a text file is not a valid carrier");

    let i = idx_of(&app, "sub");
    app.browser.as_mut().unwrap().sel = i;
    key(&mut app, KeyCode::Enter);
    assert!(names(&app).contains(&"inner.png".to_string()));
    key(&mut app, KeyCode::Backspace);
    assert!(names(&app).contains(&"a.png".to_string()), "Backspace goes up");

    let i = idx_of(&app, "a.png");
    app.browser.as_mut().unwrap().sel = i;
    key(&mut app, KeyCode::Enter);
    assert!(app.browser.is_none());
    assert_eq!(app.form().path(Id::Carrier).unwrap(), d.path().join("a.png"));

    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Esc);
    assert!(app.browser.is_none(), "Esc closes without changing anything");
    assert_eq!(app.form().path(Id::Carrier).unwrap(), d.path().join("a.png"));
    focus(&mut app, Id::Carrier);
    key(&mut app, KeyCode::Delete);
    assert!(app.form().path(Id::Carrier).is_none(), "Delete clears a path field");
}

#[test]
fn folder_picker_uses_s_to_choose() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("shares")).unwrap();
    let mut app = fresh(d.path());
    app.set_screen(Screen::Split.index());
    focus(&mut app, Id::OutDir);
    key(&mut app, KeyCode::Enter);
    let i = app.browser.as_ref().unwrap().rows.iter().position(|r| row_name(r) == "shares").unwrap();
    app.browser.as_mut().unwrap().sel = i;
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('s'));
    assert_eq!(app.form().path(Id::OutDir).unwrap(), d.path().join("shares"));
}

// ---------- keys, focus, secrets ----------

#[test]
fn keyboard_navigation_and_text_entry_rules() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    assert_eq!(app.screen, Screen::Hide);
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(app.screen, Screen::Reveal, "digits switch screens when no text field has focus");
    key(&mut app, KeyCode::Char('1'));
    focus(&mut app, Id::Pass);
    typ(&mut app, "a1b2");
    assert_eq!(app.screen, Screen::Hide);
    assert_eq!(app.form().text(Id::Pass), "a1b2", "digits and q are typed into text fields, not treated as commands");
    typ(&mut app, "q");
    assert!(!app.quit);
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.form().text(Id::Pass), "a1b2");
    app.update(Msg::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)));
    assert_eq!(app.form().text(Id::Pass), "");

    app.update(Msg::Paste("pasted secret".into()));
    assert_eq!(app.form().text(Id::Pass), "pasted secret");

    // Tab skips hidden fields on the split screen
    app.set_screen(Screen::Split.index());
    let before = app.form().focus;
    for _ in 0..30 {
        key(&mut app, KeyCode::Tab);
        assert!(!app.form().focused().hidden, "focus never lands on a hidden field");
    }
    let _ = before;

    focus(&mut app, Id::Direction);
    key(&mut app, KeyCode::Char('?'));
    assert!(app.help);
    key(&mut app, KeyCode::Char('x'));
    assert!(!app.help, "any key closes help");
    key(&mut app, KeyCode::Char('q'));
    assert!(app.quit, "q quits when no text field has focus");
}

#[test]
fn ctrl_q_always_quits() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    focus(&mut app, Id::Pass);
    app.update(Msg::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)));
    assert!(app.quit);
}

#[test]
fn passphrases_are_never_drawn() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    focus(&mut app, Id::Pass);
    typ(&mut app, "hunter2hunter2");
    for s in 0..4 {
        app.set_screen(s);
        let screen = render(&mut app, 110, 34);
        assert!(!screen.contains("hunter2"), "screen {s} must not show the passphrase");
    }
    app.set_screen(0);
    assert!(render(&mut app, 110, 34).contains("••••••••••••••"), "a masked placeholder is shown instead");
    let log: String = app.log.iter().map(|l| l.text.clone()).collect();
    assert!(!log.contains("hunter2"));
}

#[test]
fn secrets_are_wiped_on_success_only() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "c.png", 96, 96, 9);
    let secret = write_file(d.path(), "s.bin", b"data");
    let mut app = fresh(d.path());
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(secret));
    focus(&mut app, Id::Pass);
    typ(&mut app, "temporary");
    app.after_change();
    app.run_current();
    assert!(app.wait_idle(WAIT));
    assert_eq!(app.form().text(Id::Pass), "");
    assert!(!app.derived.problems.is_empty(), "with the passphrase gone the form asks for it again");
}

// ---------- rendering and mouse ----------

#[test]
fn every_screen_renders_at_the_minimum_and_a_large_size() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    for s in 0..4 {
        app.set_screen(s);
        for (w, h) in [(ui::MIN_W, ui::MIN_H), (140, 45)] {
            let screen = render(&mut app, w, h);
            assert!(screen.contains("Stegosaur") && screen.contains("Preview") && screen.contains("Log"), "screen {s} at {w}x{h}");
            for name in ["Hide", "Reveal", "Analyze", "Split / Join"] {
                assert!(screen.contains(name), "tab {name} visible at {w}x{h}");
            }
        }
    }
}

#[test]
fn too_small_a_window_shows_a_clear_message() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    let screen = render(&mut app, 60, 20);
    assert!(screen.contains("enlarge the window") && screen.contains("60x20"));
    let tiny = render(&mut app, 10, 1);
    assert!(!tiny.is_empty(), "even a one-row window does not panic");
}

#[test]
fn clicking_tabs_and_buttons_works() {
    let d = tempfile::tempdir().unwrap();
    let mut app = fresh(d.path());
    render(&mut app, 110, 34);
    let click = |app: &mut App, x: u16, y: u16| {
        app.update(Msg::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE }));
    };
    let tab = app.hits.iter().find(|h| matches!(h.kind, HitKind::Tab(2))).copied().unwrap();
    click(&mut app, tab.rect.x + 1, tab.rect.y);
    assert_eq!(app.screen, Screen::Analyze);

    render(&mut app, 110, 34);
    let btn = app.hits.iter().filter(|h| matches!(h.kind, HitKind::Field(_))).last().copied().unwrap();
    click(&mut app, btn.rect.x + 2, btn.rect.y);
    assert!(last_err(&app).unwrap().contains("Choose an image"), "clicking the button with an empty form explains what is missing");

    // clicking a choice cycles it
    app.set_screen(Screen::Hide.index());
    render(&mut app, 110, 34);
    let mode = app.hits.iter().find(|h| matches!(h.kind, HitKind::Field(i) if app.form().fields[i].id == Id::Mode)).copied().unwrap();
    click(&mut app, mode.rect.x + 30, mode.rect.y);
    assert_eq!(app.form().choice(Id::Mode), 1);

    // clicking a path field opens the browser, and a click on a file row picks it
    write_png(d.path(), "pick.png", 32, 32, 1);
    let carrier = app.hits.iter().find(|h| matches!(h.kind, HitKind::Field(i) if app.form().fields[i].id == Id::Carrier)).copied().unwrap();
    click(&mut app, carrier.rect.x + 30, carrier.rect.y);
    assert!(app.browser.is_some());
    render(&mut app, 110, 34);
    let target = app.browser.as_ref().unwrap().rows.iter().position(|r| row_name(r) == "pick.png").unwrap();
    let row = app.hits.iter().find(|h| matches!(h.kind, HitKind::BrowserRow(i) if i == target)).copied().unwrap();
    click(&mut app, row.rect.x + 3, row.rect.y);
    assert!(app.browser.is_none());
    assert_eq!(app.form().path(Id::Carrier).unwrap(), d.path().join("pick.png"));
}

#[test]
fn browser_renders_over_the_form() {
    let d = tempfile::tempdir().unwrap();
    write_png(d.path(), "visible.png", 32, 32, 1);
    let mut app = fresh(d.path());
    focus(&mut app, Id::Carrier);
    key(&mut app, KeyCode::Enter);
    let screen = render(&mut app, 110, 34);
    assert!(screen.contains("visible.png") && screen.contains("Esc cancel") && screen.contains("can carry data"));
}

#[test]
fn equivalent_command_keeps_its_flags_visible() {
    let d = tempfile::tempdir().unwrap();
    let cover = write_png(d.path(), "beach.png", 96, 96, 1);
    let secret = write_file(d.path(), "report.pdf", b"data");
    let mut app = fresh(d.path());
    app.form_mut().set_path(Id::Carrier, Some(cover));
    app.form_mut().set_path(Id::Payload, Some(secret));
    app.form_mut().set_choice(Id::Mode, 1);
    app.after_change();
    let cmd = app.derived.command.clone();
    assert_eq!(cmd, "stegosaur embed beach.png beach_1.png --data report.pdf --pass ****** --parity 16 --mode replace");
    assert!(render(&mut app, 110, 34).contains("--mode replace"), "the whole command fits on the line");
}

fn snapshot(dir: &std::path::Path) -> std::collections::HashSet<String> {
    std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()
}

/// Files with extension `ext` created since `before`, sorted by name.
fn new_files(dir: &std::path::Path, before: &std::collections::HashSet<String>, ext: &str) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext) && !before.contains(p.file_name().unwrap().to_str().unwrap()))
        .collect();
    v.sort();
    v
}

/// The one new PNG a job wrote; its camera-style name must not mention stego.
fn new_png(dir: &std::path::Path, before: &std::collections::HashSet<String>) -> std::path::PathBuf {
    let v = new_files(dir, before, "png");
    assert_eq!(v.len(), 1, "exactly one new image: {v:?}");
    assert!(!v[0].file_name().unwrap().to_str().unwrap().to_lowercase().contains("stego"));
    v[0].clone()
}

fn recovered_of(p: &std::path::Path) -> std::path::PathBuf {
    p.with_file_name(format!("{}.recovered.bin", p.file_stem().unwrap().to_str().unwrap()))
}
