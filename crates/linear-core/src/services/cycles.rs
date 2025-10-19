use serde_json::{json, Map, Value};

use crate::graphql::{
    CycleListParams, CycleListResponse, CycleSummary, CycleUpdateInput, GraphqlResult,
    LinearGraphqlClient,
};

#[derive(Clone)]
pub struct CycleService {
    client: LinearGraphqlClient,
}

impl CycleService {
    pub fn new(client: LinearGraphqlClient) -> Self {
        Self { client }
    }

    pub async fn list(&self, options: CycleQueryOptions) -> GraphqlResult<CycleListResponse> {
        let params = options.into_params();
        self.client.cycles(params).await
    }

    pub async fn update(
        &self,
        cycle_id: &str,
        input: CycleUpdateInput,
    ) -> GraphqlResult<CycleSummary> {
        self.client.cycle_update(cycle_id, input).await
    }
}

#[derive(Debug, Clone, Default)]
pub struct CycleQueryOptions {
    pub limit: usize,
    pub after: Option<String>,
    pub team_id: Option<String>,
    pub state: Option<String>,
    pub sort: Option<CycleSort>,
}

#[derive(Debug, Clone, Copy)]
pub enum CycleSort {
    StartAsc,
    StartDesc,
    EndAsc,
    EndDesc,
}

impl CycleSort {
    fn as_order_by(self) -> Value {
        match self {
            CycleSort::StartAsc => json!({ "field": "startsAt", "direction": "ASC" }),
            CycleSort::StartDesc => json!({ "field": "startsAt", "direction": "DESC" }),
            CycleSort::EndAsc => json!({ "field": "endsAt", "direction": "ASC" }),
            CycleSort::EndDesc => json!({ "field": "endsAt", "direction": "DESC" }),
        }
    }
}

impl CycleQueryOptions {
    fn into_params(self) -> CycleListParams {
        let mut filter = Map::new();

        if let Some(team_id) = self.team_id {
            filter.insert("team".into(), json!({ "id": { "eq": team_id } }));
        }

        if let Some(state) = self.state {
            filter.insert("state".into(), json!({ "eq": state }));
        }

        let filter = if filter.is_empty() {
            None
        } else {
            Some(Value::Object(filter))
        };

        let order_by = self.sort.map(|s| s.as_order_by());

        CycleListParams {
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
