use reqwest::StatusCode;
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
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("token endpoint error {status}: {body}")]
    TokenEndpoint { status: StatusCode, body: String },
    #[error("invalid token type '{0}'")]
    InvalidTokenType(String),
    #[error("token refresh unavailable")]
    RefreshUnavailable,
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("authorization listener terminated before receiving redirect")]
    ListenerClosed,
    #[error("authorization flow cancelled")]
    Cancelled,
    #[error("authorization request denied ({0})")]
    AccessDenied(String),
    #[error("authorization response missing code parameter")]
    MissingAuthorizationCode,
    #[error("authorization state mismatch")]
    StateMismatch,
    #[error("failed to launch system browser: {0}")]
    BrowserLaunch(String),
    #[error("invalid authorization response: {0}")]
    InvalidAuthorizationResponse(String),
    #[error("unsupported auth flow: {0}")]
    UnsupportedFlow(&'static str),
}
