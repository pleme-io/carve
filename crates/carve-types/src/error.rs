//! Error type for carve-types operations.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid ticket key {key:?}: {reason}")]
    InvalidTicketKey { key: String, reason: &'static str },

    #[error("invalid commit sha {sha:?}: must be 7-40 hex chars")]
    InvalidCommitSha { sha: String },

    #[error("invalid blake3 hash {hash:?}: must be 64 hex chars")]
    InvalidBlakeHash { hash: String },

    #[error("plan parse error: {0}")]
    PlanParse(#[from] serde_yaml::Error),

    #[error("plan schema invariant violated: {0}")]
    PlanInvariant(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
