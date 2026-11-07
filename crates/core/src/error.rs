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

impl Error {
    /// A network failure that may pass on its own: a dropped or reset connection, a stalled body,
    /// a timeout, a gateway error. Unstable connections (and DPI that cuts long transfers) produce
    /// these mid-download; a retry resumes from what is already stored.
    pub fn is_transient_network(&self) -> bool {
        match self {
            Self::Http(e) => is_transient_reqwest(e),
            Self::HttpStatus { status, .. } => matches!(status, 408 | 429 | 500 | 502 | 503 | 504),
            Self::Io(e) => is_transient_io(e),
            _ => false,
        }
    }
}

fn is_transient_reqwest(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_body() || e.is_decode() || e.is_request()
}

fn is_transient_io(e: &std::io::Error) -> bool {
    use std::io::ErrorKind as K;
    if matches!(
        e.kind(),
        K::TimedOut
            | K::ConnectionReset
            | K::ConnectionAborted
            | K::NotConnected
            | K::BrokenPipe
            | K::UnexpectedEof
            | K::Interrupted
    ) {
        return true;
    }
    // A body stream turned into a reader carries the reqwest error inside an `io::Error`.
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
        .is_some_and(is_transient_reqwest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn network_drops_are_transient_and_disk_errors_are_not() {
        let transient = [
            Error::Io(io::Error::new(io::ErrorKind::ConnectionReset, "reset")),
            Error::Io(io::Error::new(io::ErrorKind::TimedOut, "stalled")),
            Error::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "eof")),
            Error::HttpStatus {
                status: 503,
                url: "https://cdn.example/".into(),
            },
        ];
        for e in &transient {
            assert!(e.is_transient_network(), "{e}");
        }
        let fatal = [
            Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
            Error::Io(io::Error::new(io::ErrorKind::StorageFull, "full")),
            Error::Io(io::Error::other("some other failure")),
            Error::HttpStatus {
                status: 404,
                url: "https://cdn.example/".into(),
            },
            Error::HashMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            Error::Other("bad zstd data".into()),
        ];
        for e in &fatal {
            assert!(!e.is_transient_network(), "{e}");
        }
    }
}
