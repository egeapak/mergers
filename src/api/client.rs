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

    /// # Filter PRs Without Merged Tag
    ///
    /// Tests filtering of PRs that don't have a "merged-" tag.
    ///
    /// ## Test Scenario
    /// - Creates PRs with various labels including "merged-" prefixed ones
    /// - Filters out PRs with merged tags
    ///
    /// ## Expected Outcome
    /// - PRs with "merged-" tags are filtered out
    /// - PRs without such tags are retained
    #[test]
    fn test_filter_prs_without_merged_tag() {
        use crate::models::{CreatedBy, Label, PullRequest};

        let pr_no_labels = PullRequest {
            id: 1,
            title: "PR without labels".to_string(),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels: None,
        };

        let pr_with_other_label = PullRequest {
            id: 2,
            title: "PR with other label".to_string(),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels: Some(vec![Label {
                name: "bug".to_string(),
            }]),
        };

        let pr_with_merged_tag = PullRequest {
            id: 3,
            title: "PR with merged tag".to_string(),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels: Some(vec![Label {
                name: "merged-v1.0".to_string(),
            }]),
        };

        let pr_with_mixed_labels = PullRequest {
            id: 4,
            title: "PR with mixed labels".to_string(),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels: Some(vec![
                Label {
                    name: "feature".to_string(),
                },
                Label {
                    name: "merged-hotfix".to_string(),
                },
            ]),
        };

        let prs = vec![
            pr_no_labels,
            pr_with_other_label,
            pr_with_merged_tag,
            pr_with_mixed_labels,
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, 1);
        assert_eq!(filtered[1].id, 2);
    }

    /// # Filter PRs Empty List
    ///
    /// Tests filtering with an empty list.
    ///
    /// ## Test Scenario
    /// - Provides an empty PR list
    ///
    /// ## Expected Outcome
    /// - Returns an empty list
    #[test]
    fn test_filter_prs_without_merged_tag_empty() {
        let prs: Vec<PullRequest> = vec![];
        let filtered = filter_prs_without_merged_tag(prs);
        assert!(filtered.is_empty());
    }

    /// # Filter PRs All Have Merged Tag
    ///
    /// Tests filtering when all PRs have merged tags.
    ///
    /// ## Test Scenario
    /// - All PRs have "merged-" prefixed labels
    ///
    /// ## Expected Outcome
    /// - Returns an empty list
    #[test]
    fn test_filter_prs_all_merged() {
        use crate::models::{CreatedBy, Label, PullRequest};

        let pr1 = PullRequest {
            id: 1,
            title: "PR 1".to_string(),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels: Some(vec![Label {
                name: "merged-v1".to_string(),
            }]),
        };

        let pr2 = PullRequest {
            id: 2,
            title: "PR 2".to_string(),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels: Some(vec![Label {
                name: "merged-v2".to_string(),
            }]),
        };

        let filtered = filter_prs_without_merged_tag(vec![pr1, pr2]);
        assert!(filtered.is_empty());
    }

    /// # Client Creation and Accessors
    ///
    /// Tests that the client can be created and accessor methods work.
    ///
    /// ## Test Scenario
    /// - Creates a client with test values
    /// - Verifies accessor methods return correct values
    ///
    /// ## Expected Outcome
    /// - All accessor methods return the values passed to the constructor
    #[test]
    fn test_client_creation_and_accessors() {
        let client = AzureDevOpsClient::new(
            "test-org".to_string(),
            "test-project".to_string(),
            "test-repo".to_string(),
            "test-pat".to_string(),
        )
        .unwrap();

        assert_eq!(client.organization(), "test-org");
        assert_eq!(client.project(), "test-project");
        assert_eq!(client.repository(), "test-repo");
    }

    /// # Client Creation with SecretString
    ///
    /// Tests client creation with SecretString PAT.
    ///
    /// ## Test Scenario
    /// - Creates a client using new_with_secret
    /// - Verifies the client is created successfully
    ///
    /// ## Expected Outcome
    /// - Client is created without errors
    #[test]
    fn test_client_creation_with_secret() {
        use secrecy::SecretString;

        let pat = SecretString::from("test-pat".to_string());
        let client = AzureDevOpsClient::new_with_secret(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            pat,
        )
        .unwrap();

        assert_eq!(client.organization(), "org");
    }

    /// # Analyze Work Items - All Terminal
    ///
    /// Tests analyze_work_items_for_pr when all work items are in terminal state.
    ///
    /// ## Test Scenario
    /// - Creates a PR with work items all in terminal states
    /// - Analyzes the work items
    ///
    /// ## Expected Outcome
    /// - Returns (true, empty vec) indicating all are terminal
    #[test]
    fn test_analyze_work_items_all_terminal() {
        use crate::models::{CreatedBy, PullRequestWithWorkItems, WorkItem, WorkItemFields};

        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![
            WorkItem {
                id: 1,
                fields: WorkItemFields {
                    title: Some("Item 1".to_string()),
                    state: Some("Closed".to_string()),
                    work_item_type: None,
                    assigned_to: None,
                    iteration_path: None,
                    description: None,
                    repro_steps: None,
                },
                history: vec![],
            },
            WorkItem {
                id: 2,
                fields: WorkItemFields {
                    title: Some("Item 2".to_string()),
                    state: Some("Done".to_string()),
                    work_item_type: None,
                    assigned_to: None,
                    iteration_path: None,
                    description: None,
                    repro_steps: None,
                },
                history: vec![],
            },
        ];

        let pr_with_items = PullRequestWithWorkItems {
            pr: PullRequest {
                id: 100,
                title: "Test PR".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: None,
                labels: None,
            },
            work_items,
            selected: false,
        };

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];
        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(all_terminal);
        assert!(non_terminal.is_empty());
    }

    /// # Analyze Work Items - Mixed States
    ///
    /// Tests analyze_work_items_for_pr with mixed terminal/non-terminal states.
    ///
    /// ## Test Scenario
    /// - Creates a PR with some work items in terminal state, some not
    /// - Analyzes the work items
    ///
    /// ## Expected Outcome
    /// - Returns (false, vec of non-terminal items)
    #[test]
    fn test_analyze_work_items_mixed() {
        use crate::models::{CreatedBy, PullRequestWithWorkItems, WorkItem, WorkItemFields};

        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![
            WorkItem {
                id: 1,
                fields: WorkItemFields {
                    title: Some("Closed Item".to_string()),
                    state: Some("Closed".to_string()),
                    work_item_type: None,
                    assigned_to: None,
                    iteration_path: None,
                    description: None,
                    repro_steps: None,
                },
                history: vec![],
            },
            WorkItem {
                id: 2,
                fields: WorkItemFields {
                    title: Some("Active Item".to_string()),
                    state: Some("Active".to_string()),
                    work_item_type: None,
                    assigned_to: None,
                    iteration_path: None,
                    description: None,
                    repro_steps: None,
                },
                history: vec![],
            },
        ];

        let pr_with_items = PullRequestWithWorkItems {
            pr: PullRequest {
                id: 100,
                title: "Test PR".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: None,
                labels: None,
            },
            work_items,
            selected: false,
        };

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];
        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(!all_terminal);
        assert_eq!(non_terminal.len(), 1);
        assert_eq!(non_terminal[0].id, 2);
    }

    /// # Analyze Work Items - Empty Work Items
    ///
    /// Tests analyze_work_items_for_pr with no work items.
    ///
    /// ## Test Scenario
    /// - Creates a PR with no work items
    /// - Analyzes the work items
    ///
    /// ## Expected Outcome
    /// - Returns (false, empty vec) - not "all terminal" if no items
    #[test]
    fn test_analyze_work_items_empty() {
        use crate::models::{CreatedBy, PullRequestWithWorkItems};

        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let pr_with_items = PullRequestWithWorkItems {
            pr: PullRequest {
                id: 100,
                title: "Test PR".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: None,
                labels: None,
            },
            work_items: vec![],
            selected: false,
        };

        let terminal_states = vec!["Closed".to_string()];
        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(!all_terminal); // Empty is not "all terminal"
        assert!(non_terminal.is_empty());
    }

    /// # Is Work Item In Terminal State - With Client
    ///
    /// Tests is_work_item_in_terminal_state method using actual client.
    ///
    /// ## Test Scenario
    /// - Creates client and work items
    /// - Tests terminal state checking
    ///
    /// ## Expected Outcome
    /// - Correctly identifies terminal and non-terminal states
    #[test]
    fn test_is_work_item_in_terminal_state_with_client() {
        use crate::models::{WorkItem, WorkItemFields};

        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let closed_item = WorkItem {
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

        let active_item = WorkItem {
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

        let no_state_item = WorkItem {
            id: 3,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: None,
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
            },
            history: vec![],
        };

        assert!(client.is_work_item_in_terminal_state(&closed_item, &terminal_states));
        assert!(!client.is_work_item_in_terminal_state(&active_item, &terminal_states));
        assert!(!client.is_work_item_in_terminal_state(&no_state_item, &terminal_states));
    }

    /// # Parse Terminal States Edge Cases
    ///
    /// Tests edge cases for parsing terminal states.
    ///
    /// ## Test Scenario
    /// - Various edge case inputs like extra whitespace, commas
    ///
    /// ## Expected Outcome
    /// - Handles edge cases gracefully
    #[test]
    fn test_parse_terminal_states_edge_cases() {
        // Extra whitespace
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("  Closed  ,  Done  "),
            vec!["Closed", "Done"]
        );

        // Single state
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed"),
            vec!["Closed"]
        );

        // Trailing comma
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed,Done,"),
            vec!["Closed", "Done"]
        );

        // Multiple commas (empty entries filtered)
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed,,Done"),
            vec!["Closed", "Done"]
        );
    }
}
