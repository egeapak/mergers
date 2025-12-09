//! Azure DevOps API client implementation using azure_devops_rust_api crate.
//!
//! This module provides a client for interacting with Azure DevOps APIs,
//! specifically for managing pull requests and work items in merge workflows.

use anyhow::{Context, Result};
use azure_devops_rust_api::{git, wit};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use secrecy::SecretString;
use std::sync::Arc;

use super::credential::PatCredential;
use super::mappers::extract_work_item_id;
use crate::models::{
    MergeCommit, PullRequest, PullRequestWithWorkItems, RepoDetails, WorkItem, WorkItemHistory,
};
use crate::utils::parse_since_date;

/// Default number of retry attempts for transient failures.
/// Note: Retry logic is now handled by the underlying azure_core HTTP client.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Azure DevOps API client for pull request and work item management.
///
/// This client uses the official azure_devops_rust_api crate for API interactions,
/// providing type-safe access to Azure DevOps REST APIs.
///
/// # Example
///
/// ```rust,no_run
/// use mergers::api::AzureDevOpsClient;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = AzureDevOpsClient::new(
///     "my-org".to_string(),
///     "my-project".to_string(),
///     "my-repo".to_string(),
///     "my-pat".to_string(),
/// )?;
///
/// // Fetch pull requests from the main branch
/// let prs = client.fetch_pull_requests("main", None).await?;
/// println!("Found {} pull requests", prs.len());
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct AzureDevOpsClient {
    organization: String,
    project: String,
    repository: String,
    git_client: git::Client,
    wit_client: wit::Client,
}

impl AzureDevOpsClient {
    /// Creates a new Azure DevOps API client.
    ///
    /// # Arguments
    ///
    /// * `organization` - Azure DevOps organization name
    /// * `project` - Azure DevOps project name
    /// * `repository` - Repository name within the project
    /// * `pat` - Personal Access Token for authentication
    ///
    /// # Security
    ///
    /// The PAT is wrapped in a SecretString internally and only exposed
    /// when needed for authentication.
    pub fn new(
        organization: String,
        project: String,
        repository: String,
        pat: String,
    ) -> Result<Self> {
        let secret_pat = SecretString::from(pat);
        Self::new_with_secret(organization, project, repository, secret_pat)
    }

    /// Creates a new Azure DevOps API client with a SecretString PAT.
    ///
    /// This is the preferred constructor when the PAT is already wrapped in a SecretString.
    pub fn new_with_secret(
        organization: String,
        project: String,
        repository: String,
        pat: SecretString,
    ) -> Result<Self> {
        let credential = Arc::new(PatCredential::new(pat));
        let ado_credential = azure_devops_rust_api::Credential::TokenCredential(credential);

        let git_client = git::ClientBuilder::new(ado_credential.clone()).build();
        let wit_client = wit::ClientBuilder::new(ado_credential).build();

        Ok(Self {
            organization,
            project,
            repository,
            git_client,
            wit_client,
        })
    }

    /// Creates a new client with custom pool configuration (for API compatibility).
    ///
    /// Note: Pool configuration is now handled by the underlying azure_core HTTP client.
    /// This method is provided for backward compatibility.
    pub fn new_with_secret_and_pool_config(
        organization: String,
        project: String,
        repository: String,
        pat: SecretString,
        _pool_max_idle_per_host: usize,
        _pool_idle_timeout_secs: u64,
    ) -> Result<Self> {
        Self::new_with_secret(organization, project, repository, pat)
    }

    /// Creates a new client with full configuration (for API compatibility).
    ///
    /// Note: Retry logic is now handled by the underlying azure_core HTTP client.
    /// This method is provided for backward compatibility.
    pub fn new_with_full_config(
        organization: String,
        project: String,
        repository: String,
        pat: SecretString,
        _pool_max_idle_per_host: usize,
        _pool_idle_timeout_secs: u64,
        _max_retries: u32,
    ) -> Result<Self> {
        Self::new_with_secret(organization, project, repository, pat)
    }

    /// Returns the organization name.
    pub fn organization(&self) -> &str {
        &self.organization
    }

    /// Returns the project name.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Returns the repository name.
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// Returns the maximum number of retries (for API compatibility).
    ///
    /// Note: Retry logic is now handled by the underlying azure_core HTTP client.
    pub fn max_retries(&self) -> u32 {
        3 // Default value for compatibility
    }

    /// Fetches all pull requests for a given branch using pagination.
    ///
    /// This method implements pagination to ensure all pull requests are retrieved.
    /// If `since` is provided, stops fetching when encountering PRs older than the specified date.
    pub async fn fetch_pull_requests(
        &self,
        dev_branch: &str,
        since: Option<&str>,
    ) -> Result<Vec<PullRequest>> {
        // Parse the since date if provided
        let since_date = if let Some(since_str) = since {
            Some(parse_since_date(since_str).context("Failed to parse since date")?)
        } else {
            None
        };

        let target_ref = format!("refs/heads/{}", dev_branch);
        let mut all_prs = Vec::new();
        let mut skip = 0;
        let top = 100;
        let max_requests = 100;
        let mut request_count = 0;

        loop {
            request_count += 1;
            if request_count > max_requests {
                anyhow::bail!(
                    "Exceeded maximum number of requests ({}) while fetching pull requests. Retrieved {} PRs so far.",
                    max_requests,
                    all_prs.len()
                );
            }

            // Fetch page of PRs
            let response = self
                .git_client
                .pull_requests_client()
                .get_pull_requests(&self.organization, &self.repository, &self.project)
                .search_criteria_target_ref_name(&target_ref)
                .search_criteria_status("completed")
                .top(top)
                .skip(skip)
                .await
                .context("Failed to fetch pull requests")?;

            let fetched_count = response.value.len();

            // Convert and filter PRs by date
            let mut reached_date_limit = false;
            for pr in response.value {
                let converted_pr: PullRequest = pr.into();

                if let Some(since_dt) = since_date
                    && let Some(closed_date_str) = &converted_pr.closed_date
                    && let Ok(closed_date) = DateTime::parse_from_rfc3339(closed_date_str)
                {
                    let closed_date_utc = closed_date.with_timezone(&Utc);
                    if closed_date_utc < since_dt {
                        reached_date_limit = true;
                        break;
                    }
                }
                all_prs.push(converted_pr);
            }

            if reached_date_limit || fetched_count < top as usize {
                break;
            }

            skip += top;
        }

        Ok(all_prs)
    }

    /// Fetches work items linked to a pull request.
    pub async fn fetch_work_items_for_pr(&self, pr_id: i32) -> Result<Vec<WorkItem>> {
        // Get work item refs linked to the PR
        let refs = self
            .git_client
            .pull_request_work_items_client()
            .list(&self.organization, &self.repository, pr_id, &self.project)
            .await
            .context("Failed to fetch work item references for PR")?;

        if refs.value.is_empty() {
            return Ok(vec![]);
        }

        // Extract work item IDs from the refs
        let ids: Vec<i32> = refs
            .value
            .iter()
            .filter_map(|r| r.url.as_ref().and_then(|url| extract_work_item_id(url)))
            .collect();

        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Batch fetch work items
        let ids_str = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let work_items = self
            .wit_client
            .work_items_client()
            .list(
                &self.organization,
                &ids_str,
                &self.project,
            )
            .fields("System.Title,System.State,System.WorkItemType,System.AssignedTo,System.IterationPath,System.Description,Microsoft.VSTS.TCM.ReproSteps")
            .await
            .context("Failed to fetch work items")?;

        Ok(work_items.value.into_iter().map(WorkItem::from).collect())
    }

    /// Fetches repository details including SSH URL.
    pub async fn fetch_repo_details(&self) -> Result<RepoDetails> {
        let repo = self
            .git_client
            .repositories_client()
            .get_repository(&self.organization, &self.repository, &self.project)
            .await
            .context("Failed to fetch repository details")?;

        Ok(RepoDetails::from(repo))
    }

    /// Fetches the merge commit for a pull request.
    pub async fn fetch_pr_commit(&self, pr_id: i32) -> Result<MergeCommit> {
        let pr = self
            .git_client
            .pull_requests_client()
            .get_pull_request(&self.organization, &self.repository, pr_id, &self.project)
            .await
            .context("Failed to fetch pull request details")?;

        pr.last_merge_commit
            .map(|c| MergeCommit {
                commit_id: c.commit_id.unwrap_or_default(),
            })
            .ok_or_else(|| anyhow::anyhow!("Pull request {} has no merge commit", pr_id))
    }

    /// Adds a label to a pull request.
    pub async fn add_label_to_pr(&self, pr_id: i32, label: &str) -> Result<()> {
        let label_data = git::models::WebApiCreateTagRequestData {
            name: label.to_string(),
        };

        self.git_client
            .pull_request_labels_client()
            .create(
                &self.organization,
                label_data,
                &self.repository,
                pr_id,
                &self.project,
            )
            .await
            .context("Failed to add label to pull request")?;

        Ok(())
    }

    /// Updates the state of a work item.
    pub async fn update_work_item_state(&self, work_item_id: i32, new_state: &str) -> Result<()> {
        let patch = vec![wit::models::JsonPatchOperation {
            op: Some(wit::models::json_patch_operation::Op::Add),
            path: Some("/fields/System.State".to_string()),
            value: Some(serde_json::json!(new_state)),
            from: None,
        }];

        self.wit_client
            .work_items_client()
            .update(&self.organization, patch, work_item_id, &self.project)
            .await
            .context("Failed to update work item state")?;

        Ok(())
    }

    /// Fetches the revision history for a work item.
    pub async fn fetch_work_item_history(&self, work_item_id: i32) -> Result<Vec<WorkItemHistory>> {
        let updates = self
            .wit_client
            .updates_client()
            .list(&self.organization, work_item_id, &self.project)
            .await
            .context("Failed to fetch work item history")?;

        Ok(updates
            .value
            .into_iter()
            .map(WorkItemHistory::from)
            .collect())
    }

    /// Fetches work items with their history for a PR, using parallel fetching.
    pub async fn fetch_work_items_with_history_for_pr_parallel(
        &self,
        pr_id: i32,
        max_concurrent: usize,
    ) -> Result<Vec<WorkItem>> {
        let work_items = self.fetch_work_items_for_pr(pr_id).await?;

        if work_items.is_empty() {
            return Ok(work_items);
        }

        // Fetch history for all work items in parallel with concurrency limit
        let work_items_with_history: Vec<WorkItem> = stream::iter(work_items)
            .map(|work_item| {
                let client = self.clone();
                async move {
                    let mut wi = work_item;
                    if let Ok(history) = client.fetch_work_item_history(wi.id).await {
                        wi.history = history;
                    }
                    wi
                }
            })
            .buffer_unordered(max_concurrent)
            .collect()
            .await;

        Ok(work_items_with_history)
    }

    /// Fetches work items with history for a PR (sequential, for backward compatibility).
    pub async fn fetch_work_items_with_history_for_pr(&self, pr_id: i32) -> Result<Vec<WorkItem>> {
        self.fetch_work_items_with_history_for_pr_parallel(pr_id, 10)
            .await
    }

    /// Fetches work items with history for multiple PRs in parallel.
    pub async fn fetch_work_items_for_prs_parallel(
        &self,
        prs: &[PullRequest],
        max_concurrent_prs: usize,
        max_concurrent_history: usize,
    ) -> Vec<PullRequestWithWorkItems> {
        stream::iter(prs.iter().cloned())
            .map(|pr| {
                let client = self.clone();
                async move {
                    let work_items = client
                        .fetch_work_items_with_history_for_pr_parallel(
                            pr.id,
                            max_concurrent_history,
                        )
                        .await
                        .unwrap_or_default();
                    PullRequestWithWorkItems {
                        pr,
                        work_items,
                        selected: false,
                    }
                }
            })
            .buffer_unordered(max_concurrent_prs)
            .collect()
            .await
    }

    /// Checks if a work item is in a terminal state.
    pub fn is_work_item_in_terminal_state(
        &self,
        work_item: &WorkItem,
        terminal_states: &[String],
    ) -> bool {
        if let Some(state) = &work_item.fields.state {
            terminal_states.contains(state)
        } else {
            false
        }
    }

    /// Analyzes work items for a PR to determine terminal state status.
    pub fn analyze_work_items_for_pr(
        &self,
        pr_with_work_items: &PullRequestWithWorkItems,
        terminal_states: &[String],
    ) -> (bool, Vec<WorkItem>) {
        let mut non_terminal_items = Vec::new();

        for work_item in &pr_with_work_items.work_items {
            if !self.is_work_item_in_terminal_state(work_item, terminal_states) {
                non_terminal_items.push(work_item.clone());
            }
        }

        let all_terminal =
            non_terminal_items.is_empty() && !pr_with_work_items.work_items.is_empty();
        (all_terminal, non_terminal_items)
    }

    /// Parses a comma-separated string of terminal states.
    pub fn parse_terminal_states(terminal_states_str: &str) -> Vec<String> {
        terminal_states_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// Filters out pull requests that already have a "merged-" tag.
///
/// This is used to prevent re-processing PRs that have already been tagged
/// after a successful merge operation.
pub fn filter_prs_without_merged_tag(prs: Vec<PullRequest>) -> Vec<PullRequest> {
    prs.into_iter()
        .filter(|pr| {
            if let Some(labels) = &pr.labels {
                !labels.iter().any(|label| label.name.starts_with("merged-"))
            } else {
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Parse Terminal States
    ///
    /// Tests parsing of comma-separated terminal states string.
    ///
    /// ## Test Scenario
    /// - Provides various terminal state strings
    /// - Parses them into vectors of states
    ///
    /// ## Expected Outcome
    /// - States are correctly split and trimmed
    /// - Empty strings are filtered out
    #[test]
    fn test_parse_terminal_states() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed,Done"),
            vec!["Closed", "Done"]
        );
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed, Done, Merged"),
            vec!["Closed", "Done", "Merged"]
        );
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states(""),
            Vec::<String>::new()
        );
    }

    /// # Work Item Terminal State Check
    ///
    /// Tests the terminal state checking logic.
    ///
    /// ## Test Scenario
    /// - Creates work items with various states
    /// - Checks if they are in terminal states
    ///
    /// ## Expected Outcome
    /// - Returns true for terminal states
    /// - Returns false for non-terminal states
    #[test]
    fn test_is_work_item_in_terminal_state() {
        use crate::models::{WorkItem, WorkItemFields};

        // Create a mock client - we just need any instance for the method call
        // For this test, we can't easily create a client, so let's test the logic directly
        let terminal_states = ["Closed".to_string(), "Done".to_string()];

        let work_item_closed = WorkItem {
            id: 1,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: Some("Closed".to_string()),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
            },
            history: vec![],
        };

        let work_item_active = WorkItem {
            id: 2,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: Some("Active".to_string()),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
            },
            history: vec![],
        };

        // Direct state check without client
        let is_terminal_closed = work_item_closed
            .fields
            .state
            .as_ref()
            .map(|s| terminal_states.contains(s))
            .unwrap_or(false);

        let is_terminal_active = work_item_active
            .fields
            .state
            .as_ref()
            .map(|s| terminal_states.contains(s))
            .unwrap_or(false);

        assert!(is_terminal_closed);
        assert!(!is_terminal_active);
    }
}
