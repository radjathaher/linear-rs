use crate::graphql::{
    GraphqlResult, IssueLabel, IssueLabelCreateInput, IssueLabelUpdateInput, LinearGraphqlClient,
};

#[derive(Clone)]
pub struct LabelService {
    client: LinearGraphqlClient,
}

impl LabelService {
    pub fn new(client: LinearGraphqlClient) -> Self {
        Self { client }
    }

    pub async fn list(&self, team_id: &str) -> GraphqlResult<Vec<IssueLabel>> {
        self.client.issue_labels(team_id).await
    }

    pub async fn create(&self, input: IssueLabelCreateInput) -> GraphqlResult<IssueLabel> {
        self.client.create_issue_label(input).await
    }

    pub async fn update(
        &self,
        label_id: &str,
        input: IssueLabelUpdateInput,
    ) -> GraphqlResult<IssueLabel> {
        self.client.update_issue_label(label_id, input).await
    }
}
