use serde_json::{json, Map, Value};

use crate::graphql::{
    GraphqlResult, LinearGraphqlClient, ProjectCreateInput, ProjectDetail, ProjectListParams,
    ProjectListResponse, ProjectUpdateInput,
};

#[derive(Clone)]
pub struct ProjectService {
    client: LinearGraphqlClient,
}

impl ProjectService {
    pub fn new(client: LinearGraphqlClient) -> Self {
        Self { client }
    }

    pub async fn list(&self, options: ProjectQueryOptions) -> GraphqlResult<ProjectListResponse> {
        let params = options.into_params();
        self.client.projects(params).await
    }

    pub async fn create(&self, input: ProjectCreateInput) -> GraphqlResult<ProjectDetail> {
        self.client.project_create(input).await
    }

    pub async fn update(
        &self,
        id: &str,
        input: ProjectUpdateInput,
    ) -> GraphqlResult<ProjectDetail> {
        self.client.project_update(id, input).await
    }

    pub async fn archive(&self, id: &str, archive: bool) -> GraphqlResult<ProjectDetail> {
        self.client.project_archive(id, archive).await
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectQueryOptions {
    pub limit: usize,
    pub after: Option<String>,
    pub state: Option<String>,
    pub status: Option<String>,
    pub team_id: Option<String>,
    pub sort: Option<ProjectSort>,
}

#[derive(Debug, Clone, Copy)]
pub enum ProjectSort {
    UpdatedAsc,
    UpdatedDesc,
    CreatedAsc,
    CreatedDesc,
    TargetAsc,
    TargetDesc,
}

impl ProjectSort {
    pub fn as_order_by(self) -> Value {
        match self {
            ProjectSort::UpdatedAsc => json!({ "field": "updatedAt", "direction": "ASC" }),
            ProjectSort::UpdatedDesc => json!({ "field": "updatedAt", "direction": "DESC" }),
            ProjectSort::CreatedAsc => json!({ "field": "createdAt", "direction": "ASC" }),
            ProjectSort::CreatedDesc => json!({ "field": "createdAt", "direction": "DESC" }),
            ProjectSort::TargetAsc => json!({ "field": "targetDate", "direction": "ASC" }),
            ProjectSort::TargetDesc => json!({ "field": "targetDate", "direction": "DESC" }),
        }
    }
}

impl ProjectQueryOptions {
    fn into_params(self) -> ProjectListParams {
        let mut filter = Map::new();

        if let Some(state) = self.state {
            filter.insert("state".into(), json!({ "eq": state }));
        }
        if let Some(status) = self.status {
            filter.insert("status".into(), json!({ "eq": status }));
        }
        if let Some(team_id) = self.team_id {
            filter.insert("team".into(), json!({ "id": { "eq": team_id } }));
        }

        let filter = if filter.is_empty() {
            None
        } else {
            Some(Value::Object(filter))
        };

        let order_by = self.sort.map(|s| s.as_order_by());

        ProjectListParams {
            first: if self.limit == 0 {
                20
            } else {
                self.limit.min(200)
            },
            filter,
            order_by,
            after: self.after,
        }
    }
}
