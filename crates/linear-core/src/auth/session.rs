use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Type of token returned by Linear authentication endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    Bearer,
    ApiKey,
}

/// Represents a persisted Linear authentication session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: TokenType,
    pub expires_at: Option<DateTime<Utc>>,
    pub scope: Vec<String>,
    #[serde(default = "default_created_at")]
    pub created_at: DateTime<Utc>,
}

fn default_created_at() -> DateTime<Utc> {
    Utc::now()
}

impl AuthSession {
    pub fn new_access_token(
        access_token: String,
        refresh_token: Option<String>,
        expires_at: DateTime<Utc>,
        scope: Vec<String>,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            token_type: TokenType::Bearer,
            expires_at: Some(expires_at),
            scope,
            created_at: Utc::now(),
        }
    }

    pub fn new_api_key(key: String) -> Self {
        Self {
            access_token: key,
            refresh_token: None,
            token_type: TokenType::ApiKey,
            expires_at: None,
            scope: vec![],
            created_at: Utc::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(ts) => Utc::now() >= ts,
            None => false,
        }
    }

    pub fn will_expire_within(&self, window: Duration) -> bool {
        match self.expires_at {
            Some(ts) => Utc::now() + window >= ts,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_expiry_detection() {
        let session = AuthSession::new_access_token(
            "token".into(),
            Some("refresh".into()),
            Utc::now() + Duration::minutes(1),
            vec![],
        );
        assert!(!session.is_expired());
        assert!(session.will_expire_within(Duration::minutes(2)));
    }

    #[test]
    fn api_key_never_expires() {
        let session = AuthSession::new_api_key("key".into());
        assert!(!session.is_expired());
        assert!(!session.will_expire_within(Duration::hours(1)));
    }
}
