use zeroize::Zeroizing;

/// A passphrase that is wiped from memory when dropped and never printed.
/// Wiping is best effort: copies made by the caller before wrapping are not tracked.
#[derive(Clone)]
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn new(s: impl AsRef<[u8]>) -> Self {
        Self(Zeroizing::new(s.as_ref().to_vec()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}
