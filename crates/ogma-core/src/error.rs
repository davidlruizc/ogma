use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("audio device error: {0}")]
    Audio(String),

    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{provider} API error ({status}): {message}")]
    Api {
        provider: &'static str,
        status: u16,
        message: String,
    },

    #[error("config error: {0}")]
    Config(String),

    #[error("invalid state: {0}")]
    InvalidState(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Other(format!("json error: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
