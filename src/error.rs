use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
    #[error("wav: {0}")]
    Wav(#[from] hound::Error),
    #[error("payload needs {needed} bytes but carrier holds {available}")]
    Capacity { needed: usize, available: usize },
    #[error("no payload found (wrong passphrase, wrong layout, or nothing embedded)")]
    NoPayload,
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("stopped")]
    Cancelled,
    #[error("crypto failure")]
    Crypto,
}

pub type Result<T> = std::result::Result<T, Error>;
