//! Data loading operations for fetching PRs and work items.
//!
//! This module provides the core logic for fetching pull requests and their
//! associated work items from Azure DevOps, independent of any UI concerns.
//!
//! Note: The full implementation integrates with the UI data loading state.
//! This module provides types and interfaces for non-interactive mode.

use chrono::{DateTime, Utc};

use crate::models::PullRequestWithWorkItems;

/// Result of a data loading operation.
#[derive(Debug, Clone)]
pub struct DataLoadingResult {
    /// The pull requests with their associated work items.
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    /// Total number of PRs fetched (before filtering).
    pub total_fetched: usize,
    /// Number of PRs after filtering out already-merged ones.
    pub after_filter: usize,
    /// Number of PRs with commit info populated.
    pub with_commits: usize,
}

impl DataLoadingResult {
    /// Creates a new result from a list of pull requests.
    pub fn from_prs(prs: Vec<PullRequestWithWorkItems>) -> Self {
        let count = prs.len();
        let with_commits = prs
            .iter()
            .filter(|pr| pr.pr.last_merge_commit.is_some())
            .count();
        Self {
            pull_requests: prs,
            total_fetched: count,
            after_filter: count,
            with_commits,
        }
    }
}

/// Progress update for data loading operations.
#[derive(Debug, Clone)]
pub enum DataLoadingProgress {
    /// Starting to fetch pull requests.
    FetchingPullRequests,
    /// Pull requests fetched, now fetching work items.
    FetchingWorkItems {
        /// Number of PRs to fetch work items for.
        pr_count: usize,
    },
    /// Work items progress update.
    WorkItemsProgress {
        /// Number of PRs completed.
        completed: usize,
        /// Total number of PRs.
        total: usize,
    },
    /// Fetching commit information for PRs.
    FetchingCommitInfo {
        /// Number of PRs needing commit info.
        pr_count: usize,
    },
    /// All data loading complete.
    Complete,
}

/// Configuration for data loading operations.
#[derive(Debug, Clone)]
pub struct DataLoadingConfig {
    /// The development branch to fetch PRs from.
    pub dev_branch: String,
    /// Only fetch PRs created after this date.
    pub since: Option<DateTime<Utc>>,
    /// Tag prefix for identifying already-merged PRs.
    pub tag_prefix: String,
    /// Maximum concurrent network requests.
    pub max_concurrent: usize,
}

impl Default for DataLoadingConfig {
    fn default() -> Self {
        Self {
            dev_branch: "dev".to_string(),
            since: None,
            tag_prefix: "merged-".to_string(),
            max_concurrent: 5,
        }
    }
}

/// Core data loading operation.
///
/// This struct encapsulates all the logic for fetching PRs, work items,
/// and commit information from Azure DevOps.
pub struct DataLoadingOperation {
    config: DataLoadingConfig,
}

impl DataLoadingOperation {
    /// Creates a new data loading operation.
    pub fn new(config: DataLoadingConfig) -> Self {
        Self { config }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DataLoadingConfig {
        &self.config
    }

    /// Returns the dev branch.
    pub fn dev_branch(&self) -> &str {
        &self.config.dev_branch
    }

    /// Returns the since date filter.
    pub fn since(&self) -> Option<DateTime<Utc>> {
        self.config.since
    }

    /// Returns the tag prefix.
    pub fn tag_prefix(&self) -> &str {
        &self.config.tag_prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Data Loading Config Default
    ///
    /// Verifies that default config has sensible values.
    ///
    /// ## Test Scenario
    /// - Creates default DataLoadingConfig
    ///
    /// ## Expected Outcome
    /// - Fields have expected default values
    #[test]
    fn test_data_loading_config_default() {
        let config = DataLoadingConfig::default();

        assert_eq!(config.dev_branch, "dev");
        assert!(config.since.is_none());
        assert_eq!(config.tag_prefix, "merged-");
        assert_eq!(config.max_concurrent, 5);
    }

    /// # Data Loading Result Fields
    ///
    /// Verifies that DataLoadingResult can be constructed.
    ///
    /// ## Test Scenario
    /// - Creates a DataLoadingResult with sample values
    ///
    /// ## Expected Outcome
    /// - All fields are accessible
    #[test]
    fn test_data_loading_result() {
        let result = DataLoadingResult {
            pull_requests: Vec::new(),
            total_fetched: 10,
            after_filter: 8,
            with_commits: 8,
        };

        assert_eq!(result.total_fetched, 10);
        assert_eq!(result.after_filter, 8);
        assert_eq!(result.with_commits, 8);
        assert!(result.pull_requests.is_empty());
    }

    /// # Data Loading Progress Variants
    ///
    /// Verifies that all progress variants can be created.
    ///
    /// ## Test Scenario
    /// - Creates each progress variant
    ///
    /// ## Expected Outcome
    /// - All variants construct successfully
    #[test]
    fn test_data_loading_progress_variants() {
        let _p1 = DataLoadingProgress::FetchingPullRequests;
        let _p2 = DataLoadingProgress::FetchingWorkItems { pr_count: 10 };
        let _p3 = DataLoadingProgress::WorkItemsProgress {
            completed: 5,
            total: 10,
        };
        let _p4 = DataLoadingProgress::FetchingCommitInfo { pr_count: 3 };
        let _p5 = DataLoadingProgress::Complete;
    }

    /// # Data Loading Operation Creation
    ///
    /// Verifies that DataLoadingOperation can be created.
    ///
    /// ## Test Scenario
    /// - Creates a DataLoadingOperation with custom config
    ///
    /// ## Expected Outcome
    /// - Operation is created and config is accessible
    #[test]
    fn test_data_loading_operation_creation() {
        let config = DataLoadingConfig {
            dev_branch: "develop".to_string(),
            since: None,
            tag_prefix: "v-".to_string(),
            max_concurrent: 10,
        };

        let operation = DataLoadingOperation::new(config);
        assert_eq!(operation.dev_branch(), "develop");
        assert_eq!(operation.tag_prefix(), "v-");
    }

    /// # Data Loading Result From PRs
    ///
    /// Verifies that DataLoadingResult::from_prs works correctly.
    ///
    /// ## Test Scenario
    /// - Creates a result from empty list
    ///
    /// ## Expected Outcome
    /// - All counts are zero
    #[test]
    fn test_data_loading_result_from_prs() {
        let result = DataLoadingResult::from_prs(Vec::new());

        assert_eq!(result.total_fetched, 0);
        assert_eq!(result.after_filter, 0);
        assert_eq!(result.with_commits, 0);
    }
}
