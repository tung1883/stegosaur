//! Keyed file-name tags: a name that looks like any camera or recorder file but lets the owner of a
//! passphrase recognise their own files later, from the name alone.
//!
//! The stem ends in six digits: two random digits (the nonce) and four tag digits computed from the
//! passphrase, the nonce and what the file is. Anyone without the passphrase sees an ordinary
//! counter. The tag goes through the same scrypt cost as the data, so it gives a guesser no cheaper
//! way to test passphrases than the file itself.
//!
//! It is a label, not security: a wrong passphrase matches with probability about 1 in 10,000
//! per kind, and a name proves nothing about the contents.
use crate::crypto::{derive, master_key, sha256, KdfParams};
use crate::error::Result;
use rand::Rng;
use std::path::{Path, PathBuf};

/// What a tagged file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One file under one passphrase.
    Single,
    /// The two-passphrase layout (tagged with the first passphrase).
    Hidden,
    /// One share of a split secret.
    Share,
}

const KINDS: [Kind; 3] = [Kind::Single, Kind::Hidden, Kind::Share];

fn kind_info(k: Kind) -> &'static str {
    match k {
        Kind::Single => "tag/single",
        Kind::Hidden => "tag/hidden",
        Kind::Share => "tag/share",
    }
}

fn tag_digits(master: &[u8; 32], kind: Kind) -> u32 {
    let d = derive(master, kind_info(kind));
    u32::from_le_bytes([d[0], d[1], d[2], d[3]]) % 10_000
}

fn master_for(pass: &[u8], nonce: u32, kdf: KdfParams) -> Result<[u8; 32]> {
    let salt = sha256(&[b"stegosaur/tag/v1", format!("{nonce:02}").as_bytes()]);
    master_key(pass, &salt, kdf)
}

const IMAGE_PREFIX: [&str; 5] = ["IMG_", "DSC_", "DSCN", "PICT", "IMG-"];
const AUDIO_PREFIX: [&str; 4] = ["REC_", "AUD_", "Voice ", "Track"];

/// A fresh tagged file name (without folder) that does not exist in `dir`.
pub fn tagged_name(pass: &[u8], kind: Kind, dir: &Path, ext: &str, audio: bool, kdf: KdfParams) -> Result<String> {
    let mut rng = rand::thread_rng();
    let prefixes: &[&str] = if audio { &AUDIO_PREFIX } else { &IMAGE_PREFIX };
    let mut nonces: Vec<u32> = (0..100).collect();
    for i in (1..nonces.len()).rev() {
        nonces.swap(i, rng.gen_range(0..=i));
    }
    let mut last = String::new();
    for nonce in nonces {
        let tag = tag_digits(&master_for(pass, nonce, kdf)?, kind);
        let prefix = prefixes[rng.gen_range(0..prefixes.len())];
        last = format!("{prefix}{nonce:02}{tag:04}.{ext}");
        if !dir.join(&last).exists() {
            return Ok(last);
        }
    }
    Ok(last)
}

/// The tagged path next to `carrier`.
pub fn tagged_output(carrier: &Path, pass: &[u8], kind: Kind, kdf: KdfParams) -> Result<PathBuf> {
    let ext = carrier.extension().and_then(|s| s.to_str()).unwrap_or("png");
    let dir = carrier.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let audio = crate::carrier::is_audio_ext(&ext.to_ascii_lowercase());
    Ok(dir.join(tagged_name(pass, kind, dir, ext, audio, kdf)?))
}

/// Which kinds of file `name` could be for this passphrase. Empty when the name carries no matching tag.
pub fn check_name(pass: &[u8], name: &str, kdf: KdfParams) -> Result<Vec<Kind>> {
    let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let digits: Vec<u32> = stem.chars().rev().take_while(|c| c.is_ascii_digit()).map(|c| c.to_digit(10).unwrap()).collect();
    if digits.len() < 6 {
        return Ok(Vec::new());
    }
    // `digits` is reversed: [d6, d5, ..., d1]; the last six digits are nonce(2) + tag(4).
    let six: Vec<u32> = digits[..6].iter().rev().copied().collect();
    let nonce = six[0] * 10 + six[1];
    let tag = six[2..].iter().fold(0, |a, d| a * 10 + d);
    let master = master_for(pass, nonce, kdf)?;
    Ok(KINDS.iter().copied().filter(|&k| tag_digits(&master, k) == tag).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_verify_for_their_passphrase_and_kind_only() {
        let kdf = KdfParams::fast();
        let dir = std::env::temp_dir();
        for kind in KINDS {
            let name = tagged_name(b"correct horse", kind, &dir, "png", false, kdf).unwrap();
            assert!(check_name(b"correct horse", &name, kdf).unwrap().contains(&kind), "{name}");
            let other = check_name(b"another phrase", &name, kdf).unwrap();
            assert!(!other.contains(&kind) || other.len() <= 1, "{name}");
        }
    }

    #[test]
    fn plain_names_carry_no_tag_and_wrong_passphrases_rarely_match() {
        let kdf = KdfParams::fast();
        assert!(check_name(b"pw", "holiday.png", kdf).unwrap().is_empty());
        assert!(check_name(b"pw", "IMG_12.png", kdf).unwrap().is_empty());
        let mut hits = 0;
        for i in 0..2000 {
            let name = format!("IMG_{:02}{:04}.png", i % 100, (i * 7919) % 10_000);
            hits += check_name(b"some passphrase", &name, kdf).unwrap().len();
        }
        assert!(hits < 10, "{hits}");
    }

    #[test]
    fn names_look_ordinary_and_do_not_repeat_in_a_folder() {
        let kdf = KdfParams::fast();
        let d = std::env::temp_dir().join(format!("stego_tag_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..30 {
            let n = tagged_name(b"pw", Kind::Single, &d, "png", false, kdf).unwrap();
            assert!(!n.to_lowercase().contains("stego"));
            assert!(n.ends_with(".png") && n.chars().filter(|c| c.is_ascii_digit()).count() >= 6);
            std::fs::write(d.join(&n), b"x").unwrap();
            assert!(seen.insert(n));
        }
        std::fs::remove_dir_all(&d).ok();
    }
}
