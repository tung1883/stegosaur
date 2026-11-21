use crate::dto::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use stegosaur::analysis::{Report, Risk};
use stegosaur::app::files::{classify, scan_dir, CarrierCache, CarrierKind, FileKind};
use stegosaur::app::hide::{HideSpec, Layout, Naming, Payload};
use stegosaur::app::jobs::{self, JobEvent, JobResult, JobSpec, RevealLayout};
use stegosaur::app::preview::{fit_embed, fit_hidden, measure_hidden, measure_jpeg_hidden, measure_jpeg_single, measure_single, preview_split, simulate_hidden, simulate_single, AUDIO_NOTE, JPEG_NOTE};
use stegosaur::app::views::render_views;
use stegosaur::app::{friendly, text, AppConfig, Secret};
use stegosaur::carrier::Mode;
use stegosaur::corpora::Style;
use stegosaur::{AnyCarrier, ImageCarrier};
use zeroize::Zeroize;

fn msg(e: stegosaur::Error) -> String {
    friendly::message(&e)
}

fn cfg(mode: &str, parity: usize, rate: f64) -> AppConfig {
    AppConfig { mode: if mode == "replace" { Mode::Replace } else { Mode::Match }, parity: parity.min(254), rate: rate.clamp(0.02, 0.5), ..AppConfig::default() }
}

fn style_of(id: &str) -> Result<Style, String> {
    text::all_styles().into_iter().find(|s| text::style_name(*s) == id).ok_or_else(|| format!("Unknown cover-text style: {id}."))
}

/// Takes ownership so the original text can be wiped as soon as the wrapped copy exists.
fn secret(mut s: String) -> Secret {
    let r = Secret::new(&s);
    s.zeroize();
    r
}

fn kdf_of(name: &str) -> stegosaur::crypto::KdfParams {
    if name == "strong" { stegosaur::crypto::KdfParams::strong() } else { stegosaur::crypto::KdfParams::default() }
}

fn cfg_full(mode: &str, parity: usize, rate: f64, kdf: &str) -> AppConfig {
    AppConfig { kdf: kdf_of(kdf), ..cfg(mode, parity, rate) }
}

fn payload_of(p: PayloadDto) -> Result<Payload, String> {
    match (p.path.filter(|s| !s.trim().is_empty()), p.text) {
        (Some(path), _) => Ok(Payload::File(PathBuf::from(path))),
        (None, Some(t)) => Ok(Payload::Text(secret(t))),
        _ => Err("A secret needs a file or some text.".into()),
    }
}

fn hide_dto(done: stegosaur::app::hide::HideDone) -> ResultDto {
    ResultDto::Hide {
        layout: done.kind,
        items: done
            .items
            .into_iter()
            .map(|i| HideItemDto { what: i.what, cover: path_str(&i.cover), output: i.output.as_ref().map(|p| path_str(p)), bytes: i.bytes, report: i.report.map(report_dto), error: i.error })
            .collect(),
    }
}

fn is_text_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".txt") || l.ends_with(".log")
}

fn accepts(e: &stegosaur::app::files::Entry, accept: &str) -> bool {
    let carrier = matches!(e.kind, FileKind::Carrier(_));
    match accept {
        "cover" => carrier,
        "image" => matches!(e.kind, FileKind::Carrier(k) if !k.is_audio()),
        "reveal" => carrier || is_text_name(&e.name),
        _ => true,
    }
}

fn entry_dto(e: stegosaur::app::files::Entry) -> EntryDto {
    EntryDto {
        path: path_str(&e.path),
        name: e.name,
        kind: match e.kind {
            FileKind::Dir => "dir",
            FileKind::Carrier(_) => "carrier",
            FileKind::Unusable => "unusable",
            FileKind::Other => "other",
        },
        carrier: match e.kind {
            FileKind::Carrier(k) => Some(match k {
                CarrierKind::Png => "png",
                CarrierKind::Bmp => "bmp",
                CarrierKind::Tiff => "tiff",
                CarrierKind::Webp => "webp",
                CarrierKind::Ppm => "ppm",
                CarrierKind::Jpeg => "jpeg",
                CarrierKind::Wav => "wav",
                CarrierKind::Flac => "flac",
            }),
            _ => None,
        },
        size: e.size,
        summary: e.info.as_ref().map(|i| i.summary.clone()),
        samples: e.info.as_ref().map(|i| i.samples),
        note: e.note,
    }
}

fn report_dto(r: Report) -> ReportDto {
    ReportDto {
        rs: r.rs,
        spa: r.spa,
        chi_p: r.chi_p,
        score: r.score,
        risk: match r.risk {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        },
    }
}

fn path_str(p: &Path) -> String {
    p.display().to_string()
}

fn opt_path(s: Option<String>) -> Option<PathBuf> {
    s.filter(|s| !s.trim().is_empty()).map(PathBuf::from)
}

fn bulk_dto(report: stegosaur::app::bulk::BulkReport, checked: usize) -> ResultDto {
    use stegosaur::app::bulk::Found;
    let mut items = Vec::new();
    for e in &report.entries {
        let name = e.file.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if let Some(why) = &e.skipped {
            items.push(BulkItemDto { name, status: "skipped", detail: why.clone(), output: None, bytes: 0 });
        } else if e.found.is_empty() {
            items.push(BulkItemDto { name, status: "nothing", detail: "Nothing found".into(), output: None, bytes: 0 });
        } else {
            for f in &e.found {
                items.push(match f {
                    Found::File { layout, path, bytes, .. } => BulkItemDto { name: name.clone(), status: "file", detail: (*layout).to_string(), output: Some(path_str(path)), bytes: *bytes },
                    Found::Share { set, k, n, .. } => BulkItemDto { name: name.clone(), status: "share", detail: format!("Share of split {} (needs {k} of {n})", &set[..6]), output: None, bytes: 0 },
                });
            }
        }
    }
    let sets = report
        .sets
        .iter()
        .map(|s| BulkSetDto { set: s.set.clone(), k: s.k, n: s.n, found: s.found, missing: s.missing, output: s.output.as_ref().map(|p| path_str(p)), bytes: s.bytes })
        .collect();
    ResultDto::Bulk { out_dir: path_str(&report.out_dir), checked, items, sets }
}

fn result_dto(r: JobResult) -> ResultDto {
    match r {
        JobResult::Embedded { output, bytes, report } => ResultDto::Embedded { output: path_str(&output), bytes, report: report.map(report_dto) },
        JobResult::Hidden { output, report } => ResultDto::Hidden { output: path_str(&output), report: report.map(report_dto) },
        JobResult::Split { outputs } => ResultDto::Split { outputs: outputs.iter().map(|p| path_str(p)).collect() },
        JobResult::Revealed { output, bytes, layout } => ResultDto::Revealed { output: path_str(&output), bytes, layout: if layout == RevealLayout::Hidden { "hidden" } else { "single" } },
        JobResult::Joined { output, bytes, used } => ResultDto::Joined { output: path_str(&output), bytes, used },
        JobResult::Analyzed { report } => ResultDto::Analyzed { report: report_dto(report) },
        JobResult::Bulk { report, checked } => bulk_dto(report, checked),
        JobResult::Hide(done) => hide_dto(done),
        JobResult::TextHidden { text, words, saved } => ResultDto::TextHidden { text, words, saved: saved.map(|p| path_str(&p)) },
        JobResult::TextRevealed { bytes, saved } => ResultDto::TextRevealed { text: String::from_utf8_lossy(&bytes).into_owned(), bytes: bytes.len(), saved: saved.map(|p| path_str(&p)) },
    }
}

fn to_spec(dto: JobDto) -> Result<JobSpec, String> {
    Ok(match dto {
        JobDto::Embed { carrier, payload, output, pass, mode, parity, overwrite } => {
            let carrier = PathBuf::from(carrier);
            let (pass, c) = (secret(pass), cfg(&mode, parity, 0.1));
            let output = opt_path(output).unwrap_or_else(|| jobs::auto_output(&carrier, &pass, stegosaur::tag::Kind::Single, &c));
            JobSpec::Embed { carrier, payload: payload.into(), output, pass, cfg: c, overwrite }
        }
        JobDto::Hidden { carrier, public_file, public_pass, hidden_file, hidden_pass, output, mode, parity, rate, overwrite } => {
            let carrier = PathBuf::from(carrier);
            let (public_pass, c) = (secret(public_pass), cfg(&mode, parity, rate));
            let output = opt_path(output).unwrap_or_else(|| jobs::auto_output(&carrier, &public_pass, stegosaur::tag::Kind::Hidden, &c));
            let hidden = match (opt_path(hidden_file), hidden_pass) {
                (Some(f), Some(p)) => Some((f, secret(p))),
                (None, _) => None,
                (Some(_), None) => return Err("Enter a passphrase for the hidden file.".into()),
            };
            JobSpec::Hidden { carrier, public_file: public_file.into(), public_pass, hidden, output, cfg: c, overwrite }
        }
        JobDto::Split { secret: secret_file, k, pass, carriers, out_dir, mode, parity, overwrite } => {
            let carriers: Vec<PathBuf> = carriers.into_iter().map(PathBuf::from).collect();
            let out_dir = opt_path(out_dir).or_else(|| carriers.first().and_then(|c| c.parent().map(|p| p.to_path_buf()))).unwrap_or_default();
            JobSpec::Split { secret: secret_file.into(), k, pass: secret(pass), carriers, out_dir, cfg: cfg(&mode, parity, 0.1), overwrite }
        }
        JobDto::Bulk { inputs, passes, out_dir, recursive, names_only, text, parity, rate, kdf } => {
            let inputs: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
            let first = inputs.first().cloned().ok_or("Choose the files to open.")?;
            let base = if first.is_dir() { first.clone() } else { first.parent().map(|p| p.to_path_buf()).unwrap_or_default() };
            let out_dir = opt_path(out_dir).unwrap_or_else(|| base.join("recovered"));
            let passes: Vec<Secret> = passes.into_iter().filter(|p| !p.is_empty()).map(secret).collect();
            if passes.is_empty() {
                return Err("Enter at least one passphrase.".into());
            }
            let text = match text {
                Some(t) => {
                    let style = style_of(&t.style)?;
                    Some((style, text::adjustable_order(style).then_some(t.order).flatten()))
                }
                None => None,
            };
            JobSpec::Bulk { inputs, recursive, passes, out_dir, names_only, cfg: cfg_full("match", parity, rate, &kdf), text }
        }
        JobDto::Hide { layout, payloads, covers, second, pass, pass2, k, naming, pattern, out_dir, mode, parity, rate, kdf, overwrite } => {
            let covers: Vec<PathBuf> = covers.into_iter().map(PathBuf::from).collect();
            let mut payloads: Vec<Payload> = payloads.into_iter().map(payload_of).collect::<Result<_, _>>()?;
            let cover1 = |c: &Vec<PathBuf>| c.first().cloned().ok_or("Choose where to hide it.".to_string());
            let first = |p: &mut Vec<Payload>| if p.is_empty() { Err("Choose what to hide.".to_string()) } else { Ok(p.remove(0)) };
            let layout = match layout.as_str() {
                "single" => Layout::Single { payload: first(&mut payloads)?, cover: cover1(&covers)? },
                "same" => Layout::SameLook { payload: first(&mut payloads)?, cover: cover1(&covers)? },
                "two" => {
                    let second = payload_of(second.ok_or("Choose the second secret.")?)?;
                    let sp = pass2.filter(|p| !p.is_empty()).ok_or("Enter the second passphrase.")?;
                    Layout::Two { payload: first(&mut payloads)?, second, second_pass: secret(sp), cover: cover1(&covers)? }
                }
                "split" => Layout::Split { payload: first(&mut payloads)?, covers, k },
                "bulk" => {
                    if payloads.len() > covers.len() {
                        return Err(format!("{} files need {} covers, but you chose {}.", payloads.len(), payloads.len(), covers.len()));
                    }
                    Layout::Bulk { pairs: payloads.into_iter().zip(covers).collect() }
                }
                other => return Err(format!("Unknown layout: {other}.")),
            };
            let naming = match naming.as_str() {
                "suffix" => Naming::Suffix,
                "pattern" => Naming::Pattern(pattern.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "{name}_{n}".into())),
                _ => Naming::Ordinary,
            };
            JobSpec::Hide(HideSpec { layout, pass: secret(pass), cfg: cfg_full(&mode, parity, rate, &kdf), naming, out_dir: opt_path(out_dir), overwrite })
        }
        JobDto::Reveal { carrier, pass, output, parity, rate, overwrite } => {
            let carrier = PathBuf::from(carrier);
            let stem = carrier.file_stem().and_then(|s| s.to_str()).unwrap_or("recovered").to_string();
            let output = opt_path(output).unwrap_or_else(|| carrier.with_file_name(format!("{stem}.recovered.bin")));
            JobSpec::Reveal { carrier, pass: secret(pass), output, cfg: cfg("match", parity, rate), overwrite }
        }
        JobDto::Join { carriers, pass, output, parity, overwrite } => {
            let carriers: Vec<PathBuf> = carriers.into_iter().map(PathBuf::from).collect();
            let first = carriers.first().cloned().ok_or("Add the images that hold shares.")?;
            let output = opt_path(output).unwrap_or_else(|| first.with_file_name("recovered.bin"));
            JobSpec::Join { carriers, pass: secret(pass), output, cfg: cfg("match", parity, 0.1), overwrite }
        }
        JobDto::Analyze { image } => JobSpec::Analyze { image: image.into() },
        JobDto::TextHide { message, pass, style, order, save, overwrite } => {
            let style = style_of(&style)?;
            JobSpec::TextHide { message: message.into_bytes(), pass: secret(pass), style, order: text::adjustable_order(style).then_some(order).flatten(), cfg: AppConfig::default(), save: opt_path(save), overwrite }
        }
        JobDto::TextReveal { text_file, pass, style, order, save, overwrite } => {
            let style = style_of(&style)?;
            JobSpec::TextReveal { text_file: text_file.into(), pass: secret(pass), style, order: text::adjustable_order(style).then_some(order).flatten(), cfg: AppConfig::default(), save: opt_path(save), overwrite }
        }
    })
}

pub struct Core {
    cache: Arc<CarrierCache>,
    baselines: Mutex<HashMap<String, Report>>,
    next_job: AtomicU64,
    running: Arc<AtomicBool>,
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

impl Core {
    pub fn new() -> Self {
        Self { cache: Arc::new(CarrierCache::new(4)), baselines: Mutex::new(HashMap::new()), next_job: AtomicU64::new(1), running: Arc::new(AtomicBool::new(false)) }
    }

    pub fn defaults(&self) -> Defaults {
        let d = AppConfig::default();
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        let cwd = std::env::current_dir().map(|p| path_str(&p)).unwrap_or_default();
        let styles = text::all_styles()
            .into_iter()
            .map(|s| {
                let st = text::stats(s, s.default_order());
                StyleDto { id: text::style_name(s), adjustable_order: text::adjustable_order(s), default_order: s.default_order(), bits_per_word: st.bits_per_word, well_formed: st.well_formed_percent }
            })
            .collect();
        Defaults { parity: d.parity, rate: d.rate, mode: "match", home, cwd, styles }
    }

    /// Places to start browsing: the home folder and, on Windows, every drive.
    pub fn roots(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Ok(h) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            v.push(h);
        }
        if cfg!(windows) {
            v.extend((b'A'..=b'Z').map(|c| format!("{}:\\", c as char)).filter(|p| Path::new(p).exists()));
        } else {
            v.push("/".into());
        }
        v
    }

    /// Describes chosen files (and, for a folder, the files directly inside it) for the Hide, Reveal and Analyze lists.
    /// `accept` is "any", "cover", "image" or "reveal"; files inside a folder that do not fit are left out,
    /// while a file picked by name is always described so the screen can say why it cannot be used.
    pub fn describe(&self, paths: Vec<String>, accept: &str, recursive: bool) -> Result<Vec<EntryDto>, String> {
        let mut out: Vec<EntryDto> = Vec::new();
        for p in paths {
            let path = PathBuf::from(&p);
            if path.is_dir() && recursive {
                for f in stegosaur::app::bulk::list_files(&path, true, None).map_err(msg)? {
                    let (kind, info, note) = classify(&f);
                    let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                    let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let e = stegosaur::app::files::Entry { path: f, name, kind, size, info, note };
                    if e.name.starts_with('.') || !accepts(&e, accept) {
                        continue;
                    }
                    out.push(entry_dto(e));
                }
            } else if path.is_dir() {
                for e in scan_dir(&path).map_err(msg)? {
                    if e.kind != FileKind::Dir && accepts(&e, accept) {
                        out.push(entry_dto(e));
                    }
                }
            } else if path.is_file() {
                let (kind, info, note) = classify(&path);
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(entry_dto(stegosaur::app::files::Entry { path, name, kind, size, info, note }));
            }
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|e| seen.insert(e.path.clone()));
        Ok(out)
    }

    pub fn list(&self, path: &str) -> Result<ListingDto, String> {
        let dir = PathBuf::from(path);
        let entries = scan_dir(&dir).map_err(msg)?;
        let parent = dir.parent().map(path_str).filter(|p| !p.is_empty());
        let entries = entries
            .into_iter()
            .map(|e| EntryDto {
                path: path_str(&e.path),
                name: e.name,
                kind: match e.kind {
                    FileKind::Dir => "dir",
                    FileKind::Carrier(_) => "carrier",
                    FileKind::Unusable => "unusable",
                    FileKind::Other => "other",
                },
                carrier: match e.kind {
                    FileKind::Carrier(k) => Some(match k {
                        CarrierKind::Png => "png",
                        CarrierKind::Bmp => "bmp",
                        CarrierKind::Tiff => "tiff",
                        CarrierKind::Webp => "webp",
                        CarrierKind::Ppm => "ppm",
                        CarrierKind::Jpeg => "jpeg",
                        CarrierKind::Wav => "wav",
                        CarrierKind::Flac => "flac",
                    }),
                    _ => None,
                },
                size: e.size,
                summary: e.info.as_ref().map(|i| i.summary.clone()),
                samples: e.info.as_ref().map(|i| i.samples),
                note: e.note,
            })
            .collect();
        Ok(ListingDto { dir: path_str(&dir), parent, entries })
    }

    fn measure<F, G>(&self, path: &str, run: F, run_jpeg: G, out: &mut PreviewDto) -> Result<(), String>
    where
        F: FnOnce(&ImageCarrier, Option<Report>) -> Option<stegosaur::app::preview::Measured>,
        G: FnOnce(&stegosaur::JpegCarrier, Option<Report>) -> Option<stegosaur::app::preview::Measured>,
    {
        let loaded = self.cache.load(Path::new(path)).map_err(msg)?;
        match &*loaded {
            AnyCarrier::Wav(_) => out.detect_note = Some(AUDIO_NOTE.to_string()),
            AnyCarrier::Jpeg(j) => {
                let baseline = self.baselines.lock().unwrap().get(path).copied();
                match run_jpeg(j, baseline) {
                    Some(m) => {
                        self.baselines.lock().unwrap().insert(path.to_string(), m.before);
                        out.measured = Some(MeasuredDto { before: report_dto(m.before), after: report_dto(m.after) });
                    }
                    None => out.detect_note = Some(JPEG_NOTE.to_string()),
                }
            }
            AnyCarrier::Image(img) => {
                let baseline = self.baselines.lock().unwrap().get(path).copied();
                if let Some(m) = run(img, baseline) {
                    self.baselines.lock().unwrap().insert(path.to_string(), m.before);
                    out.measured = Some(MeasuredDto { before: report_dto(m.before), after: report_dto(m.after) });
                }
            }
        }
        Ok(())
    }

    pub fn preview(&self, r: PreviewReq) -> Result<PreviewDto, String> {
        let c = cfg(&r.mode, r.parity, r.rate);
        match r.op.as_str() {
            "embed" => {
                let path = r.carrier.ok_or("Choose a cover image.")?;
                let info = self.cache.info_exact(Path::new(&path)).map_err(msg)?;
                let need = r.payload_len.unwrap_or(0);
                let p = fit_embed(info.samples, need, &c);
                let mut d = PreviewDto { summary: Some(info.summary), samples: Some(info.samples), capacity: p.capacity, need, fits: p.fits, fill: p.fill, error: p.error, ..Default::default() };
                if r.measure && p.fits && need > 0 {
                    self.measure(&path, |img, base| measure_single(img, need, &c, base), |j, base| measure_jpeg_single(j, need, &c, base), &mut d)?;
                }
                Ok(d)
            }
            "hidden" => {
                let path = r.carrier.ok_or("Choose a cover image.")?;
                let info = self.cache.info_exact(Path::new(&path)).map_err(msg)?;
                let public = r.payload_len.unwrap_or(0);
                let p = fit_hidden(info.samples, public, r.hidden_len, &c);
                let ok = p.error.is_none();
                let mut d = PreviewDto { summary: Some(info.summary), samples: Some(info.samples), capacity: p.slot_capacity, need: public, fits: ok, fill: p.fill, error: p.error, public_fits: Some(p.public_fits), hidden_fits: p.hidden_fits, ..Default::default() };
                if r.measure && ok && public > 0 {
                    let hidden = r.hidden_len;
                    self.measure(&path, |img, base| measure_hidden(img, public, hidden, &c, base), |j, base| measure_jpeg_hidden(j, public, hidden, &c, base), &mut d)?;
                }
                Ok(d)
            }
            "split" => {
                let mut samples = Vec::new();
                for p in &r.carriers {
                    samples.push(self.cache.info_exact(Path::new(p)).map_err(msg)?.samples);
                }
                let sp = preview_split(&samples, r.secret_len.unwrap_or(0), r.k.unwrap_or(1), &c);
                Ok(PreviewDto {
                    need: sp.share_len,
                    fits: sp.error.is_none(),
                    error: sp.error.clone(),
                    split: Some(SplitDto { share_len: sp.share_len, carriers: sp.carriers.iter().map(|c| SplitCarrierDto { capacity: c.capacity, fits: c.fits }).collect() }),
                    ..Default::default()
                })
            }
            other => Err(format!("No preview for '{other}'.")),
        }
    }

    /// Four RGBA pictures in one buffer: a 12-byte header (width u32, height u32, changed-fraction f32,
    /// all little-endian) followed by original, stego, difference and LSB-plane images.
    pub fn views(&self, r: ViewsReq) -> Result<Vec<u8>, String> {
        let c = cfg(&r.mode, r.parity, r.rate);
        let loaded = self.cache.load(Path::new(&r.carrier)).map_err(msg)?;
        let need = r.payload_len.unwrap_or(0);
        let fits = "The payload does not fit in this image.";
        let (orig, stego): (ImageCarrier, ImageCarrier) = match &*loaded {
            AnyCarrier::Image(orig) => {
                let stego = match &r.stego {
                    Some(p) => pixels_of(&*self.cache.load(Path::new(p)).map_err(msg)?)?,
                    None => {
                        let sim = if r.op == "hidden" { simulate_hidden(orig, need, r.hidden_len, &c) } else { simulate_single(orig, need, &c) };
                        sim.ok_or(fits)?
                    }
                };
                (orig.clone(), stego)
            }
            AnyCarrier::Jpeg(j) => {
                let stego = match &r.stego {
                    Some(p) => pixels_of(&*self.cache.load(Path::new(p)).map_err(msg)?)?,
                    None => {
                        let sim = if r.op == "hidden" { simulate_hidden(j, need, r.hidden_len, &c) } else { simulate_single(j, need, &c) };
                        sim.ok_or(fits)?.pixels().map_err(msg)?
                    }
                };
                (j.pixels().map_err(msg)?, stego)
            }
            AnyCarrier::Wav(_) => return Err("Pixel views are only available for images.".into()),
        };
        let (orig, stego) = (&orig, &stego);
        let v = render_views(orig, stego, r.max_dim).ok_or("The two images have different sizes.")?;
        let mut out = Vec::with_capacity(12 + 4 * v.original.len());
        out.extend_from_slice(&v.width.to_le_bytes());
        out.extend_from_slice(&v.height.to_le_bytes());
        out.extend_from_slice(&(v.changed as f32).to_le_bytes());
        for part in [&v.original, &v.stego, &v.diff, &v.lsb] {
            out.extend_from_slice(part);
        }
        Ok(out)
    }

    /// Starts a job and returns immediately; progress arrives through `emit`.
    /// Only one job runs at a time, so a UI never has to match events to jobs.
    pub fn start_job(&self, dto: JobDto, emit: impl Fn(EventDto) + Send + 'static) -> Result<u64, String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("Another job is still running.".into());
        }
        let spec = match to_spec(dto) {
            Ok(s) => s,
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };
        let id = self.next_job.fetch_add(1, Ordering::SeqCst);
        let (handle, running) = (jobs::spawn(spec), self.running.clone());
        std::thread::spawn(move || {
            for ev in handle.events.iter() {
                let last = !matches!(ev, JobEvent::Step(_));
                let out = match ev {
                    JobEvent::Step(message) => EventDto::Step { id, message },
                    JobEvent::Done(r) => EventDto::Done { id, result: result_dto(r) },
                    JobEvent::Error(message) => EventDto::Error { id, message },
                };
                if last {
                    running.store(false, Ordering::SeqCst);
                }
                emit(out);
            }
            running.store(false, Ordering::SeqCst);
        });
        Ok(id)
    }

    /// Writes generated cover text to a file the person chose. Never overwrites a different kind of file silently:
    /// the save window has already asked about replacing.
    pub fn save_text(&self, path: &str, text: &str) -> Result<(), String> {
        let p = PathBuf::from(path);
        if p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() != Some("txt") {
            return Err("Save the text as a .txt file.".into());
        }
        std::fs::write(&p, text).map_err(|e| format!("Could not save the file: {e}."))
    }

    pub fn text_estimate(&self, r: TextEstimateReq) -> Result<TextEstimateDto, String> {
        let style = style_of(&r.style)?;
        let order = if text::adjustable_order(style) { r.order.unwrap_or(style.default_order()) } else { style.default_order() };
        let st = text::stats(style, order);
        let (words, lines) = text::estimate(style, order, r.len);
        Ok(TextEstimateDto { words, lines, bits_per_word: st.bits_per_word, well_formed: st.well_formed_percent })
    }
}

/// Decoded pixels of any image-like carrier, for side-by-side views.
fn pixels_of(c: &AnyCarrier) -> Result<ImageCarrier, String> {
    match c {
        AnyCarrier::Image(i) => Ok(i.clone()),
        AnyCarrier::Jpeg(j) => j.pixels().map_err(msg),
        AnyCarrier::Wav(_) => Err("The output is not an image.".into()),
    }
}
