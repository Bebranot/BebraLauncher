use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid server address: {0}")]
    InvalidAddress(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unexpected http status {status} from {url}")]
    HttpStatus { status: u16, url: String },

    #[error("content database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("invalid hash: {0}")]
    InvalidHash(String),

    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("{0}")]
    Other(String),
}
