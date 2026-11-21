//! Service layer shared by every front end (terminal UI, desktop app). It owns no UI types:
//! front ends translate their input into these plain structs and render what comes back.

pub mod bulk;
pub mod config;
pub mod files;
pub mod friendly;
pub mod hide;
pub mod jobs;
pub mod preview;
pub mod secret;
pub mod sniff;
pub mod text;
pub mod views;
pub mod wire;

pub use config::AppConfig;
pub use secret::Secret;
