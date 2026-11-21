//! The unified Hide job behind the desktop redesign. One passphrase (and optionally a second) and a
//! layout that follows from what was chosen: one file in one cover, the same-look layout, two secrets
//! in one cover, one file split over several covers, or many files with one cover each.
//!
//! The secret can be a file or text typed on screen; typed text never touches the disk.

use crate::analysis::Report;
use crate::app::config::AppConfig;
use crate::app::jobs::{check_ext, check_output, report_of, require_pass};
use crate::app::secret::Secret;
use crate::carrier::{is_audio_ext, AnyCarrier};
use crate::container::{embed_deniable, embed_single};
use crate::distribute::distribute;
use crate::error::{Error, Result};
use crate::tag::{self, Kind};
use std::path::{Path, PathBuf};

/// What gets hidden.
#[derive(Debug, Clone)]
pub enum Payload {
    File(PathBuf),
    Text(Secret),
}

impl Payload {
    pub fn name(&self) -> String {
        match self {
            Self::File(p) => p.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string(),
            Self::Text(_) => "typed message".into(),
        }
    }

    fn stem(&self) -> String {
        match self {
            Self::File(p) => p.file_stem().and_then(|n| n.to_str()).unwrap_or("file").to_string(),
            Self::Text(_) => "message".into(),
        }
    }

    fn read(&self) -> Result<Vec<u8>> {
        match self {
            Self::File(p) => Ok(std::fs::read(p)?),
            Self::Text(t) => {
                if t.is_empty() {
                    Err(Error::Invalid("the typed message is empty".into()))
                } else {
                    Ok(t.as_bytes().to_vec())
                }
            }
        }
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Self::File(p) => Some(p),
            Self::Text(_) => None,
        }
    }
}

/// How the new files are named.
#[derive(Debug, Clone)]
pub enum Naming {
    /// An ordinary camera or recorder name carrying a private tag only the passphrase can check.
    Ordinary,
    /// The cover's own name plus `_1`.
    Suffix,
    /// A pattern with `{name}` (cover), `{payload}` and `{n}` (01, 02, ...).
    Pattern(String),
}

#[derive(Debug, Clone)]
pub enum Layout {
    Single { payload: Payload, cover: PathBuf },
    /// One file in the two-slot layout, so the cover looks the same whether it holds one secret, two or none.
    SameLook { payload: Payload, cover: PathBuf },
    Two { payload: Payload, second: Payload, second_pass: Secret, cover: PathBuf },
    Split { payload: Payload, covers: Vec<PathBuf>, k: u8 },
    Bulk { pairs: Vec<(Payload, PathBuf)> },
}

#[derive(Debug, Clone)]
pub struct HideSpec {
    pub layout: Layout,
    pub pass: Secret,
    pub cfg: AppConfig,
    pub naming: Naming,
    pub out_dir: Option<PathBuf>,
    pub overwrite: bool,
}

#[derive(Debug, Clone)]
pub struct HideItem {
    /// What went in ("report.pdf", "typed message", "share 2 of 3").
    pub what: String,
    pub cover: PathBuf,
    pub output: Option<PathBuf>,
    pub bytes: usize,
    pub report: Option<Report>,
    /// Set when this one item failed while others went through (bulk only).
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HideDone {
    pub kind: &'static str,
    pub items: Vec<HideItem>,
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("png").to_string()
}

/// One output path per cover, all different, none an input, none existing unless overwriting.
fn plan_outputs(spec: &HideSpec, covers: &[(&Path, String)], kind: Kind, inputs: &[&Path]) -> Result<Vec<PathBuf>> {
    let kdf = spec.cfg.to_config().kdf;
    let mut out: Vec<PathBuf> = Vec::new();
    for (i, (cover, payload_stem)) in covers.iter().enumerate() {
        let dir: PathBuf = match &spec.out_dir {
            Some(d) => d.clone(),
            None => cover.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")),
        };
        let ext = ext_of(cover);
        let cover_stem = cover.file_stem().and_then(|s| s.to_str()).unwrap_or("file").to_string();
        let taken = |name: &str, out: &Vec<PathBuf>| out.iter().any(|o| o.file_name().is_some_and(|n| n == name));
        let name = match &spec.naming {
            Naming::Ordinary => {
                let audio = is_audio_ext(&ext.to_ascii_lowercase());
                let mut n = tag::tagged_name(spec.pass.as_bytes(), kind, &dir, &ext, audio, kdf)?;
                let mut tries = 0;
                while taken(&n, &out) && tries < 200 {
                    n = tag::tagged_name(spec.pass.as_bytes(), kind, &dir, &ext, audio, kdf)?;
                    tries += 1;
                }
                n
            }
            Naming::Suffix => {
                let mut k = 1;
                let mut n = format!("{cover_stem}_{k}.{ext}");
                while (dir.join(&n).exists() && !spec.overwrite) || taken(&n, &out) {
                    k += 1;
                    n = format!("{cover_stem}_{k}.{ext}");
                }
                n
            }
            Naming::Pattern(p) => {
                let stem = p.replace("{name}", &cover_stem).replace("{payload}", payload_stem).replace("{n}", &format!("{:02}", i + 1));
                if stem.trim().is_empty() || stem.contains(['/', '\\']) {
                    return Err(Error::Invalid("the name pattern is empty or contains a slash".into()));
                }
                format!("{stem}.{ext}")
            }
        };
        out.push(dir.join(name));
    }
    for (i, o) in out.iter().enumerate() {
        check_output(o, inputs, spec.overwrite)?;
        if out[..i].iter().any(|p| p == o) {
            return Err(Error::Invalid("two files would get the same name; add {n} to the name pattern".into()));
        }
    }
    Ok(out)
}

fn load_for(cover: &Path, output: &Path) -> Result<AnyCarrier> {
    let c = AnyCarrier::load(cover)?;
    check_ext(output, &c)?;
    Ok(c)
}

pub fn run_hide(spec: HideSpec, step: &mut dyn FnMut(&str)) -> Result<HideDone> {
    require_pass(&spec.pass)?;
    let cfg = spec.cfg.to_config();
    match &spec.layout {
        Layout::Single { payload, cover } | Layout::SameLook { payload, cover } => {
            let same = matches!(spec.layout, Layout::SameLook { .. });
            let mut inputs: Vec<&Path> = vec![cover];
            inputs.extend(payload.path());
            let outs = plan_outputs(&spec, &[(cover.as_path(), payload.stem())], if same { Kind::Hidden } else { Kind::Single }, &inputs)?;
            step("Reading the cover");
            let mut c = load_for(cover, &outs[0])?;
            let data = payload.read()?;
            step("Deriving the key and hiding the data");
            if same {
                embed_deniable(&mut c, (spec.pass.as_bytes(), &data), None, &cfg)?;
            } else {
                embed_single(&mut c, spec.pass.as_bytes(), &data, &cfg)?;
            }
            step("Writing the new file");
            c.save(&outs[0])?;
            Ok(HideDone { kind: if same { "same_look" } else { "single" }, items: vec![HideItem { what: payload.name(), cover: cover.clone(), output: Some(outs[0].clone()), bytes: data.len(), report: report_of(&c), error: None }] })
        }
        Layout::Two { payload, second, second_pass, cover } => {
            require_pass(second_pass)?;
            if second_pass.as_bytes() == spec.pass.as_bytes() {
                return Err(Error::Invalid("the two passphrases must differ".into()));
            }
            let mut inputs: Vec<&Path> = vec![cover];
            inputs.extend(payload.path());
            inputs.extend(second.path());
            let outs = plan_outputs(&spec, &[(cover.as_path(), payload.stem())], Kind::Hidden, &inputs)?;
            step("Reading the cover");
            let mut c = load_for(cover, &outs[0])?;
            let (a, b) = (payload.read()?, second.read()?);
            step("Deriving both keys and filling both slots");
            embed_deniable(&mut c, (spec.pass.as_bytes(), &a), Some((second_pass.as_bytes(), &b)), &cfg)?;
            step("Writing the new file");
            c.save(&outs[0])?;
            Ok(HideDone { kind: "two", items: vec![HideItem { what: format!("{} and {}", payload.name(), second.name()), cover: cover.clone(), output: Some(outs[0].clone()), bytes: a.len() + b.len(), report: report_of(&c), error: None }] })
        }
        Layout::Split { payload, covers, k } => {
            if covers.len() < 2 {
                return Err(Error::Invalid("splitting needs at least two covers".into()));
            }
            let mut inputs: Vec<&Path> = covers.iter().map(|c| c.as_path()).collect();
            inputs.extend(payload.path());
            let pairs: Vec<(&Path, String)> = covers.iter().map(|c| (c.as_path(), payload.stem())).collect();
            let outs = plan_outputs(&spec, &pairs, Kind::Share, &inputs)?;
            step("Reading the covers");
            let mut loaded = Vec::new();
            for (c, o) in covers.iter().zip(&outs) {
                loaded.push(load_for(c, o)?);
            }
            let data = payload.read()?;
            step("Splitting the secret and hiding one piece in each cover");
            distribute(&data, spec.pass.as_bytes(), (*k).clamp(1, covers.len() as u8), &mut loaded, &cfg)?;
            step("Writing the new files");
            for (c, o) in loaded.iter().zip(&outs) {
                c.save(o)?;
            }
            let n = covers.len();
            Ok(HideDone {
                kind: "split",
                items: covers.iter().zip(outs).enumerate().map(|(i, (c, o))| HideItem { what: format!("piece {} of {}", i + 1, n), cover: c.clone(), output: Some(o), bytes: data.len(), report: None, error: None }).collect(),
            })
        }
        Layout::Bulk { pairs } => {
            if pairs.is_empty() {
                return Err(Error::Invalid("nothing to hide".into()));
            }
            let mut inputs: Vec<&Path> = pairs.iter().map(|(_, c)| c.as_path()).collect();
            inputs.extend(pairs.iter().filter_map(|(p, _)| p.path()));
            let named: Vec<(&Path, String)> = pairs.iter().map(|(p, c)| (c.as_path(), p.stem())).collect();
            let outs = plan_outputs(&spec, &named, Kind::Single, &inputs)?;
            let mut items = Vec::new();
            let total = pairs.len();
            for (i, ((payload, cover), out)) in pairs.iter().zip(outs).enumerate() {
                step(&format!("Hiding {} of {}: {}", i + 1, total, payload.name()));
                let one = (|| -> Result<(usize, Option<Report>)> {
                    let mut c = load_for(cover, &out)?;
                    let data = payload.read()?;
                    embed_single(&mut c, spec.pass.as_bytes(), &data, &cfg)?;
                    c.save(&out)?;
                    Ok((data.len(), report_of(&c)))
                })();
                items.push(match one {
                    Ok((bytes, report)) => HideItem { what: payload.name(), cover: cover.clone(), output: Some(out), bytes, report, error: None },
                    Err(e) => HideItem { what: payload.name(), cover: cover.clone(), output: None, bytes: 0, report: None, error: Some(crate::app::friendly::message(&e)) },
                });
            }
            Ok(HideDone { kind: "bulk", items })
        }
    }
}
