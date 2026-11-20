use crate::carrier::Mode;
use crate::container::Config;
use crate::crypto::KdfParams;

/// The settings every front end (CLI, terminal UI, desktop app) shares, so they all agree on defaults.
#[derive(Clone, Copy, Debug)]
pub struct AppConfig {
    pub mode: Mode,
    /// Reed-Solomon check bytes per 255-byte block; 0 turns error correction off.
    pub parity: usize,
    /// Hidden layout: fraction of each half used by a slot.
    pub rate: f64,
    pub kdf: KdfParams,
}

impl Default for AppConfig {
    fn default() -> Self {
        let c = Config::default();
        Self { mode: c.mode, parity: c.parity, rate: c.rate, kdf: c.kdf }
    }
}

impl AppConfig {
    /// The real configuration used when writing files.
    pub fn to_config(&self) -> Config {
        Config { kdf: self.kdf, mode: self.mode, rate: self.rate, parity: self.parity, seed: None }
    }

    /// Configuration for what-if measurements: cheap key derivation and a fixed random stream.
    /// Placement statistics do not depend on the key, so this measures the same thing faster.
    pub fn preview_config(&self) -> Config {
        Config { kdf: KdfParams::fast(), seed: Some(1), ..self.to_config() }
    }
}
