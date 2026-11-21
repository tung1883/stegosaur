//! Application state and behaviour. Everything here is independent of drawing, so it can be driven by tests.

use crate::browser::{Browser, Row};
use crate::form::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use stegosaur::analysis::Report;
use stegosaur::app::files::{CarrierCache, CarrierInfo};
use stegosaur::app::jobs::{self, default_output, JobEvent, JobHandle, JobResult, JobSpec, RevealLayout};
use stegosaur::app::preview::{fit_embed, fit_hidden, measure_hidden, measure_jpeg_hidden, measure_jpeg_single, measure_single, preview_split, EmbedPreview, HiddenPreview, Measured, SplitPreview, AUDIO_NOTE};
use stegosaur::app::{text, AppConfig, Secret};
use stegosaur::carrier::Mode;
use stegosaur::AnyCarrier;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Hide,
    Reveal,
    Analyze,
    Split,
}

pub const SCREENS: [Screen; 4] = [Screen::Hide, Screen::Reveal, Screen::Analyze, Screen::Split];

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Hide => "Hide",
            Screen::Reveal => "Reveal",
            Screen::Analyze => "Analyze",
            Screen::Split => "Split / Join",
        }
    }

    pub fn index(self) -> usize {
        SCREENS.iter().position(|s| *s == self).unwrap()
    }
}

/// What the Hide screen will run, decided by the cover switch and the second-file switches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HideKind {
    /// One file, the whole cover is one slot.
    Embed,
    /// The two-slot layout (a second file, or the "look the same" option with one file).
    Hidden,
    /// A message hidden in generated text.
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    Info,
    Ok,
    Err,
    Dim,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub secs: f32,
    pub text: String,
    pub kind: LogKind,
}

#[derive(Clone, Copy, Debug)]
pub enum HitKind {
    Tab(usize),
    Field(usize),
    BrowserRow(usize),
    CloseBrowser,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub rect: Rect,
    pub kind: HitKind,
}

pub enum Msg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Tick,
}

/// Everything the preview panel shows, recomputed whenever a field changes.
#[derive(Default, Clone)]
pub struct Derived {
    pub info: Option<Result<CarrierInfo, String>>,
    pub need: Option<usize>,
    pub embed: Option<EmbedPreview>,
    pub hidden: Option<HiddenPreview>,
    pub split: Option<SplitPreview>,
    pub command: String,
    /// Why the run button cannot be used yet; empty when ready.
    pub problems: Vec<String>,
    pub words_lines: Option<(usize, usize)>,
    /// Reveal is reading a text file (cover text) rather than an image or audio file.
    pub reveal_text: bool,
}

#[derive(Clone)]
struct MeasureReq {
    hidden: bool,
    path: PathBuf,
    a: usize,
    b: Option<usize>,
    cfg: AppConfig,
}

enum MeasureOut {
    Done(Measured),
    Note(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Main,
    AnalyzeA,
    AnalyzeB,
}

struct Running {
    handle: JobHandle,
    tag: Tag,
    wipe: bool,
}

pub struct App {
    pub screen: Screen,
    pub forms: Vec<Form>,
    pub browser: Option<Browser>,
    pub help: bool,
    pub log: Vec<LogLine>,
    pub derived: Derived,
    pub hits: Vec<Hit>,
    pub quit: bool,
    pub spinner: usize,
    pub analysis: [Option<Report>; 2],
    pub text_out: Option<String>,
    pub recovered: Option<String>,
    pub measured: Option<Measured>,
    pub measuring: bool,
    pub measure_note: Option<String>,
    pub last_dir: PathBuf,
    started: Instant,
    cache: Arc<CarrierCache>,
    infos: HashMap<PathBuf, Result<CarrierInfo, String>>,
    baselines: HashMap<PathBuf, Report>,
    running: Option<Running>,
    queued: Option<(Tag, JobSpec)>,
    meas_tx: mpsc::Sender<(u64, MeasureOut)>,
    meas_rx: mpsc::Receiver<(u64, MeasureOut)>,
    meas_gen: u64,
    meas_key: String,
    meas_due: Option<(Instant, MeasureReq)>,
}

fn build_forms() -> Vec<Form> {
    use PathFilter::*;
    let mode = || choice(Id::Mode, "Mode", &["match (recommended)", "replace (easy to detect)"], 0);
    let parity = || step(Id::Parity, "Error correction", 16, 0, 64, 2, "bytes");
    let styles = || choice(Id::Style, "Style", &["prose", "syslog", "postgres", "nginx", "env"], 1);
    vec![
        Form::new(vec![
            choice(Id::Cover, "Hide it in", &["an image or audio file", "generated text"], 0),
            path(Id::Carrier, "Cover", Carrier),
            path(Id::Payload, "File to hide", Any),
            choice(Id::Second, "Add a second file", &["no", "yes - own passphrase"], 0),
            path(Id::HiddenFile, "Second file", Any),
            text(Id::Pass, "Passphrase", "", true),
            text(Id::HiddenPass, "Second passphrase", "", true),
            choice(Id::TwoSlot, "Always look the same", &["no", "yes"], 0),
            styles(),
            step(Id::Order, "Realism (order)", 4, 1, 6, 1, ""),
            text(Id::Message, "Message", "type what to hide", false),
            step(Id::Slot, "Slot size", 10, 2, 30, 2, "%"),
            mode(),
            parity(),
            text(Id::Output, "Save as", "auto: natural name (private tag)", false),
            text(Id::TextOutput, "Save to file (optional)", "", false),
            button("Hide it"),
        ]),
        Form::new(vec![
            path(Id::Carrier, "File to read", Readable),
            text(Id::Pass, "Passphrase", "", true),
            styles(),
            step(Id::Order, "Realism (order)", 4, 1, 6, 1, ""),
            parity(),
            step(Id::Slot, "Slot size", 10, 2, 30, 2, "%"),
            text(Id::Output, "Save recovered file as", "auto: <name>.recovered.bin", false),
            text(Id::TextOutput, "Save recovered message to (optional)", "", false),
            button("Recover"),
        ]),
        Form::new(vec![
            path(Id::Image, "Image to scan", Carrier),
            path(Id::Compare, "Compare with original", Carrier),
            button("Scan"),
        ]),
        Form::new(vec![
            choice(Id::Direction, "Task", &["Split a secret", "Join shares"], 0),
            path(Id::Secret, "Secret file", Any),
            paths(Id::Carriers, "Carrier images", Carrier),
            step(Id::K, "Shares needed (k)", 2, 1, 255, 1, ""),
            text(Id::Pass, "Passphrase", "", true),
            parity(),
            path(Id::OutDir, "Save shares in", Dir),
            paths(Id::JoinCarriers, "Images with shares", Carrier),
            text(Id::JoinOutput, "Save recovered file as", "auto: recovered.bin next to the first image", false),
            button("Run"),
        ]),
    ]
}

/// File name only, quoted if needed. The equivalent-command line is meant to be read, and full temp or
/// user-folder paths push the flags off the screen; run the command from the file's folder.
fn shown(p: &Path) -> String {
    let s = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    if s.contains(' ') { format!("\"{s}\"") } else { s }
}

impl App {
    pub fn new(start_dir: PathBuf) -> Self {
        let (meas_tx, meas_rx) = mpsc::channel();
        let mut app = Self {
            screen: Screen::Hide,
            forms: build_forms(),
            browser: None,
            help: false,
            log: Vec::new(),
            derived: Derived::default(),
            hits: Vec::new(),
            quit: false,
            spinner: 0,
            analysis: [None, None],
            text_out: None,
            recovered: None,
            measured: None,
            measuring: false,
            measure_note: None,
            last_dir: start_dir,
            started: Instant::now(),
            cache: Arc::new(CarrierCache::new(4)),
            infos: HashMap::new(),
            baselines: HashMap::new(),
            running: None,
            queued: None,
            meas_tx,
            meas_rx,
            meas_gen: 0,
            meas_key: String::new(),
            meas_due: None,
        };
        app.refresh();
        app
    }

    pub fn form(&self) -> &Form {
        &self.forms[self.screen.index()]
    }

    pub fn form_mut(&mut self) -> &mut Form {
        let i = self.screen.index();
        &mut self.forms[i]
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    pub fn set_screen(&mut self, i: usize) {
        if let Some(s) = SCREENS.get(i) {
            self.screen = *s;
            self.refresh();
        }
    }

    pub fn log_line(&mut self, text: impl Into<String>, kind: LogKind) {
        let secs = self.started.elapsed().as_secs_f32();
        self.log.push(LogLine { secs, text: text.into(), kind });
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    // ---------- derived state ----------

    fn info_of(&mut self, p: &Path) -> Result<CarrierInfo, String> {
        if let Some(r) = self.infos.get(p) {
            return r.clone();
        }
        let r = self.cache.info_exact(p).map_err(|e| stegosaur::app::friendly::message(&e));
        self.infos.insert(p.to_path_buf(), r.clone());
        r
    }

    fn file_len(p: &Path) -> Result<usize, String> {
        std::fs::metadata(p).map(|m| m.len() as usize).map_err(|e| format!("Cannot read {}: {e}", p.display()))
    }

    pub fn cfg(&self) -> AppConfig {
        let f = self.form();
        let mut c = AppConfig::default();
        if f.has(Id::Mode) {
            c.mode = if f.choice(Id::Mode) == 1 { Mode::Replace } else { Mode::Match };
        }
        if f.has(Id::Parity) {
            c.parity = f.step(Id::Parity) as usize;
        }
        if f.has(Id::Slot) {
            c.rate = f.step(Id::Slot) as f64 / 100.0;
        }
        c
    }

    fn style(&self) -> stegosaur::corpora::Style {
        text::all_styles()[self.form().choice(Id::Style).min(4)]
    }

    /// Which job the Hide screen would run right now.
    pub fn hide_kind(&self) -> HideKind {
        let f = &self.forms[Screen::Hide.index()];
        if f.choice(Id::Cover) == 1 {
            HideKind::Text
        } else if f.choice(Id::Second) == 1 || f.choice(Id::TwoSlot) == 1 {
            HideKind::Hidden
        } else {
            HideKind::Embed
        }
    }

    /// True when the file chosen on the Reveal screen is a text file (cover text), not an image or audio file.
    fn reveal_is_text(&self) -> bool {
        use stegosaur::app::files::{classify, FileKind};
        self.forms[Screen::Reveal.index()].path(Id::Carrier).is_some_and(|p| matches!(classify(&p).0, FileKind::Other))
    }

    fn apply_visibility(&mut self) {
        match self.screen {
            Screen::Hide => {
                let kind = self.hide_kind();
                let text = kind == HideKind::Text;
                let second = self.form().choice(Id::Second) == 1;
                let adjustable = text::adjustable_order(self.style());
                let f = self.form_mut();
                for id in [Id::Carrier, Id::Payload, Id::Second, Id::Mode, Id::Parity, Id::Output] {
                    f.set_hidden(id, text);
                }
                for id in [Id::HiddenFile, Id::HiddenPass] {
                    f.set_hidden(id, text || !second);
                }
                f.set_hidden(Id::TwoSlot, text || second);
                f.set_hidden(Id::Slot, text || kind != HideKind::Hidden);
                for id in [Id::Style, Id::Message, Id::TextOutput] {
                    f.set_hidden(id, !text);
                }
                f.set_hidden(Id::Order, !text || !adjustable);
                f.get_mut(Id::Pass).label = if second && !text { "First passphrase" } else { "Passphrase" };
                f.get_mut(Id::Run).label = match kind {
                    HideKind::Text => "Generate text",
                    HideKind::Hidden if second => "Hide both",
                    _ => "Hide it",
                };
            }
            Screen::Reveal => {
                let text = self.reveal_is_text();
                let adjustable = text::adjustable_order(self.style());
                let f = self.form_mut();
                for id in [Id::Style, Id::TextOutput] {
                    f.set_hidden(id, !text);
                }
                f.set_hidden(Id::Order, !text || !adjustable);
                for id in [Id::Parity, Id::Slot, Id::Output] {
                    f.set_hidden(id, text);
                }
            }
            Screen::Split => {
                let join = self.form().choice(Id::Direction) == 1;
                let f = self.form_mut();
                for id in [Id::Secret, Id::Carriers, Id::K, Id::Parity, Id::OutDir] {
                    f.set_hidden(id, join);
                }
                for id in [Id::JoinCarriers, Id::JoinOutput] {
                    f.set_hidden(id, !join);
                }
                if join {
                    f.set_hidden(Id::Parity, false);
                }
            }
            _ => {}
        }
        let f = self.form_mut();
        if f.focused().hidden {
            f.focus_move(1);
        }
    }

    /// Recomputes the preview and, when needed, schedules a background measurement.
    pub fn refresh(&mut self) {
        self.apply_visibility();
        let mut d = Derived::default();
        let cfg = self.cfg();
        let mut req: Option<MeasureReq> = None;
        let mut key = String::new();
        let form = self.form().clone();

        let need_pass = |problems: &mut Vec<String>, id: Id, label: &str| {
            if form.text(id).is_empty() {
                problems.push(format!("Enter the {label}."));
            }
        };

        match self.screen {
            Screen::Hide => match self.hide_kind() {
                HideKind::Embed => {
                    let (carrier, payload) = (form.path(Id::Carrier), form.path(Id::Payload));
                    if let Some(c) = &carrier {
                        d.info = Some(self.info_of(c));
                    }
                    if let Some(p) = &payload {
                        match Self::file_len(p) {
                            Ok(n) => d.need = Some(n),
                            Err(e) => d.problems.push(e),
                        }
                    }
                    match &d.info {
                        None => d.problems.push("Choose a cover image.".into()),
                        Some(Err(e)) => d.problems.push(e.clone()),
                        Some(Ok(info)) => {
                            if let Some(n) = d.need {
                                let p = fit_embed(info.samples, n, &cfg);
                                if let Some(e) = &p.error {
                                    d.problems.push(e.clone());
                                } else if let Some(c) = &carrier {
                                    key = format!("e|{}|{n}|{:?}|{}", c.display(), cfg.mode, cfg.parity);
                                    req = Some(MeasureReq { hidden: false, path: c.clone(), a: n, b: None, cfg });
                                }
                                d.embed = Some(p);
                            }
                        }
                    }
                    if payload.is_none() {
                        d.problems.push("Choose the file to hide.".into());
                    }
                    need_pass(&mut d.problems, Id::Pass, "passphrase");
                    d.command = self.command_for(&form);
                }
                HideKind::Hidden => {
                    let carrier = form.path(Id::Carrier);
                    let second = form.choice(Id::Second) == 1;
                    let public = form.path(Id::Payload);
                    let hidden = if second { form.path(Id::HiddenFile) } else { None };
                    if let Some(c) = &carrier {
                        d.info = Some(self.info_of(c));
                    }
                    let pub_len = public.as_ref().map(|p| Self::file_len(p));
                    let hid_len = hidden.as_ref().map(|p| Self::file_len(p));
                    for r in [&pub_len, &hid_len].into_iter().flatten() {
                        if let Err(e) = r {
                            d.problems.push(e.clone());
                        }
                    }
                    match &d.info {
                        None => d.problems.push("Choose a cover image.".into()),
                        Some(Err(e)) => d.problems.push(e.clone()),
                        Some(Ok(info)) => {
                            if let Some(Ok(pl)) = pub_len {
                                let hl = hid_len.as_ref().and_then(|r| r.as_ref().ok().copied());
                                let p = fit_hidden(info.samples, pl, hl, &cfg);
                                if let Some(e) = &p.error {
                                    d.problems.push(e.clone());
                                } else if let Some(c) = &carrier {
                                    key = format!("h|{}|{pl}|{:?}|{:?}|{}|{}", c.display(), hl, cfg.mode, cfg.parity, (cfg.rate * 100.0) as u32);
                                    req = Some(MeasureReq { hidden: true, path: c.clone(), a: pl, b: hl, cfg });
                                }
                                d.hidden = Some(p);
                            }
                        }
                    }
                    if public.is_none() {
                        d.problems.push("Choose the file to hide.".into());
                    }
                    need_pass(&mut d.problems, Id::Pass, if second { "first passphrase" } else { "passphrase" });
                    if second {
                        if form.path(Id::HiddenFile).is_none() {
                            d.problems.push("Choose the second file.".into());
                        }
                        need_pass(&mut d.problems, Id::HiddenPass, "second passphrase");
                        if !form.text(Id::Pass).is_empty() && form.text(Id::Pass) == form.text(Id::HiddenPass) {
                            d.problems.push("The two passphrases must differ.".into());
                        }
                    }
                    d.command = self.command_for(&form);
                }
                HideKind::Text => {
                    let style = self.style();
                    let order = form.step(Id::Order) as usize;
                    let order = if text::adjustable_order(style) { order } else { style.default_order() };
                    let len = form.text(Id::Message).len();
                    if len == 0 {
                        d.problems.push("Type a message to hide.".into());
                    } else {
                        d.words_lines = Some(text::estimate(style, order, len));
                    }
                    need_pass(&mut d.problems, Id::Pass, "passphrase");
                    d.command = self.command_for(&form);
                }
            },
            Screen::Reveal => {
                d.reveal_text = self.reveal_is_text();
                if let Some(c) = form.path(Id::Carrier) {
                    if !d.reveal_text {
                        d.info = Some(self.info_of(&c));
                    }
                } else {
                    d.problems.push("Choose the file to read.".into());
                }
                if let Some(Err(e)) = &d.info {
                    d.problems.push(e.clone());
                }
                need_pass(&mut d.problems, Id::Pass, "passphrase");
                d.command = self.command_for(&form);
            }
            Screen::Analyze => {
                for id in [Id::Image, Id::Compare] {
                    if let Some(c) = form.path(id) {
                        let i = self.info_of(&c);
                        if id == Id::Image {
                            d.info = Some(i);
                        }
                    }
                }
                match &d.info {
                    None => d.problems.push("Choose an image to scan.".into()),
                    Some(Err(e)) => d.problems.push(e.clone()),
                    Some(Ok(i)) if !i.kind.has_pixel_detectors() => d.problems.push("Analysis only works on images stored as pixels (not audio or JPEG).".into()),
                    _ => {}
                }
                d.command = self.command_for(&form);
            }
            Screen::Split => {
                let join = form.choice(Id::Direction) == 1;
                if join {
                    if form.paths(Id::JoinCarriers).is_empty() {
                        d.problems.push("Add the images that hold shares.".into());
                    }
                    need_pass(&mut d.problems, Id::Pass, "passphrase");
                } else {
                    let carriers = form.paths(Id::Carriers);
                    let secret = form.path(Id::Secret);
                    let mut samples = Vec::new();
                    for c in &carriers {
                        match self.info_of(c) {
                            Ok(i) => samples.push(i.samples),
                            Err(e) => d.problems.push(e),
                        }
                    }
                    if let Some(s) = &secret {
                        match Self::file_len(s) {
                            Ok(n) => {
                                let p = preview_split(&samples, n, form.step(Id::K) as usize, &cfg);
                                if let Some(e) = &p.error {
                                    d.problems.push(e.clone());
                                }
                                d.split = Some(p);
                                d.need = Some(n);
                            }
                            Err(e) => d.problems.push(e),
                        }
                    } else {
                        d.problems.push("Choose the secret file.".into());
                    }
                    if carriers.is_empty() {
                        d.problems.push("Add at least one carrier image.".into());
                    }
                    need_pass(&mut d.problems, Id::Pass, "passphrase");
                }
                d.command = self.command_for(&form);
            }
        }

        self.derived = d;
        self.schedule_measure(key, req);
    }

    fn schedule_measure(&mut self, key: String, req: Option<MeasureReq>) {
        if key != self.meas_key {
            self.meas_key = key;
            self.measured = None;
            self.measure_note = None;
            self.meas_gen += 1;
            self.meas_due = None;
            self.measuring = false;
            if let Some(r) = req {
                self.measuring = true;
                self.meas_due = Some((Instant::now() + Duration::from_millis(120), r));
            }
        }
    }

    fn spawn_measure(&mut self, req: MeasureReq) {
        let (generation, tx, cache) = (self.meas_gen, self.meas_tx.clone(), self.cache.clone());
        let baseline = self.baselines.get(&req.path).copied();
        std::thread::spawn(move || {
            let out = match cache.load(&req.path) {
                Err(e) => MeasureOut::Failed(stegosaur::app::friendly::message(&e)),
                Ok(c) => match &*c {
                    AnyCarrier::Wav(_) => MeasureOut::Note(AUDIO_NOTE.to_string()),
                    AnyCarrier::Jpeg(j) => {
                        let m = if req.hidden { measure_jpeg_hidden(j, req.a, req.b, &req.cfg, baseline) } else { measure_jpeg_single(j, req.a, &req.cfg, baseline) };
                        match m {
                            Some(m) => MeasureOut::Done(m),
                            None => MeasureOut::Note(stegosaur::app::preview::JPEG_NOTE.to_string()),
                        }
                    }
                    AnyCarrier::Image(img) => {
                        let m = if req.hidden { measure_hidden(img, req.a, req.b, &req.cfg, baseline) } else { measure_single(img, req.a, &req.cfg, baseline) };
                        match m {
                            Some(m) => MeasureOut::Done(m),
                            None => MeasureOut::Failed("The payload does not fit.".into()),
                        }
                    }
                },
            };
            let _ = tx.send((generation, out));
        });
    }

    fn command_for(&self, f: &Form) -> String {
        let cfg = self.cfg();
        let mode = if cfg.mode == Mode::Replace { " --mode replace" } else { "" };
        let p = |id: Id| f.path(id).map(|p| shown(&p)).unwrap_or_else(|| "<...>".into());
        let out_name = || match f.text(Id::Output) {
            "" => f.path(Id::Carrier).map(|c| shown(&default_output(&c))).unwrap_or_else(|| "<out>".into()),
            s => s.to_string(),
        };
        match self.screen {
            Screen::Hide => match self.hide_kind() {
                HideKind::Embed => format!("stegosaur embed {} {} --data {} --pass ****** --parity {}{mode}", p(Id::Carrier), out_name(), p(Id::Payload), cfg.parity),
                HideKind::Hidden => {
                    let hidden = if f.choice(Id::Second) == 1 { format!(" --hidden-data {} --hidden-pass ******", p(Id::HiddenFile)) } else { String::new() };
                    format!("stegosaur hidden-embed {} {} --public-data {} --public-pass ******{hidden} --rate {:.2} --parity {}{mode}", p(Id::Carrier), out_name(), p(Id::Payload), cfg.rate, cfg.parity)
                }
                HideKind::Text => {
                    let style = text::style_name(self.style());
                    let order = if text::adjustable_order(self.style()) { format!(" --order {}", f.step(Id::Order)) } else { String::new() };
                    format!("stegosaur text-embed --data <message.txt> --pass ****** --style {style}{order} --out <file>")
                }
            },
            Screen::Reveal => {
                if self.reveal_is_text() {
                    let style = text::style_name(self.style());
                    format!("stegosaur text-extract {} --pass ****** --style {style} --out <file>", p(Id::Carrier))
                } else {
                    format!("stegosaur extract {} --out <file> --pass ****** --parity {}", p(Id::Carrier), cfg.parity)
                }
            }
            Screen::Analyze => format!("stegosaur analyze {}", p(Id::Image)),
            Screen::Split => {
                if f.choice(Id::Direction) == 1 {
                    let imgs: Vec<String> = f.paths(Id::JoinCarriers).iter().map(|c| shown(c)).collect();
                    format!("stegosaur join --out <file> --pass ****** {}", imgs.join(" "))
                } else {
                    let imgs: Vec<String> = f.paths(Id::Carriers).iter().map(|c| shown(c)).collect();
                    format!("stegosaur split --secret {} --k {} --pass ****** --out-dir {} {}", p(Id::Secret), f.step(Id::K), p(Id::OutDir), imgs.join(" "))
                }
            }
        }
    }

    // ---------- jobs ----------

    fn build_job(&mut self) -> Result<JobSpec, String> {
        let cfg = self.cfg();
        let f = self.form().clone();
        let need = |id: Id, msg: &str| f.path(id).ok_or_else(|| msg.to_string());
        let out_or = |id: Id, default: PathBuf| match f.text(id) {
            "" => default,
            s => PathBuf::from(s),
        };
        let save_text = || match f.text(Id::TextOutput) {
            "" => None,
            s => Some(PathBuf::from(s)),
        };
        match self.screen {
            Screen::Hide => match self.hide_kind() {
                HideKind::Embed => {
                    let carrier = need(Id::Carrier, "Choose a cover image.")?;
                    let payload = need(Id::Payload, "Choose the file to hide.")?;
                    let pass = Secret::new(f.text(Id::Pass));
                    let output = match out_or(Id::Output, PathBuf::new()) {
                        o if o.as_os_str().is_empty() => jobs::auto_output(&carrier, &pass, stegosaur::tag::Kind::Single, &cfg),
                        o => o,
                    };
                    Ok(JobSpec::Embed { carrier, payload, output, pass, cfg, overwrite: false })
                }
                HideKind::Hidden => {
                    let carrier = need(Id::Carrier, "Choose a cover image.")?;
                    let public_file = need(Id::Payload, "Choose the file to hide.")?;
                    let pass = Secret::new(f.text(Id::Pass));
                    let output = match out_or(Id::Output, PathBuf::new()) {
                        o if o.as_os_str().is_empty() => jobs::auto_output(&carrier, &pass, stegosaur::tag::Kind::Hidden, &cfg),
                        o => o,
                    };
                    let hidden = if f.choice(Id::Second) == 1 { Some((need(Id::HiddenFile, "Choose the second file.")?, Secret::new(f.text(Id::HiddenPass)))) } else { None };
                    Ok(JobSpec::Hidden { carrier, public_file, public_pass: pass, hidden, output, cfg, overwrite: false })
                }
                HideKind::Text => {
                    let style = self.style();
                    let order = text::adjustable_order(style).then(|| f.step(Id::Order) as usize);
                    Ok(JobSpec::TextHide { message: f.text(Id::Message).as_bytes().to_vec(), pass: Secret::new(f.text(Id::Pass)), style, order, cfg, save: save_text(), overwrite: false })
                }
            },
            Screen::Reveal => {
                if self.reveal_is_text() {
                    let style = self.style();
                    let order = text::adjustable_order(style).then(|| f.step(Id::Order) as usize);
                    Ok(JobSpec::TextReveal { text_file: need(Id::Carrier, "Choose the file to read.")?, pass: Secret::new(f.text(Id::Pass)), style, order, cfg, save: save_text(), overwrite: false })
                } else {
                    let carrier = need(Id::Carrier, "Choose the file to read.")?;
                    let stem = carrier.file_stem().and_then(|s| s.to_str()).unwrap_or("recovered").to_string();
                    let output = out_or(Id::Output, carrier.with_file_name(format!("{stem}.recovered.bin")));
                    Ok(JobSpec::Reveal { carrier, pass: Secret::new(f.text(Id::Pass)), output, cfg, overwrite: false })
                }
            }
            Screen::Analyze => Ok(JobSpec::Analyze { image: need(Id::Image, "Choose an image to scan.")? }),
            Screen::Split => {
                if f.choice(Id::Direction) == 1 {
                    let carriers = f.paths(Id::JoinCarriers);
                    let first = carriers.first().cloned().ok_or("Add the images that hold shares.")?;
                    let output = out_or(Id::JoinOutput, first.with_file_name("recovered.bin"));
                    Ok(JobSpec::Join { carriers, pass: Secret::new(f.text(Id::Pass)), output, cfg, overwrite: false })
                } else {
                    let carriers = f.paths(Id::Carriers);
                    let first = carriers.first().cloned().ok_or("Add at least one carrier image.")?;
                    let out_dir = f.path(Id::OutDir).unwrap_or_else(|| first.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                    Ok(JobSpec::Split { secret: need(Id::Secret, "Choose the secret file.")?, k: f.step(Id::K) as u8, pass: Secret::new(f.text(Id::Pass)), carriers, out_dir, cfg, overwrite: false })
                }
            }
        }
    }

    /// Starts the current screen's job, or logs why it cannot start.
    pub fn run_current(&mut self) {
        if self.running.is_some() {
            self.log_line("A job is already running.", LogKind::Dim);
            return;
        }
        if let Some(p) = self.derived.problems.first().cloned() {
            self.log_line(p, LogKind::Err);
            return;
        }
        match self.build_job() {
            Err(e) => self.log_line(e, LogKind::Err),
            Ok(spec) => {
                let wipe = !matches!(spec, JobSpec::Analyze { .. });
                if matches!(spec, JobSpec::Analyze { .. }) {
                    self.analysis = [None, None];
                    if let Some(c) = self.form().path(Id::Compare) {
                        self.queued = Some((Tag::AnalyzeB, JobSpec::Analyze { image: c }));
                    }
                    self.start(Tag::AnalyzeA, spec, wipe);
                } else {
                    self.text_out = None;
                    self.recovered = None;
                    self.start(Tag::Main, spec, wipe);
                }
            }
        }
    }

    fn start(&mut self, tag: Tag, spec: JobSpec, wipe: bool) {
        self.running = Some(Running { handle: jobs::spawn(spec), tag, wipe });
    }

    fn on_done(&mut self, tag: Tag, result: JobResult, wipe: bool) {
        let ok = |s: String| (s, LogKind::Ok);
        let line = match result {
            JobResult::Embedded { output, bytes, report } => {
                let mut s = format!("Saved {} ({} bytes hidden)", output.display(), bytes);
                if let Some(r) = report {
                    s.push_str(&format!(" - detectability {:.0}/100 {:?}", r.score, r.risk));
                }
                ok(s)
            }
            JobResult::Hidden { output, report } => {
                let mut s = format!("Saved {}", output.display());
                if let Some(r) = report {
                    s.push_str(&format!(" - detectability {:.0}/100 {:?}", r.score, r.risk));
                }
                ok(s)
            }
            JobResult::Split { outputs } => ok(format!("Wrote {} images: {}", outputs.len(), outputs.iter().map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()).collect::<Vec<_>>().join(", "))),
            JobResult::Revealed { output, bytes, layout } => ok(format!("Recovered {bytes} bytes ({}) into {}", if layout == RevealLayout::Hidden { "Hidden layout" } else { "normal layout" }, output.display())),
            JobResult::Joined { output, bytes, used } => ok(format!("Recovered {bytes} bytes from {used} images into {}", output.display())),
            JobResult::Hide(done) => ok(format!("Saved {} file(s)", done.items.iter().filter(|i| i.output.is_some()).count())),
            JobResult::Bulk { report, checked } => ok(format!("Checked {checked} files: {} item(s) recovered into {}", report.recovered_files(), report.out_dir.display())),
            JobResult::Analyzed { report } => {
                self.analysis[if tag == Tag::AnalyzeB { 1 } else { 0 }] = Some(report);
                ok(format!("Scan finished: score {:.0}/100 {:?}", report.score, report.risk))
            }
            JobResult::TextHidden { text, words, saved } => {
                self.text_out = Some(text);
                ok(format!("Cover text ready: {words} words{}", saved.map(|p| format!(", saved to {}", p.display())).unwrap_or_default()))
            }
            JobResult::TextRevealed { bytes, saved } => {
                self.recovered = Some(String::from_utf8_lossy(&bytes).into_owned());
                ok(format!("Recovered {} bytes{}", bytes.len(), saved.map(|p| format!(", saved to {}", p.display())).unwrap_or_default()))
            }
        };
        self.log_line(line.0, line.1);
        if wipe {
            for f in &mut self.forms {
                f.wipe_secrets();
            }
        }
        self.infos.clear();
        self.refresh();
    }

    /// Advances jobs and measurements. Call regularly (the event loop does, every ~50 ms).
    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);

        let mut events = Vec::new();
        let mut gone = false;
        if let Some(r) = &self.running {
            loop {
                match r.handle.events.try_recv() {
                    Ok(e) => events.push(e),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        gone = true;
                        break;
                    }
                }
            }
        }
        for ev in events {
            let (tag, wipe) = self.running.as_ref().map(|r| (r.tag, r.wipe)).unwrap_or((Tag::Main, false));
            match ev {
                JobEvent::Step(s) => self.log_line(s, LogKind::Dim),
                JobEvent::Done(res) => {
                    self.running = None;
                    self.on_done(tag, res, wipe);
                }
                JobEvent::Error(msg) => {
                    self.running = None;
                    self.queued = None;
                    self.log_line(msg, LogKind::Err);
                }
            }
        }
        if gone && self.running.is_some() {
            self.running = None;
        }
        if self.running.is_none() {
            if let Some((tag, spec)) = self.queued.take() {
                self.start(tag, spec, false);
            }
        }

        if let Some((due, _)) = &self.meas_due {
            if Instant::now() >= *due {
                let (_, req) = self.meas_due.take().unwrap();
                self.spawn_measure(req);
            }
        }
        while let Ok((generation, out)) = self.meas_rx.try_recv() {
            if generation != self.meas_gen {
                continue;
            }
            self.measuring = false;
            match out {
                MeasureOut::Done(m) => {
                    if let Some(path) = self.measured_path() {
                        self.baselines.insert(path, m.before);
                    }
                    self.measured = Some(m);
                }
                MeasureOut::Note(n) => self.measure_note = Some(n),
                MeasureOut::Failed(e) => self.measure_note = Some(e),
            }
        }
    }

    fn measured_path(&self) -> Option<PathBuf> {
        self.form().path(Id::Carrier)
    }

    /// For tests and scripts: ticks until nothing is running or pending.
    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let t0 = Instant::now();
        loop {
            self.tick();
            if self.running.is_none() && self.queued.is_none() && self.meas_due.is_none() && !self.measuring {
                return true;
            }
            if t0.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // ---------- input ----------

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Key(k) => self.on_key(k),
            Msg::Mouse(m) => self.on_mouse(m),
            Msg::Paste(s) => self.on_paste(s),
            Msg::Tick => self.tick(),
        }
    }

    pub fn after_change(&mut self) {
        self.refresh();
    }

    fn on_paste(&mut self, s: String) {
        if self.browser.is_some() || self.help {
            return;
        }
        let f = self.form_mut().focused_mut();
        if f.is_text() {
            for c in s.chars().filter(|c| !c.is_control()) {
                f.type_char(c);
            }
            self.after_change();
        }
    }

    fn browser_start(&self, id: Id) -> PathBuf {
        let from_value = match &self.form().get(id).kind {
            Kind::Path { value: Some(p), .. } => p.parent().map(|d| d.to_path_buf()),
            Kind::Paths { items, .. } => items.last().and_then(|p| p.parent().map(|d| d.to_path_buf())),
            _ => None,
        };
        from_value.filter(|d| d.is_dir()).unwrap_or_else(|| self.last_dir.clone())
    }

    pub fn open_browser(&mut self) {
        let (id, label, filter) = {
            let f = self.form().focused();
            let filter = match &f.kind {
                Kind::Path { filter, .. } | Kind::Paths { filter, .. } => *filter,
                _ => return,
            };
            (f.id, f.label, filter)
        };
        let start = self.browser_start(id);
        self.browser = Some(Browser::open(&start, filter, id, label));
    }

    fn apply_pick(&mut self, id: Id, path: PathBuf) {
        if let Some(d) = path.parent() {
            self.last_dir = d.to_path_buf();
        }
        let form = self.form_mut();
        match &mut form.get_mut(id).kind {
            Kind::Path { value, .. } => *value = Some(path),
            Kind::Paths { items, cursor, .. } => {
                if !items.contains(&path) {
                    items.push(path);
                    *cursor = items.len() - 1;
                }
            }
            _ => {}
        }
        self.browser = None;
        self.after_change();
    }

    fn browser_key(&mut self, k: KeyEvent) {
        let Some(b) = self.browser.as_mut() else { return };
        let mut picked: Option<(Id, PathBuf)> = None;
        match k.code {
            KeyCode::Esc => {
                self.browser = None;
                return;
            }
            KeyCode::Up => b.move_sel(-1),
            KeyCode::Down => b.move_sel(1),
            KeyCode::PageUp => b.move_sel(-10),
            KeyCode::PageDown => b.move_sel(10),
            KeyCode::Home => b.jump(false),
            KeyCode::End => b.jump(true),
            KeyCode::Backspace | KeyCode::Left => b.up(),
            KeyCode::Char('s') => {
                if let Some(d) = b.choose_dir() {
                    picked = Some((b.target, d));
                }
            }
            KeyCode::Enter | KeyCode::Right => {
                let target = b.target;
                if let Some(p) = b.activate() {
                    picked = Some((target, p));
                }
            }
            _ => {}
        }
        if let Some((id, p)) = picked {
            self.apply_pick(id, p);
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if self.help {
            self.help = false;
            return;
        }
        if self.browser.is_some() {
            return self.browser_key(k);
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Char('c' | 'q') if ctrl => {
                self.quit = true;
                return;
            }
            KeyCode::F(n) if (1..=4).contains(&n) => {
                self.set_screen(n as usize - 1);
                return;
            }
            KeyCode::Char(c @ '1'..='4') if alt => {
                self.set_screen(c as usize - '1' as usize);
                return;
            }
            KeyCode::Tab => {
                self.form_mut().focus_move(1);
                return;
            }
            KeyCode::BackTab => {
                self.form_mut().focus_move(-1);
                return;
            }
            _ => {}
        }
        if !self.form().focused().is_text() {
            match k.code {
                KeyCode::Char(c @ '1'..='4') if !ctrl => return self.set_screen(c as usize - '1' as usize),
                KeyCode::Char('q') => {
                    self.quit = true;
                    return;
                }
                KeyCode::Char('?') => {
                    self.help = true;
                    return;
                }
                _ => {}
            }
        }
        self.field_key(k);
    }

    fn field_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let kind = self.form().focused().kind.clone();
        let mut changed = false;
        match kind {
            Kind::Text { .. } => match k.code {
                KeyCode::Char('u') if ctrl => {
                    self.form_mut().focused_mut().clear();
                    changed = true;
                }
                KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => changed = self.form_mut().focused_mut().type_char(c),
                KeyCode::Backspace => {
                    self.form_mut().focused_mut().backspace();
                    changed = true;
                }
                KeyCode::Enter | KeyCode::Down => self.form_mut().focus_move(1),
                KeyCode::Up => self.form_mut().focus_move(-1),
                _ => {}
            },
            Kind::Choice { .. } | Kind::Step { .. } => match k.code {
                KeyCode::Left | KeyCode::Char('-') => changed = self.form_mut().focused_mut().change(-1),
                KeyCode::Right | KeyCode::Char('+') | KeyCode::Char(' ') | KeyCode::Enter => changed = self.form_mut().focused_mut().change(1),
                KeyCode::Down => self.form_mut().focus_move(1),
                KeyCode::Up => self.form_mut().focus_move(-1),
                _ => {}
            },
            Kind::Path { .. } => match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open_browser(),
                KeyCode::Delete | KeyCode::Backspace => {
                    self.form_mut().focused_mut().clear();
                    changed = true;
                }
                KeyCode::Down => self.form_mut().focus_move(1),
                KeyCode::Up => self.form_mut().focus_move(-1),
                _ => {}
            },
            Kind::Paths { items, cursor, .. } => match k.code {
                KeyCode::Enter | KeyCode::Char('a') => self.open_browser(),
                KeyCode::Delete | KeyCode::Char('d') | KeyCode::Backspace => {
                    if let Kind::Paths { items, cursor, .. } = &mut self.form_mut().focused_mut().kind {
                        if !items.is_empty() {
                            items.remove(*cursor);
                            *cursor = (*cursor).min(items.len().saturating_sub(1));
                        }
                    }
                    changed = true;
                }
                KeyCode::Down => {
                    if cursor + 1 < items.len() {
                        if let Kind::Paths { cursor, .. } = &mut self.form_mut().focused_mut().kind {
                            *cursor += 1;
                        }
                    } else {
                        self.form_mut().focus_move(1);
                    }
                }
                KeyCode::Up => {
                    if cursor > 0 {
                        if let Kind::Paths { cursor, .. } = &mut self.form_mut().focused_mut().kind {
                            *cursor -= 1;
                        }
                    } else {
                        self.form_mut().focus_move(-1);
                    }
                }
                _ => {}
            },
            Kind::Button => match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.run_current(),
                KeyCode::Up => self.form_mut().focus_move(-1),
                _ => {}
            },
        }
        if changed {
            self.after_change();
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let inside = |r: Rect| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
        if self.browser.is_some() {
            match m.kind {
                MouseEventKind::ScrollUp => self.browser.as_mut().unwrap().move_sel(-3),
                MouseEventKind::ScrollDown => self.browser.as_mut().unwrap().move_sel(3),
                MouseEventKind::Down(MouseButton::Left) => {
                    let hit = self.hits.iter().rev().find(|h| inside(h.rect) && matches!(h.kind, HitKind::BrowserRow(_) | HitKind::CloseBrowser)).copied();
                    match hit.map(|h| h.kind) {
                        Some(HitKind::BrowserRow(i)) => {
                            let b = self.browser.as_mut().unwrap();
                            b.sel = i;
                            let target = b.target;
                            if let Some(p) = b.activate() {
                                self.apply_pick(target, p);
                            }
                        }
                        Some(HitKind::CloseBrowser) | None => {
                            if hit.is_none() {
                                // clicking outside the popup closes it
                            }
                            self.browser = None;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            return;
        }
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            let Some(hit) = self.hits.iter().rev().find(|h| inside(h.rect) && !matches!(h.kind, HitKind::BrowserRow(_) | HitKind::CloseBrowser)).copied() else { return };
            match hit.kind {
                HitKind::Tab(i) => self.set_screen(i),
                HitKind::Field(i) => {
                    self.form_mut().focus = i;
                    let kind = self.form().focused().kind.clone();
                    match kind {
                        Kind::Button => self.run_current(),
                        Kind::Path { .. } | Kind::Paths { .. } => self.open_browser(),
                        Kind::Choice { .. } | Kind::Step { .. } => {
                            if self.form_mut().focused_mut().change(1) {
                                self.after_change();
                            }
                        }
                        Kind::Text { .. } => {}
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        for f in &mut self.forms {
            f.wipe_secrets();
        }
    }
}

/// Convenience for tests: the displayed name of a browser row.
pub fn row_name(r: &Row) -> String {
    match r {
        Row::Parent => "..".into(),
        Row::Entry(e) => e.name.clone(),
    }
}
