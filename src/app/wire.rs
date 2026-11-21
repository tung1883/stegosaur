//! A small header placed in front of a hidden file so the original name comes back on reveal.
//!
//! It sits inside the encrypted data, so it is as private as the file itself. Data without the header
//! (typed text, files hidden by older versions or by the plain command line) is returned as it is.

/// Marks a header. Eight fixed bytes: a genuine file starting with them is not a realistic worry.
const MAGIC: [u8; 8] = [0x8F, b'S', b'T', b'G', b'N', 0x01, 0xD2, 0x5A];
const MAX_NAME: usize = 255;

/// The header for a file called `name` (only the last path part is kept, cut to 255 bytes).
pub fn header(name: &str) -> Vec<u8> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut end = base.len().min(MAX_NAME);
    while !base.is_char_boundary(end) {
        end -= 1;
    }
    let mut h = Vec::with_capacity(MAGIC.len() + 1 + end);
    h.extend_from_slice(&MAGIC);
    h.push(end as u8);
    h.extend_from_slice(&base.as_bytes()[..end]);
    h
}

/// Splits recovered bytes into the stored name (if any) and the offset where the file itself starts.
pub fn split(data: &[u8]) -> (Option<String>, usize) {
    if data.len() < MAGIC.len() + 1 || data[..MAGIC.len()] != MAGIC {
        return (None, 0);
    }
    let n = data[MAGIC.len()] as usize;
    let start = MAGIC.len() + 1 + n;
    if data.len() < start {
        return (None, 0);
    }
    match std::str::from_utf8(&data[MAGIC.len() + 1..start]) {
        Ok(name) => (safe_name(name), start),
        Err(_) => (None, start),
    }
}

/// A name that is safe to create in a folder of our choosing: no folders, no characters Windows refuses,
/// no reserved device names. `None` when nothing usable is left.
pub fn safe_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')).collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if cleaned.is_empty() {
        return None;
    }
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];
    if reserved.contains(&stem.as_str()) {
        return None;
    }
    Some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_survives_the_round_trip() {
        let mut data = header("holiday plans.pdf");
        data.extend_from_slice(b"%PDF-1.7 body");
        let (name, at) = split(&data);
        assert_eq!(name.as_deref(), Some("holiday plans.pdf"));
        assert_eq!(&data[at..], b"%PDF-1.7 body");
    }

    #[test]
    fn data_without_a_header_is_untouched() {
        assert_eq!(split(b"meet at noon"), (None, 0));
        assert_eq!(split(b""), (None, 0));
        assert_eq!(split(&MAGIC), (None, 0));
    }

    #[test]
    fn folders_and_bad_names_are_cleaned() {
        assert_eq!(safe_name("..\\..\\windows\\evil.exe").as_deref(), Some("evil.exe"));
        assert_eq!(safe_name("/etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_name("a<b>c?.txt").as_deref(), Some("abc.txt"));
        assert_eq!(safe_name("..").as_deref(), None);
        assert_eq!(safe_name("CON.txt").as_deref(), None);
        assert_eq!(safe_name("").as_deref(), None);
        // a header that smuggles a path stores only the last part
        let mut d = header("sub/dir/file.bin");
        d.push(1);
        assert_eq!(split(&d).0.as_deref(), Some("file.bin"));
    }

    #[test]
    fn long_names_are_cut_on_a_character_boundary() {
        let long = "\u{4e16}".repeat(200);
        let h = header(&long);
        assert!(h.len() <= MAGIC.len() + 1 + MAX_NAME);
        assert!(split(&h).0.is_some());
    }
}
