use std::ops::RangeInclusive;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use super::{AuthError, AuthSession, PkcePair, TokenType};

pub const DEFAULT_CLIENT_ID: &str = "linear-rs-public";
pub const DEFAULT_REDIRECT_HOST: &str = "127.0.0.1";
pub const DEFAULT_REDIRECT_PATH: &str = "/callback";
pub const DEFAULT_REDIRECT_PORT_START: u16 = 9000;
pub const DEFAULT_REDIRECT_PORT_END: u16 = 9999;
pub const DEFAULT_SCOPES: &[&str; 2] = &["read", "write"];

pub fn default_redirect_ports() -> RangeInclusive<u16> {
    DEFAULT_REDIRECT_PORT_START..=DEFAULT_REDIRECT_PORT_END
}

pub fn default_redirect_uri(port: u16) -> Result<Url, url::ParseError> {
    Url::parse(&format!(
        "http://{DEFAULT_REDIRECT_HOST}:{port}{DEFAULT_REDIRECT_PATH}"
    ))
}

const DEFAULT_USER_AGENT: &str = "linear-rs/0.1.0";

/// OAuth client configuration supplied by consumers.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: Url,
    pub scopes: Vec<String>,
}

impl OAuthConfig {
    pub fn new<S: Into<String>>(client_id: S, redirect_uri: Url) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            redirect_uri,
            scopes: vec![],
        }
    }

    pub fn with_defaults() -> Self {
        let redirect_uri =
            default_redirect_uri(DEFAULT_REDIRECT_PORT_START).expect("valid redirect URI");
        let mut config = Self::new(DEFAULT_CLIENT_ID, redirect_uri);
        config.scopes = DEFAULT_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .collect();
        config
    }

    pub fn with_secret<S: Into<String>>(mut self, secret: S) -> Self {
        self.client_secret = Some(secret.into());
        self
    }

    pub fn with_scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }
}

/// OAuth endpoints used for browser/manual flows.
#[derive(Debug, Clone)]
pub struct OAuthEndpoints {
    pub authorization_url: Url,
    pub token_url: Url,
}

impl Default for OAuthEndpoints {
    fn default() -> Self {
        Self {
            authorization_url: Url::parse("https://linear.app/oauth/authorize").unwrap(),
            token_url: Url::parse("https://api.linear.app/oauth/token").unwrap(),
        }
    }
}

/// Bundles the results of a token exchange.
#[derive(Debug, Clone)]
pub struct TokenExchangeResult {
    pub session: AuthSession,
    pub received_at: DateTime<Utc>,
}

/// Performs OAuth token exchanges with the Linear API.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    http: Client,
    config: OAuthConfig,
    endpoints: OAuthEndpoints,
}

impl OAuthClient {
    pub fn new(config: OAuthConfig) -> Result<Self, AuthError> {
        Self::with_endpoints(config, OAuthEndpoints::default())
    }

    pub fn with_endpoints(
        config: OAuthConfig,
        endpoints: OAuthEndpoints,
    ) -> Result<Self, AuthError> {
        let http = Client::builder().user_agent(DEFAULT_USER_AGENT).build()?;
        Ok(Self {
            http,
            config,
            endpoints,
        })
    }

    /// Clone the OAuth client while overriding the redirect URI.
    pub fn clone_with_redirect(&self, redirect_uri: Url) -> Self {
        let mut config = self.config.clone();
        config.redirect_uri = redirect_uri;
        Self {
            http: self.http.clone(),
            config,
            endpoints: self.endpoints.clone(),
        }
    }

    pub fn config(&self) -> &OAuthConfig {
        &self.config
    }

    pub fn endpoints(&self) -> &OAuthEndpoints {
        &self.endpoints
    }

    pub fn authorization_url(&self, pkce: &PkcePair, state: &str) -> Result<Url, AuthError> {
        let mut url = self.endpoints.authorization_url.clone();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("response_type", "code");
            pairs.append_pair("client_id", &self.config.client_id);
            pairs.append_pair("redirect_uri", self.config.redirect_uri.as_str());
            if !self.config.scopes.is_empty() {
                pairs.append_pair("scope", &self.config.scopes.join(" "));
            }
            pairs.append_pair("code_challenge", pkce.challenge());
            pairs.append_pair("code_challenge_method", "S256");
            pairs.append_pair("state", state);
        }
        Ok(url)
    }

    /// Exchange an authorization code for access/refresh tokens.
    pub async fn exchange_code(
        &self,
        code: &str,
        pkce: &PkcePair,
    ) -> Result<TokenExchangeResult, AuthError> {
        let mut form = vec![
            ("grant_type".to_string(), "authorization_code".to_string()),
            ("code".to_string(), code.to_owned()),
            (
                "redirect_uri".to_string(),
                self.config.redirect_uri.to_string(),
            ),
            ("code_verifier".to_string(), pkce.verifier().to_owned()),
            ("client_id".to_string(), self.config.client_id.clone()),
        ];

        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret".to_string(), secret.clone()));
        }

        if !self.config.scopes.is_empty() {
            form.push(("scope".to_string(), self.config.scopes.join(" ")));
        }

        let response = self
            .http
            .post(self.endpoints.token_url.clone())
            .form(&form)
            .send()
            .await?;

        self.handle_token_response(response).await
    }

    /// Refresh an existing session using its refresh token.
    pub async fn refresh_session(
        &self,
        existing: &AuthSession,
    ) -> Result<TokenExchangeResult, AuthError> {
        let refresh_token = existing
            .refresh_token
            .as_ref()
            .ok_or(AuthError::RefreshUnavailable)?;

        let mut form = vec![
            ("grant_type".to_string(), "refresh_token".to_string()),
            ("refresh_token".to_string(), refresh_token.clone()),
            ("client_id".to_string(), self.config.client_id.clone()),
        ];

        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret".to_string(), secret.clone()));
        }

        let response = self
            .http
            .post(self.endpoints.token_url.clone())
            .form(&form)
            .timeout(StdDuration::from_secs(30))
            .send()
            .await?;

        let mut token_result = self.handle_token_response(response).await?;

        if token_result.session.refresh_token.is_none() {
            token_result.session.refresh_token = existing.refresh_token.clone();
        }

        Ok(token_result)
    }

    /// Request client credentials (machine-to-machine) tokens.
    pub async fn client_credentials(
        &self,
        scopes: &[String],
    ) -> Result<TokenExchangeResult, AuthError> {
        let mut form = vec![
            ("grant_type".to_string(), "client_credentials".to_string()),
            ("client_id".to_string(), self.config.client_id.clone()),
        ];

        if let Some(secret) = &self.config.client_secret {
            form.push(("client_secret".to_string(), secret.clone()));
        }

        if !scopes.is_empty() {
            form.push(("scope".to_string(), scopes.join(" ")));
        }

        let response = self
            .http
            .post(self.endpoints.token_url.clone())
            .form(&form)
            .timeout(StdDuration::from_secs(30))
            .send()
            .await?;

        self.handle_token_response(response).await
    }

    async fn handle_token_response(
        &self,
        response: reqwest::Response,
    ) -> Result<TokenExchangeResult, AuthError> {
        let status = response.status();
        let received_at = Utc::now();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_else(|_| "".into());
            return Err(AuthError::TokenEndpoint { status, body });
        }

        let payload: TokenResponse = response.json().await?;
        let session = payload.into_session(received_at)?;
        Ok(TokenExchangeResult {
            session,
            received_at,
        })
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    expires_in: Option<i64>,
    scope: Option<String>,
}

impl TokenResponse {
    fn into_session(self, received_at: DateTime<Utc>) -> Result<AuthSession, AuthError> {
        let token_type = match self.token_type.to_ascii_lowercase().as_str() {
            "bearer" => TokenType::Bearer,
            other => return Err(AuthError::InvalidTokenType(other.to_owned())),
        };

        let expires_at = self
            .expires_in
            .map(|seconds| received_at + Duration::seconds(seconds.into()));

        let scope = self
            .scope
            .unwrap_or_default()
            .split_whitespace()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();

        Ok(AuthSession {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            token_type,
            expires_at,
            scope,
            created_at: received_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use reqwest::StatusCode;
    use tokio::runtime::Runtime;

    fn runtime() -> Runtime {
        Runtime::new().unwrap()
    }

    #[test]
    fn exchange_code_success() {
        let rt = runtime();
        rt.block_on(async {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST)
                    .path("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded");
                then.status(200).json_body_obj(&serde_json::json!({
                    "access_token": "abc123",
                    "refresh_token": "refresh456",
                    "token_type": "bearer",
                    "expires_in": 3600,
                    "scope": "read write"
                }));
            });

            let config = OAuthConfig::new(
                "client-id",
                Url::parse("http://localhost/callback").unwrap(),
            );
            let endpoints = OAuthEndpoints {
                authorization_url: Url::parse("http://localhost/authorize").unwrap(),
                token_url: Url::parse(&format!("{}{}", server.base_url(), "/oauth/token")).unwrap(),
            };
            let client = OAuthClient::with_endpoints(config, endpoints).unwrap();
            let pkce = PkcePair::generate();
            let result = client.exchange_code("code123", &pkce).await.unwrap();
            mock.assert();
            assert_eq!(result.session.access_token, "abc123");
            assert_eq!(result.session.refresh_token.as_deref(), Some("refresh456"));
            assert_eq!(result.session.scope, vec!["read", "write"]);
            assert_eq!(result.session.token_type, TokenType::Bearer);
            assert!(result.session.expires_at.is_some());
        });
    }

    #[test]
    fn refresh_token_missing() {
        let session = AuthSession::new_api_key("key".into());
        let config = OAuthConfig::new(
            "client-id",
            Url::parse("http://localhost/callback").unwrap(),
        );
        let client = OAuthClient::new(config).unwrap();
        let rt = runtime();
        let result = rt.block_on(async { client.refresh_session(&session).await });
        assert!(matches!(result.unwrap_err(), AuthError::RefreshUnavailable));
    }

    #[test]
    fn refresh_uses_existing_refresh_token_when_not_returned() {
        let rt = runtime();
        rt.block_on(async {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/oauth/token");
                then.status(200).json_body_obj(&serde_json::json!({
                    "access_token": "new-access",
                    "token_type": "bearer",
                    "expires_in": 7200,
                    "scope": "read"
                }));
            });

            let config = OAuthConfig::new(
                "client-id",
                Url::parse("http://localhost/callback").unwrap(),
            );
            let endpoints = OAuthEndpoints {
                authorization_url: Url::parse("http://localhost/authorize").unwrap(),
                token_url: Url::parse(&format!("{}{}", server.base_url(), "/oauth/token")).unwrap(),
            };
            let client = OAuthClient::with_endpoints(config, endpoints).unwrap();
            let mut session = AuthSession::new_api_key("temp".into());
            session.access_token = "old-access".into();
            session.refresh_token = Some("refresh456".into());
            let result = client.refresh_session(&session).await.unwrap();
            mock.assert();
            assert_eq!(result.session.access_token, "new-access");
            assert_eq!(result.session.refresh_token.as_deref(), Some("refresh456"));
        });
    }

    #[test]
    fn token_endpoint_failure() {
        let rt = runtime();
        rt.block_on(async {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/oauth/token");
                then.status(400).body("invalid_grant");
            });

            let config = OAuthConfig::new(
                "client-id",
                Url::parse("http://localhost/callback").unwrap(),
            );
            let endpoints = OAuthEndpoints {
                authorization_url: Url::parse("http://localhost/authorize").unwrap(),
                token_url: Url::parse(&format!("{}{}", server.base_url(), "/oauth/token")).unwrap(),
            };

            let client = OAuthClient::with_endpoints(config, endpoints).unwrap();
            let pkce = PkcePair::generate();
            let err = client.exchange_code("bad", &pkce).await.unwrap_err();
            mock.assert();
            match err {
                AuthError::TokenEndpoint { status, body } => {
                    assert_eq!(status, StatusCode::BAD_REQUEST);
                    assert_eq!(body, "invalid_grant");
                }
                other => panic!("unexpected error: {other:?}"),
            }
        });
    }

    #[test]
    fn client_credentials_success() {
        let rt = runtime();
        rt.block_on(async {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST)
                    .path("/oauth/token")
                    .body_contains("grant_type=client_credentials");
                then.status(200).json_body_obj(&serde_json::json!({
                    "access_token": "machine-token",
                    "token_type": "bearer",
                    "expires_in": 3600,
                    "scope": "read write"
                }));
            });

            let config = OAuthConfig::new(
                "client-id",
                Url::parse("http://localhost/callback").unwrap(),
            )
            .with_secret("client-secret");
            let endpoints = OAuthEndpoints {
                authorization_url: Url::parse("http://localhost/authorize").unwrap(),
                token_url: Url::parse(&format!("{}{}", server.base_url(), "/oauth/token")).unwrap(),
            };
            let client = OAuthClient::with_endpoints(config, endpoints).unwrap();
            let result = client
                .client_credentials(&["read".into(), "write".into()])
                .await
                .unwrap();
            mock.assert();
            assert_eq!(result.session.access_token, "machine-token");
            assert_eq!(result.session.scope, vec!["read", "write"]);
        });
    }
}
