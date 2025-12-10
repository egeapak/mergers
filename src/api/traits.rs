//! Traits for Azure DevOps API operations.
//!
//! These traits abstract the Azure DevOps API operations to enable:
//! - Mocking for unit tests
//! - Alternative implementations
//! - Easier testing of async code

use anyhow::Result;
use async_trait::async_trait;
use azure_devops_rust_api::git::models as git_models;
use azure_devops_rust_api::wit::models as wit_models;

/// Trait for pull request operations.
///
/// This trait abstracts the operations on Azure DevOps pull requests,
/// allowing for both real and mock implementations.
#[allow(clippy::too_many_arguments)]
#[async_trait]
pub trait PullRequestOperations: Send + Sync {
    /// Fetches pull requests for a specific target branch with pagination.
    ///
    /// # Arguments
    ///
    /// * `organization` - Azure DevOps organization name
    /// * `repository` - Repository name
    /// * `project` - Project name
    /// * `target_ref` - Target branch reference (e.g., "refs/heads/main")
    /// * `status` - PR status filter (e.g., "completed")
    /// * `top` - Maximum number of PRs to fetch per page
    /// * `skip` - Number of PRs to skip (for pagination)
    async fn get_pull_requests(
        &self,
        organization: &str,
        repository: &str,
        project: &str,
        target_ref: &str,
        status: &str,
        top: i32,
        skip: i32,
    ) -> Result<Vec<git_models::GitPullRequest>>;

    /// Fetches a single pull request by ID.
    async fn get_pull_request(
        &self,
        organization: &str,
        repository: &str,
        pull_request_id: i32,
        project: &str,
    ) -> Result<git_models::GitPullRequest>;

    /// Adds a label to a pull request.
    async fn create_label(
        &self,
        organization: &str,
        repository: &str,
        pull_request_id: i32,
        project: &str,
        label_name: &str,
    ) -> Result<()>;
}

/// Trait for pull request work items operations.
#[async_trait]
pub trait PullRequestWorkItemsOperations: Send + Sync {
    /// Lists work item references linked to a pull request.
    async fn list(
        &self,
        organization: &str,
        repository: &str,
        pull_request_id: i32,
        project: &str,
    ) -> Result<Vec<git_models::ResourceRef>>;
}

/// Trait for repository operations.
#[async_trait]
pub trait RepositoryOperations: Send + Sync {
    /// Gets repository details.
    async fn get_repository(
        &self,
        organization: &str,
        repository: &str,
        project: &str,
    ) -> Result<git_models::GitRepository>;
}

/// Trait for work item operations.
#[async_trait]
pub trait WorkItemOperations: Send + Sync {
    /// Lists work items by IDs.
    ///
    /// # Arguments
    ///
    /// * `organization` - Azure DevOps organization name
    /// * `ids` - Comma-separated list of work item IDs
    /// * `project` - Project name
    /// * `fields` - Comma-separated list of fields to retrieve
    async fn get_work_items(
        &self,
        organization: &str,
        ids: &str,
        project: &str,
        fields: &str,
    ) -> Result<Vec<wit_models::WorkItem>>;

    /// Updates a work item.
    ///
    /// # Arguments
    ///
    /// * `organization` - Azure DevOps organization name
    /// * `work_item_id` - Work item ID
    /// * `project` - Project name
    /// * `patch` - JSON patch operations to apply
    async fn update_work_item(
        &self,
        organization: &str,
        work_item_id: i32,
        project: &str,
        patch: Vec<wit_models::JsonPatchOperation>,
    ) -> Result<wit_models::WorkItem>;
}

/// Trait for work item updates/history operations.
#[async_trait]
pub trait WorkItemUpdatesOperations: Send + Sync {
    /// Lists work item updates (history).
    async fn get_work_item_updates(
        &self,
        organization: &str,
        work_item_id: i32,
        project: &str,
    ) -> Result<Vec<wit_models::WorkItemUpdate>>;
}

/// Combined trait for all Git-related operations.
///
/// This trait provides a unified interface for all Git-related Azure DevOps operations.
pub trait GitOperations:
    PullRequestOperations + PullRequestWorkItemsOperations + RepositoryOperations + Send + Sync
{
}

/// Combined trait for all Work Item Tracking operations.
pub trait WitOperations: WorkItemOperations + WorkItemUpdatesOperations + Send + Sync {}

// Blanket implementations for combined traits
impl<T> GitOperations for T where
    T: PullRequestOperations + PullRequestWorkItemsOperations + RepositoryOperations + Send + Sync
{
}
impl<T> WitOperations for T where T: WorkItemOperations + WorkItemUpdatesOperations + Send + Sync {}

/// Real implementation wrapping azure_devops_rust_api::git::Client.
///
/// This struct implements all Git-related traits by delegating to the
/// azure_devops_rust_api git client.
#[derive(Clone)]
pub struct RealGitOperations {
    client: azure_devops_rust_api::git::Client,
}

impl RealGitOperations {
    /// Creates a new RealGitOperations wrapper.
    pub fn new(client: azure_devops_rust_api::git::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PullRequestOperations for RealGitOperations {
    async fn get_pull_requests(
        &self,
        organization: &str,
        repository: &str,
        project: &str,
        target_ref: &str,
        status: &str,
        top: i32,
        skip: i32,
    ) -> Result<Vec<git_models::GitPullRequest>> {
        let response = self
            .client
            .pull_requests_client()
            .get_pull_requests(organization, repository, project)
            .search_criteria_target_ref_name(target_ref)
            .search_criteria_status(status)
            .top(top)
            .skip(skip)
            .await?;
        Ok(response.value)
    }

    async fn get_pull_request(
        &self,
        organization: &str,
        repository: &str,
        pull_request_id: i32,
        project: &str,
    ) -> Result<git_models::GitPullRequest> {
        let pr = self
            .client
            .pull_requests_client()
            .get_pull_request(organization, repository, pull_request_id, project)
            .await?;
        Ok(pr)
    }

    async fn create_label(
        &self,
        organization: &str,
        repository: &str,
        pull_request_id: i32,
        project: &str,
        label_name: &str,
    ) -> Result<()> {
        let label_data = git_models::WebApiCreateTagRequestData {
            name: label_name.to_string(),
        };

        self.client
            .pull_request_labels_client()
            .create(
                organization,
                label_data,
                repository,
                pull_request_id,
                project,
            )
            .await?;
        Ok(())
    }
}

#[async_trait]
impl PullRequestWorkItemsOperations for RealGitOperations {
    async fn list(
        &self,
        organization: &str,
        repository: &str,
        pull_request_id: i32,
        project: &str,
    ) -> Result<Vec<git_models::ResourceRef>> {
        let refs = self
            .client
            .pull_request_work_items_client()
            .list(organization, repository, pull_request_id, project)
            .await?;
        Ok(refs.value)
    }
}

#[async_trait]
impl RepositoryOperations for RealGitOperations {
    async fn get_repository(
        &self,
        organization: &str,
        repository: &str,
        project: &str,
    ) -> Result<git_models::GitRepository> {
        let repo = self
            .client
            .repositories_client()
            .get_repository(organization, repository, project)
            .await?;
        Ok(repo)
    }
}

/// Real implementation wrapping azure_devops_rust_api::wit::Client.
///
/// This struct implements all Work Item Tracking traits by delegating to the
/// azure_devops_rust_api wit client.
#[derive(Clone)]
pub struct RealWitOperations {
    client: azure_devops_rust_api::wit::Client,
}

impl RealWitOperations {
    /// Creates a new RealWitOperations wrapper.
    pub fn new(client: azure_devops_rust_api::wit::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl WorkItemOperations for RealWitOperations {
    async fn get_work_items(
        &self,
        organization: &str,
        ids: &str,
        project: &str,
        fields: &str,
    ) -> Result<Vec<wit_models::WorkItem>> {
        let work_items = self
            .client
            .work_items_client()
            .list(organization, ids, project)
            .fields(fields)
            .await?;
        Ok(work_items.value)
    }

    async fn update_work_item(
        &self,
        organization: &str,
        work_item_id: i32,
        project: &str,
        patch: Vec<wit_models::JsonPatchOperation>,
    ) -> Result<wit_models::WorkItem> {
        let work_item = self
            .client
            .work_items_client()
            .update(organization, patch, work_item_id, project)
            .await?;
        Ok(work_item)
    }
}

#[async_trait]
impl WorkItemUpdatesOperations for RealWitOperations {
    async fn get_work_item_updates(
        &self,
        organization: &str,
        work_item_id: i32,
        project: &str,
    ) -> Result<Vec<wit_models::WorkItemUpdate>> {
        let updates = self
            .client
            .updates_client()
            .list(organization, work_item_id, project)
            .await?;
        Ok(updates.value)
    }
}

#[cfg(test)]
pub mod mocks {
    //! Mock implementations for testing.

    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Mock implementation for pull request operations.
    #[derive(Default)]
    pub struct MockPullRequestOperations {
        /// Pre-configured responses for get_pull_requests.
        pub get_pull_requests_response: Arc<Mutex<Option<Result<Vec<git_models::GitPullRequest>>>>>,
        /// Pre-configured response for get_pull_request.
        pub get_pull_request_response: Arc<Mutex<Option<Result<git_models::GitPullRequest>>>>,
        /// Track if create_label was called.
        pub create_label_called: Arc<Mutex<bool>>,
        /// Track the label name that was passed.
        pub last_label_name: Arc<Mutex<Option<String>>>,
    }

    impl MockPullRequestOperations {
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the response for get_pull_requests.
        pub async fn set_get_pull_requests_response(
            &self,
            response: Result<Vec<git_models::GitPullRequest>>,
        ) {
            *self.get_pull_requests_response.lock().await = Some(response);
        }

        /// Sets the response for get_pull_request.
        pub async fn set_get_pull_request_response(
            &self,
            response: Result<git_models::GitPullRequest>,
        ) {
            *self.get_pull_request_response.lock().await = Some(response);
        }
    }

    #[async_trait]
    impl PullRequestOperations for MockPullRequestOperations {
        async fn get_pull_requests(
            &self,
            _organization: &str,
            _repository: &str,
            _project: &str,
            _target_ref: &str,
            _status: &str,
            _top: i32,
            _skip: i32,
        ) -> Result<Vec<git_models::GitPullRequest>> {
            self.get_pull_requests_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Ok(vec![]))
        }

        async fn get_pull_request(
            &self,
            _organization: &str,
            _repository: &str,
            _pull_request_id: i32,
            _project: &str,
        ) -> Result<git_models::GitPullRequest> {
            self.get_pull_request_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Err(anyhow::anyhow!("No mock response configured")))
        }

        async fn create_label(
            &self,
            _organization: &str,
            _repository: &str,
            _pull_request_id: i32,
            _project: &str,
            label_name: &str,
        ) -> Result<()> {
            *self.create_label_called.lock().await = true;
            *self.last_label_name.lock().await = Some(label_name.to_string());
            Ok(())
        }
    }

    /// Mock implementation for pull request work items operations.
    #[derive(Default)]
    pub struct MockPullRequestWorkItemsOperations {
        /// Pre-configured response for list.
        pub list_response: Arc<Mutex<Option<Result<Vec<git_models::ResourceRef>>>>>,
    }

    impl MockPullRequestWorkItemsOperations {
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the response for list.
        pub async fn set_list_response(&self, response: Result<Vec<git_models::ResourceRef>>) {
            *self.list_response.lock().await = Some(response);
        }
    }

    #[async_trait]
    impl PullRequestWorkItemsOperations for MockPullRequestWorkItemsOperations {
        async fn list(
            &self,
            _organization: &str,
            _repository: &str,
            _pull_request_id: i32,
            _project: &str,
        ) -> Result<Vec<git_models::ResourceRef>> {
            self.list_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Ok(vec![]))
        }
    }

    /// Mock implementation for repository operations.
    #[derive(Default)]
    pub struct MockRepositoryOperations {
        /// Pre-configured response for get_repository.
        pub get_repository_response: Arc<Mutex<Option<Result<git_models::GitRepository>>>>,
    }

    impl MockRepositoryOperations {
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the response for get_repository.
        pub async fn set_get_repository_response(
            &self,
            response: Result<git_models::GitRepository>,
        ) {
            *self.get_repository_response.lock().await = Some(response);
        }
    }

    #[async_trait]
    impl RepositoryOperations for MockRepositoryOperations {
        async fn get_repository(
            &self,
            _organization: &str,
            _repository: &str,
            _project: &str,
        ) -> Result<git_models::GitRepository> {
            self.get_repository_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Err(anyhow::anyhow!("No mock response configured")))
        }
    }

    /// Mock implementation for work item operations.
    #[derive(Default)]
    pub struct MockWorkItemOperations {
        /// Pre-configured response for list.
        pub list_response: Arc<Mutex<Option<Result<Vec<wit_models::WorkItem>>>>>,
        /// Pre-configured response for update.
        pub update_response: Arc<Mutex<Option<Result<wit_models::WorkItem>>>>,
        /// Track update calls.
        pub update_called: Arc<Mutex<bool>>,
        /// Track the last state that was set.
        pub last_update_state: Arc<Mutex<Option<String>>>,
    }

    impl MockWorkItemOperations {
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the response for list.
        pub async fn set_list_response(&self, response: Result<Vec<wit_models::WorkItem>>) {
            *self.list_response.lock().await = Some(response);
        }

        /// Sets the response for update.
        pub async fn set_update_response(&self, response: Result<wit_models::WorkItem>) {
            *self.update_response.lock().await = Some(response);
        }
    }

    #[async_trait]
    impl WorkItemOperations for MockWorkItemOperations {
        async fn get_work_items(
            &self,
            _organization: &str,
            _ids: &str,
            _project: &str,
            _fields: &str,
        ) -> Result<Vec<wit_models::WorkItem>> {
            self.list_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Ok(vec![]))
        }

        async fn update_work_item(
            &self,
            _organization: &str,
            _work_item_id: i32,
            _project: &str,
            patch: Vec<wit_models::JsonPatchOperation>,
        ) -> Result<wit_models::WorkItem> {
            *self.update_called.lock().await = true;

            // Extract state from patch if present
            for op in &patch {
                if let Some(path) = &op.path
                    && path == "/fields/System.State"
                    && let Some(value) = &op.value
                    && let Some(state) = value.as_str()
                {
                    *self.last_update_state.lock().await = Some(state.to_string());
                }
            }

            self.update_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Err(anyhow::anyhow!("No mock response configured")))
        }
    }

    /// Mock implementation for work item updates operations.
    #[derive(Default)]
    pub struct MockWorkItemUpdatesOperations {
        /// Pre-configured response for list.
        pub list_response: Arc<Mutex<Option<Result<Vec<wit_models::WorkItemUpdate>>>>>,
    }

    impl MockWorkItemUpdatesOperations {
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the response for list.
        pub async fn set_list_response(&self, response: Result<Vec<wit_models::WorkItemUpdate>>) {
            *self.list_response.lock().await = Some(response);
        }
    }

    #[async_trait]
    impl WorkItemUpdatesOperations for MockWorkItemUpdatesOperations {
        async fn get_work_item_updates(
            &self,
            _organization: &str,
            _work_item_id: i32,
            _project: &str,
        ) -> Result<Vec<wit_models::WorkItemUpdate>> {
            self.list_response
                .lock()
                .await
                .take()
                .unwrap_or_else(|| Ok(vec![]))
        }
    }

    /// Combined mock for all Git operations.
    pub struct MockGitOperations {
        pub pr_ops: MockPullRequestOperations,
        pub pr_work_items_ops: MockPullRequestWorkItemsOperations,
        pub repo_ops: MockRepositoryOperations,
    }

    impl MockGitOperations {
        pub fn new() -> Self {
            Self {
                pr_ops: MockPullRequestOperations::new(),
                pr_work_items_ops: MockPullRequestWorkItemsOperations::new(),
                repo_ops: MockRepositoryOperations::new(),
            }
        }
    }

    impl Default for MockGitOperations {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl PullRequestOperations for MockGitOperations {
        async fn get_pull_requests(
            &self,
            organization: &str,
            repository: &str,
            project: &str,
            target_ref: &str,
            status: &str,
            top: i32,
            skip: i32,
        ) -> Result<Vec<git_models::GitPullRequest>> {
            self.pr_ops
                .get_pull_requests(
                    organization,
                    repository,
                    project,
                    target_ref,
                    status,
                    top,
                    skip,
                )
                .await
        }

        async fn get_pull_request(
            &self,
            organization: &str,
            repository: &str,
            pull_request_id: i32,
            project: &str,
        ) -> Result<git_models::GitPullRequest> {
            self.pr_ops
                .get_pull_request(organization, repository, pull_request_id, project)
                .await
        }

        async fn create_label(
            &self,
            organization: &str,
            repository: &str,
            pull_request_id: i32,
            project: &str,
            label_name: &str,
        ) -> Result<()> {
            self.pr_ops
                .create_label(
                    organization,
                    repository,
                    pull_request_id,
                    project,
                    label_name,
                )
                .await
        }
    }

    #[async_trait]
    impl PullRequestWorkItemsOperations for MockGitOperations {
        async fn list(
            &self,
            organization: &str,
            repository: &str,
            pull_request_id: i32,
            project: &str,
        ) -> Result<Vec<git_models::ResourceRef>> {
            self.pr_work_items_ops
                .list(organization, repository, pull_request_id, project)
                .await
        }
    }

    #[async_trait]
    impl RepositoryOperations for MockGitOperations {
        async fn get_repository(
            &self,
            organization: &str,
            repository: &str,
            project: &str,
        ) -> Result<git_models::GitRepository> {
            self.repo_ops
                .get_repository(organization, repository, project)
                .await
        }
    }

    /// Combined mock for all WIT operations.
    pub struct MockWitOperations {
        pub work_item_ops: MockWorkItemOperations,
        pub updates_ops: MockWorkItemUpdatesOperations,
    }

    impl MockWitOperations {
        pub fn new() -> Self {
            Self {
                work_item_ops: MockWorkItemOperations::new(),
                updates_ops: MockWorkItemUpdatesOperations::new(),
            }
        }
    }

    impl Default for MockWitOperations {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl WorkItemOperations for MockWitOperations {
        async fn get_work_items(
            &self,
            organization: &str,
            ids: &str,
            project: &str,
            fields: &str,
        ) -> Result<Vec<wit_models::WorkItem>> {
            self.work_item_ops
                .get_work_items(organization, ids, project, fields)
                .await
        }

        async fn update_work_item(
            &self,
            organization: &str,
            work_item_id: i32,
            project: &str,
            patch: Vec<wit_models::JsonPatchOperation>,
        ) -> Result<wit_models::WorkItem> {
            self.work_item_ops
                .update_work_item(organization, work_item_id, project, patch)
                .await
        }
    }

    #[async_trait]
    impl WorkItemUpdatesOperations for MockWitOperations {
        async fn get_work_item_updates(
            &self,
            organization: &str,
            work_item_id: i32,
            project: &str,
        ) -> Result<Vec<wit_models::WorkItemUpdate>> {
            self.updates_ops
                .get_work_item_updates(organization, work_item_id, project)
                .await
        }
    }
}
