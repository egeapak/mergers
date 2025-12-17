//! Shared application state common to all app modes.
//!
//! This module provides [`AppBase`] which contains state shared across
//! merge, migration, and cleanup modes.

use crate::{
    api::AzureDevOpsClient,
    models::{AppConfig, PullRequestWithWorkItems, WorkItem},
    ui::{WorktreeContext, browser::BrowserOpener},
};
use std::{path::Path, sync::Arc};

/// Shared state common to all app modes.
///
/// `AppBase` contains the core application state that is needed regardless
/// of whether the app is running in merge, migration, or cleanup mode.
/// This includes configuration, the API client, and worktree management.
///
/// Mode-specific apps (MergeApp, MigrationApp, CleanupApp) contain an
/// `AppBase` and implement `Deref`/`DerefMut` to it for ergonomic access.
pub struct AppBase {
    /// Application configuration (shared via Arc for thread-safety).
    pub config: Arc<AppConfig>,

    /// Loaded pull requests with their associated work items.
    pub pull_requests: Vec<PullRequestWithWorkItems>,

    /// Azure DevOps API client for fetching data.
    pub client: AzureDevOpsClient,

    /// Version string (e.g., for tagging).
    pub version: Option<String>,

    /// Worktree context for managing temporary git worktrees.
    pub worktree: WorktreeContext,

    /// Error message to display to the user.
    pub error_message: Option<String>,

    /// Browser opener for opening URLs (trait object for testing).
    browser: Box<dyn BrowserOpener>,
}

impl AppBase {
    /// Creates a new AppBase with the given configuration, client, and browser opener.
    pub fn new(
        config: Arc<AppConfig>,
        client: AzureDevOpsClient,
        browser: Box<dyn BrowserOpener>,
    ) -> Self {
        Self {
            config,
            pull_requests: Vec::new(),
            client,
            version: None,
            worktree: WorktreeContext::new(),
            error_message: None,
            browser,
        }
    }

    // ========================================================================
    // Field Accessors
    // ========================================================================

    /// Returns a reference to the API client.
    pub fn client(&self) -> &AzureDevOpsClient {
        &self.client
    }

    /// Returns a reference to the pull requests.
    pub fn pull_requests(&self) -> &Vec<PullRequestWithWorkItems> {
        &self.pull_requests
    }

    /// Returns a mutable reference to the pull requests.
    pub fn pull_requests_mut(&mut self) -> &mut Vec<PullRequestWithWorkItems> {
        &mut self.pull_requests
    }

    /// Returns the version string if set.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Sets the version string.
    pub fn set_version(&mut self, version: Option<String>) {
        self.version = version;
    }

    /// Returns the error message if set.
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    /// Sets the error message.
    pub fn set_error_message(&mut self, msg: Option<String>) {
        self.error_message = msg;
    }

    /// Returns the repository path (for worktree operations).
    pub fn repo_path(&self) -> Option<&Path> {
        self.worktree.repo_path()
    }

    /// Sets the repository path.
    pub fn set_repo_path(&mut self, path: Option<std::path::PathBuf>) {
        self.worktree.set_repo_path(path);
    }

    // ========================================================================
    // Configuration Getters
    // ========================================================================

    /// Returns the Azure DevOps organization name.
    pub fn organization(&self) -> &str {
        self.config.shared().organization.value()
    }

    /// Returns the Azure DevOps project name.
    pub fn project(&self) -> &str {
        self.config.shared().project.value()
    }

    /// Returns the repository name.
    pub fn repository(&self) -> &str {
        self.config.shared().repository.value()
    }

    /// Returns the development branch name.
    pub fn dev_branch(&self) -> &str {
        self.config.shared().dev_branch.value()
    }

    /// Returns the target branch name.
    pub fn target_branch(&self) -> &str {
        self.config.shared().target_branch.value()
    }

    /// Returns the local repository path, if configured.
    pub fn local_repo(&self) -> Option<&str> {
        self.config
            .shared()
            .local_repo
            .as_ref()
            .map(|p| p.value().as_str())
    }

    /// Returns the maximum concurrent network operations allowed.
    pub fn max_concurrent_network(&self) -> usize {
        *self.config.shared().max_concurrent_network.value()
    }

    /// Returns the maximum concurrent processing operations allowed.
    pub fn max_concurrent_processing(&self) -> usize {
        *self.config.shared().max_concurrent_processing.value()
    }

    /// Returns the tag prefix for merged PRs.
    pub fn tag_prefix(&self) -> &str {
        self.config.shared().tag_prefix.value()
    }

    /// Returns the "since" date filter as originally specified.
    pub fn since(&self) -> Option<&str> {
        self.config
            .shared()
            .since
            .as_ref()
            .and_then(|d| d.original())
    }

    // ========================================================================
    // Pull Request Helpers
    // ========================================================================

    /// Returns all selected pull requests, sorted by closed date.
    pub fn get_selected_prs(&self) -> Vec<&PullRequestWithWorkItems> {
        let mut prs = self
            .pull_requests
            .iter()
            .filter(|pr| pr.selected)
            .collect::<Vec<_>>();
        prs.sort_by_key(|pr| pr.pr.closed_date.as_ref().unwrap());
        prs
    }

    // ========================================================================
    // Browser Helpers
    // ========================================================================

    /// Opens a pull request in the default browser.
    pub fn open_pr_in_browser(&self, pr_id: i32) {
        let url = format!(
            "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{}",
            self.organization(),
            self.project(),
            self.repository(),
            pr_id
        );

        self.browser.open_url(&url);
    }

    /// Opens work items in the default browser.
    pub fn open_work_items_in_browser(&self, work_items: &[WorkItem]) {
        for wi in work_items {
            let url = format!(
                "https://dev.azure.com/{}/{}/_workitems/edit/{}",
                self.organization(),
                self.project(),
                wi.id
            );

            self.browser.open_url(&url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{DefaultModeConfig, SharedConfig},
        parsed_property::ParsedProperty,
        ui::browser::MockBrowserOpener,
    };

    fn create_test_shared_config() -> SharedConfig {
        SharedConfig {
            organization: ParsedProperty::Default("test_org".to_string()),
            project: ParsedProperty::Default("test_project".to_string()),
            repository: ParsedProperty::Default("test_repo".to_string()),
            pat: ParsedProperty::Default("test_pat".to_string()),
            dev_branch: ParsedProperty::Default("develop".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: Some(ParsedProperty::Default("/path/to/repo".to_string())),
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged/".to_string()),
            since: None,
            skip_confirmation: false,
        }
    }

    fn create_test_client() -> AzureDevOpsClient {
        AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap()
    }

    /// # AppBase Initialization
    ///
    /// Tests that AppBase initializes correctly with all fields.
    ///
    /// ## Test Scenario
    /// - Creates AppBase with test config and client
    /// - Verifies all fields are properly initialized
    ///
    /// ## Expected Outcome
    /// - All fields have expected values
    /// - Worktree context is empty by default
    #[test]
    fn test_app_base_initialization() {
        let config = Arc::new(AppConfig::Default {
            shared: create_test_shared_config(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        });
        let client = create_test_client();

        let base = AppBase::new(config.clone(), client, Box::new(MockBrowserOpener::new()));

        assert!(Arc::ptr_eq(&base.config, &config));
        assert!(base.pull_requests.is_empty());
        assert!(base.version.is_none());
        assert!(base.error_message.is_none());
        assert!(!base.worktree.has_worktree());
    }

    /// # AppBase Configuration Getters
    ///
    /// Tests all configuration getter methods.
    ///
    /// ## Test Scenario
    /// - Creates AppBase with specific config values
    /// - Tests each getter method
    ///
    /// ## Expected Outcome
    /// - All getters return correct values from config
    #[test]
    fn test_config_getters() {
        let config = Arc::new(AppConfig::Default {
            shared: create_test_shared_config(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        });
        let client = create_test_client();
        let base = AppBase::new(config, client, Box::new(MockBrowserOpener::new()));

        assert_eq!(base.organization(), "test_org");
        assert_eq!(base.project(), "test_project");
        assert_eq!(base.repository(), "test_repo");
        assert_eq!(base.dev_branch(), "develop");
        assert_eq!(base.target_branch(), "main");
        assert_eq!(base.local_repo(), Some("/path/to/repo"));
        assert_eq!(base.max_concurrent_network(), 100);
        assert_eq!(base.max_concurrent_processing(), 10);
        assert_eq!(base.tag_prefix(), "merged/");
        assert!(base.since().is_none());
    }

    /// # AppBase Optional Config Fields
    ///
    /// Tests behavior when optional config fields are None.
    ///
    /// ## Test Scenario
    /// - Creates config with local_repo and since as None
    /// - Tests getter behavior
    ///
    /// ## Expected Outcome
    /// - Optional getters return None appropriately
    #[test]
    fn test_optional_config_fields() {
        let mut shared = create_test_shared_config();
        shared.local_repo = None;
        shared.since = None;

        let config = Arc::new(AppConfig::Default {
            shared,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        });
        let client = create_test_client();
        let base = AppBase::new(config, client, Box::new(MockBrowserOpener::new()));

        assert!(base.local_repo().is_none());
        assert!(base.since().is_none());
    }

    /// # AppBase Get Selected PRs
    ///
    /// Tests the get_selected_prs() method.
    ///
    /// ## Test Scenario
    /// - Creates AppBase with mix of selected and unselected PRs
    /// - Calls get_selected_prs()
    ///
    /// ## Expected Outcome
    /// - Only selected PRs are returned
    /// - PRs are sorted by closed date
    #[test]
    fn test_get_selected_prs() {
        use crate::models::{CreatedBy, PullRequest, PullRequestWithWorkItems};

        let config = Arc::new(AppConfig::Default {
            shared: create_test_shared_config(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        });
        let client = create_test_client();
        let mut base = AppBase::new(config, client, Box::new(MockBrowserOpener::new()));

        let created_by = CreatedBy {
            display_name: "Test User".to_string(),
        };

        // Add some test PRs with different closed dates (ISO 8601 format)
        let pr1 = PullRequestWithWorkItems {
            pr: PullRequest {
                id: 1,
                title: "PR 1".to_string(),
                closed_date: Some("2024-01-01T10:00:00Z".to_string()),
                created_by: created_by.clone(),
                last_merge_commit: None,
                labels: None,
            },
            work_items: vec![],
            selected: true,
        };
        let pr2 = PullRequestWithWorkItems {
            pr: PullRequest {
                id: 2,
                title: "PR 2".to_string(),
                closed_date: Some("2024-01-02T10:00:00Z".to_string()),
                created_by: created_by.clone(),
                last_merge_commit: None,
                labels: None,
            },
            work_items: vec![],
            selected: false, // Not selected
        };
        let pr3 = PullRequestWithWorkItems {
            pr: PullRequest {
                id: 3,
                title: "PR 3".to_string(),
                closed_date: Some("2024-01-03T10:00:00Z".to_string()),
                created_by,
                last_merge_commit: None,
                labels: None,
            },
            work_items: vec![],
            selected: true,
        };

        base.pull_requests = vec![pr3, pr1, pr2]; // Add in wrong order

        let selected = base.get_selected_prs();

        // Should only have 2 selected PRs, sorted by closed_date
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].pr.id, 1); // Oldest first
        assert_eq!(selected[1].pr.id, 3); // Newest last
    }
}
