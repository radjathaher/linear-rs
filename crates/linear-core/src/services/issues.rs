use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::RwLock;

use crate::graphql::{
    Comment, CommentCreateInput, GraphqlResult, IssueCreateInput, IssueDetail, IssueListParams,
    IssueListResponse, IssueSummary, IssueUpdateInput, LinearGraphqlClient, TeamSummary,
    WorkflowStateSummary,
};

/// Provides higher-level helpers around Linear issues.
#[derive(Clone)]
pub struct IssueService {
    client: LinearGraphqlClient,
    cache: Cache,
}

impl IssueService {
    pub fn new(client: LinearGraphqlClient) -> Self {
        Self {
            client,
            cache: Cache::default(),
        }
    }

    pub async fn list(&self, options: IssueQueryOptions) -> GraphqlResult<IssueListResult> {
        let params = options.into_params();
        let response: IssueListResponse = self.client.list_issues(params).await?;
        Ok(IssueListResult {
            issues: response.nodes,
            end_cursor: response.end_cursor,
            has_next_page: response.has_next_page,
        })
    }

    pub async fn get_by_key(&self, key: &str) -> GraphqlResult<IssueDetail> {
        self.client.issue_by_key(key).await
    }

    pub async fn teams(&self) -> GraphqlResult<Vec<TeamSummary>> {
        if let Some(teams) = self.cache.read_teams().await {
            Ok(teams)
        } else {
            let teams = self.client.teams().await?;
            self.cache.write_teams(teams.clone()).await;
            Ok(teams)
        }
    }

    pub async fn workflow_states(&self, team_id: &str) -> GraphqlResult<Vec<WorkflowStateSummary>> {
        if let Some(states) = self.cache.read_states(team_id).await {
            Ok(states)
        } else {
            let states = self.client.workflow_states(team_id).await?;
            self.cache.write_states(team_id, states.clone()).await;
            Ok(states)
        }
    }

    pub async fn resolve_team_id(&self, identifier: &str) -> GraphqlResult<Option<String>> {
        let teams = self.teams().await?;
        Ok(teams
            .into_iter()
            .find(|team| {
                team.id == identifier
                    || team.key.eq_ignore_ascii_case(identifier)
                    || team.name.eq_ignore_ascii_case(identifier)
            })
            .map(|team| team.id))
    }

    pub async fn resolve_state_id(
        &self,
        team_id: &str,
        identifier: &str,
    ) -> GraphqlResult<Option<String>> {
        let states = self.workflow_states(team_id).await?;
        Ok(states
            .into_iter()
            .find(|state| state.id == identifier || state.name.eq_ignore_ascii_case(identifier))
            .map(|state| state.id))
    }

    pub async fn workflow_states_for_team(
        &self,
        team_identifier: &str,
    ) -> GraphqlResult<Option<(TeamSummary, Vec<WorkflowStateSummary>)>> {
        let teams = self.teams().await?;
        if let Some(team) = teams.into_iter().find(|team| {
            team.id == team_identifier
                || team.key.eq_ignore_ascii_case(team_identifier)
                || team.name.eq_ignore_ascii_case(team_identifier)
        }) {
            let states = self.workflow_states(&team.id).await?;
            Ok(Some((team, states)))
        } else {
            Ok(None)
        }
    }

    pub async fn create(&self, options: IssueCreateOptions) -> GraphqlResult<IssueDetail> {
        let IssueCreateOptions {
            team_id,
            title,
            description,
            assignee_id,
            state_id,
            label_ids,
            priority,
        } = options;

        let mut input = IssueCreateInput::new(team_id, title);
        input.description = description;
        input.assignee_id = assignee_id;
        input.state_id = state_id;
        if !label_ids.is_empty() {
            input.label_ids = label_ids;
        }
        input.priority = priority;

        self.client.create_issue(input).await
    }

    pub async fn update(
        &self,
        issue_id: &str,
        input: IssueUpdateInput,
    ) -> GraphqlResult<IssueDetail> {
        self.client.update_issue(issue_id, input).await
    }

    pub async fn archive(&self, issue_id: &str, archive: bool) -> GraphqlResult<IssueDetail> {
        self.client.archive_issue(issue_id, archive).await
    }

    pub async fn delete(&self, issue_id: &str) -> GraphqlResult<bool> {
        self.client.delete_issue(issue_id).await
    }

    pub async fn comment(&self, issue_id: &str, body: &str) -> GraphqlResult<Comment> {
        self.client
            .create_comment(CommentCreateInput {
                issue_id: issue_id.to_owned(),
                body: body.to_owned(),
            })
            .await
    }
}

/// Options used to constrain issue queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueQueryOptions {
    pub limit: usize,
    pub team_id: Option<String>,
    pub team_key: Option<String>,
    pub assignee_id: Option<String>,
    pub state_id: Option<String>,
    pub project_id: Option<String>,
    pub label_ids: Vec<String>,
    pub title_contains: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueListResult {
    pub issues: Vec<IssueSummary>,
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

/// Settings to create a new issue.
#[derive(Debug, Clone)]
pub struct IssueCreateOptions {
    pub team_id: String,
    pub title: String,
    pub description: Option<String>,
    pub assignee_id: Option<String>,
    pub state_id: Option<String>,
    pub label_ids: Vec<String>,
    pub priority: Option<i32>,
}

impl IssueCreateOptions {
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

impl IssueQueryOptions {
    fn into_params(self) -> IssueListParams {
        let first = if self.limit == 0 {
            20
        } else {
            self.limit.min(200)
        };
        let mut filter = Map::new();

        if let Some(team_id) = self.team_id {
            filter.insert("team".into(), json!({ "id": { "eq": team_id } }));
        } else if let Some(team_key) = self.team_key {
            filter.insert("team".into(), json!({ "key": { "eq": team_key } }));
        }

        if let Some(state_id) = self.state_id {
            filter.insert("state".into(), json!({ "id": { "eq": state_id } }));
        }

        if let Some(assignee_id) = self.assignee_id {
            filter.insert("assignee".into(), json!({ "id": { "eq": assignee_id } }));
        }

        if let Some(project_id) = self.project_id {
            filter.insert("project".into(), json!({ "id": { "eq": project_id } }));
        }

        if !self.label_ids.is_empty() {
            filter.insert("labels".into(), json!({ "id": { "in": self.label_ids } }));
        }

        if let Some(search) = self.title_contains {
            filter.insert("title".into(), json!({ "contains": search }));
        }

        let filter = if filter.is_empty() {
            None
        } else {
            Some(Value::Object(filter))
        };

        IssueListParams {
            first,
            filter,
            after: self.after,
        }
    }
}

#[derive(Default, Clone)]
struct Cache {
    teams: Arc<RwLock<Option<Vec<TeamSummary>>>>,
    workflow_states: Arc<RwLock<HashMap<String, Vec<WorkflowStateSummary>>>>,
}

impl Cache {
    async fn read_teams(&self) -> Option<Vec<TeamSummary>> {
        self.teams.read().await.clone()
    }

    async fn write_teams(&self, teams: Vec<TeamSummary>) {
        *self.teams.write().await = Some(teams);
    }

    async fn read_states(&self, team_id: &str) -> Option<Vec<WorkflowStateSummary>> {
        self.workflow_states.read().await.get(team_id).cloned()
    }

    async fn write_states(&self, team_id: &str, states: Vec<WorkflowStateSummary>) {
        self.workflow_states
            .write()
            .await
            .insert(team_id.to_owned(), states);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_options_to_filter() {
        let options = IssueQueryOptions {
            limit: 10,
            team_key: Some("ENG".into()),
            assignee_id: Some("user-1".into()),
            state_id: Some("state-1".into()),
            label_ids: vec!["label-1".into(), "label-2".into()],
            project_id: Some("proj-1".into()),
            title_contains: Some("bug".into()),
            after: Some("cursor".into()),
            ..Default::default()
        };

        let params = options.into_params();
        assert_eq!(params.first, 10);
        let filter = params.filter.expect("filter present");
        assert_eq!(filter["team"]["key"]["eq"], "ENG");
        assert_eq!(filter["assignee"]["id"]["eq"], "user-1");
        assert_eq!(filter["project"]["id"]["eq"], "proj-1");
        assert_eq!(filter["labels"]["id"]["in"].as_array().unwrap().len(), 2);
        assert_eq!(filter["title"]["contains"], "bug");
        assert_eq!(params.after.as_deref(), Some("cursor"));
    }
}
