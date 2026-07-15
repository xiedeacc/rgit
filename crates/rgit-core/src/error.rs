//! Unified error type for the domain layer.

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,

    #[error("unauthorized")]
    Unauthorized,

    /// Authenticated but not allowed to perform the action.
    #[error("forbidden")]
    Forbidden,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("git command failed: {0}")]
    Git(String),

    #[error("{0}")]
    Internal(#[from] anyhow::Error),
}

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Error::Conflict(msg.into())
    }
}
