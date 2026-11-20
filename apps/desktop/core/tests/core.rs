use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use stegosaur::container::{deniable_capacity_for_len, single_capacity_for_len};
use stegosaur::ImageCarrier;
use stegosaur_desktop_core::dto::*;
use stegosaur_desktop_core::Core;

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

fn png(dir: &Path, name: &str, w: u32, h: u32, seed: u32) -> PathBuf {
    let p = dir.join(name);
    noise_image(w, h, seed).save(&p).unwrap();
    p
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

fn data(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(29).wrapping_add(seed)).collect()
}

/// Runs a job from its JSON form and returns every event as JSON, ending with `done` or `error`.
fn run_json(core: &Core, job: Value) -> Vec<Value> {
    let dto: JobDto = serde_json::from_value(job).expect("valid job JSON");
    let (tx, rx) = mpsc::channel();
    core.start_job(dto, move |e| {
        let _ = tx.send(serde_json::to_value(&e).unwrap());
    })
    .expect("job starts");
    let mut events = Vec::new();
    loop {
        let e = rx.recv_timeout(Duration::from_secs(30)).expect("job finishes");
        let last = e["event"] != "step";
        events.push(e);
        if last {
            return events;
        }
    }
}

fn last(events: &[Value]) -> &Value {
    events.last().unwrap()
}

// ---------- listing and defaults ----------

#[test]
fn listing_orders_and_classifies_files() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("Sub")).unwrap();
    png(d.path(), "a.png", 40, 30, 1);
    file(d.path(), "n.txt", b"x");
    file(d.path(), "p.jpg", b"jpg");
    let l = Core::new().list(&s(d.path())).unwrap();
    assert_eq!(l.entries[0].name, "Sub");
    assert_eq!(l.entries[0].kind, "dir");
    let get = |n: &str| l.entries.iter().find(|e| e.name == n).unwrap();
    assert_eq!((get("a.png").kind, get("a.png").carrier), ("carrier", Some("png")));
    assert_eq!(get("a.png").summary.as_deref(), Some("40x30"));
    assert_eq!(get("a.png").samples, Some(40 * 30 * 3));
    assert_eq!(get("n.txt").kind, "other");
    assert_eq!(get("p.jpg").kind, "unusable");
    assert!(get("p.jpg").note.as_ref().unwrap().to_lowercase().contains("jpeg"), "a damaged JPEG says why it is unusable");
    assert!(l.parent.is_some());
    assert!(Core::new().list(&s(&d.path().join("missing"))).is_err());
}

#[test]
fn defaults_and_roots() {
    let c = Core::new();
    let d = c.defaults();
    assert_eq!((d.parity, d.mode), (16, "match"));
    assert!((d.rate - 0.10).abs() < 1e-9);
    let names: Vec<&str> = d.styles.iter().map(|s| s.id).collect();
    assert_eq!(names, ["prose", "syslog", "postgres", "nginx", "env"]);
    let syslog = d.styles.iter().find(|s| s.id == "syslog").unwrap();
    assert!(syslog.adjustable_order && syslog.default_order == 4 && syslog.well_formed == Some(95));
    assert!(!d.styles.iter().find(|s| s.id == "env").unwrap().adjustable_order);
    assert!(!c.roots().is_empty());
}

// ---------- previews ----------

fn embed_req(carrier: &Path, payload_len: usize, mode: &str, measure: bool) -> PreviewReq {
    serde_json::from_value(json!({"op": "embed", "carrier": s(carrier), "payload_len": payload_len, "mode": mode, "measure": measure})).unwrap()
}

#[test]
fn embed_preview_matches_library_capacity_and_flags_oversize() {
    let d = tempfile::tempdir().unwrap();
    let p = png(d.path(), "a.png", 128, 96, 1);
    let core = Core::new();
    let r = core.preview(embed_req(&p, 100, "match", false)).unwrap();
    let cfg = stegosaur::app::AppConfig::default().to_config();
    assert_eq!(r.capacity, single_capacity_for_len(128 * 96 * 3, &cfg));
    assert!(r.fits && r.error.is_none() && r.measured.is_none(), "no measurement unless asked");
    let big = core.preview(embed_req(&p, r.capacity + 1, "match", true)).unwrap();
    assert!(!big.fits && big.error.unwrap().contains("bigger image") && big.measured.is_none());
}

#[test]
fn embed_preview_measures_and_replace_reads_worse_than_match() {
    let d = tempfile::tempdir().unwrap();
    let p = png(d.path(), "a.png", 256, 256, 2);
    let core = Core::new();
    let cap = core.preview(embed_req(&p, 0, "match", false)).unwrap().capacity;
    let need = cap * 3 / 4;
    let (rep, mat) = (core.preview(embed_req(&p, need, "replace", true)).unwrap(), core.preview(embed_req(&p, need, "match", true)).unwrap());
    let (rep, mat) = (rep.measured.unwrap(), mat.measured.unwrap());
    assert_eq!(rep.after.risk, "high");
    assert_eq!(mat.after.risk, "low");
    assert_eq!(rep.before.risk, "low", "the untouched image reads clean");
    assert!(rep.after.score > mat.after.score + 30.0);
}

#[test]
fn hidden_and_split_previews() {
    let d = tempfile::tempdir().unwrap();
    let p = png(d.path(), "a.png", 192, 192, 3);
    let core = Core::new();
    let req = |v: Value| -> PreviewReq { serde_json::from_value(v).unwrap() };
    let cap = deniable_capacity_for_len(192 * 192 * 3, &stegosaur::app::AppConfig::default().to_config());
    let ok = core.preview(req(json!({"op":"hidden","carrier":s(&p),"payload_len":100,"hidden_len":100,"measure":true}))).unwrap();
    assert_eq!((ok.capacity, ok.public_fits, ok.hidden_fits), (cap, Some(true), Some(true)));
    assert!(ok.measured.is_some());
    let bad = core.preview(req(json!({"op":"hidden","carrier":s(&p),"payload_len":100,"hidden_len":cap+1}))).unwrap();
    assert!(!bad.fits && bad.hidden_fits == Some(false) && bad.error.unwrap().contains("hidden file"));

    let q = png(d.path(), "b.png", 96, 96, 4);
    let sp = core.preview(req(json!({"op":"split","carriers":[s(&p), s(&q)],"secret_len":200,"k":2}))).unwrap();
    let split = sp.split.unwrap();
    assert_eq!((split.share_len, split.carriers.len(), sp.fits), (200 + 48, 2, true));
    let too_many = core.preview(req(json!({"op":"split","carriers":[s(&p)],"secret_len":10,"k":3}))).unwrap();
    assert!(!too_many.fits && too_many.error.unwrap().contains("between 1 and 1"));
    assert!(core.preview(req(json!({"op":"reveal"}))).is_err());
}

#[test]
fn audio_previews_carry_a_note_instead_of_a_measurement() {
    let d = tempfile::tempdir().unwrap();
    let wav = d.path().join("a.wav");
    let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    for i in 0..20000 {
        w.write_sample(((i as f64 / 20.0).sin() * 9000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    let r = Core::new().preview(embed_req(&wav, 100, "match", true)).unwrap();
    assert!(r.fits && r.measured.is_none());
    assert!(r.detect_note.unwrap().contains("images"));
}

// ---------- views ----------

fn views_req(v: Value) -> ViewsReq {
    serde_json::from_value(v).unwrap()
}

fn header(b: &[u8]) -> (u32, u32, f32) {
    (u32::from_le_bytes(b[0..4].try_into().unwrap()), u32::from_le_bytes(b[4..8].try_into().unwrap()), f32::from_le_bytes(b[8..12].try_into().unwrap()))
}

#[test]
fn simulated_views_have_the_documented_layout() {
    let d = tempfile::tempdir().unwrap();
    let p = png(d.path(), "a.png", 200, 100, 5);
    let b = Core::new().views(views_req(json!({"op":"embed","carrier":s(&p),"payload_len":400,"max_dim":100}))).unwrap();
    let (w, h, changed) = header(&b);
    assert_eq!((w, h), (100, 50));
    assert_eq!(b.len(), 12 + 4 * (w * h * 4) as usize, "header plus four RGBA images");
    assert!(changed > 0.0 && changed < 0.2, "{changed}");
    let diff = &b[12 + 2 * (w * h * 4) as usize..12 + 3 * (w * h * 4) as usize];
    assert!(diff.chunks(4).any(|p| p[0] == 238), "changed areas are orange");
    assert!(Core::new().views(views_req(json!({"op":"embed","carrier":s(&p),"payload_len":10_000_000}))).is_err(), "oversize payload cannot be shown");
}

#[test]
fn views_can_compare_against_a_real_output() {
    let d = tempfile::tempdir().unwrap();
    let cover = png(d.path(), "c.png", 160, 120, 6);
    let secret = file(d.path(), "s.bin", &data(600, 1));
    let core = Core::new();
    let ev = run_json(&core, json!({"kind":"embed","carrier":s(&cover),"payload":s(&secret),"pass":"pw"}));
    assert_eq!(last(&ev)["event"], "done");
    let out = std::path::PathBuf::from(last(&ev)["result"]["output"].as_str().unwrap());
    let b = core.views(views_req(json!({"op":"embed","carrier":s(&cover),"stego":s(&out)}))).unwrap();
    let (w, h, changed) = header(&b);
    assert_eq!((w, h), (160, 120));
    assert!(changed > 0.01 && changed < 0.1, "real embedding of 600 bytes changes a few percent of samples: {changed}");
    let other = png(d.path(), "other.png", 50, 50, 1);
    assert!(core.views(views_req(json!({"op":"embed","carrier":s(&cover),"stego":s(&other)}))).unwrap_err().contains("different sizes"));
}

// ---------- jobs through the JSON layer ----------

#[test]
fn embed_then_reveal_with_events_and_no_passphrase_leaks() {
    let d = tempfile::tempdir().unwrap();
    let cover = png(d.path(), "c.png", 128, 96, 7);
    let payload = data(400, 2);
    let secret = file(d.path(), "s.bin", &payload);
    let core = Core::new();

    let ev = run_json(&core, json!({"kind":"embed","carrier":s(&cover),"payload":s(&secret),"pass":"hunter2-hunter2"}));
    let kinds: Vec<&str> = ev.iter().map(|e| e["event"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["step", "step", "step", "done"], "{ev:?}");
    let done = last(&ev);
    assert_eq!(done["result"]["kind"], "embedded");
    assert_eq!(done["result"]["bytes"], 400);
    assert!(done["result"]["report"]["score"].is_number());
    assert!(!serde_json::to_string(&ev).unwrap().contains("hunter2"), "no event may contain the passphrase");

    let ev = run_json(&core, json!({"kind":"reveal","carrier": done["result"]["output"], "pass":"hunter2-hunter2"}));
    assert_eq!(last(&ev)["result"]["kind"], "revealed");
    assert_eq!(last(&ev)["result"]["layout"], "single");
    let stego = std::path::PathBuf::from(done["result"]["output"].as_str().unwrap());
    let stem = stego.file_stem().unwrap().to_str().unwrap();
    assert!(!stem.contains("stego") && stem.chars().filter(|c| c.is_ascii_digit()).count() >= 6, "camera-style name: {stem}");
    assert_eq!(std::fs::read(stego.with_file_name(format!("{stem}.recovered.bin"))).unwrap(), payload);

    let ev = run_json(&core, json!({"kind":"reveal","carrier": done["result"]["output"], "pass":"wrong", "output": s(&d.path().join("x.bin"))}));
    assert_eq!(last(&ev)["event"], "error");
    assert!(last(&ev)["message"].as_str().unwrap().contains("Nothing could be recovered"));
    assert!(!d.path().join("x.bin").exists());
}

#[test]
fn hidden_layout_round_trips_both_files() {
    let d = tempfile::tempdir().unwrap();
    let cover = png(d.path(), "c.png", 256, 192, 8);
    let public = file(d.path(), "pub.txt", b"holiday plans");
    let hidden_data = data(300, 5);
    let hidden = file(d.path(), "hid.bin", &hidden_data);
    let core = Core::new();
    let ev = run_json(&core, json!({"kind":"hidden","carrier":s(&cover),"public_file":s(&public),"public_pass":"first","hidden_file":s(&hidden),"hidden_pass":"second"}));
    let out = last(&ev)["result"]["output"].clone();
    assert_eq!(last(&ev)["result"]["kind"], "hidden");
    for (pass, name, expect) in [("first", "a.bin", b"holiday plans".to_vec()), ("second", "b.bin", hidden_data)] {
        let ev = run_json(&core, json!({"kind":"reveal","carrier":out,"pass":pass,"output":s(&d.path().join(name))}));
        assert_eq!(last(&ev)["result"]["layout"], "hidden", "{pass}");
        assert_eq!(std::fs::read(d.path().join(name)).unwrap(), expect);
    }
    let missing = serde_json::from_value::<JobDto>(json!({"kind":"hidden","carrier":s(&cover),"public_file":s(&public),"public_pass":"a","hidden_file":s(&hidden)})).unwrap();
    assert!(Core::new().start_job(missing, |_| {}).unwrap_err().contains("passphrase for the hidden file"));
}

#[test]
fn split_and_join() {
    let d = tempfile::tempdir().unwrap();
    let covers: Vec<PathBuf> = (0..3).map(|i| png(d.path(), &format!("c{i}.png"), 96, 96, 20 + i)).collect();
    let secret_data = data(150, 9);
    let secret = file(d.path(), "s.bin", &secret_data);
    let core = Core::new();
    let ev = run_json(&core, json!({"kind":"split","secret":s(&secret),"k":2,"pass":"pw","carriers":covers.iter().map(|p| s(p)).collect::<Vec<_>>()}));
    assert_eq!(last(&ev)["result"]["kind"], "split");
    let outputs = last(&ev)["result"]["outputs"].as_array().unwrap().clone();
    assert_eq!(outputs.len(), 3);
    let ev = run_json(&core, json!({"kind":"join","carriers":[outputs[0], outputs[2]],"pass":"pw"}));
    assert_eq!(last(&ev)["result"]["used"], 2);
    assert_eq!(std::fs::read(d.path().join("recovered.bin")).unwrap(), secret_data);
}

#[test]
fn analyze_and_cover_text_jobs() {
    let d = tempfile::tempdir().unwrap();
    let img = png(d.path(), "a.png", 128, 128, 10);
    let core = Core::new();
    let ev = run_json(&core, json!({"kind":"analyze","image":s(&img)}));
    assert_eq!(last(&ev)["result"]["report"]["risk"], "low");

    let ev = run_json(&core, json!({"kind":"text_hide","message":"meet at the usual place","pass":"pw","style":"syslog","order":4,"save":s(&d.path().join("c.log"))}));
    let text = last(&ev)["result"]["text"].as_str().unwrap().to_string();
    assert!(text.lines().count() > 3 && last(&ev)["result"]["words"].as_u64().unwrap() > 50);
    let ev = run_json(&core, json!({"kind":"text_reveal","text_file":s(&d.path().join("c.log")),"pass":"pw","style":"syslog","order":4}));
    assert_eq!(last(&ev)["result"]["text"], "meet at the usual place");
    let bad = serde_json::from_value::<JobDto>(json!({"kind":"text_hide","message":"x","pass":"pw","style":"klingon"})).unwrap();
    assert!(Core::new().start_job(bad, |_| {}).unwrap_err().contains("Unknown cover-text style"));
}

#[test]
fn only_one_job_runs_at_a_time_and_failures_free_the_slot() {
    let d = tempfile::tempdir().unwrap();
    let cover = png(d.path(), "c.png", 96, 96, 11);
    let secret = file(d.path(), "s.bin", b"data");
    let core = Core::new();
    let job = |out: &str| serde_json::from_value::<JobDto>(json!({"kind":"embed","carrier":s(&cover),"payload":s(&secret),"pass":"pw","output":s(&d.path().join(out))})).unwrap();
    let (tx, rx) = mpsc::channel();
    core.start_job(job("a.png"), move |e| {
        let _ = tx.send(serde_json::to_value(&e).unwrap());
    })
    .unwrap();
    assert!(core.start_job(job("b.png"), |_| {}).unwrap_err().contains("still running"), "the real key derivation takes tens of milliseconds");
    while rx.recv_timeout(Duration::from_secs(30)).unwrap()["event"] == "step" {}
    // the slot is free again, and a job that fails also releases it
    let ev = run_json(&core, json!({"kind":"embed","carrier":s(&cover),"payload":s(&secret),"pass":"","output":s(&d.path().join("c.png"))}));
    assert_eq!(last(&ev)["event"], "error");
    let ev = run_json(&core, json!({"kind":"analyze","image":s(&cover)}));
    assert_eq!(last(&ev)["event"], "done");
}

// ---------- JSON shapes ----------

#[test]
fn requests_use_defaults_and_unknown_jobs_are_rejected() {
    let j: JobDto = serde_json::from_value(json!({"kind":"embed","carrier":"a.png","payload":"p","pass":"x"})).unwrap();
    assert!(matches!(j, JobDto::Embed { ref mode, parity: 16, overwrite: false, .. } if mode == "match"));
    let r: PreviewReq = serde_json::from_value(json!({"op":"embed"})).unwrap();
    assert_eq!((r.parity, r.measure, r.carriers.len()), (16, false, 0));
    assert!(serde_json::from_value::<JobDto>(json!({"kind":"detonate"})).is_err());
    assert!(serde_json::from_value::<JobDto>(json!({"kind":"embed","carrier":"a.png"})).is_err(), "required fields are enforced");
}

#[test]
fn debug_output_of_a_job_never_contains_a_passphrase() {
    let j: JobDto = serde_json::from_value(json!({"kind":"embed","carrier":"a.png","payload":"p","pass":"correct-horse-battery"})).unwrap();
    let shown = format!("{j:?}");
    assert!(!shown.contains("correct-horse") && shown.contains("embed"), "{shown}");
}

#[test]
fn cover_text_estimates() {
    let c = Core::new();
    let e = c.text_estimate(serde_json::from_value(json!({"style":"syslog","order":4,"len":30})).unwrap()).unwrap();
    assert!((e.bits_per_word - 0.85).abs() < 1e-9 && e.well_formed == Some(95) && e.words > 400 && e.lines > 20);
    let env = c.text_estimate(serde_json::from_value(json!({"style":"env","order":6,"len":30})).unwrap()).unwrap();
    assert!((env.bits_per_word - 0.57).abs() < 1e-9, "a fixed-order style ignores the requested order");
    assert!(c.text_estimate(serde_json::from_value(json!({"style":"nope","len":1})).unwrap()).is_err());
}
