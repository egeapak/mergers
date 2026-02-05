//! Azure DevOps API client implementation using azure_devops_rust_api crate.
//!
//! This module provides a client for interacting with Azure DevOps APIs,
//! specifically for managing pull requests and work items in merge workflows.

use super::mappers::extract_work_item_id;
use crate::models::{
    MergeCommit, PullRequest, PullRequestWithWorkItems, RepoDetails, WorkItem, WorkItemHistory,
};
use crate::utils::parse_since_date;
use anyhow::{Context, Result};
use azure_devops_rust_api::{git, wit};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use secrecy::{ExposeSecret, SecretString};

/// Default maximum retries for backward compatibility (no longer used with azure_devops_rust_api).
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Type alias for state color cache: state_name -> (r, g, b)
type StateColorCache =
    std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, (u8, u8, u8)>>>;

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
    /// Cache of work item state colors: state_name -> (r, g, b)
    state_color_cache: StateColorCache,
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
        let ado_credential =
            azure_devops_rust_api::Credential::Pat(pat.expose_secret().to_string());

        let git_client = git::ClientBuilder::new(ado_credential.clone()).build();
        let wit_client = wit::ClientBuilder::new(ado_credential).build();

        Ok(Self {
            organization,
            project,
            repository,
            git_client,
            wit_client,
            state_color_cache: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        })
    }

    /// Creates a new client with pool configuration (backward compatibility).
    ///
    /// Note: Pool configuration is handled internally by azure_devops_rust_api.
    /// These parameters are accepted for API compatibility but not used.
    #[allow(unused_variables)]
    pub fn new_with_secret_and_pool_config(
        organization: String,
        project: String,
        repository: String,
        pat: SecretString,
        pool_max_idle_per_host: usize,
        pool_idle_timeout_secs: u64,
    ) -> Result<Self> {
        Self::new_with_secret(organization, project, repository, pat)
    }

    /// Creates a new client with full configuration (backward compatibility).
    ///
    /// Note: Pool and retry configuration is handled internally by azure_devops_rust_api.
    /// These parameters are accepted for API compatibility but not used.
    #[allow(unused_variables)]
    pub fn new_with_full_config(
        organization: String,
        project: String,
        repository: String,
        pat: SecretString,
        pool_max_idle_per_host: usize,
        pool_idle_timeout_secs: u64,
        max_retries: u32,
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

    /// Returns the max retries value (backward compatibility).
    ///
    /// Note: Retry logic is handled internally by azure_devops_rust_api.
    pub fn max_retries(&self) -> u32 {
        DEFAULT_MAX_RETRIES
    }

    /// Fetches all pull requests for a given branch using pagination.
    ///
    /// This method implements pagination to ensure all pull requests are retrieved.
    /// If `since` is provided, stops fetching when encountering PRs older than the specified date.
    #[must_use = "this returns the fetched pull requests which should be used"]
    #[tracing::instrument(skip(self), fields(dev_branch = %dev_branch))]
    pub async fn fetch_pull_requests(
        &self,
        dev_branch: &str,
        since: Option<&str>,
    ) -> Result<Vec<PullRequest>> {
        tracing::info!("Fetching pull requests for branch: {}", dev_branch);

        // Parse the since date if provided
        let since_date = if let Some(since_str) = since {
            tracing::debug!("Filtering PRs since: {}", since_str);
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
            tracing::debug!(
                "Fetching PR page: request #{}, skip={}, top={}",
                request_count,
                skip,
                top
            );

            if request_count > max_requests {
                tracing::error!(
                    "Exceeded maximum number of requests ({}), retrieved {} PRs",
                    max_requests,
                    all_prs.len()
                );
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
            tracing::debug!(
                "Retrieved {} PRs in this batch, {} total so far",
                fetched_count,
                all_prs.len()
            );

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
                        tracing::debug!("Reached date limit at PR {}", converted_pr.id);
                        reached_date_limit = true;
                        break;
                    }
                }
                all_prs.push(converted_pr);
            }

            if reached_date_limit || fetched_count < top as usize {
                tracing::debug!(
                    "Fetch complete: reached_date_limit={}, is_last_page={}",
                    reached_date_limit,
                    fetched_count < top as usize
                );
                break;
            }

            skip += top;
        }

        tracing::info!("Fetched {} total pull requests", all_prs.len());
        Ok(all_prs)
    }

    /// Fetches work items linked to a pull request.
    #[must_use = "this returns the fetched work items which should be used"]
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

    /// Fetches work items by a list of IDs directly.
    ///
    /// This is useful when you already have work item IDs and want to fetch
    /// their details without going through a PR reference.
    ///
    /// # Arguments
    ///
    /// * `ids` - Slice of work item IDs to fetch
    ///
    /// # Returns
    ///
    /// Vector of WorkItem objects for the requested IDs.
    #[must_use = "this returns the fetched work items which should be used"]
    pub async fn fetch_work_items_by_ids(&self, ids: &[i32]) -> Result<Vec<WorkItem>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let ids_str = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let work_items = self
            .wit_client
            .work_items_client()
            .list(&self.organization, &ids_str, &self.project)
            .fields("System.Title,System.State,System.WorkItemType,System.AssignedTo,System.IterationPath")
            .await
            .context("Failed to fetch work items by IDs")?;

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
    #[must_use = "this returns the merge commit which should be used"]
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
    #[must_use = "this operation can fail and the result should be checked"]
    #[tracing::instrument(skip(self))]
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
    #[must_use = "this operation can fail and the result should be checked"]
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
    #[must_use = "this returns the work item history which should be used"]
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

    /// Fetches state colors for a specific work item type.
    ///
    /// Returns a map of state name to hex color string (e.g., "007acc").
    #[must_use = "this returns the state colors which should be used"]
    pub async fn fetch_work_item_type_state_colors(
        &self,
        work_item_type: &str,
    ) -> Result<std::collections::HashMap<String, String>> {
        let states = self
            .wit_client
            .work_item_type_states_client()
            .list(&self.organization, &self.project, work_item_type)
            .await
            .context("Failed to fetch work item type state colors")?;

        let mut color_map = std::collections::HashMap::new();
        for state in states.value {
            if let (Some(name), Some(color)) = (state.name, state.color) {
                color_map.insert(name, color);
            }
        }

        Ok(color_map)
    }

    /// Fetches state colors for all work item types used by the given work items.
    ///
    /// Returns a nested map: work_item_type -> state_name -> hex_color
    pub async fn fetch_state_colors_for_work_items(
        &self,
        work_items: &[WorkItem],
    ) -> std::collections::HashMap<String, std::collections::HashMap<String, String>> {
        use std::collections::{HashMap, HashSet};

        // Collect unique work item types
        let work_item_types: HashSet<String> = work_items
            .iter()
            .filter_map(|wi| wi.fields.work_item_type.clone())
            .collect();

        let mut all_colors: HashMap<String, HashMap<String, String>> = HashMap::new();

        // Fetch colors for each work item type
        for wit_type in work_item_types {
            if let Ok(colors) = self.fetch_work_item_type_state_colors(&wit_type).await {
                all_colors.insert(wit_type, colors);
            }
        }

        all_colors
    }

    /// Enriches work items with their state colors from the API.
    ///
    /// This fetches state colors for all work item types and populates
    /// the `state_color` field on each work item as RGB tuples.
    pub async fn enrich_work_items_with_colors(&self, work_items: &mut [WorkItem]) {
        let color_map = self.fetch_state_colors_for_work_items(work_items).await;

        for work_item in work_items {
            if let (Some(wit_type), Some(state)) =
                (&work_item.fields.work_item_type, &work_item.fields.state)
                && let Some(type_colors) = color_map.get(wit_type)
                && let Some(hex_color) = type_colors.get(state)
                && let Some(rgb) = hex_to_rgb(hex_color)
            {
                work_item.fields.state_color = Some(rgb);
            }
        }
    }

    /// Fetches work items with their history for a PR, using parallel fetching.
    #[must_use = "this returns work items with history which should be used"]
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
    #[must_use = "this returns work items with history which should be used"]
    pub async fn fetch_work_items_with_history_for_pr(&self, pr_id: i32) -> Result<Vec<WorkItem>> {
        self.fetch_work_items_with_history_for_pr_parallel(pr_id, 10)
            .await
    }

    /// Fetches work items with history for multiple PRs in parallel.
    ///
    /// This method also enriches work items with state colors from the API.
    pub async fn fetch_work_items_for_prs_parallel(
        &self,
        prs: &[PullRequest],
        max_concurrent_prs: usize,
        max_concurrent_history: usize,
    ) -> Vec<PullRequestWithWorkItems> {
        // First, fetch all work items with history
        let mut results: Vec<PullRequestWithWorkItems> = stream::iter(prs.iter().cloned())
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
            .await;

        // Collect all work items to fetch their colors
        let all_work_items: Vec<WorkItem> = results
            .iter()
            .flat_map(|pr| pr.work_items.clone())
            .collect();

        // Fetch state colors for all work item types
        let color_map = self
            .fetch_state_colors_for_work_items(&all_work_items)
            .await;

        // Convert hex color map to RGB and store in flattened cache (state -> RGB)
        // States have the same colors across work item types
        let mut rgb_color_map: std::collections::HashMap<String, (u8, u8, u8)> =
            std::collections::HashMap::new();
        for state_colors in color_map.values() {
            for (state, hex_color) in state_colors {
                if let Some(rgb) = hex_to_rgb(hex_color) {
                    rgb_color_map.insert(state.clone(), rgb);
                }
            }
        }

        // Update the cache
        if let Ok(mut cache) = self.state_color_cache.write() {
            cache.extend(rgb_color_map);
        }

        // Apply colors to work items in each PR (converting hex to RGB)
        for pr_with_wi in &mut results {
            for work_item in &mut pr_with_wi.work_items {
                if let (Some(wit_type), Some(state)) =
                    (&work_item.fields.work_item_type, &work_item.fields.state)
                    && let Some(type_colors) = color_map.get(wit_type)
                    && let Some(hex_color) = type_colors.get(state)
                    && let Some(rgb) = hex_to_rgb(hex_color)
                {
                    work_item.fields.state_color = Some(rgb);
                }
            }
        }

        results
    }

    /// Gets the cached color for a work item state.
    ///
    /// Returns the RGB color tuple if available in cache, None otherwise.
    /// State colors are consistent across work item types.
    #[must_use]
    pub fn get_cached_state_color(&self, state: &str) -> Option<(u8, u8, u8)> {
        self.state_color_cache
            .read()
            .ok()
            .and_then(|cache| cache.get(state).copied())
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

/// Converts a hex color string (e.g., "007acc" or "#007acc") to an RGB tuple.
///
/// Returns None if the hex string is invalid.
fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Filters out pull requests that already have a "merged-" tag.
///
/// This is used to prevent re-processing PRs that have already been tagged
/// after a successful merge operation.
#[must_use]
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

/// Generic Azure DevOps client that uses trait objects for operations.
///
/// This struct enables dependency injection of mock implementations for testing.
/// It mirrors the functionality of `AzureDevOpsClient` but uses trait objects
/// instead of concrete types.
#[allow(dead_code)]
pub struct GenericAzureDevOpsClient<G, W>
where
    G: super::traits::PullRequestOperations
        + super::traits::PullRequestWorkItemsOperations
        + super::traits::RepositoryOperations,
    W: super::traits::WorkItemOperations + super::traits::WorkItemUpdatesOperations,
{
    organization: String,
    project: String,
    repository: String,
    git_ops: G,
    wit_ops: W,
}

#[allow(dead_code)]
impl<G, W> GenericAzureDevOpsClient<G, W>
where
    G: super::traits::PullRequestOperations
        + super::traits::PullRequestWorkItemsOperations
        + super::traits::RepositoryOperations,
    W: super::traits::WorkItemOperations + super::traits::WorkItemUpdatesOperations,
{
    /// Creates a new generic client with the provided operations.
    pub fn new(
        organization: String,
        project: String,
        repository: String,
        git_ops: G,
        wit_ops: W,
    ) -> Self {
        Self {
            organization,
            project,
            repository,
            git_ops,
            wit_ops,
        }
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
    pub async fn fetch_pull_requests(
        &self,
        dev_branch: &str,
        since: Option<&str>,
    ) -> Result<Vec<PullRequest>> {
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

            let prs = self
                .git_ops
                .get_pull_requests(
                    &self.organization,
                    &self.repository,
                    &self.project,
                    &target_ref,
                    "completed",
                    top,
                    skip,
                )
                .await
                .context("Failed to fetch pull requests")?;

            let fetched_count = prs.len();

            let mut reached_date_limit = false;
            for pr in prs {
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
        let refs = self
            .git_ops
            .list(&self.organization, &self.repository, pr_id, &self.project)
            .await
            .context("Failed to fetch work item references for PR")?;

        if refs.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<i32> = refs
            .iter()
            .filter_map(|r| r.url.as_ref().and_then(|url| extract_work_item_id(url)))
            .collect();

        if ids.is_empty() {
            return Ok(vec![]);
        }

        let ids_str = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let work_items = self
            .wit_ops
            .get_work_items(
                &self.organization,
                &ids_str,
                &self.project,
                "System.Title,System.State,System.WorkItemType,System.AssignedTo,System.IterationPath,System.Description,Microsoft.VSTS.TCM.ReproSteps",
            )
            .await
            .context("Failed to fetch work items")?;

        Ok(work_items.into_iter().map(WorkItem::from).collect())
    }

    /// Fetches repository details including SSH URL.
    pub async fn fetch_repo_details(&self) -> Result<RepoDetails> {
        let repo = self
            .git_ops
            .get_repository(&self.organization, &self.repository, &self.project)
            .await
            .context("Failed to fetch repository details")?;

        Ok(RepoDetails::from(repo))
    }

    /// Fetches the merge commit for a pull request.
    pub async fn fetch_pr_commit(&self, pr_id: i32) -> Result<MergeCommit> {
        let pr = self
            .git_ops
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
        self.git_ops
            .create_label(
                &self.organization,
                &self.repository,
                pr_id,
                &self.project,
                label,
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

        self.wit_ops
            .update_work_item(&self.organization, work_item_id, &self.project, patch)
            .await
            .context("Failed to update work item state")?;

        Ok(())
    }

    /// Fetches the revision history for a work item.
    pub async fn fetch_work_item_history(&self, work_item_id: i32) -> Result<Vec<WorkItemHistory>> {
        let updates = self
            .wit_ops
            .get_work_item_updates(&self.organization, work_item_id, &self.project)
            .await
            .context("Failed to fetch work item history")?;

        Ok(updates.into_iter().map(WorkItemHistory::from).collect())
    }

    /// Checks if a work item is in a terminal state.
    pub fn is_work_item_in_terminal_state(
        work_item: &WorkItem,
        terminal_states: &[String],
    ) -> bool {
        if let Some(state) = &work_item.fields.state {
            terminal_states.contains(state)
        } else {
            false
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreatedBy, Label, WorkItem, WorkItemFields};

    // ==================== Constants ====================

    /// # Default Max Retries Constant
    ///
    /// Tests that the DEFAULT_MAX_RETRIES constant has expected value.
    #[test]
    fn test_default_max_retries_constant() {
        assert_eq!(DEFAULT_MAX_RETRIES, 3);
    }

    // ==================== Hex to RGB Conversion ====================

    /// # Hex to RGB - Valid 6-character Hex
    ///
    /// Tests conversion of a valid 6-character hex string to RGB tuple.
    ///
    /// ## Test Scenario
    /// - Provides a standard 6-character hex color
    /// - Converts to RGB tuple
    ///
    /// ## Expected Outcome
    /// - Returns correct RGB values for the hex color
    #[test]
    fn test_hex_to_rgb_valid() {
        assert_eq!(super::hex_to_rgb("007acc"), Some((0, 122, 204)));
        assert_eq!(super::hex_to_rgb("ff0000"), Some((255, 0, 0)));
        assert_eq!(super::hex_to_rgb("00ff00"), Some((0, 255, 0)));
        assert_eq!(super::hex_to_rgb("0000ff"), Some((0, 0, 255)));
        assert_eq!(super::hex_to_rgb("ffffff"), Some((255, 255, 255)));
        assert_eq!(super::hex_to_rgb("000000"), Some((0, 0, 0)));
    }

    /// # Hex to RGB - With Hash Prefix
    ///
    /// Tests that hex strings with leading '#' are handled correctly.
    ///
    /// ## Test Scenario
    /// - Provides hex strings with '#' prefix
    /// - Converts to RGB tuple
    ///
    /// ## Expected Outcome
    /// - Hash is stripped and RGB values are correctly extracted
    #[test]
    fn test_hex_to_rgb_with_hash() {
        assert_eq!(super::hex_to_rgb("#007acc"), Some((0, 122, 204)));
        assert_eq!(super::hex_to_rgb("#FF5733"), Some((255, 87, 51)));
    }

    /// # Hex to RGB - Case Insensitive
    ///
    /// Tests that hex conversion is case-insensitive.
    ///
    /// ## Test Scenario
    /// - Provides hex strings with mixed case
    ///
    /// ## Expected Outcome
    /// - Both upper and lower case produce same RGB values
    #[test]
    fn test_hex_to_rgb_case_insensitive() {
        assert_eq!(super::hex_to_rgb("AABBCC"), Some((170, 187, 204)));
        assert_eq!(super::hex_to_rgb("aabbcc"), Some((170, 187, 204)));
        assert_eq!(super::hex_to_rgb("AaBbCc"), Some((170, 187, 204)));
    }

    /// # Hex to RGB - Invalid Length
    ///
    /// Tests that hex strings with invalid length return None.
    ///
    /// ## Test Scenario
    /// - Provides hex strings that are too short or too long
    ///
    /// ## Expected Outcome
    /// - Returns None for invalid lengths
    #[test]
    fn test_hex_to_rgb_invalid_length() {
        assert_eq!(super::hex_to_rgb(""), None);
        assert_eq!(super::hex_to_rgb("fff"), None);
        assert_eq!(super::hex_to_rgb("ffff"), None);
        assert_eq!(super::hex_to_rgb("fffff"), None);
        assert_eq!(super::hex_to_rgb("fffffff"), None);
    }

    /// # Hex to RGB - Invalid Characters
    ///
    /// Tests that hex strings with invalid characters return None.
    ///
    /// ## Test Scenario
    /// - Provides hex strings with non-hex characters
    ///
    /// ## Expected Outcome
    /// - Returns None for invalid hex characters
    #[test]
    fn test_hex_to_rgb_invalid_characters() {
        assert_eq!(super::hex_to_rgb("gggggg"), None);
        assert_eq!(super::hex_to_rgb("00gg00"), None);
        assert_eq!(super::hex_to_rgb("zzzzzz"), None);
    }

    // ==================== Client Creation ====================

    /// # Client Creation with String PAT
    ///
    /// Tests client creation with a plain string PAT.
    #[test]
    fn test_client_creation_with_string_pat() {
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

    /// # Client Creation with SecretString PAT
    ///
    /// Tests client creation with a SecretString PAT.
    #[test]
    fn test_client_creation_with_secret_string() {
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
        assert_eq!(client.project(), "proj");
        assert_eq!(client.repository(), "repo");
    }

    /// # Client Creation with Pool Config
    ///
    /// Tests backward-compatible constructor with pool configuration.
    #[test]
    fn test_client_creation_with_pool_config() {
        use secrecy::SecretString;

        let pat = SecretString::from("test-pat".to_string());
        let client = AzureDevOpsClient::new_with_secret_and_pool_config(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            pat,
            10, // pool_max_idle_per_host
            30, // pool_idle_timeout_secs
        )
        .unwrap();

        assert_eq!(client.organization(), "org");
        assert_eq!(client.project(), "proj");
        assert_eq!(client.repository(), "repo");
    }

    /// # Client Creation with Full Config
    ///
    /// Tests backward-compatible constructor with full configuration.
    #[test]
    fn test_client_creation_with_full_config() {
        use secrecy::SecretString;

        let pat = SecretString::from("test-pat".to_string());
        let client = AzureDevOpsClient::new_with_full_config(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            pat,
            10, // pool_max_idle_per_host
            30, // pool_idle_timeout_secs
            5,  // max_retries
        )
        .unwrap();

        assert_eq!(client.organization(), "org");
        assert_eq!(client.project(), "proj");
        assert_eq!(client.repository(), "repo");
    }

    // ==================== Accessor Methods ====================

    /// # Max Retries Accessor
    ///
    /// Tests backward-compatible max_retries accessor.
    #[test]
    fn test_max_retries_accessor() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        assert_eq!(client.max_retries(), 3);
    }

    /// # All Accessor Methods
    ///
    /// Tests all accessor methods return correct values.
    #[test]
    fn test_all_accessor_methods() {
        let client = AzureDevOpsClient::new(
            "my-organization".to_string(),
            "my-project".to_string(),
            "my-repository".to_string(),
            "my-pat".to_string(),
        )
        .unwrap();

        assert_eq!(client.organization(), "my-organization");
        assert_eq!(client.project(), "my-project");
        assert_eq!(client.repository(), "my-repository");
        assert_eq!(client.max_retries(), 3);
    }

    // ==================== Parse Terminal States ====================

    /// # Parse Terminal States - Basic
    ///
    /// Tests basic parsing of comma-separated terminal states.
    #[test]
    fn test_parse_terminal_states_basic() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed,Done"),
            vec!["Closed", "Done"]
        );
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed, Done, Merged"),
            vec!["Closed", "Done", "Merged"]
        );
    }

    /// # Parse Terminal States - Empty
    ///
    /// Tests parsing of empty string.
    #[test]
    fn test_parse_terminal_states_empty() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states(""),
            Vec::<String>::new()
        );
    }

    /// # Parse Terminal States - Whitespace
    ///
    /// Tests parsing with extra whitespace.
    #[test]
    fn test_parse_terminal_states_whitespace() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("  Closed  ,  Done  "),
            vec!["Closed", "Done"]
        );
    }

    /// # Parse Terminal States - Single State
    ///
    /// Tests parsing of single state.
    #[test]
    fn test_parse_terminal_states_single() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed"),
            vec!["Closed"]
        );
    }

    /// # Parse Terminal States - Trailing Comma
    ///
    /// Tests parsing with trailing comma.
    #[test]
    fn test_parse_terminal_states_trailing_comma() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed,Done,"),
            vec!["Closed", "Done"]
        );
    }

    /// # Parse Terminal States - Multiple Commas
    ///
    /// Tests parsing with multiple consecutive commas.
    #[test]
    fn test_parse_terminal_states_multiple_commas() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("Closed,,Done"),
            vec!["Closed", "Done"]
        );
    }

    /// # Parse Terminal States - Only Whitespace
    ///
    /// Tests parsing with only whitespace entries.
    #[test]
    fn test_parse_terminal_states_only_whitespace() {
        assert_eq!(
            AzureDevOpsClient::parse_terminal_states("  ,  ,  "),
            Vec::<String>::new()
        );
    }

    // ==================== Is Work Item In Terminal State ====================

    /// # Is Work Item Terminal - Closed State
    ///
    /// Tests terminal state check for closed work item.
    #[test]
    fn test_is_work_item_in_terminal_state_closed() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let work_item = WorkItem {
            id: 1,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: Some("Closed".to_string()),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                state_color: None,
            },
            history: vec![],
        };

        assert!(client.is_work_item_in_terminal_state(&work_item, &terminal_states));
    }

    /// # Is Work Item Terminal - Done State
    ///
    /// Tests terminal state check for done work item.
    #[test]
    fn test_is_work_item_in_terminal_state_done() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let work_item = WorkItem {
            id: 1,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: Some("Done".to_string()),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                state_color: None,
            },
            history: vec![],
        };

        assert!(client.is_work_item_in_terminal_state(&work_item, &terminal_states));
    }

    /// # Is Work Item Terminal - Active State
    ///
    /// Tests terminal state check for non-terminal work item.
    #[test]
    fn test_is_work_item_in_terminal_state_active() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let work_item = WorkItem {
            id: 1,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: Some("Active".to_string()),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                state_color: None,
            },
            history: vec![],
        };

        assert!(!client.is_work_item_in_terminal_state(&work_item, &terminal_states));
    }

    /// # Is Work Item Terminal - No State
    ///
    /// Tests terminal state check when work item has no state.
    #[test]
    fn test_is_work_item_in_terminal_state_no_state() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let work_item = WorkItem {
            id: 1,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: None,
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                state_color: None,
            },
            history: vec![],
        };

        assert!(!client.is_work_item_in_terminal_state(&work_item, &terminal_states));
    }

    /// # Is Work Item Terminal - Empty Terminal States
    ///
    /// Tests terminal state check with empty terminal states list.
    #[test]
    fn test_is_work_item_in_terminal_state_empty_list() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let terminal_states: Vec<String> = vec![];

        let work_item = WorkItem {
            id: 1,
            fields: WorkItemFields {
                title: Some("Test".to_string()),
                state: Some("Closed".to_string()),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                state_color: None,
            },
            history: vec![],
        };

        assert!(!client.is_work_item_in_terminal_state(&work_item, &terminal_states));
    }

    // ==================== Analyze Work Items For PR ====================

    fn create_test_pr_with_work_items(work_items: Vec<WorkItem>) -> PullRequestWithWorkItems {
        PullRequestWithWorkItems {
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
        }
    }

    fn create_work_item(id: i32, state: Option<&str>) -> WorkItem {
        WorkItem {
            id,
            fields: WorkItemFields {
                title: Some(format!("Work Item {}", id)),
                state: state.map(String::from),
                work_item_type: None,
                assigned_to: None,
                iteration_path: None,
                description: None,
                repro_steps: None,
                state_color: None,
            },
            history: vec![],
        }
    }

    /// # Analyze Work Items - All Terminal
    ///
    /// Tests analyze_work_items_for_pr when all work items are terminal.
    #[test]
    fn test_analyze_work_items_all_terminal() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![
            create_work_item(1, Some("Closed")),
            create_work_item(2, Some("Done")),
        ];

        let pr_with_items = create_test_pr_with_work_items(work_items);
        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(all_terminal);
        assert!(non_terminal.is_empty());
    }

    /// # Analyze Work Items - Mixed States
    ///
    /// Tests analyze_work_items_for_pr with mixed terminal/non-terminal states.
    #[test]
    fn test_analyze_work_items_mixed() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![
            create_work_item(1, Some("Closed")),
            create_work_item(2, Some("Active")),
            create_work_item(3, Some("Done")),
        ];

        let pr_with_items = create_test_pr_with_work_items(work_items);
        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(!all_terminal);
        assert_eq!(non_terminal.len(), 1);
        assert_eq!(non_terminal[0].id, 2);
    }

    /// # Analyze Work Items - None Terminal
    ///
    /// Tests analyze_work_items_for_pr when no work items are terminal.
    #[test]
    fn test_analyze_work_items_none_terminal() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![
            create_work_item(1, Some("Active")),
            create_work_item(2, Some("In Progress")),
        ];

        let pr_with_items = create_test_pr_with_work_items(work_items);
        let terminal_states = vec!["Closed".to_string(), "Done".to_string()];

        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(!all_terminal);
        assert_eq!(non_terminal.len(), 2);
    }

    /// # Analyze Work Items - Empty Work Items
    ///
    /// Tests analyze_work_items_for_pr with no work items.
    #[test]
    fn test_analyze_work_items_empty() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let pr_with_items = create_test_pr_with_work_items(vec![]);
        let terminal_states = vec!["Closed".to_string()];

        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(!all_terminal); // Empty is not "all terminal"
        assert!(non_terminal.is_empty());
    }

    /// # Analyze Work Items - With None States
    ///
    /// Tests analyze_work_items_for_pr when some work items have no state.
    #[test]
    fn test_analyze_work_items_with_none_states() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![
            create_work_item(1, Some("Closed")),
            create_work_item(2, None),
        ];

        let pr_with_items = create_test_pr_with_work_items(work_items);
        let terminal_states = vec!["Closed".to_string()];

        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(!all_terminal);
        assert_eq!(non_terminal.len(), 1);
        assert_eq!(non_terminal[0].id, 2);
    }

    /// # Analyze Work Items - Single Terminal
    ///
    /// Tests analyze_work_items_for_pr with single terminal work item.
    #[test]
    fn test_analyze_work_items_single_terminal() {
        let client = AzureDevOpsClient::new(
            "org".to_string(),
            "proj".to_string(),
            "repo".to_string(),
            "pat".to_string(),
        )
        .unwrap();

        let work_items = vec![create_work_item(1, Some("Closed"))];

        let pr_with_items = create_test_pr_with_work_items(work_items);
        let terminal_states = vec!["Closed".to_string()];

        let (all_terminal, non_terminal) =
            client.analyze_work_items_for_pr(&pr_with_items, &terminal_states);

        assert!(all_terminal);
        assert!(non_terminal.is_empty());
    }

    // ==================== Filter PRs Without Merged Tag ====================

    fn create_test_pr(id: i32, labels: Option<Vec<Label>>) -> PullRequest {
        PullRequest {
            id,
            title: format!("PR {}", id),
            closed_date: None,
            created_by: CreatedBy {
                display_name: "Test".to_string(),
            },
            last_merge_commit: None,
            labels,
        }
    }

    /// # Filter PRs - No Labels
    ///
    /// Tests filtering PRs without any labels.
    #[test]
    fn test_filter_prs_no_labels() {
        let prs = vec![create_test_pr(1, None), create_test_pr(2, None)];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 2);
    }

    /// # Filter PRs - Empty Labels
    ///
    /// Tests filtering PRs with empty label vectors.
    #[test]
    fn test_filter_prs_empty_labels() {
        let prs = vec![
            create_test_pr(1, Some(vec![])),
            create_test_pr(2, Some(vec![])),
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 2);
    }

    /// # Filter PRs - Non-Merged Labels
    ///
    /// Tests filtering PRs with labels that don't start with "merged-".
    #[test]
    fn test_filter_prs_non_merged_labels() {
        let prs = vec![
            create_test_pr(
                1,
                Some(vec![Label {
                    name: "bug".to_string(),
                }]),
            ),
            create_test_pr(
                2,
                Some(vec![Label {
                    name: "feature".to_string(),
                }]),
            ),
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 2);
    }

    /// # Filter PRs - With Merged Tags
    ///
    /// Tests filtering PRs with "merged-" prefixed labels.
    #[test]
    fn test_filter_prs_with_merged_tags() {
        let prs = vec![
            create_test_pr(
                1,
                Some(vec![Label {
                    name: "merged-v1.0".to_string(),
                }]),
            ),
            create_test_pr(
                2,
                Some(vec![Label {
                    name: "merged-hotfix".to_string(),
                }]),
            ),
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert!(filtered.is_empty());
    }

    /// # Filter PRs - Mixed Labels
    ///
    /// Tests filtering with mix of merged and non-merged labels.
    #[test]
    fn test_filter_prs_mixed_labels() {
        let prs = vec![
            create_test_pr(1, None),
            create_test_pr(
                2,
                Some(vec![Label {
                    name: "bug".to_string(),
                }]),
            ),
            create_test_pr(
                3,
                Some(vec![Label {
                    name: "merged-v1.0".to_string(),
                }]),
            ),
            create_test_pr(
                4,
                Some(vec![
                    Label {
                        name: "feature".to_string(),
                    },
                    Label {
                        name: "merged-hotfix".to_string(),
                    },
                ]),
            ),
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, 1);
        assert_eq!(filtered[1].id, 2);
    }

    /// # Filter PRs - Empty List
    ///
    /// Tests filtering an empty list.
    #[test]
    fn test_filter_prs_empty_list() {
        let prs: Vec<PullRequest> = vec![];
        let filtered = filter_prs_without_merged_tag(prs);
        assert!(filtered.is_empty());
    }

    /// # Filter PRs - Merged Prefix Only
    ///
    /// Tests that "merged" without hyphen is not filtered.
    #[test]
    fn test_filter_prs_merged_without_hyphen() {
        let prs = vec![create_test_pr(
            1,
            Some(vec![Label {
                name: "merged".to_string(),
            }]),
        )];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 1);
    }

    /// # Filter PRs - Merged At End
    ///
    /// Tests that labels ending with "merged-" are not filtered incorrectly.
    #[test]
    fn test_filter_prs_merged_at_end() {
        let prs = vec![create_test_pr(
            1,
            Some(vec![Label {
                name: "not-merged-".to_string(),
            }]),
        )];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 1);
    }

    /// # Filter PRs - Multiple Labels With One Merged
    ///
    /// Tests PR with multiple labels where one is merged.
    #[test]
    fn test_filter_prs_multiple_labels_one_merged() {
        let prs = vec![create_test_pr(
            1,
            Some(vec![
                Label {
                    name: "bug".to_string(),
                },
                Label {
                    name: "priority-high".to_string(),
                },
                Label {
                    name: "merged-v2.0".to_string(),
                },
            ]),
        )];

        let filtered = filter_prs_without_merged_tag(prs);

        assert!(filtered.is_empty());
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
                state_color: None,
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
                state_color: None,
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
                state_color: None,
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

    // ==================== Async Tests with Mocks ====================

    mod async_tests {
        use super::*;
        use crate::api::traits::mocks::{MockGitOperations, MockWitOperations};
        use azure_devops_rust_api::git::models as git_models;
        use azure_devops_rust_api::wit::models as wit_models;

        /// Helper to create a minimal TeamProjectReference for testing
        fn create_test_project_ref() -> git_models::TeamProjectReference {
            git_models::TeamProjectReference {
                abbreviation: None,
                default_team_image_url: None,
                description: None,
                id: None,
                last_update_time: None,
                name: "test-project".to_string(),
                revision: None,
                state: None,
                url: None,
                visibility: git_models::team_project_reference::Visibility::Private,
            }
        }

        /// Helper to create a minimal GitRepository for testing
        fn create_test_repository(ssh_url: Option<String>) -> git_models::GitRepository {
            git_models::GitRepository {
                links: None,
                default_branch: None,
                id: "test-repo-id".to_string(),
                is_disabled: None,
                is_fork: None,
                is_in_maintenance: None,
                name: "test-repo".to_string(),
                parent_repository: None,
                project: create_test_project_ref(),
                remote_url: None,
                size: None,
                ssh_url,
                url: "https://test.url".to_string(),
                valid_remote_urls: vec![],
                web_url: None,
            }
        }

        fn create_mock_git_pull_request(id: i32, title: &str) -> git_models::GitPullRequest {
            let identity_ref = git_models::IdentityRef {
                graph_subject_base: git_models::GraphSubjectBase {
                    descriptor: None,
                    display_name: Some("Test User".to_string()),
                    url: None,
                    links: None,
                },
                directory_alias: None,
                id: String::new(),
                image_url: None,
                inactive: None,
                is_aad_identity: None,
                is_container: None,
                is_deleted_in_origin: None,
                profile_url: None,
                unique_name: None,
            };

            let last_merge_commit = git_models::GitCommitRef {
                commit_id: Some("abc123".to_string()),
                url: None,
                author: None,
                change_counts: None,
                changes: vec![],
                comment: None,
                comment_truncated: None,
                commit_too_many_changes: None,
                committer: None,
                links: None,
                parents: vec![],
                push: None,
                remote_url: None,
                statuses: vec![],
                work_items: vec![],
            };

            git_models::GitPullRequest {
                links: None,
                artifact_id: None,
                auto_complete_set_by: None,
                closed_by: None,
                closed_date: Some(time::OffsetDateTime::now_utc()),
                code_review_id: None,
                commits: vec![],
                completion_options: None,
                completion_queue_time: None,
                created_by: identity_ref,
                creation_date: time::OffsetDateTime::now_utc(),
                description: None,
                fork_source: None,
                has_multiple_merge_bases: None,
                is_draft: false,
                labels: vec![],
                last_merge_commit: Some(last_merge_commit),
                last_merge_source_commit: None,
                last_merge_target_commit: None,
                merge_failure_message: None,
                merge_failure_type: None,
                merge_id: None,
                merge_options: None,
                merge_status: None,
                pull_request_id: id,
                remote_url: None,
                repository: create_test_repository(None),
                reviewers: vec![],
                source_ref_name: "refs/heads/feature".to_string(),
                status: git_models::git_pull_request::Status::Active,
                supports_iterations: None,
                target_ref_name: "refs/heads/main".to_string(),
                title: Some(title.to_string()),
                url: "https://test.url".to_string(),
                work_item_refs: vec![],
            }
        }

        fn create_mock_git_repository(ssh_url: Option<&str>) -> git_models::GitRepository {
            git_models::GitRepository {
                links: None,
                default_branch: Some("refs/heads/main".to_string()),
                id: "test-repo-id".to_string(),
                is_disabled: None,
                is_fork: None,
                is_in_maintenance: None,
                name: "test-repo".to_string(),
                parent_repository: None,
                project: create_test_project_ref(),
                remote_url: None,
                size: None,
                ssh_url: ssh_url.map(String::from),
                url: "https://test.url".to_string(),
                valid_remote_urls: vec![],
                web_url: None,
            }
        }

        fn create_mock_work_item(id: i32, title: &str, state: &str) -> wit_models::WorkItem {
            let fields = serde_json::json!({
                "System.Title": title,
                "System.State": state,
                "System.WorkItemType": "User Story"
            });

            wit_models::WorkItem {
                work_item_tracking_resource: wit_models::WorkItemTrackingResource {
                    work_item_tracking_resource_reference:
                        wit_models::WorkItemTrackingResourceReference { url: String::new() },
                    links: None,
                },
                comment_version_ref: None,
                id,
                rev: None,
                fields,
                relations: vec![],
            }
        }

        fn create_mock_resource_ref(url: &str) -> git_models::ResourceRef {
            git_models::ResourceRef {
                id: None,
                url: Some(url.to_string()),
            }
        }

        /// # Fetch Pull Requests - Empty Result
        ///
        /// Tests fetch_pull_requests when no PRs are returned.
        #[tokio::test]
        async fn test_generic_client_fetch_pull_requests_empty() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            git_ops
                .pr_ops
                .set_get_pull_requests_response(Ok(vec![]))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_pull_requests("main", None).await;
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        /// # Fetch Pull Requests - With Results
        ///
        /// Tests fetch_pull_requests when PRs are returned.
        #[tokio::test]
        async fn test_generic_client_fetch_pull_requests_with_results() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let mock_prs = vec![
                create_mock_git_pull_request(1, "PR 1"),
                create_mock_git_pull_request(2, "PR 2"),
            ];

            git_ops
                .pr_ops
                .set_get_pull_requests_response(Ok(mock_prs))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_pull_requests("main", None).await;
            assert!(result.is_ok());
            let prs = result.unwrap();
            assert_eq!(prs.len(), 2);
            assert_eq!(prs[0].id, 1);
            assert_eq!(prs[1].id, 2);
        }

        /// # Fetch Repository Details
        ///
        /// Tests fetch_repo_details returns correct data.
        #[tokio::test]
        async fn test_generic_client_fetch_repo_details() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let mock_repo = create_mock_git_repository(Some(
                "git@ssh.dev.azure.com:v3/test-org/test-project/test-repo",
            ));

            git_ops
                .repo_ops
                .set_get_repository_response(Ok(mock_repo))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_repo_details().await;
            assert!(result.is_ok());
            let repo = result.unwrap();
            assert!(!repo.ssh_url.is_empty());
            assert!(repo.ssh_url.contains("ssh.dev.azure.com"));
        }

        /// # Fetch Repository Details - No SSH URL
        ///
        /// Tests fetch_repo_details when SSH URL is not set.
        #[tokio::test]
        async fn test_generic_client_fetch_repo_details_no_ssh() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let mock_repo = create_mock_git_repository(None);

            git_ops
                .repo_ops
                .set_get_repository_response(Ok(mock_repo))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_repo_details().await;
            assert!(result.is_ok());
            let repo = result.unwrap();
            assert!(repo.ssh_url.is_empty()); // ssh_url defaults to empty string when None
        }

        /// # Fetch PR Commit
        ///
        /// Tests fetch_pr_commit returns merge commit info.
        #[tokio::test]
        async fn test_generic_client_fetch_pr_commit() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let mock_pr = create_mock_git_pull_request(123, "Test PR");

            git_ops
                .pr_ops
                .set_get_pull_request_response(Ok(mock_pr))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_pr_commit(123).await;
            assert!(result.is_ok());
            let commit = result.unwrap();
            assert_eq!(commit.commit_id, "abc123");
        }

        /// # Add Label to PR
        ///
        /// Tests add_label_to_pr calls the API correctly.
        #[tokio::test]
        async fn test_generic_client_add_label_to_pr() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.add_label_to_pr(123, "merged-v1.0").await;
            assert!(result.is_ok());

            // Verify the label was set
            let label_called = *client.git_ops.pr_ops.create_label_called.lock().await;
            let label_name = client.git_ops.pr_ops.last_label_name.lock().await.clone();
            assert!(label_called);
            assert_eq!(label_name, Some("merged-v1.0".to_string()));
        }

        /// # Fetch Work Items for PR - Empty
        ///
        /// Tests fetch_work_items_for_pr when no work items are linked.
        #[tokio::test]
        async fn test_generic_client_fetch_work_items_empty() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            git_ops
                .pr_work_items_ops
                .set_list_response(Ok(vec![]))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_work_items_for_pr(123).await;
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        /// # Fetch Work Items for PR - With Results
        ///
        /// Tests fetch_work_items_for_pr when work items are linked.
        #[tokio::test]
        async fn test_generic_client_fetch_work_items_with_results() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let refs = vec![
                create_mock_resource_ref(
                    "https://dev.azure.com/test-org/test-project/_apis/wit/workItems/101",
                ),
                create_mock_resource_ref(
                    "https://dev.azure.com/test-org/test-project/_apis/wit/workItems/102",
                ),
            ];

            let work_items = vec![
                create_mock_work_item(101, "Feature 1", "Active"),
                create_mock_work_item(102, "Bug 1", "Closed"),
            ];

            git_ops.pr_work_items_ops.set_list_response(Ok(refs)).await;
            wit_ops
                .work_item_ops
                .set_list_response(Ok(work_items))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_work_items_for_pr(123).await;
            assert!(result.is_ok());
            let items = result.unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, 101);
            assert_eq!(items[1].id, 102);
        }

        /// # Update Work Item State
        ///
        /// Tests update_work_item_state calls the API correctly.
        #[tokio::test]
        async fn test_generic_client_update_work_item_state() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let updated_item = create_mock_work_item(101, "Test Item", "Closed");
            wit_ops
                .work_item_ops
                .set_update_response(Ok(updated_item))
                .await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.update_work_item_state(101, "Closed").await;
            assert!(result.is_ok());

            // Verify update was called
            let update_called = *client.wit_ops.work_item_ops.update_called.lock().await;
            let last_state = client
                .wit_ops
                .work_item_ops
                .last_update_state
                .lock()
                .await
                .clone();
            assert!(update_called);
            assert_eq!(last_state, Some("Closed".to_string()));
        }

        /// # Fetch Work Item History - Empty
        ///
        /// Tests fetch_work_item_history when no updates exist.
        #[tokio::test]
        async fn test_generic_client_fetch_work_item_history_empty() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            wit_ops.updates_ops.set_list_response(Ok(vec![])).await;

            let client = GenericAzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                git_ops,
                wit_ops,
            );

            let result = client.fetch_work_item_history(101).await;
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        /// # GenericClient Accessors
        ///
        /// Tests that accessor methods return correct values.
        #[tokio::test]
        async fn test_generic_client_accessors() {
            let git_ops = MockGitOperations::new();
            let wit_ops = MockWitOperations::new();

            let client = GenericAzureDevOpsClient::new(
                "my-org".to_string(),
                "my-project".to_string(),
                "my-repo".to_string(),
                git_ops,
                wit_ops,
            );

            assert_eq!(client.organization(), "my-org");
            assert_eq!(client.project(), "my-project");
            assert_eq!(client.repository(), "my-repo");
        }

        /// # Is Work Item In Terminal State - Static Method
        ///
        /// Tests the static is_work_item_in_terminal_state method.
        #[test]
        fn test_generic_client_is_work_item_in_terminal_state() {
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
                    state_color: None,
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
                    state_color: None,
                },
                history: vec![],
            };

            assert!(GenericAzureDevOpsClient::<
                MockGitOperations,
                MockWitOperations,
            >::is_work_item_in_terminal_state(
                &closed_item, &terminal_states
            ));
            assert!(!GenericAzureDevOpsClient::<
                MockGitOperations,
                MockWitOperations,
            >::is_work_item_in_terminal_state(
                &active_item, &terminal_states
            ));
        }

        /// # Parse Terminal States - Static Method
        ///
        /// Tests the static parse_terminal_states method.
        #[test]
        fn test_generic_client_parse_terminal_states() {
            assert_eq!(
                GenericAzureDevOpsClient::<MockGitOperations, MockWitOperations>::parse_terminal_states("Closed,Done"),
                vec!["Closed", "Done"]
            );
        }
    }
}
