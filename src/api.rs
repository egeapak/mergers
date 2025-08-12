use anyhow::{Context, Result};
use base64::Engine;
use reqwest::{Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use chrono::{DateTime, Utc};

use crate::models::{PullRequest, RepoDetails, WorkItem, WorkItemHistory, WorkItemRef};
use crate::utils::parse_since_date;

#[derive(Clone)]
pub struct AzureDevOpsClient {
    client: Client,
    organization: String,
    project: String,
    repository: String,
}

impl AzureDevOpsClient {
    pub fn new(
        organization: String,
        project: String,
        repository: String,
        pat: String,
    ) -> Result<Self> {
        let client = Client::builder()
            .default_headers({
                let mut headers = HeaderMap::new();
                let auth_value =
                    base64::engine::general_purpose::STANDARD.encode(format!(":{}", pat));
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    reqwest::header::HeaderValue::from_str(&format!("Basic {}", auth_value))?,
                );
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    reqwest::header::HeaderValue::from_static("application/json"),
                );
                headers
            })
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            organization,
            project,
            repository,
        })
    }

    /// Fetches all pull requests for a given branch using pagination.
    ///
    /// This method implements pagination to ensure all pull requests are retrieved,
    /// regardless of the total number. The Azure DevOps API has default limits on
    /// the number of items returned per request, so we use $top and $skip parameters
    /// to fetch all pages until no more results are available.
    /// 
    /// If `since` is provided, stops fetching when encountering PRs older than the specified date.
    pub async fn fetch_pull_requests(&self, dev_branch: &str, since: Option<&str>) -> Result<Vec<PullRequest>> {
        let mut all_prs = Vec::new();
        let mut skip = 0;
        let top = 100; // Number of PRs to fetch per request
        let max_requests = 100; // Safety limit to prevent infinite loops
        let mut request_count = 0;

        // Parse the since date if provided
        let since_date = if let Some(since_str) = since {
            Some(parse_since_date(since_str).context("Failed to parse since date")?)
        } else {
            None
        };

        #[derive(Deserialize)]
        struct PullRequestsResponse {
            value: Vec<PullRequest>,
        }

        loop {
            request_count += 1;
            if request_count > max_requests {
                anyhow::bail!(
                    "Exceeded maximum number of requests ({}) while fetching pull requests. Retrieved {} PRs so far.",
                    max_requests,
                    all_prs.len()
                );
            }

            let url = format!(
                "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullrequests?searchCriteria.targetRefName=refs/heads/{}&searchCriteria.status=completed&api-version=7.0&$expand=lastMergeCommit&$top={}&$skip={}",
                self.organization, self.project, self.repository, dev_branch, top, skip
            );

            let response = self
                .client
                .get(&url)
                .send()
                .await
                .context("Failed to fetch pull requests")?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await?;
                anyhow::bail!("API request failed with status {}: {}", status, text);
            }

            let text = response.text().await?;
            let prs: PullRequestsResponse = serde_json::from_str(&text)
                .with_context(|| format!("Failed to parse PR response: {}", text))?;

            let fetched_count = prs.value.len();
            
            // Filter PRs by date if since_date is specified
            let mut filtered_prs = Vec::new();
            let mut reached_date_limit = false;
            
            for pr in prs.value {
                if let Some(since_dt) = since_date {
                    if let Some(closed_date_str) = &pr.closed_date {
                        if let Ok(closed_date) = DateTime::parse_from_rfc3339(closed_date_str) {
                            let closed_date_utc = closed_date.with_timezone(&Utc);
                            if closed_date_utc < since_dt {
                                reached_date_limit = true;
                                break;
                            }
                        }
                    }
                }
                filtered_prs.push(pr);
            }
            
            all_prs.extend(filtered_prs);
            
            // If we reached the date limit, stop fetching
            if reached_date_limit {
                break;
            }

            // If we received fewer PRs than requested, we've reached the end
            if fetched_count < top {
                break;
            }

            skip += top;
        }

        // Total PRs fetched: all_prs.len(), using request_count API requests

        Ok(all_prs)
    }

    pub async fn fetch_work_items_for_pr(&self, pr_id: i32) -> Result<Vec<WorkItem>> {
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullRequests/{}/workitems?api-version=7.0",
            self.organization, self.project, self.repository, pr_id
        );

        let response = self.client.get(&url).send().await?;

        #[derive(Deserialize)]
        struct WorkItemsResponse {
            value: Vec<WorkItemRef>,
        }

        let work_item_refs: WorkItemsResponse = response.json().await?;

        if work_item_refs.value.is_empty() {
            return Ok(Vec::new());
        }

        // Use batch API to get work items with specific fields
        let work_item_ids: Vec<String> = work_item_refs
            .value
            .iter()
            .map(|wi| wi.id.clone())
            .collect();
        let ids_param = work_item_ids.join(",");

        let batch_url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems?ids={}&fields=System.Title,System.State,System.WorkItemType,System.AssignedTo,System.AreaPath,System.IterationPath,System.Description,Microsoft.VSTS.TCM.ReproSteps,System.CreatedDate&api-version=7.0",
            self.organization, self.project, ids_param
        );

        let batch_response = self.client.get(&batch_url).send().await?;

        if !batch_response.status().is_success() {
            // Fallback to basic fetch
            let mut work_items = Vec::new();
            for wi_ref in work_item_refs.value {
                let wi_url = format!("{}?api-version=7.0", wi_ref.url);
                let wi_response = self.client.get(&wi_url).send().await?;
                if let Ok(work_item) = wi_response.json::<WorkItem>().await {
                    work_items.push(work_item);
                }
            }
            return Ok(work_items);
        }

        #[derive(Deserialize)]
        struct BatchWorkItemsResponse {
            value: Vec<WorkItem>,
        }

        let batch_result: BatchWorkItemsResponse = batch_response.json().await?;
        Ok(batch_result.value)
    }

    pub async fn fetch_repo_details(&self) -> Result<RepoDetails> {
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}?api-version=7.0",
            self.organization, self.project, self.repository
        );

        let response = self.client.get(&url).send().await?;
        let repo_details: RepoDetails = response.json().await?;
        Ok(repo_details)
    }

    pub async fn fetch_pr_commit(&self, pr_id: i32) -> Result<crate::models::MergeCommit> {
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullRequests/{}?api-version=7.0",
            self.organization, self.project, self.repository, pr_id
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch pull request details")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!("API request failed with status {}: {}", status, text);
        }

        let pr: PullRequest = response
            .json()
            .await
            .context("Failed to parse pull request response")?;

        pr.last_merge_commit
            .ok_or_else(|| anyhow::anyhow!("Pull request {} has no merge commit", pr_id))
    }

    pub async fn add_label_to_pr(&self, pr_id: i32, label: &str) -> Result<()> {
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullRequests/{}/labels?api-version=7.0",
            self.organization, self.project, self.repository, pr_id
        );

        #[derive(Serialize)]
        struct LabelRequest {
            name: String,
        }

        let label_request = LabelRequest {
            name: label.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .json(&label_request)
            .send()
            .await
            .context("Failed to add label to pull request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!(
                "Failed to add label to PR {}: status {}, body: {}",
                pr_id,
                status,
                text
            );
        }

        Ok(())
    }

    pub async fn update_work_item_state(&self, work_item_id: i32, new_state: &str) -> Result<()> {
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems/{}?api-version=7.0",
            self.organization, self.project, work_item_id
        );

        #[derive(Serialize)]
        struct WorkItemUpdate {
            op: String,
            path: String,
            value: String,
        }

        let update = vec![WorkItemUpdate {
            op: "add".to_string(),
            path: "/fields/System.State".to_string(),
            value: new_state.to_string(),
        }];

        let response = self
            .client
            .patch(&url)
            .header("Content-Type", "application/json-patch+json")
            .json(&update)
            .send()
            .await
            .context("Failed to update work item state")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!(
                "Failed to update work item {} state: status {}, body: {}",
                work_item_id,
                status,
                text
            );
        }

        Ok(())
    }

    pub async fn fetch_work_item_history(&self, work_item_id: i32) -> Result<Vec<WorkItemHistory>> {
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems/{}/updates?api-version=7.0",
            self.organization, self.project, work_item_id
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch work item history")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!("API request failed with status {}: {}", status, text);
        }

        #[derive(Deserialize)]
        struct WorkItemHistoryResponse {
            value: Vec<WorkItemHistory>,
        }

        let history_response: WorkItemHistoryResponse = response
            .json()
            .await
            .context("Failed to parse work item history response")?;
        Ok(history_response.value)
    }

    pub async fn fetch_work_items_with_history_for_pr(&self, pr_id: i32) -> Result<Vec<WorkItem>> {
        // First get the basic work items
        let mut work_items = self.fetch_work_items_for_pr(pr_id).await?;

        // Then fetch history for each work item
        for work_item in &mut work_items {
            if let Ok(history) = self.fetch_work_item_history(work_item.id).await {
                work_item.history = history;
            }
        }

        Ok(work_items)
    }

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

    pub fn analyze_work_items_for_pr(
        &self,
        pr_with_work_items: &crate::models::PullRequestWithWorkItems,
        terminal_states: &[String],
    ) -> (bool, Vec<WorkItem>, Vec<WorkItem>) {
        let mut terminal_items = Vec::new();
        let mut non_terminal_items = Vec::new();

        for work_item in &pr_with_work_items.work_items {
            if self.is_work_item_in_terminal_state(work_item, terminal_states) {
                terminal_items.push(work_item.clone());
            } else {
                non_terminal_items.push(work_item.clone());
            }
        }

        let all_terminal = non_terminal_items.is_empty() && !terminal_items.is_empty();
        (all_terminal, terminal_items, non_terminal_items)
    }

    pub fn parse_terminal_states(terminal_states_str: &str) -> Vec<String> {
        terminal_states_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

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
