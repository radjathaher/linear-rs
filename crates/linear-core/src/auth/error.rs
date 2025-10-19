use thiserror::Error;

use crate::config::ConfigError;

/// Errors surfaced by authentication and credential management routines.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("token refresh unavailable")]
    RefreshUnavailable,
}
