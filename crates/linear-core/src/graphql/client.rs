use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::auth::{AuthSession, TokenType};

const DEFAULT_ENDPOINT: &str = "https://api.linear.app/graphql";
const USER_AGENT: &str = "linear-rs/0.1.0";

/// Errors returned by the GraphQL client.
#[derive(Debug, Error)]
pub enum GraphqlError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("HTTP status {status} body: {body}")]
    HttpStatus { status: StatusCode, body: String },
    #[error("invalid GraphQL endpoint: {0}")]
    InvalidEndpoint(#[from] url::ParseError),
    #[error("GraphQL returned errors: {0:?}")]
    ResponseErrors(Vec<GraphqlResponseError>),
    #[error("failed to deserialize response: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("missing viewer payload in response")]
    MissingViewer,
}

pub type GraphqlResult<T> = Result<T, GraphqlError>;

/// Minimal GraphQL client for interacting with Linear.
#[derive(Debug, Clone)]
pub struct LinearGraphqlClient {
    http: Client,
    endpoint: Url,
    auth_header: String,
}

impl LinearGraphqlClient {
    /// Build a client targeting the default Linear GraphQL endpoint for the given session.
    pub fn from_session(session: &AuthSession) -> GraphqlResult<Self> {
        Self::with_endpoint(session, DEFAULT_ENDPOINT)
    }

    /// Build a client with a custom GraphQL endpoint (useful for testing).
    pub fn with_endpoint(session: &AuthSession, endpoint: &str) -> GraphqlResult<Self> {
        let endpoint = Url::parse(endpoint)?;
        let auth_header = match session.token_type {
            TokenType::Bearer => format!("Bearer {}", session.access_token),
            TokenType::ApiKey => session.access_token.clone(),
        };
        let http = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self {
            http,
            endpoint,
            auth_header,
        })
    }

    /// Fetch the current user (`viewer`) object.
    pub async fn viewer(&self) -> GraphqlResult<Viewer> {
        #[derive(Serialize)]
        struct RequestBody<'a> {
            query: &'a str,
            variables: (),
        }

        const QUERY: &str = r#"
            query ViewerQuery {
                viewer {
                    id
                    name
                    email
                    displayName
                    handle
                    createdAt
                }
            }
        "#;

        let request = RequestBody {
            query: QUERY,
            variables: (),
        };

        let response: GraphqlEnvelope<ViewerEnvelope> = self.post(request).await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let data = response.data.ok_or(GraphqlError::MissingViewer)?;
        Ok(data.viewer)
    }

    async fn post<T, R>(&self, body: T) -> GraphqlResult<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let response = self
            .http
            .post(self.endpoint.clone())
            .header("Authorization", &self.auth_header)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(GraphqlError::HttpStatus { status, body: text });
        }

        let payload = response.json::<R>().await?;
        Ok(payload)
    }
}

#[derive(Debug, Deserialize)]
struct GraphqlEnvelope<T> {
    data: Option<T>,
    errors: Option<Vec<GraphqlResponseError>>,
}

#[derive(Debug, Deserialize)]
struct ViewerEnvelope {
    viewer: Viewer,
}

/// Subset of viewer fields useful for identity-aware commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewer {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub handle: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphqlResponseError {
    pub message: String,
    #[serde(default)]
    pub path: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthSession;
    use httpmock::prelude::*;

    fn sample_session() -> AuthSession {
        AuthSession::new_api_key("test-key".into())
    }

    #[tokio::test]
    async fn viewer_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body_obj(&serde_json::json!({
                "data": {
                    "viewer": {
                        "id": "user-1",
                        "name": "Ada Lovelace",
                        "displayName": "Ada",
                        "email": "ada@example.com",
                        "handle": "ada",
                        "createdAt": "2024-01-01T00:00:00.000Z"
                    }
                }
            }));
        });

        let client = LinearGraphqlClient::with_endpoint(
            &sample_session(),
            &format!("{}{}", server.base_url(), "/graphql"),
        )
        .unwrap();

        let viewer = client.viewer().await.unwrap();
        mock.assert();
        assert_eq!(viewer.id, "user-1");
        assert_eq!(viewer.handle.as_deref(), Some("ada"));
    }
}
