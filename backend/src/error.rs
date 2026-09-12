use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotImplemented(&'static str),
    InvalidInput(&'static str),
    AuthenticationFailed,
    KeyUnavailable,
    State(&'static str),
    ReplayRejected,
    EpochConflict { retry_epoch: crate::ids::Epoch },
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented(operation) => write!(f, "not implemented: {operation}"),
            Self::InvalidInput(message) => write!(f, "invalid input: {message}"),
            Self::AuthenticationFailed => f.write_str("authentication failed"),
            Self::KeyUnavailable => f.write_str("key unavailable or retired"),
            Self::State(message) => write!(f, "invalid state: {message}"),
            Self::ReplayRejected => f.write_str("replayed or expired counter"),
            Self::EpochConflict { retry_epoch } => {
                write!(f, "epoch conflict; retry at epoch {}", retry_epoch.0)
            }
            Self::Io(error) => write!(f, "I/O error: {error}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
