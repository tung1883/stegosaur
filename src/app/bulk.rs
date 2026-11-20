//! Bulk recovery: given a folder of files and a list of passphrases, find and recover everything that
//! any of those passphrases opens (one-file, two-passphrase and split layouts), without being told
//! which file is which.
//!
//! A file that no passphrase opens simply reports "nothing found" - by design that is the same
//! as a file with nothing in it.

use crate::app::config::AppConfig;
use crate::app::secret::Secret;
use crate::carrier::{ext_of, is_audio_ext, is_image_ext, is_jpeg_ext, AnyCarrier};
use crate::container::{extract_deniable, extract_single};
use crate::distribute::{combine_group, ShareBlob};
use crate::error::{Error, Result};
use crate::tag::{check_name, Kind};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;

#[derive(Clone)]
pub struct BulkOptions {
    pub passes: Vec<Secret>,
    pub out_dir: PathBuf,
    pub cfg: AppConfig,
    /// Only try a passphrase on files whose name carries its tag (fast, relies on the naming).
    pub names_only: bool,
}

/// What one passphrase opened in one file.
#[derive(Clone, Debug)]
pub enum Found {
    /// A recovered file written to `path`.
    File { layout: &'static str, path: PathBuf, bytes: usize, pass_index: usize },
    /// A share of a split secret (set id in hex).
    Share { set: String, k: u8, n: u8, pass_index: usize },
}

#[derive(Clone, Debug)]
pub struct BulkEntry {
    pub file: PathBuf,
    pub found: Vec<Found>,
    /// Set when the file could not be looked at (not a carrier type, unreadable).
    pub skipped: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BulkSet {
    pub set: String,
    pub k: u8,
    pub n: u8,
    pub found: usize,
    pub files: Vec<PathBuf>,
    pub output: Option<PathBuf>,
    pub bytes: usize,
    /// How many more shares are needed (0 when rebuilt).
    pub missing: usize,
}

#[derive(Clone, Debug, Default)]
pub struct BulkReport {
    pub entries: Vec<BulkEntry>,
    pub sets: Vec<BulkSet>,
    pub out_dir: PathBuf,
}

impl BulkReport {
    pub fn recovered_files(&self) -> usize {
        self.entries.iter().flat_map(|e| &e.found).filter(|f| matches!(f, Found::File { .. })).count() + self.sets.iter().filter(|s| s.output.is_some()).count()
    }
}

/// Files in `dir` worth looking at (all regular files; non-carriers are reported as skipped later).
pub fn list_files(dir: &Path, recursive: bool, skip_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut names: Vec<PathBuf> = std::fs::read_dir(&d)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        names.sort();
        for p in names {
            if p.is_dir() {
                let skip = skip_dir.is_some_and(|s| std::fs::canonicalize(&p).ok() == std::fs::canonicalize(s).ok());
                if recursive && !skip {
                    stack.push(p);
                }
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_carrier(p: &Path) -> bool {
    ext_of(p).is_some_and(|e| is_image_ext(&e) || is_audio_ext(&e) || is_jpeg_ext(&e))
}

/// Writes `data` to a new file next to `stem` in `dir`, never overwriting.
fn write_new(dir: &Path, stem: &str, data: &[u8]) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for n in 0..1000 {
        let name = if n == 0 { format!("{stem}.recovered.bin") } else { format!("{stem}.{n}.recovered.bin") };
        let path = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(data)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::Invalid("too many recovered files with the same name".into()))
}

struct FileResult {
    entry: BulkEntry,
    shares: Vec<(ShareBlob, PathBuf)>,
}

fn look_at(file: &Path, opts: &BulkOptions) -> FileResult {
    let mut entry = BulkEntry { file: file.to_path_buf(), found: Vec::new(), skipped: None };
    let mut shares = Vec::new();
    if !is_carrier(file) {
        entry.skipped = Some("not an image or audio file".into());
        return FileResult { entry, shares };
    }
    let carrier = match AnyCarrier::load(file) {
        Ok(c) => c,
        Err(e) => {
            entry.skipped = Some(crate::app::friendly::message(&e));
            return FileResult { entry, shares };
        }
    };
    let cfg = opts.cfg.to_config();
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let stem = file.file_stem().and_then(|n| n.to_str()).unwrap_or("file");
    let mut idx_for_file = 0usize;
    for (pi, pass) in opts.passes.iter().enumerate() {
        if pass.is_empty() {
            continue;
        }
        let (try_single, try_hidden) = if opts.names_only {
            let kinds = check_name(pass.as_bytes(), name, cfg.kdf).unwrap_or_default();
            (kinds.iter().any(|k| matches!(k, Kind::Single | Kind::Share)), kinds.contains(&Kind::Hidden))
        } else {
            (true, true)
        };
        let mut hit = false;
        if try_single {
            if let Ok(raw) = extract_single(&carrier, pass.as_bytes(), &cfg) {
                hit = true;
                if let Some(blob) = ShareBlob::decode(&raw) {
                    entry.found.push(Found::Share { set: hex(&blob.id), k: blob.k, n: blob.n, pass_index: pi });
                    shares.push((blob, file.to_path_buf()));
                } else if let Ok(path) = write_new(&opts.out_dir, &numbered(stem, &mut idx_for_file), &raw) {
                    entry.found.push(Found::File { layout: "one file", path, bytes: raw.len(), pass_index: pi });
                }
            }
        }
        if !hit && try_hidden {
            if let Ok(data) = extract_deniable(&carrier, pass.as_bytes(), &cfg) {
                if let Ok(path) = write_new(&opts.out_dir, &numbered(stem, &mut idx_for_file), &data) {
                    entry.found.push(Found::File { layout: "two-passphrase layout", path, bytes: data.len(), pass_index: pi });
                }
            }
        }
    }
    FileResult { entry, shares }
}

/// First recovery from a file keeps the plain stem; later ones from the same file get a number.
fn numbered(stem: &str, n: &mut usize) -> String {
    *n += 1;
    if *n == 1 { stem.to_string() } else { format!("{stem}.{}", *n) }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Looks at every file with every passphrase, writes what it finds to `opts.out_dir` and rebuilds split
/// secrets once enough shares are present. `progress(done, total, file name)` is called as files finish.
pub fn run(files: &[PathBuf], opts: &BulkOptions, progress: &mut dyn FnMut(usize, usize, &str)) -> Result<BulkReport> {
    if opts.passes.iter().all(|p| p.is_empty()) {
        return Err(Error::Invalid("enter at least one passphrase".into()));
    }
    let total = files.len();
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8).min(total.max(1));
    let (tx, rx) = channel::<(usize, FileResult)>();
    let mut results: Vec<Option<FileResult>> = (0..total).map(|_| None).collect();
    std::thread::scope(|sc| {
        for _ in 0..threads {
            let tx = tx.clone();
            let next = &next;
            sc.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let r = look_at(&files[i], opts);
                if tx.send((i, r)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        let mut done = 0;
        for (i, r) in rx {
            done += 1;
            progress(done, total, files[i].file_name().and_then(|n| n.to_str()).unwrap_or(""));
            results[i] = Some(r);
        }
    });

    let mut report = BulkReport { out_dir: opts.out_dir.clone(), ..Default::default() };
    let mut groups: BTreeMap<String, Vec<(ShareBlob, PathBuf)>> = BTreeMap::new();
    for r in results.into_iter().flatten() {
        for (blob, path) in r.shares {
            groups.entry(hex(&blob.id)).or_default().push((blob, path));
        }
        report.entries.push(r.entry);
    }
    for (set, members) in groups {
        let blobs: Vec<&ShareBlob> = members.iter().map(|m| &m.0).collect();
        let (k, n) = (blobs[0].k, blobs[0].n);
        let mut distinct = members.iter().map(|m| m.1.clone()).collect::<Vec<_>>();
        distinct.sort();
        distinct.dedup();
        let secret = combine_group(&blobs);
        let (output, bytes, missing) = match secret {
            Some(s) => (write_new(&opts.out_dir, &format!("split_{}", &set[..6]), &s).ok(), s.len(), 0),
            None => (None, 0, (k as usize).saturating_sub(distinct.len()).max(1)),
        };
        report.sets.push(BulkSet { set: set[..6].to_string(), k, n, found: distinct.len(), files: distinct, output, bytes, missing });
    }
    Ok(report)
}
