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
    #[error("requested resource not found")]
    NotFound,
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

    /// Fetch a list of recent issues.
    pub async fn list_issues(&self, first: usize) -> GraphqlResult<Vec<IssueSummary>> {
        #[derive(Serialize)]
        struct Variables {
            first: i64,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct IssuesEnvelope {
            issues: IssueConnection<IssueSummary>,
        }

        const QUERY: &str = r#"
            query ListIssues($first: Int!) {
                issues(first: $first, orderBy: updatedAt, sortOrder: Desc) {
                    nodes {
                        id
                        identifier
                        title
                        url
                        priority
                        createdAt
                        updatedAt
                        state { id name type }
                        assignee { id name displayName handle }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssuesEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables {
                    first: first as i64,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let data = response
            .data
            .ok_or(GraphqlError::NotFound)?
            .issues
            .nodes;
        Ok(data)
    }

    /// Fetch a single issue by its identifier (e.g. "ENG-123").
    pub async fn issue_by_key(&self, key: &str) -> GraphqlResult<IssueDetail> {
        #[derive(Serialize)]
        struct Variables<'a> {
            key: &'a str,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        #[derive(Deserialize)]
        struct IssueEnvelope {
            issues: IssueConnection<IssueDetail>,
        }

        const QUERY: &str = r#"
            query IssueByKey($key: String!) {
                issues(first: 1, filter: { identifier: { eq: $key } }) {
                    nodes {
                        id
                        identifier
                        title
                        description
                        url
                        priority
                        createdAt
                        updatedAt
                        state { id name type }
                        assignee { id name displayName handle }
                        labels(first: 20) {
                            nodes { id name color }
                        }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssueEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables { key },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let data = response
            .data
            .and_then(|payload| payload.issues.nodes.into_iter().next())
            .ok_or(GraphqlError::NotFound)?;

        Ok(data)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueSummary {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub url: Option<String>,
    pub state: Option<IssueState>,
    pub assignee: Option<IssueAssignee>,
    pub priority: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub url: Option<String>,
    pub state: Option<IssueState>,
    pub assignee: Option<IssueAssignee>,
    pub priority: Option<i32>,
    pub labels: Option<IssueLabelConnection>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueLabelConnection {
    pub nodes: Vec<IssueLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueState {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueAssignee {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueLabel {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphqlResponseError {
    pub message: String,
    #[serde(default)]
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct IssueConnection<T> {
    nodes: Vec<T>,
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

    #[tokio::test]
    async fn list_issues_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body_obj(&serde_json::json!({
                "data": {
                    "issues": {
                        "nodes": [
                            {
                                "id": "issue-1",
                                "identifier": "ENG-1",
                                "title": "Fix login bug",
                                "url": "https://linear.app/eng-1",
                                "priority": 1,
                                "createdAt": "2024-07-01T12:00:00.000Z",
                                "updatedAt": "2024-07-02T12:00:00.000Z",
                                "state": { "id": "state-1", "name": "Todo", "type": "backlog" },
                                "assignee": { "id": "user-1", "name": "Ada", "displayName": "Ada", "handle": "ada" }
                            }
                        ]
                    }
                }
            }));
        });

        let client = LinearGraphqlClient::with_endpoint(
            &sample_session(),
            &format!("{}{}", server.base_url(), "/graphql"),
        )
        .unwrap();

        let issues = client.list_issues(5).await.unwrap();
        mock.assert();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].identifier, "ENG-1");
    }

    #[tokio::test]
    async fn issue_by_key_not_found() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body_obj(&serde_json::json!({
                "data": {
                    "issues": { "nodes": [] }
                }
            }));
        });

        let client = LinearGraphqlClient::with_endpoint(
            &sample_session(),
            &format!("{}{}", server.base_url(), "/graphql"),
        )
        .unwrap();

        let err = client.issue_by_key("ENG-404").await.unwrap_err();
        assert!(matches!(err, GraphqlError::NotFound));
    }
}
