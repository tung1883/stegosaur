//! Modal file browser state. Rendering lives in `ui`; this is only navigation and selection rules.

use crate::form::{Id, PathFilter};
use std::path::{Path, PathBuf};
use stegosaur::app::files::{scan_dir, Entry, FileKind};

#[derive(Clone, Debug)]
pub enum Row {
    Parent,
    Entry(Entry),
}

pub struct Browser {
    pub title: String,
    /// `None` shows the list of drives (Windows) above a root folder.
    pub dir: Option<PathBuf>,
    pub rows: Vec<Row>,
    pub sel: usize,
    pub filter: PathFilter,
    pub target: Id,
    pub note: Option<String>,
}

fn drives() -> Vec<Entry> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .filter(|p| p.exists())
        .map(|p| Entry { name: p.display().to_string(), path: p, kind: FileKind::Dir, size: 0, info: None, note: None })
        .collect()
}

impl Browser {
    pub fn open(start: &Path, filter: PathFilter, target: Id, title: &str) -> Self {
        let mut b = Self { title: title.to_string(), dir: Some(start.to_path_buf()), rows: Vec::new(), sel: 0, filter, target, note: None };
        b.reload();
        b
    }

    pub fn reload(&mut self) {
        self.note = None;
        self.rows.clear();
        match self.dir.clone() {
            Some(dir) => {
                self.rows.push(Row::Parent);
                match scan_dir(&dir) {
                    Ok(entries) => self.rows.extend(entries.into_iter().map(Row::Entry)),
                    Err(e) => self.note = Some(format!("Cannot open this folder: {e}")),
                }
            }
            None => self.rows.extend(drives().into_iter().map(Row::Entry)),
        }
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
    }

    /// Can this row be chosen with Enter?
    pub fn selectable(&self, row: &Row) -> bool {
        match row {
            Row::Parent => false,
            Row::Entry(e) => match (self.filter, e.kind) {
                (_, FileKind::Dir) => false,
                (PathFilter::Carrier, FileKind::Carrier(_)) => true,
                (PathFilter::Readable, FileKind::Carrier(_) | FileKind::Other) => true,
                (PathFilter::Any, FileKind::Carrier(_) | FileKind::Other | FileKind::Unusable) => true,
                _ => false,
            },
        }
    }

    pub fn move_sel(&mut self, delta: i32) {
        let n = self.rows.len() as i32;
        if n > 0 {
            self.sel = (self.sel as i32 + delta).clamp(0, n - 1) as usize;
        }
    }

    pub fn jump(&mut self, to_end: bool) {
        self.sel = if to_end { self.rows.len().saturating_sub(1) } else { 0 };
    }

    pub fn up(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        match dir.parent() {
            Some(p) if !p.as_os_str().is_empty() => self.dir = Some(p.to_path_buf()),
            _ => self.dir = None,
        }
        self.sel = 0;
        self.reload();
    }

    fn enter(&mut self, dir: PathBuf) {
        self.dir = Some(dir);
        self.sel = 0;
        self.reload();
    }

    /// Enter on the selected row: opens folders, goes up on "..", returns a path for a selectable file.
    pub fn activate(&mut self) -> Option<PathBuf> {
        match self.rows.get(self.sel).cloned()? {
            Row::Parent => {
                self.up();
                None
            }
            Row::Entry(e) if e.kind == FileKind::Dir => {
                self.enter(e.path);
                None
            }
            row @ Row::Entry(_) => {
                if !self.selectable(&row) {
                    if let Row::Entry(e) = &row {
                        self.note = Some(e.note.clone().unwrap_or_else(|| "That file cannot be used here.".into()));
                    }
                    return None;
                }
                match row {
                    Row::Entry(e) => Some(e.path),
                    Row::Parent => None,
                }
            }
        }
    }

    /// For folder pickers: the folder currently shown.
    pub fn choose_dir(&self) -> Option<PathBuf> {
        if self.filter == PathFilter::Dir { self.dir.clone() } else { None }
    }
}
