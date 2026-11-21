//! Types that cross the boundary between the web UI and Rust, as JSON.
//! Requests accept missing optional fields; responses use stable lowercase names.

use serde::{Deserialize, Serialize};

fn default_mode() -> String {
    "match".into()
}
fn default_parity() -> usize {
    16
}
fn default_rate() -> f64 {
    0.10
}

#[derive(Serialize)]
pub struct StyleDto {
    pub id: &'static str,
    pub adjustable_order: bool,
    pub default_order: usize,
    pub bits_per_word: f64,
    pub well_formed: Option<u8>,
}

#[derive(Serialize)]
pub struct Defaults {
    pub parity: usize,
    pub rate: f64,
    pub mode: &'static str,
    pub home: String,
    pub cwd: String,
    pub styles: Vec<StyleDto>,
}

#[derive(Serialize)]
pub struct EntryDto {
    pub path: String,
    pub name: String,
    /// "dir", "carrier", "unusable" or "other".
    pub kind: &'static str,
    /// "png", "bmp", "tiff", "webp", "ppm", "wav" or "flac" for usable carriers.
    pub carrier: Option<&'static str>,
    pub size: u64,
    pub summary: Option<String>,
    pub samples: Option<usize>,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct ListingDto {
    pub dir: String,
    pub parent: Option<String>,
    pub entries: Vec<EntryDto>,
}

#[derive(Serialize, Clone, Copy)]
pub struct ReportDto {
    pub rs: f64,
    pub spa: f64,
    pub chi_p: f64,
    pub score: f64,
    /// "low", "medium" or "high".
    pub risk: &'static str,
}

#[derive(Serialize)]
pub struct MeasuredDto {
    pub before: ReportDto,
    pub after: ReportDto,
}

#[derive(Deserialize)]
pub struct PreviewReq {
    /// "embed", "hidden" or "split".
    pub op: String,
    pub carrier: Option<String>,
    #[serde(default)]
    pub carriers: Vec<String>,
    pub payload_len: Option<usize>,
    pub hidden_len: Option<usize>,
    pub secret_len: Option<usize>,
    pub k: Option<usize>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_parity")]
    pub parity: usize,
    #[serde(default = "default_rate")]
    pub rate: f64,
    /// Also measure detectability on a copy (slower than capacity alone).
    #[serde(default)]
    pub measure: bool,
}

#[derive(Serialize, Default)]
pub struct SplitDto {
    pub share_len: usize,
    pub carriers: Vec<SplitCarrierDto>,
}

#[derive(Serialize)]
pub struct SplitCarrierDto {
    pub capacity: usize,
    pub fits: bool,
}

#[derive(Serialize, Default)]
pub struct PreviewDto {
    pub summary: Option<String>,
    pub samples: Option<usize>,
    pub capacity: usize,
    pub need: usize,
    pub fits: bool,
    pub fill: f64,
    pub error: Option<String>,
    pub public_fits: Option<bool>,
    pub hidden_fits: Option<bool>,
    pub measured: Option<MeasuredDto>,
    pub detect_note: Option<String>,
    pub split: Option<SplitDto>,
}

#[derive(Deserialize)]
pub struct ViewsReq {
    /// "embed" or "hidden".
    pub op: String,
    pub carrier: String,
    /// A real output file to compare against; when absent the embed is simulated on a copy.
    pub stego: Option<String>,
    pub payload_len: Option<usize>,
    pub hidden_len: Option<usize>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_parity")]
    pub parity: usize,
    #[serde(default = "default_rate")]
    pub rate: f64,
    #[serde(default = "default_max_dim")]
    pub max_dim: u32,
}

fn default_max_dim() -> u32 {
    512
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobDto {
    /// Look at files and folders with every passphrase and recover what opens.
    Bulk {
        inputs: Vec<String>,
        passes: Vec<String>,
        out_dir: Option<String>,
        #[serde(default)]
        recursive: bool,
        #[serde(default)]
        names_only: bool,
        /// Read .txt and .log files as cover text in this style.
        text: Option<TextOptsDto>,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default = "default_rate")]
        rate: f64,
        #[serde(default = "default_kdf")]
        kdf: String,
    },
    /// The unified Hide job: every layout, typed or file secrets, naming schemes.
    Hide {
        /// single, same, two, split or bulk
        layout: String,
        payloads: Vec<PayloadDto>,
        covers: Vec<String>,
        second: Option<PayloadDto>,
        pass: String,
        pass2: Option<String>,
        #[serde(default = "default_k")]
        k: u8,
        /// ordinary, suffix or pattern
        #[serde(default = "default_naming")]
        naming: String,
        pattern: Option<String>,
        out_dir: Option<String>,
        #[serde(default = "default_mode")]
        mode: String,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default = "default_rate")]
        rate: f64,
        #[serde(default = "default_kdf")]
        kdf: String,
        #[serde(default)]
        overwrite: bool,
        /// Use an encrypted block at the end of the file when the cover is too small.
        #[serde(default = "default_true")]
        extra: bool,
    },
    Embed {
        carrier: String,
        payload: String,
        output: Option<String>,
        pass: String,
        #[serde(default = "default_mode")]
        mode: String,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default)]
        overwrite: bool,
    },
    Hidden {
        carrier: String,
        public_file: String,
        public_pass: String,
        hidden_file: Option<String>,
        hidden_pass: Option<String>,
        output: Option<String>,
        #[serde(default = "default_mode")]
        mode: String,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default = "default_rate")]
        rate: f64,
        #[serde(default)]
        overwrite: bool,
    },
    Split {
        secret: String,
        k: u8,
        pass: String,
        carriers: Vec<String>,
        out_dir: Option<String>,
        #[serde(default = "default_mode")]
        mode: String,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default)]
        overwrite: bool,
    },
    Reveal {
        carrier: String,
        pass: String,
        output: Option<String>,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default = "default_rate")]
        rate: f64,
        #[serde(default)]
        overwrite: bool,
    },
    Join {
        carriers: Vec<String>,
        pass: String,
        output: Option<String>,
        #[serde(default = "default_parity")]
        parity: usize,
        #[serde(default)]
        overwrite: bool,
    },
    Analyze {
        image: String,
    },
    TextHide {
        message: String,
        pass: String,
        style: String,
        order: Option<usize>,
        save: Option<String>,
        #[serde(default)]
        overwrite: bool,
    },
    TextReveal {
        text_file: String,
        pass: String,
        style: String,
        order: Option<usize>,
        save: Option<String>,
        #[serde(default)]
        overwrite: bool,
    },
}

/// A secret: a file, or text typed on screen.
#[derive(Deserialize)]
pub struct PayloadDto {
    pub path: Option<String>,
    pub text: Option<String>,
}

#[derive(Deserialize)]
pub struct TextOptsDto {
    pub style: String,
    pub order: Option<usize>,
}

fn default_k() -> u8 {
    2
}
fn default_naming() -> String {
    "ordinary".into()
}
fn default_kdf() -> String {
    "standard".into()
}

// Passphrases must never reach a log, so Debug shows only which kind of job this is.
impl std::fmt::Debug for JobDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let k = match self {
            JobDto::Embed { .. } => "embed",
            JobDto::Hidden { .. } => "hidden",
            JobDto::Split { .. } => "split",
            JobDto::Reveal { .. } => "reveal",
            JobDto::Join { .. } => "join",
            JobDto::Analyze { .. } => "analyze",
            JobDto::Bulk { .. } => "bulk",
            JobDto::Hide { .. } => "hide",
            JobDto::TextHide { .. } => "text_hide",
            JobDto::TextReveal { .. } => "text_reveal",
        };
        write!(f, "JobDto::{k}(..)")
    }
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultDto {
    Embedded { output: String, bytes: usize, report: Option<ReportDto> },
    Hidden { output: String, report: Option<ReportDto> },
    Split { outputs: Vec<String> },
    Revealed { output: String, bytes: usize, layout: &'static str },
    Joined { output: String, bytes: usize, used: usize },
    Analyzed { report: ReportDto },
    Bulk { out_dir: String, checked: usize, items: Vec<BulkItemDto>, sets: Vec<BulkSetDto> },
    Hide { layout: &'static str, items: Vec<HideItemDto> },
    TextHidden { text: String, words: usize, saved: Option<String> },
    TextRevealed { text: String, bytes: usize, saved: Option<String> },
}

/// One result of the Hide job.
#[derive(Serialize, Clone)]
pub struct HideItemDto {
    pub what: String,
    pub cover: String,
    pub output: Option<String>,
    pub bytes: usize,
    pub report: Option<ReportDto>,
    pub error: Option<String>,
}

/// One line of a bulk report. `status` is file, share, nothing or skipped.
#[derive(Serialize, Clone)]
pub struct BulkItemDto {
    pub name: String,
    pub status: &'static str,
    pub detail: String,
    pub output: Option<String>,
    pub bytes: usize,
}

#[derive(Serialize, Clone)]
pub struct BulkSetDto {
    pub set: String,
    pub k: u8,
    pub n: u8,
    pub found: usize,
    pub missing: usize,
    pub output: Option<String>,
    pub bytes: usize,
}

#[derive(Serialize, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventDto {
    Step { id: u64, message: String },
    Done { id: u64, result: ResultDto },
    Error { id: u64, message: String },
}

#[derive(Deserialize)]
pub struct TextEstimateReq {
    pub style: String,
    pub order: Option<usize>,
    pub len: usize,
}

#[derive(Serialize)]
pub struct TextEstimateDto {
    pub words: usize,
    pub lines: usize,
    pub bits_per_word: f64,
    pub well_formed: Option<u8>,
}

fn default_true() -> bool {
    true
}
