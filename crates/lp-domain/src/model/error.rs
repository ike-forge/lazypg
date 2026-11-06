//! Failures from a cluster: typed and matched on, never a panic
//! (architecture.md §12).

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DbError {
    #[error("could not reach the server: {0}")]
    Unreachable(String),

    #[error("authentication failed: {0}")]
    Authentication(String),

    #[error("TLS failed: {0}")]
    Tls(String),

    #[error("query timed out")]
    Timeout,

    #[error("server rejected the statement: {0}")]
    Invalid(String),

    #[error("statement is not permitted on a read-only connection")]
    ReadOnly,

    #[error("cancelled")]
    Cancelled,

    /// The cluster is not told, so the write may still land.
    #[error("cancelled before the server answered; it may still have been applied")]
    CancelledWrite,

    #[error("{0}")]
    Other(String),
}
