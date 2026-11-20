//! Everything the Stegosaur desktop app does, with no dependency on the windowing toolkit.
//! The Tauri shell only forwards calls here, which keeps this testable on any machine.

pub mod api;
pub mod dto;

pub use api::Core;
