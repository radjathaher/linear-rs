use serde_json::{json, Map, Value};

use crate::graphql::{
    GraphqlResult, IssueDetail, IssueListParams, IssueSummary, LinearGraphqlClient,
};

/// Provides higher-level helpers around Linear issues.
#[derive(Clone)]
pub struct IssueService {
    client: LinearGraphqlClient,
}

impl IssueService {
    pub fn new(client: LinearGraphqlClient) -> Self {
        Self { client }
    }

    pub async fn list(&self, options: IssueQueryOptions) -> GraphqlResult<Vec<IssueSummary>> {
        let params = options.into_params();
        self.client.list_issues(params).await
    }

    pub async fn get_by_key(&self, key: &str) -> GraphqlResult<IssueDetail> {
        self.client.issue_by_key(key).await
    }
}

/// Options used to constrain issue queries.
#[derive(Debug, Clone, Default)]
pub struct IssueQueryOptions {
    pub limit: usize,
    pub team_key: Option<String>,
    pub assignee_id: Option<String>,
    pub state_id: Option<String>,
    pub label_ids: Vec<String>,
}

impl IssueQueryOptions {
    fn into_params(self) -> IssueListParams {
        let first = if self.limit == 0 {
            20
        } else {
            self.limit.min(200)
        };
        let mut filter = Map::new();

        if let Some(team_key) = self.team_key {
            filter.insert("team".into(), json!({ "key": { "eq": team_key } }));
        }

        if let Some(state_id) = self.state_id {
            filter.insert("state".into(), json!({ "id": { "eq": state_id } }));
        }

        if let Some(assignee_id) = self.assignee_id {
            filter.insert("assignee".into(), json!({ "id": { "eq": assignee_id } }));
        }

        if !self.label_ids.is_empty() {
            filter.insert("labels".into(), json!({ "id": { "in": self.label_ids } }));
        }

        let filter = if filter.is_empty() {
            None
        } else {
            Some(Value::Object(filter))
        };

        IssueListParams { first, filter }
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
        };

        let params = options.into_params();
        assert_eq!(params.first, 10);
        let filter = params.filter.expect("filter present");
        assert_eq!(filter["team"]["key"]["eq"], "ENG");
        assert_eq!(filter["assignee"]["id"]["eq"], "user-1");
        assert_eq!(filter["labels"]["id"]["in"].as_array().unwrap().len(), 2);
    }
}
