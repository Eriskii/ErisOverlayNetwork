/// Every way a step can refuse.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The credential is missing, forged, or belongs to someone else.
    #[error("missing or invalid credential")]
    Unauthorized,
    #[error("does not grant {permission}")]
    Forbidden { permission: String },
    #[error("bad request: {0}")]
    BadRequest(String),
    /// The step is valid in general but not in the current state.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("this code has expired")]
    Expired,
    /// The person at the prompt went away: input closed, ctrl-c, or time ran out.
    #[error("{0}")]
    Cancelled(String),
    /// The other side could not be reached, or stopped making sense.
    #[error("unreachable: {0}")]
    Unreachable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
