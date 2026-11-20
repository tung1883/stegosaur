//! Declarative form model: a screen is a list of fields, one of which has focus.

use std::path::PathBuf;
use zeroize::Zeroize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Id {
    Carrier,
    Payload,
    Pass,
    Mode,
    Parity,
    Output,
    Run,
    Cover,
    Second,
    TwoSlot,
    HiddenFile,
    HiddenPass,
    Slot,
    Image,
    Compare,
    Direction,
    Secret,
    Carriers,
    K,
    OutDir,
    JoinCarriers,
    JoinOutput,
    Style,
    Order,
    Message,
    TextOutput,
}

/// Which files the browser lets the user pick for a path field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathFilter {
    /// Lossless images (PNG, BMP, TIFF, WebP, PPM) and 16-bit audio (WAV, FLAC) that can carry data.
    Carrier,
    /// Anything the Reveal screen can read: carriers, plus other files (cover text).
    Readable,
    Any,
    /// A folder (chosen with `s` inside the browser).
    Dir,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Path { filter: PathFilter, value: Option<PathBuf> },
    Text { value: String, masked: bool, max: usize, hint: &'static str },
    Choice { options: Vec<&'static str>, idx: usize },
    Step { value: i64, min: i64, max: i64, step: i64, unit: &'static str },
    Paths { items: Vec<PathBuf>, cursor: usize, filter: PathFilter },
    Button,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub id: Id,
    pub label: &'static str,
    pub kind: Kind,
    pub hidden: bool,
}

impl Field {
    pub fn is_text(&self) -> bool {
        matches!(self.kind, Kind::Text { .. })
    }

    /// Appends a typed character to a text field; returns whether it was accepted.
    pub fn type_char(&mut self, c: char) -> bool {
        if let Kind::Text { value, max, .. } = &mut self.kind {
            if !c.is_control() && value.chars().count() < *max {
                value.push(c);
                return true;
            }
        }
        false
    }

    pub fn backspace(&mut self) {
        if let Kind::Text { value, .. } = &mut self.kind {
            value.pop();
        }
    }

    pub fn clear(&mut self) {
        match &mut self.kind {
            Kind::Text { value, .. } => value.zeroize(),
            Kind::Path { value, .. } => *value = None,
            Kind::Paths { items, cursor, .. } => {
                items.clear();
                *cursor = 0;
            }
            _ => {}
        }
    }

    /// Left/right arrows for choices and steppers (wraps for choices, clamps for steppers).
    pub fn change(&mut self, dir: i64) -> bool {
        match &mut self.kind {
            Kind::Choice { options, idx } => {
                let n = options.len() as i64;
                *idx = (((*idx as i64 + dir) % n + n) % n) as usize;
                true
            }
            Kind::Step { value, min, max, step, .. } => {
                let nv = (*value + dir * *step).clamp(*min, *max);
                let changed = nv != *value;
                *value = nv;
                changed
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Form {
    pub fields: Vec<Field>,
    pub focus: usize,
}

impl Form {
    pub fn new(fields: Vec<Field>) -> Self {
        let mut f = Self { fields, focus: 0 };
        if f.fields[0].hidden {
            f.focus_move(1);
        }
        f
    }

    fn pos(&self, id: Id) -> usize {
        self.fields.iter().position(|f| f.id == id).unwrap_or_else(|| panic!("no field {id:?}"))
    }

    pub fn get(&self, id: Id) -> &Field {
        &self.fields[self.pos(id)]
    }

    pub fn get_mut(&mut self, id: Id) -> &mut Field {
        let p = self.pos(id);
        &mut self.fields[p]
    }

    pub fn has(&self, id: Id) -> bool {
        self.fields.iter().any(|f| f.id == id)
    }

    pub fn path(&self, id: Id) -> Option<PathBuf> {
        match &self.get(id).kind {
            Kind::Path { value, .. } => value.clone(),
            _ => None,
        }
    }

    pub fn set_path(&mut self, id: Id, p: Option<PathBuf>) {
        if let Kind::Path { value, .. } = &mut self.get_mut(id).kind {
            *value = p;
        }
    }

    pub fn text(&self, id: Id) -> &str {
        match &self.get(id).kind {
            Kind::Text { value, .. } => value,
            _ => "",
        }
    }

    pub fn set_text(&mut self, id: Id, s: &str) {
        if let Kind::Text { value, .. } = &mut self.get_mut(id).kind {
            *value = s.to_string();
        }
    }

    pub fn choice(&self, id: Id) -> usize {
        match &self.get(id).kind {
            Kind::Choice { idx, .. } => *idx,
            _ => 0,
        }
    }

    pub fn set_choice(&mut self, id: Id, i: usize) {
        if let Kind::Choice { idx, options } = &mut self.get_mut(id).kind {
            *idx = i.min(options.len() - 1);
        }
    }

    pub fn step(&self, id: Id) -> i64 {
        match &self.get(id).kind {
            Kind::Step { value, .. } => *value,
            _ => 0,
        }
    }

    pub fn set_step(&mut self, id: Id, v: i64) {
        if let Kind::Step { value, min, max, .. } = &mut self.get_mut(id).kind {
            *value = v.clamp(*min, *max);
        }
    }

    pub fn paths(&self, id: Id) -> Vec<PathBuf> {
        match &self.get(id).kind {
            Kind::Paths { items, .. } => items.clone(),
            _ => Vec::new(),
        }
    }

    pub fn set_hidden(&mut self, id: Id, hidden: bool) {
        self.get_mut(id).hidden = hidden;
    }

    pub fn focused(&self) -> &Field {
        &self.fields[self.focus]
    }

    pub fn focused_mut(&mut self) -> &mut Field {
        &mut self.fields[self.focus]
    }

    /// Moves focus to the next (`1`) or previous (`-1`) visible field, wrapping around.
    pub fn focus_move(&mut self, dir: i32) {
        let n = self.fields.len() as i32;
        let mut i = self.focus as i32;
        for _ in 0..n {
            i = (i + dir + n) % n;
            if !self.fields[i as usize].hidden {
                self.focus = i as usize;
                return;
            }
        }
    }

    pub fn focus_id(&mut self, id: Id) {
        self.focus = self.pos(id);
    }

    /// Overwrites every passphrase-style (masked) text field in memory.
    pub fn wipe_secrets(&mut self) {
        for f in &mut self.fields {
            if let Kind::Text { value, masked: true, .. } = &mut f.kind {
                value.zeroize();
            }
        }
    }
}

// ---------- builders ----------

pub fn path(id: Id, label: &'static str, filter: PathFilter) -> Field {
    Field { id, label, kind: Kind::Path { filter, value: None }, hidden: false }
}

pub fn text(id: Id, label: &'static str, hint: &'static str, masked: bool) -> Field {
    Field { id, label, kind: Kind::Text { value: String::new(), masked, max: if masked { 64 } else { 200 }, hint }, hidden: false }
}

pub fn choice(id: Id, label: &'static str, options: &[&'static str], idx: usize) -> Field {
    Field { id, label, kind: Kind::Choice { options: options.to_vec(), idx }, hidden: false }
}

pub fn step(id: Id, label: &'static str, value: i64, min: i64, max: i64, step: i64, unit: &'static str) -> Field {
    Field { id, label, kind: Kind::Step { value, min, max, step, unit }, hidden: false }
}

pub fn paths(id: Id, label: &'static str, filter: PathFilter) -> Field {
    Field { id, label, kind: Kind::Paths { items: Vec::new(), cursor: 0, filter }, hidden: false }
}

pub fn button(label: &'static str) -> Field {
    Field { id: Id::Run, label, kind: Kind::Button, hidden: false }
}
