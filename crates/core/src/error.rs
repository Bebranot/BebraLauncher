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

    #[error("{0}")]
    Other(String),
}
