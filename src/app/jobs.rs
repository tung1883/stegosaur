//! Long-running operations for front ends. Every job can run synchronously (`run`) or on a worker
//! thread (`spawn`) that reports progress over a channel, so a UI never blocks.
//!
//! Safety rules applied to every job that writes a file: an input is never overwritten, an existing
//! output is refused unless `overwrite` is set, and the output type must match the carrier.

use crate::analysis::{analyze, Report};
use crate::app::config::AppConfig;
use crate::app::friendly;
use crate::app::secret::Secret;
use crate::carrier::AnyCarrier;
use crate::corpora::Style;
use crate::container::{embed_deniable, embed_single, extract_deniable, extract_single};
use crate::distribute::{distribute, recover};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread::JoinHandle;

#[derive(Debug, Clone)]
pub enum JobSpec {
    Embed { carrier: PathBuf, payload: PathBuf, output: PathBuf, pass: Secret, cfg: AppConfig, overwrite: bool },
    Hidden { carrier: PathBuf, public_file: PathBuf, public_pass: Secret, hidden: Option<(PathBuf, Secret)>, output: PathBuf, cfg: AppConfig, overwrite: bool },
    Split { secret: PathBuf, k: u8, pass: Secret, carriers: Vec<PathBuf>, out_dir: PathBuf, cfg: AppConfig, overwrite: bool },
    Reveal { carrier: PathBuf, pass: Secret, output: PathBuf, cfg: AppConfig, overwrite: bool },
    Join { carriers: Vec<PathBuf>, pass: Secret, output: PathBuf, cfg: AppConfig, overwrite: bool },
    /// Look at every file in a folder with every passphrase and recover what opens.
    Bulk { inputs: Vec<PathBuf>, recursive: bool, passes: Vec<Secret>, out_dir: PathBuf, names_only: bool, cfg: AppConfig, text: Option<(Style, Option<usize>)> },
    /// The unified Hide job: every layout, typed or file secrets, naming schemes.
    Hide(crate::app::hide::HideSpec),
    Analyze { image: PathBuf },
    /// Spell a message out as cover text; `save` writes the text to a file as well.
    TextHide { message: Vec<u8>, pass: Secret, style: Style, order: Option<usize>, cfg: AppConfig, save: Option<PathBuf>, overwrite: bool },
    /// Recover a message from cover text stored in `text_file`.
    TextReveal { text_file: PathBuf, pass: Secret, style: Style, order: Option<usize>, cfg: AppConfig, save: Option<PathBuf>, overwrite: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealLayout {
    Single,
    Hidden,
}

#[derive(Debug, Clone)]
pub enum JobResult {
    Embedded { output: PathBuf, bytes: usize, report: Option<Report> },
    Hidden { output: PathBuf, report: Option<Report> },
    Split { outputs: Vec<PathBuf> },
    Revealed { output: PathBuf, bytes: usize, layout: RevealLayout },
    Joined { output: PathBuf, bytes: usize, used: usize },
    Analyzed { report: Report },
    Bulk { report: crate::app::bulk::BulkReport, checked: usize },
    Hide(crate::app::hide::HideDone),
    TextHidden { text: String, words: usize, saved: Option<PathBuf> },
    TextRevealed { bytes: Vec<u8>, saved: Option<PathBuf> },
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    Step(String),
    Done(JobResult),
    /// Already phrased for display.
    Error(String),
}

/// `<folder>/<name>_1.<ext>` next to the carrier.
pub fn default_output(carrier: &Path) -> PathBuf {
    let stem = carrier.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = carrier.extension().and_then(|s| s.to_str()).unwrap_or("png");
    carrier.with_file_name(format!("{stem}_1.{ext}"))
}

/// Output path for a new stego file: a natural-looking name (camera or recorder style) carrying a keyed tag (see [`crate::tag`]),
/// or the plain `_1` name when there is no passphrase to key it with.
pub fn auto_output(carrier: &Path, pass: &Secret, kind: crate::tag::Kind, cfg: &AppConfig) -> PathBuf {
    if pass.is_empty() {
        return default_output(carrier);
    }
    crate::tag::tagged_output(carrier, pass.as_bytes(), kind, cfg.to_config().kdf).unwrap_or_else(|_| default_output(carrier))
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn canon_target(p: &Path) -> PathBuf {
    if p.exists() {
        return canon(p);
    }
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) if !dir.as_os_str().is_empty() => canon(dir).join(name),
        _ => p.to_path_buf(),
    }
}

pub(crate) fn check_output(output: &Path, inputs: &[&Path], overwrite: bool) -> Result<()> {
    let target = canon_target(output);
    if inputs.iter().any(|i| canon(i) == target) {
        return Err(Error::Invalid("the output would overwrite an input file".into()));
    }
    if let Some(dir) = output.parent() {
        if !dir.as_os_str().is_empty() && !dir.exists() {
            return Err(Error::Invalid("the output folder does not exist".into()));
        }
    }
    if output.exists() && !overwrite {
        return Err(Error::Invalid(format!("{} already exists", output.display())));
    }
    Ok(())
}

pub(crate) fn check_ext(output: &Path, c: &AnyCarrier) -> Result<()> {
    let ext = output.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let ok = match c {
        AnyCarrier::Image(_) => crate::carrier::is_image_ext(&ext),
        AnyCarrier::Wav(_) => crate::carrier::is_audio_ext(&ext),
        AnyCarrier::Jpeg(_) => crate::carrier::is_jpeg_ext(&ext),
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported("the output must be .png, .bmp, .tif, .webp or .ppm for images, .jpg for JPEG and .wav or .flac for audio".into()))
    }
}

pub(crate) fn report_of(c: &AnyCarrier) -> Option<Report> {
    match c {
        AnyCarrier::Image(img) => Some(analyze(img)),
        AnyCarrier::Wav(_) | AnyCarrier::Jpeg(_) => None,
    }
}

pub(crate) fn require_pass(s: &Secret) -> Result<()> {
    if s.is_empty() {
        Err(Error::Invalid("the passphrase must not be empty".into()))
    } else {
        Ok(())
    }
}

/// Runs a job on the current thread, sending progress through `emit`.
pub fn run(spec: JobSpec, emit: &mut dyn FnMut(JobEvent)) -> Result<JobResult> {
    let mut step = |m: &str| emit(JobEvent::Step(m.to_string()));
    match spec {
        JobSpec::Embed { carrier, payload, output, pass, cfg, overwrite } => {
            check_output(&output, &[&carrier, &payload], overwrite)?;
            require_pass(&pass)?;
            step("Reading the cover file");
            let mut c = AnyCarrier::load(&carrier)?;
            check_ext(&output, &c)?;
            let data = std::fs::read(&payload)?;
            step("Deriving the key and hiding the data");
            embed_single(&mut c, pass.as_bytes(), &data, &cfg.to_config())?;
            step("Writing the new file");
            c.save(&output)?;
            Ok(JobResult::Embedded { output, bytes: data.len(), report: report_of(&c) })
        }
        JobSpec::Hidden { carrier, public_file, public_pass, hidden, output, cfg, overwrite } => {
            let mut inputs: Vec<&Path> = vec![&carrier, &public_file];
            if let Some((p, _)) = &hidden {
                inputs.push(p);
            }
            check_output(&output, &inputs, overwrite)?;
            require_pass(&public_pass)?;
            if let Some((_, hp)) = &hidden {
                require_pass(hp)?;
            }
            step("Reading the cover file");
            let mut c = AnyCarrier::load(&carrier)?;
            check_ext(&output, &c)?;
            let public = std::fs::read(&public_file)?;
            let hidden_data = match &hidden {
                Some((p, hp)) => Some((std::fs::read(p)?, hp.clone())),
                None => None,
            };
            step("Deriving keys and filling both slots");
            embed_deniable(
                &mut c,
                (public_pass.as_bytes(), &public),
                hidden_data.as_ref().map(|(d, hp)| (hp.as_bytes(), d.as_slice())),
                &cfg.to_config(),
            )?;
            step("Writing the new file");
            c.save(&output)?;
            Ok(JobResult::Hidden { output, report: report_of(&c) })
        }
        JobSpec::Split { secret, k, pass, carriers, out_dir, cfg, overwrite } => {
            require_pass(&pass)?;
            let kdf = cfg.to_config().kdf;
            let mut outputs: Vec<PathBuf> = Vec::new();
            for c in &carriers {
                let ext = c.extension().and_then(|s| s.to_str()).unwrap_or("png");
                let audio = crate::carrier::is_audio_ext(&ext.to_ascii_lowercase());
                let mut name = crate::tag::tagged_name(pass.as_bytes(), crate::tag::Kind::Share, &out_dir, ext, audio, kdf)?;
                while outputs.iter().any(|o| o.file_name().is_some_and(|n| n == name.as_str())) {
                    name = crate::tag::tagged_name(pass.as_bytes(), crate::tag::Kind::Share, &out_dir, ext, audio, kdf)?;
                }
                outputs.push(out_dir.join(name));
            }
            let mut inputs: Vec<&Path> = carriers.iter().map(|p| p.as_path()).collect();
            inputs.push(&secret);
            for (i, o) in outputs.iter().enumerate() {
                check_output(o, &inputs, overwrite)?;
                if outputs[..i].iter().any(|p| canon_target(p) == canon_target(o)) {
                    return Err(Error::Invalid("two selected images would produce the same output name".into()));
                }
            }
            step("Reading the images");
            let mut loaded = carriers.iter().map(|p| AnyCarrier::load(p)).collect::<Result<Vec<_>>>()?;
            for (c, o) in loaded.iter().zip(&outputs) {
                check_ext(o, c)?;
            }
            let data = std::fs::read(&secret)?;
            step("Splitting the secret and hiding one share per image");
            distribute(&data, pass.as_bytes(), k, &mut loaded, &cfg.to_config())?;
            step("Writing the new files");
            for (c, o) in loaded.iter().zip(&outputs) {
                c.save(o)?;
            }
            Ok(JobResult::Split { outputs })
        }
        JobSpec::Reveal { carrier, pass, output, cfg, overwrite } => {
            check_output(&output, &[&carrier], overwrite)?;
            require_pass(&pass)?;
            step("Reading the file");
            let c = AnyCarrier::load(&carrier)?;
            let conf = cfg.to_config();
            step("Deriving the key and looking for hidden data");
            let (data, layout) = match extract_single(&c, pass.as_bytes(), &conf) {
                Ok(d) => (d, RevealLayout::Single),
                Err(Error::NoPayload) => (extract_deniable(&c, pass.as_bytes(), &conf)?, RevealLayout::Hidden),
                Err(e) => return Err(e),
            };
            step("Saving the recovered file");
            std::fs::write(&output, &data)?;
            Ok(JobResult::Revealed { output, bytes: data.len(), layout })
        }
        JobSpec::Join { carriers, pass, output, cfg, overwrite } => {
            let inputs: Vec<&Path> = carriers.iter().map(|p| p.as_path()).collect();
            check_output(&output, &inputs, overwrite)?;
            require_pass(&pass)?;
            step("Reading the images");
            let loaded: Vec<AnyCarrier> = carriers.iter().filter_map(|p| AnyCarrier::load(p).ok()).collect();
            if loaded.is_empty() {
                return Err(Error::Invalid("none of the selected files could be read".into()));
            }
            step("Collecting shares and rebuilding the secret");
            let data = recover(pass.as_bytes(), &loaded, &cfg.to_config())?;
            step("Saving the recovered file");
            std::fs::write(&output, &data)?;
            Ok(JobResult::Joined { output, bytes: data.len(), used: loaded.len() })
        }
        JobSpec::Hide(spec) => {
            let done = crate::app::hide::run_hide(spec, &mut |m| step(m))?;
            Ok(JobResult::Hide(done))
        }
        JobSpec::Bulk { inputs, recursive, passes, out_dir, names_only, cfg, text } => {
            step("Listing the files");
            let mut files: Vec<PathBuf> = Vec::new();
            for p in &inputs {
                if p.is_dir() {
                    files.extend(crate::app::bulk::list_files(p, recursive, Some(&out_dir))?);
                } else if p.is_file() {
                    files.push(p.clone());
                }
            }
            files.sort();
            files.dedup();
            if files.is_empty() {
                return Err(Error::Invalid("there are no files to look through".into()));
            }
            let opts = crate::app::bulk::BulkOptions { passes, out_dir, cfg, names_only, text };
            let total = files.len();
            let report = crate::app::bulk::run(&files, &opts, &mut |done, total, name| step(&format!("Checked {done} of {total}: {name}")))?;
            Ok(JobResult::Bulk { report, checked: total })
        }
        JobSpec::TextHide { message, pass, style, order, cfg, save, overwrite } => {
            if let Some(path) = &save {
                check_output(path, &[], overwrite)?;
            }
            require_pass(&pass)?;
            if message.is_empty() {
                return Err(Error::Invalid("the message must not be empty".into()));
            }
            step("Preparing the language model");
            let _ = crate::app::text::model(style, order, None);
            step("Encrypting and writing the cover text");
            let text = crate::app::text::hide(style, order, &pass, &message, cfg.kdf)?;
            if let Some(path) = &save {
                std::fs::write(path, &text)?;
            }
            let words = text.split_whitespace().count();
            Ok(JobResult::TextHidden { text, words, saved: save })
        }
        JobSpec::TextReveal { text_file, pass, style, order, cfg, save, overwrite } => {
            if let Some(path) = &save {
                check_output(path, &[&text_file], overwrite)?;
            }
            require_pass(&pass)?;
            step("Reading the text");
            let text = std::fs::read_to_string(&text_file)?;
            step("Preparing the language model");
            let _ = crate::app::text::model(style, order, None);
            step("Decoding and decrypting");
            let bytes = crate::app::text::reveal(style, order, &pass, &text, cfg.kdf)?;
            if let Some(path) = &save {
                std::fs::write(path, &bytes)?;
            }
            Ok(JobResult::TextRevealed { bytes, saved: save })
        }
        JobSpec::Analyze { image } => {
            step("Reading the image");
            let c = AnyCarrier::load(&image)?;
            step("Running the detectors");
            match c {
                AnyCarrier::Image(img) => Ok(JobResult::Analyzed { report: analyze(&img) }),
                AnyCarrier::Jpeg(j) => crate::analysis::analyze_jpeg(&j)
                    .map(|report| JobResult::Analyzed { report })
                    .ok_or_else(|| Error::Unsupported("this JPEG is too small for the coefficient detector".into())),
                AnyCarrier::Wav(_) => Err(Error::Unsupported("analysis is only available for images".into())),
            }
        }
    }
}

pub struct JobHandle {
    pub events: Receiver<JobEvent>,
    thread: Option<JoinHandle<()>>,
}

impl JobHandle {
    /// Blocks until the worker has finished and sent its last event.
    pub fn wait(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Runs a job on a worker thread. The last event is always `Done` or `Error`.
pub fn spawn(spec: JobSpec) -> JobHandle {
    let (tx, rx) = channel();
    let thread = std::thread::spawn(move || {
        let mut emit = |e: JobEvent| {
            let _ = tx.send(e);
        };
        match run(spec, &mut emit) {
            Ok(r) => emit(JobEvent::Done(r)),
            Err(e) => emit(JobEvent::Error(friendly::message(&e))),
        }
    });
    JobHandle { events: rx, thread: Some(thread) }
}
