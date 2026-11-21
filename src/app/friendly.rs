use crate::error::Error;

fn fmt_bytes(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1e6)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// Human-readable text for an error, suitable for showing directly in a UI.
pub fn message(e: &Error) -> String {
    match e {
        Error::Capacity { needed, available } => format!(
            "That file needs {} but this image holds {}. Pick a bigger image or a smaller file.",
            fmt_bytes(*needed),
            fmt_bytes(*available)
        ),
        Error::NoPayload => "Nothing could be recovered. The passphrase or settings may be wrong, or nothing is hidden here.".into(),
        Error::Unsupported(s) => format!("Not supported: {s}."),
        Error::Invalid(s) => {
            let mut c = s.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                None => String::new(),
            }
        }
        Error::Io(e) => format!("File problem: {e}."),
        Error::Image(e) => format!("Could not read or write the image: {e}."),
        Error::Wav(e) => format!("Could not read or write the audio file: {e}."),
        Error::Cancelled => "Stopped.".into(),
        Error::Crypto => "Encryption failed unexpectedly.".into(),
    }
}
