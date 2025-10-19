use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    #[error("GraphQL operation failed: {0}")]
    OperationFailed(String),
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

    /// Fetch all teams accessible to the session.
    pub async fn teams(&self) -> GraphqlResult<Vec<TeamSummary>> {
        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
        }

        #[derive(Deserialize)]
        struct TeamsEnvelope {
            teams: TeamConnection,
        }

        #[derive(Deserialize)]
        struct TeamConnection {
            nodes: Vec<TeamSummary>,
        }

        const QUERY: &str = r#"
            query TeamsQuery {
                teams {
                    nodes {
                        id
                        name
                        key
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<TeamsEnvelope> = self.post(Request { query: QUERY }).await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let data = response.data.ok_or(GraphqlError::NotFound)?;
        Ok(data.teams.nodes)
    }

    /// Fetch workflow states for a team.
    pub async fn workflow_states(&self, team_id: &str) -> GraphqlResult<Vec<WorkflowStateSummary>> {
        #[derive(Serialize)]
        struct Variables<'a> {
            team_id: &'a str,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        #[derive(Deserialize)]
        struct WorkflowEnvelope {
            team: Option<TeamWorkflowStates>,
        }

        #[derive(Deserialize)]
        struct TeamWorkflowStates {
            states: WorkflowStateConnection,
        }

        #[derive(Deserialize)]
        struct WorkflowStateConnection {
            nodes: Vec<WorkflowStateSummary>,
        }

        const QUERY: &str = r#"
            query WorkflowStates($team_id: String!) {
                team(id: $team_id) {
                    states {
                        nodes {
                            id
                            name
                            type
                        }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<WorkflowEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables { team_id },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let team = response
            .data
            .and_then(|payload| payload.team)
            .ok_or(GraphqlError::NotFound)?;

        Ok(team.states.nodes)
    }

    /// Fetch a list of recent issues.
    pub async fn list_issues(&self, params: IssueListParams) -> GraphqlResult<IssueListResponse> {
        #[derive(Serialize)]
        struct Variables {
            first: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            filter: Option<Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            after: Option<String>,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct IssuesEnvelope {
            issues: IssueEdgeConnection<IssueSummary>,
        }

        const QUERY: &str = r#"
            query ListIssues($first: Int!, $filter: IssueFilter, $after: String) {
                issues(first: $first, filter: $filter, orderBy: updatedAt, after: $after) {
                    edges {
                        cursor
                        node {
                        id
                        identifier
                        title
                        url
                        priority
                        createdAt
                        updatedAt
                        state { id name type }
                        assignee { id name displayName }
                    }
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssuesEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables {
                    first: params.first as i64,
                    filter: params.filter,
                    after: params.after,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let data = response.data.ok_or(GraphqlError::NotFound)?.issues;
        let nodes = data.edges.into_iter().map(|edge| edge.node).collect();
        Ok(IssueListResponse {
            nodes,
            end_cursor: data.page_info.end_cursor,
            has_next_page: data.page_info.has_next_page,
        })
    }

    /// Fetch a single issue by its identifier (e.g. "ENG-123").
    pub async fn issue_by_key(&self, key: &str) -> GraphqlResult<IssueDetail> {
        #[derive(Serialize)]
        struct Variables<'a> {
            id: &'a str,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        const QUERY: &str = r#"
            query IssueByKey($id: String!) {
                issue(id: $id) {
                    id
                    identifier
                    title
                    description
                    url
                    priority
                    createdAt
                    updatedAt
                    state { id name type }
                    assignee { id name displayName }
                    labels(first: 20) {
                        nodes { id name color }
                    }
                    team { id name key }
                }
            }
        "#;

        #[derive(Deserialize)]
        struct IssueEnvelope {
            issue: Option<IssueDetail>,
        }

        let response: GraphqlEnvelope<IssueEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables { id: key },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        response
            .data
            .and_then(|payload| payload.issue)
            .ok_or(GraphqlError::NotFound)
    }

    /// Create a new issue using the Linear GraphQL API.
    pub async fn create_issue(&self, input: IssueCreateInput) -> GraphqlResult<IssueDetail> {
        #[derive(Serialize)]
        struct Variables {
            input: IssueCreateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct IssueCreateEnvelope {
            #[serde(rename = "issueCreate")]
            issue_create: IssueCreatePayload,
        }

        #[derive(Deserialize)]
        struct IssueCreatePayload {
            success: bool,
            issue: Option<IssueDetail>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation CreateIssue($input: IssueCreateInput!) {
                issueCreate(input: $input) {
                    success
                    userErrors {
                        message
                    }
                    issue {
                        id
                        identifier
                        title
                        description
                        url
                        priority
                        createdAt
                        updatedAt
                        state { id name type }
                        assignee { id name displayName }
                        labels(first: 20) {
                            nodes { id name color }
                        }
                        team { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssueCreateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { input },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.issue_create;

        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            let message = if message.is_empty() {
                "unknown error".to_string()
            } else {
                message
            };
            return Err(GraphqlError::OperationFailed(message));
        }

        let issue = payload.issue.ok_or(GraphqlError::NotFound)?;
        Ok(issue)
    }

    /// Update an issue by id.
    pub async fn update_issue(
        &self,
        id: &str,
        input: IssueUpdateInput,
    ) -> GraphqlResult<IssueDetail> {
        #[derive(Serialize)]
        struct Variables {
            id: String,
            input: IssueUpdateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct IssueUpdateEnvelope {
            #[serde(rename = "issueUpdate")]
            issue_update: IssueUpdatePayload,
        }

        #[derive(Deserialize)]
        struct IssueUpdatePayload {
            success: bool,
            issue: Option<IssueDetail>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation IssueUpdate($id: String!, $input: IssueUpdateInput!) {
                issueUpdate(id: $id, input: $input) {
                    success
                    userErrors { message }
                    issue {
                        id
                        identifier
                        title
                        description
                        url
                        priority
                        createdAt
                        updatedAt
                        state { id name type }
                        assignee { id name displayName }
                        labels(first: 20) { nodes { id name color } }
                        team { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssueUpdateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables {
                    id: id.to_owned(),
                    input,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.issue_update;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "issue update failed".into()
            } else {
                message
            }));
        }

        payload.issue.ok_or(GraphqlError::NotFound)
    }

    /// Archive or restore an issue.
    pub async fn archive_issue(&self, id: &str, archive: bool) -> GraphqlResult<IssueDetail> {
        #[derive(Serialize)]
        struct Variables<'a> {
            id: &'a str,
            archive: bool,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        #[derive(Deserialize)]
        struct IssueArchiveEnvelope {
            #[serde(rename = "issueArchive")]
            issue_archive: IssueUpdatePayload,
        }

        #[derive(Deserialize)]
        struct IssueUpdatePayload {
            success: bool,
            issue: Option<IssueDetail>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation IssueArchive($id: String!, $archive: Boolean!) {
                issueArchive(id: $id, archive: $archive) {
                    success
                    userErrors { message }
                    issue {
                        id
                        identifier
                        title
                        description
                        url
                        priority
                        createdAt
                        updatedAt
                        state { id name type }
                        assignee { id name displayName }
                        labels(first: 20) { nodes { id name color } }
                        team { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssueArchiveEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { id, archive },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.issue_archive;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "issue archive failed".into()
            } else {
                message
            }));
        }

        payload.issue.ok_or(GraphqlError::NotFound)
    }

    /// Delete an issue by id.
    pub async fn delete_issue(&self, id: &str) -> GraphqlResult<bool> {
        #[derive(Serialize)]
        struct Variables<'a> {
            id: &'a str,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        #[derive(Deserialize)]
        struct IssueDeleteEnvelope {
            #[serde(rename = "issueDelete")]
            issue_delete: IssueDeletePayload,
        }

        #[derive(Deserialize)]
        struct IssueDeletePayload {
            success: bool,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation IssueDelete($id: String!) {
                issueDelete(id: $id) {
                    success
                    userErrors { message }
                }
            }
        "#;

        let response: GraphqlEnvelope<IssueDeleteEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { id },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.issue_delete;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "issue delete failed".into()
            } else {
                message
            }));
        }

        Ok(true)
    }

    /// Create a new comment on an issue.
    pub async fn create_comment(&self, input: CommentCreateInput) -> GraphqlResult<Comment> {
        #[derive(Serialize)]
        struct Variables {
            input: CommentCreateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct CommentCreateEnvelope {
            #[serde(rename = "commentCreate")]
            comment_create: CommentCreatePayload,
        }

        #[derive(Deserialize)]
        struct CommentCreatePayload {
            success: bool,
            comment: Option<Comment>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation CommentCreate($input: CommentCreateInput!) {
                commentCreate(input: $input) {
                    success
                    userErrors { message }
                    comment {
                        id
                        body
                        createdAt
                        updatedAt
                        user { id name displayName }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<CommentCreateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { input },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.comment_create;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "comment create failed".into()
            } else {
                message
            }));
        }

        payload.comment.ok_or(GraphqlError::NotFound)
    }

    /// List projects with optional filters.
    pub async fn projects(&self, params: ProjectListParams) -> GraphqlResult<ProjectListResponse> {
        #[derive(Serialize)]
        struct Variables {
            first: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            filter: Option<Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            order_by: Option<Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            after: Option<String>,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct ProjectsEnvelope {
            projects: IssueEdgeConnection<ProjectSummary>,
        }

        const QUERY: &str = r#"
            query ListProjects($first: Int!, $filter: ProjectFilter, $orderBy: ProjectOrderByInput, $after: String) {
                projects(first: $first, filter: $filter, orderBy: $orderBy, after: $after) {
                    edges {
                        cursor
                        node {
                            id
                            name
                            state
                            description
                            startDate
                            targetDate
                            status
                            updatedAt
                            createdAt
                            lead { id name displayName }
                        }
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<ProjectsEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables {
                    first: params.first as i64,
                    filter: params.filter,
                    order_by: params.order_by,
                    after: params.after,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let connection = response.data.ok_or(GraphqlError::NotFound)?.projects;
        let nodes = connection.edges.into_iter().map(|edge| edge.node).collect();
        Ok(ProjectListResponse {
            nodes,
            end_cursor: connection.page_info.end_cursor,
            has_next_page: connection.page_info.has_next_page,
        })
    }

    /// Create a project.
    pub async fn project_create(&self, input: ProjectCreateInput) -> GraphqlResult<ProjectDetail> {
        #[derive(Serialize)]
        struct Variables {
            input: ProjectCreateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct ProjectCreateEnvelope {
            #[serde(rename = "projectCreate")]
            project_create: ProjectPayload,
        }

        #[derive(Deserialize)]
        struct ProjectPayload {
            success: bool,
            project: Option<ProjectDetail>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation ProjectCreate($input: ProjectCreateInput!) {
                projectCreate(input: $input) {
                    success
                    userErrors { message }
                    project {
                        id
                        name
                        description
                        state
                        startDate
                        targetDate
                        status
                        updatedAt
                        createdAt
                        lead { id name displayName }
                        teams { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<ProjectCreateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { input },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.project_create;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "project create failed".into()
            } else {
                message
            }));
        }

        payload.project.ok_or(GraphqlError::NotFound)
    }

    /// Update a project by id.
    pub async fn project_update(
        &self,
        id: &str,
        input: ProjectUpdateInput,
    ) -> GraphqlResult<ProjectDetail> {
        #[derive(Serialize)]
        struct Variables {
            id: String,
            input: ProjectUpdateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct ProjectUpdateEnvelope {
            #[serde(rename = "projectUpdate")]
            project_update: ProjectPayload,
        }

        #[derive(Deserialize)]
        struct ProjectPayload {
            success: bool,
            project: Option<ProjectDetail>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation ProjectUpdate($id: String!, $input: ProjectUpdateInput!) {
                projectUpdate(id: $id, input: $input) {
                    success
                    userErrors { message }
                    project {
                        id
                        name
                        description
                        state
                        startDate
                        targetDate
                        status
                        updatedAt
                        createdAt
                        lead { id name displayName }
                        teams { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<ProjectUpdateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables {
                    id: id.to_owned(),
                    input,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.project_update;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "project update failed".into()
            } else {
                message
            }));
        }

        payload.project.ok_or(GraphqlError::NotFound)
    }

    /// Archive or restore a project.
    pub async fn project_archive(&self, id: &str, archive: bool) -> GraphqlResult<ProjectDetail> {
        #[derive(Serialize)]
        struct Variables<'a> {
            id: &'a str,
            archive: bool,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        #[derive(Deserialize)]
        struct ProjectArchiveEnvelope {
            #[serde(rename = "projectArchive")]
            project_archive: ProjectPayload,
        }

        #[derive(Deserialize)]
        struct ProjectPayload {
            success: bool,
            project: Option<ProjectDetail>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation ProjectArchive($id: String!, $archive: Boolean!) {
                projectArchive(id: $id, archive: $archive) {
                    success
                    userErrors { message }
                    project {
                        id
                        name
                        description
                        state
                        startDate
                        targetDate
                        status
                        updatedAt
                        createdAt
                        lead { id name displayName }
                        teams { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<ProjectArchiveEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { id, archive },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.project_archive;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "project archive failed".into()
            } else {
                message
            }));
        }

        payload.project.ok_or(GraphqlError::NotFound)
    }

    /// List cycles for teams or organization.
    pub async fn cycles(&self, params: CycleListParams) -> GraphqlResult<CycleListResponse> {
        #[derive(Serialize)]
        struct Variables {
            first: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            filter: Option<Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            order_by: Option<Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            after: Option<String>,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct CyclesEnvelope {
            cycles: IssueEdgeConnection<CycleSummary>,
        }

        const QUERY: &str = r#"
            query ListCycles($first: Int!, $filter: CycleFilter, $orderBy: CycleOrderByInput, $after: String) {
                cycles(first: $first, filter: $filter, orderBy: $orderBy, after: $after) {
                    edges {
                        cursor
                        node {
                            id
                            name
                            number
                            startsAt
                            endsAt
                            state
                            team { id name key }
                        }
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<CyclesEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables {
                    first: params.first as i64,
                    filter: params.filter,
                    order_by: params.order_by,
                    after: params.after,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let connection = response.data.ok_or(GraphqlError::NotFound)?.cycles;
        let nodes = connection.edges.into_iter().map(|edge| edge.node).collect();
        Ok(CycleListResponse {
            nodes,
            end_cursor: connection.page_info.end_cursor,
            has_next_page: connection.page_info.has_next_page,
        })
    }

    /// Update a cycle.
    pub async fn cycle_update(
        &self,
        id: &str,
        input: CycleUpdateInput,
    ) -> GraphqlResult<CycleSummary> {
        #[derive(Serialize)]
        struct Variables {
            id: String,
            input: CycleUpdateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct CycleUpdateEnvelope {
            #[serde(rename = "cycleUpdate")]
            cycle_update: CycleUpdatePayload,
        }

        #[derive(Deserialize)]
        struct CycleUpdatePayload {
            success: bool,
            cycle: Option<CycleSummary>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation CycleUpdate($id: String!, $input: CycleUpdateInput!) {
                cycleUpdate(id: $id, input: $input) {
                    success
                    userErrors { message }
                    cycle {
                        id
                        name
                        number
                        startsAt
                        endsAt
                        state
                        team { id name key }
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<CycleUpdateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables {
                    id: id.to_owned(),
                    input,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.cycle_update;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "cycle update failed".into()
            } else {
                message
            }));
        }

        payload.cycle.ok_or(GraphqlError::NotFound)
    }

    /// List issue labels for a team.
    pub async fn issue_labels(&self, team_id: &str) -> GraphqlResult<Vec<IssueLabel>> {
        #[derive(Serialize)]
        struct Variables<'a> {
            team_id: &'a str,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables<'a>,
        }

        #[derive(Deserialize)]
        struct LabelsEnvelope {
            issue_labels: IssueLabelConnection,
        }

        const QUERY: &str = r#"
            query IssueLabels($teamId: String!) {
                issueLabels(filter: { team: { id: { eq: $teamId } } }) {
                    nodes {
                        id
                        name
                        color
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<LabelsEnvelope> = self
            .post(Request {
                query: QUERY,
                variables: Variables { team_id },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let connection = response.data.ok_or(GraphqlError::NotFound)?.issue_labels;
        Ok(connection.nodes)
    }

    /// Create a new issue label.
    pub async fn create_issue_label(
        &self,
        input: IssueLabelCreateInput,
    ) -> GraphqlResult<IssueLabel> {
        #[derive(Serialize)]
        struct Variables {
            input: IssueLabelCreateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct LabelCreateEnvelope {
            #[serde(rename = "issueLabelCreate")]
            label_create: LabelPayload,
        }

        #[derive(Deserialize)]
        struct LabelPayload {
            success: bool,
            issue_label: Option<IssueLabel>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation IssueLabelCreate($input: IssueLabelCreateInput!) {
                issueLabelCreate(input: $input) {
                    success
                    userErrors { message }
                    issueLabel {
                        id
                        name
                        color
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<LabelCreateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables { input },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.label_create;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "issue label create failed".into()
            } else {
                message
            }));
        }

        payload.issue_label.ok_or(GraphqlError::NotFound)
    }

    /// Update an existing issue label.
    pub async fn update_issue_label(
        &self,
        id: &str,
        input: IssueLabelUpdateInput,
    ) -> GraphqlResult<IssueLabel> {
        #[derive(Serialize)]
        struct Variables {
            id: String,
            input: IssueLabelUpdateInput,
        }

        #[derive(Serialize)]
        struct Request<'a> {
            query: &'a str,
            variables: Variables,
        }

        #[derive(Deserialize)]
        struct LabelUpdateEnvelope {
            #[serde(rename = "issueLabelUpdate")]
            label_update: LabelPayload,
        }

        #[derive(Deserialize)]
        struct LabelPayload {
            success: bool,
            issue_label: Option<IssueLabel>,
            #[serde(rename = "userErrors", default)]
            user_errors: Vec<ApiUserError>,
        }

        #[derive(Deserialize)]
        struct ApiUserError {
            message: Option<String>,
        }

        const MUTATION: &str = r#"
            mutation IssueLabelUpdate($id: String!, $input: IssueLabelUpdateInput!) {
                issueLabelUpdate(id: $id, input: $input) {
                    success
                    userErrors { message }
                    issueLabel {
                        id
                        name
                        color
                    }
                }
            }
        "#;

        let response: GraphqlEnvelope<LabelUpdateEnvelope> = self
            .post(Request {
                query: MUTATION,
                variables: Variables {
                    id: id.to_owned(),
                    input,
                },
            })
            .await?;

        if let Some(errors) = response.errors {
            return Err(GraphqlError::ResponseErrors(errors));
        }

        let payload = response.data.ok_or(GraphqlError::NotFound)?.label_update;
        if !payload.success {
            let message = payload
                .user_errors
                .into_iter()
                .filter_map(|err| err.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GraphqlError::OperationFailed(if message.is_empty() {
                "issue label update failed".into()
            } else {
                message
            }));
        }

        payload.issue_label.ok_or(GraphqlError::NotFound)
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
#[serde(rename_all = "camelCase")]
pub struct Viewer {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
    pub team: Option<TeamSummary>,
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
#[serde(rename_all = "camelCase")]
pub struct IssueAssignee {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLabel {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSummary {
    pub id: String,
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStateSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IssueListParams {
    pub first: usize,
    pub filter: Option<Value>,
    pub after: Option<String>,
}

/// Input used when creating a new issue.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueCreateInput {
    pub team_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub label_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

impl IssueCreateInput {
    pub fn new(team_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            team_id: team_id.into(),
            title: title.into(),
            description: None,
            assignee_id: None,
            state_id: None,
            label_ids: Vec::new(),
            priority: None,
        }
    }
}

/// Input used when updating an existing issue.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueUpdateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// Input used when creating a new comment.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentCreateInput {
    pub issue_id: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct IssueListResponse {
    pub nodes: Vec<IssueSummary>,
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectListParams {
    pub first: usize,
    pub filter: Option<Value>,
    pub order_by: Option<Value>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectListResponse {
    pub nodes: Vec<ProjectSummary>,
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub team_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_id: Option<String>,
}

impl ProjectCreateInput {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            state: None,
            start_date: None,
            target_date: None,
            team_ids: Vec::new(),
            lead_id: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpdateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub team_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CycleListParams {
    pub first: usize,
    pub filter: Option<Value>,
    pub order_by: Option<Value>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CycleListResponse {
    pub nodes: Vec<CycleSummary>,
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleUpdateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLabelCreateInput {
    pub team_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLabelUpdateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub user: Option<UserSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub state: Option<String>,
    pub description: Option<String>,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub status: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub lead: Option<UserSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub state: Option<String>,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub status: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub lead: Option<UserSummary>,
    pub teams: Vec<TeamSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleSummary {
    pub id: String,
    pub name: Option<String>,
    pub number: i64,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub state: Option<String>,
    pub team: Option<TeamSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphqlResponseError {
    pub message: String,
    #[serde(default)]
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct IssueEdgeConnection<T> {
    edges: Vec<IssueEdge<T>>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
struct IssueEdge<T> {
    #[serde(rename = "cursor")]
    _cursor: String,
    node: T,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
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
    }

    #[tokio::test]
    async fn list_issues_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body_obj(&serde_json::json!({
                "data": {
                    "issues": {
                        "edges": [
                            {
                                "cursor": "cursor-1",
                                "node": {
                                    "id": "issue-1",
                                    "identifier": "ENG-1",
                                    "title": "Fix login bug",
                                    "url": "https://linear.app/eng-1",
                                    "priority": 1,
                                    "createdAt": "2024-07-01T12:00:00.000Z",
                                    "updatedAt": "2024-07-02T12:00:00.000Z",
                                    "state": { "id": "state-1", "name": "Todo", "type": "backlog" },
                                    "assignee": { "id": "user-1", "name": "Ada", "displayName": "Ada" }
                                }
                            }
                        ],
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": "cursor-1"
                        }
                    }
                }
            }));
        });

        let client = LinearGraphqlClient::with_endpoint(
            &sample_session(),
            &format!("{}{}", server.base_url(), "/graphql"),
        )
        .unwrap();

        let issues = client
            .list_issues(IssueListParams {
                first: 5,
                filter: None,
                after: None,
            })
            .await
            .unwrap();
        mock.assert();
        assert_eq!(issues.nodes.len(), 1);
        assert_eq!(issues.nodes[0].identifier, "ENG-1");
        assert_eq!(issues.end_cursor.as_deref(), Some("cursor-1"));
        assert!(!issues.has_next_page);
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

    #[tokio::test]
    async fn create_issue_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body_obj(&serde_json::json!({
                "data": {
                    "issueCreate": {
                        "success": true,
                        "userErrors": [],
                        "issue": {
                            "id": "issue-42",
                            "identifier": "ENG-42",
                            "title": "Implement issue create",
                            "description": "Body",
                            "url": "https://linear.app/issue/42",
                            "priority": 1,
                            "createdAt": "2024-07-03T12:00:00.000Z",
                            "updatedAt": "2024-07-03T12:00:00.000Z",
                            "state": { "id": "state-1", "name": "Todo", "type": "backlog" },
                            "assignee": { "id": "user-1", "name": "Ada", "displayName": "Ada" },
                            "labels": {
                                "nodes": [
                                    { "id": "label-1", "name": "bug", "color": "#ff0000" }
                                ]
                            }
                        }
                    }
                }
            }));
        });

        let client = LinearGraphqlClient::with_endpoint(
            &sample_session(),
            &format!("{}{}", server.base_url(), "/graphql"),
        )
        .unwrap();

        let mut input = IssueCreateInput::new("team-1", "Implement issue create");
        input.description = Some("Body".into());
        let issue = client.create_issue(input).await.unwrap();
        mock.assert();
        assert_eq!(issue.identifier, "ENG-42");
        assert_eq!(issue.description.as_deref(), Some("Body"));
    }

    #[tokio::test]
    async fn create_issue_failure_returns_operation_failed() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body_obj(&serde_json::json!({
                "data": {
                    "issueCreate": {
                        "success": false,
                        "userErrors": [
                            { "message": "Team not found" }
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

        let err = client
            .create_issue(IssueCreateInput::new("team-1", "New issue"))
            .await
            .unwrap_err();
        match err {
            GraphqlError::OperationFailed(message) => {
                assert!(message.contains("Team not found"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
