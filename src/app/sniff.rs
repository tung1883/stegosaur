//! Guesses a file's type from its first bytes, so a recovered file gets a useful extension.
//! Hidden data is stored as raw bytes with no name, so this is the only hint there is.

/// The extension (without a dot) that fits `head`, or `"bin"` when nothing matches.
pub fn extension(head: &[u8]) -> &'static str {
    let starts = |m: &[u8]| head.starts_with(m);
    let at = |i: usize, m: &[u8]| head.len() >= i + m.len() && &head[i..i + m.len()] == m;
    if starts(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if starts(b"\xFF\xD8\xFF") {
        "jpg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "gif"
    } else if starts(b"%PDF-") {
        "pdf"
    } else if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        "zip"
    } else if starts(b"7z\xBC\xAF\x27\x1C") {
        "7z"
    } else if starts(b"Rar!\x1A\x07") {
        "rar"
    } else if starts(b"\x1F\x8B") {
        "gz"
    } else if starts(b"RIFF") && at(8, b"WAVE") {
        "wav"
    } else if starts(b"RIFF") && at(8, b"WEBP") {
        "webp"
    } else if starts(b"BM") && head.len() > 14 && head[6..10] == [0, 0, 0, 0] {
        "bmp"
    } else if starts(b"II*\0") || starts(b"MM\0*") {
        "tiff"
    } else if starts(b"ID3") || (head.len() > 1 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        "mp3"
    } else if starts(b"fLaC") {
        "flac"
    } else if starts(b"OggS") {
        "ogg"
    } else if at(4, b"ftyp") {
        "mp4"
    } else if starts(b"SQLite format 3\0") {
        "sqlite"
    } else if starts(b"MZ") {
        "exe"
    } else if is_text(head) {
        "txt"
    } else {
        "bin"
    }
}

/// Plain text: valid UTF-8 with no control characters other than line breaks and tabs.
fn is_text(head: &[u8]) -> bool {
    if head.is_empty() {
        return false;
    }
    // the sample may cut a multi-byte character in half; ignore a trailing partial one
    let ok = match std::str::from_utf8(head) {
        Ok(s) => s,
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&head[..e.valid_up_to()]).unwrap_or(""),
        Err(_) => return false,
    };
    !ok.is_empty() && ok.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}

#[cfg(test)]
mod tests {
    use super::extension;

    #[test]
    fn common_types_are_recognised() {
        assert_eq!(extension(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), "png");
        assert_eq!(extension(b"\xFF\xD8\xFF\xE0\0\x10JFIF"), "jpg");
        assert_eq!(extension(b"%PDF-1.7\n"), "pdf");
        assert_eq!(extension(b"PK\x03\x04\x14\0\0\0"), "zip");
        assert_eq!(extension(b"RIFF\x24\0\0\0WAVEfmt "), "wav");
        assert_eq!(extension(b"RIFF\x24\0\0\0WEBPVP8 "), "webp");
        assert_eq!(extension(b"\0\0\0\x18ftypmp42"), "mp4");
        assert_eq!(extension(b"ID3\x04\0\0"), "mp3");
    }

    #[test]
    fn text_and_unknown_data() {
        assert_eq!(extension(b"meet at noon\nbring the files\n"), "txt");
        assert_eq!(extension("caf\u{e9} \u{4e16}\u{754c}".as_bytes()), "txt");
        assert_eq!(extension(&[0x00, 0x01, 0x02, 0x9f, 0x10]), "bin");
        assert_eq!(extension(b""), "bin");
        // a text sample cut in the middle of a character is still text
        let s = "\u{4e16}\u{754c}".as_bytes();
        assert_eq!(extension(&s[..4]), "txt");
    }

    #[test]
    fn random_bytes_stay_bin() {
        let mut x = 0x1234_5678u32;
        let data: Vec<u8> = (0..512)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 8) as u8
            })
            .collect();
        // a couple of leading bytes could coincide with a signature; the rest of the data cannot be text
        let e = extension(&data);
        assert!(e == "bin" || e == "mp3" || e == "gz" || e == "exe" || e == "bmp", "{e}");
    }
}
