//! A stop request for the job that is running. Jobs look at it between steps (and between pieces of a
//! long write), so a stop takes effect at the next safe point and leaves no half-written output behind.

use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

/// Asks the running job to stop.
pub fn request() {
    STOP.store(true, Ordering::SeqCst);
}

/// Clears the request. Call before starting a job.
pub fn reset() {
    STOP.store(false, Ordering::SeqCst);
}

pub fn requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// `Err(Cancelled)` once a stop was requested.
pub fn check() -> Result<()> {
    if requested() { Err(Error::Cancelled) } else { Ok(()) }
}
