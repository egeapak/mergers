//! Application container for all modes.
//!
//! This module provides the [`App`] enum which wraps mode-specific application
//! types. The enum allows the run loop to handle all modes uniformly while
//! providing type-safe access to mode-specific state when needed.

use crate::{
    api::AzureDevOpsClient,
    models::{
        AppConfig, AppModeConfig, CherryPickItem, CleanupBranch, CleanupConfig, MergeConfig,
        MigrationAnalysis, MigrationConfig, PullRequestWithWorkItems, SharedConfig, WorkItem,
    },
    ui::AppMode,
    ui::apps::{CleanupApp, MergeApp, MigrationApp},
    ui::browser::SystemBrowserOpener,
};
use std::sync::Arc;
use tempfile::TempDir;

/// Application container wrapping mode-specific app types.
///
/// `App` is an enum that contains one of the three mode-specific application
/// types. This design allows:
/// - Type-safe access to mode-specific state via pattern matching
/// - Common operations through delegating methods
/// - Backward compatibility with the legacy `AppState` trait
///
/// # Examples
///
/// ```ignore
/// // Create app for merge mode
/// let app = App::new_merge(config, client);
///
/// // Access common fields via delegation
/// let org = app.organization();
/// let prs = app.pull_requests();
///
/// // Access mode-specific fields via pattern matching
/// if let App::Merge(merge_app) = &app {
///     let items = &merge_app.cherry_pick_items;
/// }
/// ```
#[allow(clippy::large_enum_variant)]
pub enum App {
    /// Merge mode (cherry-picking PRs).
    Merge(MergeApp),
    /// Migration mode (analyzing PR migration status).
    Migration(MigrationApp),
    /// Cleanup mode (deleting merged branches).
    Cleanup(CleanupApp),
}

impl App {
    // ========================================================================
    // Constructors - Type-Safe
    // ========================================================================

    /// Creates a new App for merge mode with type-safe MergeConfig.
    pub fn new_merge(config: Arc<MergeConfig>, client: AzureDevOpsClient) -> Self {
        App::Merge(MergeApp::new(config, client, Box::new(SystemBrowserOpener)))
    }

    /// Creates a new App for migration mode with type-safe MigrationConfig.
    pub fn new_migration(config: Arc<MigrationConfig>, client: AzureDevOpsClient) -> Self {
        App::Migration(MigrationApp::new(
            config,
            client,
            Box::new(SystemBrowserOpener),
        ))
    }

    /// Creates a new App for cleanup mode with type-safe CleanupConfig.
    pub fn new_cleanup(config: Arc<CleanupConfig>, client: AzureDevOpsClient) -> Self {
        App::Cleanup(CleanupApp::new(
            config,
            client,
            Box::new(SystemBrowserOpener),
        ))
    }

    // ========================================================================
    // Constructors - From AppConfig (backward compatibility)
    // ========================================================================

    /// Creates a new App with empty pull requests for the appropriate mode
    /// based on the configuration.
    ///
    /// This is the primary constructor that determines the mode from config
    /// and converts to the appropriate type-safe config internally.
    pub fn new(
        pull_requests: Vec<PullRequestWithWorkItems>,
        config: Arc<AppConfig>,
        client: AzureDevOpsClient,
    ) -> Self {
        let mut app = Self::from_config(config, client);
        *app.pull_requests_mut() = pull_requests;
        app
    }

    /// Creates a new App from configuration, determining mode automatically.
    ///
    /// This constructor converts the `AppConfig` enum to the appropriate
    /// type-safe config struct internally.
    pub fn from_config(config: Arc<AppConfig>, client: AzureDevOpsClient) -> Self {
        // Unwrap the Arc to get owned AppConfig for conversion
        let config = Arc::try_unwrap(config).unwrap_or_else(|arc| (*arc).clone());

        match config {
            AppConfig::Migration { shared, migration } => {
                let typed_config = Arc::new(MigrationConfig {
                    shared,
                    terminal_states: migration.terminal_states,
                });
                App::new_migration(typed_config, client)
            }
            AppConfig::Cleanup { shared, cleanup } => {
                let typed_config = Arc::new(CleanupConfig {
                    shared,
                    target: cleanup.target,
                });
                App::new_cleanup(typed_config, client)
            }
            AppConfig::Default { shared, default } => {
                let typed_config = Arc::new(MergeConfig {
                    shared,
                    work_item_state: default.work_item_state,
                    run_hooks: default.run_hooks,
                });
                App::new_merge(typed_config, client)
            }
        }
    }

    /// Creates a new App with a custom browser opener (for testing).
    ///
    /// This constructor is primarily used for testing to inject a mock browser
    /// opener instead of the real system browser.
    #[cfg(test)]
    pub fn new_with_browser(
        pull_requests: Vec<PullRequestWithWorkItems>,
        config: Arc<AppConfig>,
        client: AzureDevOpsClient,
        browser: Box<dyn crate::ui::browser::BrowserOpener>,
    ) -> Self {
        // Unwrap the Arc to get owned AppConfig for conversion
        let config = Arc::try_unwrap(config).unwrap_or_else(|arc| (*arc).clone());

        let mut app = match config {
            AppConfig::Migration { shared, migration } => {
                let typed_config = Arc::new(MigrationConfig {
                    shared,
                    terminal_states: migration.terminal_states,
                });
                App::Migration(MigrationApp::new(typed_config, client, browser))
            }
            AppConfig::Cleanup { shared, cleanup } => {
                let typed_config = Arc::new(CleanupConfig {
                    shared,
                    target: cleanup.target,
                });
                App::Cleanup(CleanupApp::new(typed_config, client, browser))
            }
            AppConfig::Default { shared, default } => {
                let typed_config = Arc::new(MergeConfig {
                    shared,
                    work_item_state: default.work_item_state,
                    run_hooks: default.run_hooks,
                });
                App::Merge(MergeApp::new(typed_config, client, browser))
            }
        };

        *app.pull_requests_mut() = pull_requests;
        app
    }

    // ========================================================================
    // Shared Config Access
    // ========================================================================

    /// Returns a reference to the shared configuration.
    ///
    /// This provides access to configuration fields that are common across
    /// all modes (organization, project, repository, etc.).
    pub fn shared_config(&self) -> &SharedConfig {
        match self {
            App::Merge(app) => app.config().shared(),
            App::Migration(app) => app.config().shared(),
            App::Cleanup(app) => app.config().shared(),
        }
    }

    // ========================================================================
    // Shared Field Access
    // ========================================================================

    /// Returns a reference to the pull requests.
    pub fn pull_requests(&self) -> &Vec<PullRequestWithWorkItems> {
        match self {
            App::Merge(app) => &app.pull_requests,
            App::Migration(app) => &app.pull_requests,
            App::Cleanup(app) => &app.pull_requests,
        }
    }

    /// Returns a mutable reference to the pull requests.
    pub fn pull_requests_mut(&mut self) -> &mut Vec<PullRequestWithWorkItems> {
        match self {
            App::Merge(app) => &mut app.pull_requests,
            App::Migration(app) => &mut app.pull_requests,
            App::Cleanup(app) => &mut app.pull_requests,
        }
    }

    /// Returns a reference to the API client.
    pub fn client(&self) -> &AzureDevOpsClient {
        match self {
            App::Merge(app) => &app.client,
            App::Migration(app) => &app.client,
            App::Cleanup(app) => &app.client,
        }
    }

    /// Returns the version string if set.
    pub fn version(&self) -> Option<&str> {
        match self {
            App::Merge(app) => app.version.as_deref(),
            App::Migration(app) => app.version.as_deref(),
            App::Cleanup(app) => app.version.as_deref(),
        }
    }

    /// Sets the version string.
    pub fn set_version(&mut self, version: Option<String>) {
        match self {
            App::Merge(app) => app.version = version,
            App::Migration(app) => app.version = version,
            App::Cleanup(app) => app.version = version,
        }
    }

    /// Returns the error message if set.
    pub fn error_message(&self) -> Option<&str> {
        match self {
            App::Merge(app) => app.error_message.as_deref(),
            App::Migration(app) => app.error_message.as_deref(),
            App::Cleanup(app) => app.error_message.as_deref(),
        }
    }

    /// Sets the error message.
    pub fn set_error_message(&mut self, msg: Option<String>) {
        match self {
            App::Merge(app) => app.error_message = msg,
            App::Migration(app) => app.error_message = msg,
            App::Cleanup(app) => app.error_message = msg,
        }
    }

    // ========================================================================
    // Configuration Getters
    // ========================================================================

    /// Returns the Azure DevOps organization name.
    pub fn organization(&self) -> &str {
        self.shared_config().organization.value()
    }

    /// Returns the Azure DevOps project name.
    pub fn project(&self) -> &str {
        self.shared_config().project.value()
    }

    /// Returns the repository name.
    pub fn repository(&self) -> &str {
        self.shared_config().repository.value()
    }

    /// Returns the development branch name.
    pub fn dev_branch(&self) -> &str {
        self.shared_config().dev_branch.value()
    }

    /// Returns the target branch name.
    pub fn target_branch(&self) -> &str {
        self.shared_config().target_branch.value()
    }

    /// Returns the local repository path, if configured.
    pub fn local_repo(&self) -> Option<&str> {
        self.shared_config()
            .local_repo
            .as_ref()
            .map(|p| p.value().as_str())
    }

    /// Returns the work item state to set after merging.
    pub fn work_item_state(&self) -> &str {
        match self {
            App::Merge(app) => app.work_item_state(),
            App::Migration(_) => "Next Merged", // fallback
            App::Cleanup(_) => "Next Merged",   // fallback
        }
    }

    /// Returns the maximum concurrent network operations allowed.
    pub fn max_concurrent_network(&self) -> usize {
        *self.shared_config().max_concurrent_network.value()
    }

    /// Returns the maximum concurrent processing operations allowed.
    pub fn max_concurrent_processing(&self) -> usize {
        *self.shared_config().max_concurrent_processing.value()
    }

    /// Returns the tag prefix for merged PRs.
    pub fn tag_prefix(&self) -> &str {
        self.shared_config().tag_prefix.value()
    }

    /// Returns the "since" date filter as originally specified.
    pub fn since(&self) -> Option<&str> {
        self.shared_config()
            .since
            .as_ref()
            .and_then(|d| d.original())
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Returns all selected pull requests, sorted by closed date.
    pub fn get_selected_prs(&self) -> Vec<&PullRequestWithWorkItems> {
        match self {
            App::Merge(app) => app.get_selected_prs(),
            App::Migration(app) => app.get_selected_prs(),
            App::Cleanup(app) => app.get_selected_prs(),
        }
    }

    /// Opens a pull request in the default browser.
    pub fn open_pr_in_browser(&self, pr_id: i32) {
        match self {
            App::Merge(app) => app.open_pr_in_browser(pr_id),
            App::Migration(app) => app.open_pr_in_browser(pr_id),
            App::Cleanup(app) => app.open_pr_in_browser(pr_id),
        }
    }

    /// Opens work items in the default browser.
    pub fn open_work_items_in_browser(&self, work_items: &[WorkItem]) {
        match self {
            App::Merge(app) => app.open_work_items_in_browser(work_items),
            App::Migration(app) => app.open_work_items_in_browser(work_items),
            App::Cleanup(app) => app.open_work_items_in_browser(work_items),
        }
    }

    // ========================================================================
    // Worktree Operations
    // ========================================================================

    /// Returns the repository path (for worktree operations).
    pub fn repo_path(&self) -> Option<&std::path::Path> {
        match self {
            App::Merge(app) => app.worktree.repo_path(),
            App::Migration(app) => app.worktree.repo_path(),
            App::Cleanup(app) => app.worktree.repo_path(),
        }
    }

    /// Sets the repository path (delegating to worktree context).
    pub fn set_repo_path(&mut self, path: Option<std::path::PathBuf>) {
        match self {
            App::Merge(app) => app.worktree.set_repo_path(path),
            App::Migration(app) => app.worktree.set_repo_path(path),
            App::Cleanup(app) => app.worktree.set_repo_path(path),
        }
    }

    /// Sets the temp directory to keep it alive.
    #[allow(dead_code)]
    pub fn set_temp_dir(&mut self, temp_dir: Option<TempDir>) {
        match self {
            App::Merge(app) => app.worktree.set_temp_dir(temp_dir),
            App::Migration(app) => app.worktree.set_temp_dir(temp_dir),
            App::Cleanup(app) => app.worktree.set_temp_dir(temp_dir),
        }
    }

    // ========================================================================
    // Mode-Specific Field Access (Merge Mode)
    // ========================================================================

    /// Returns a reference to cherry pick items (merge mode only).
    /// Panics if called in non-merge mode.
    pub fn cherry_pick_items(&self) -> &Vec<CherryPickItem> {
        match self {
            App::Merge(app) => &app.cherry_pick_items,
            _ => panic!("cherry_pick_items() called in non-merge mode"),
        }
    }

    /// Returns a mutable reference to cherry pick items (merge mode only).
    /// Panics if called in non-merge mode.
    pub fn cherry_pick_items_mut(&mut self) -> &mut Vec<CherryPickItem> {
        match self {
            App::Merge(app) => &mut app.cherry_pick_items,
            _ => panic!("cherry_pick_items_mut() called in non-merge mode"),
        }
    }

    /// Returns the current cherry pick index (merge mode only).
    /// Returns 0 in non-merge mode.
    pub fn current_cherry_pick_index(&self) -> usize {
        match self {
            App::Merge(app) => app.current_cherry_pick_index,
            _ => 0,
        }
    }

    /// Sets the current cherry pick index (merge mode only).
    /// Does nothing in non-merge mode.
    pub fn set_current_cherry_pick_index(&mut self, index: usize) {
        if let App::Merge(app) = self {
            app.current_cherry_pick_index = index;
        }
    }

    // ========================================================================
    // Mode-Specific Field Access (Migration Mode)
    // ========================================================================

    /// Returns a reference to the migration analysis (migration mode only).
    pub fn migration_analysis(&self) -> Option<&MigrationAnalysis> {
        match self {
            App::Migration(app) => app.migration_analysis.as_ref(),
            _ => None,
        }
    }

    /// Returns a mutable reference to the migration analysis (migration mode only).
    pub fn migration_analysis_mut(&mut self) -> Option<&mut MigrationAnalysis> {
        match self {
            App::Migration(app) => app.migration_analysis.as_mut(),
            _ => None,
        }
    }

    /// Sets the migration analysis (migration mode only).
    pub fn set_migration_analysis(&mut self, analysis: Option<MigrationAnalysis>) {
        if let App::Migration(app) = self {
            app.migration_analysis = analysis;
        }
    }

    /// Mark a PR as manually eligible (migration mode only).
    pub fn mark_pr_as_eligible(&mut self, pr_id: i32) {
        if let App::Migration(app) = self {
            app.mark_pr_as_eligible(pr_id);
        }
    }

    /// Mark a PR as manually not eligible (migration mode only).
    pub fn mark_pr_as_not_eligible(&mut self, pr_id: i32) {
        if let App::Migration(app) = self {
            app.mark_pr_as_not_eligible(pr_id);
        }
    }

    /// Remove manual override for a PR (migration mode only).
    pub fn remove_manual_override(&mut self, pr_id: i32) {
        if let App::Migration(app) = self {
            app.remove_manual_override(pr_id);
        }
    }

    /// Check if a PR has a manual override (migration mode only).
    pub fn has_manual_override(&self, pr_id: i32) -> Option<bool> {
        match self {
            App::Migration(app) => app.has_manual_override(pr_id),
            _ => None,
        }
    }

    // ========================================================================
    // Mode-Specific Field Access (Cleanup Mode)
    // ========================================================================

    /// Returns a reference to cleanup branches (cleanup mode only).
    pub fn cleanup_branches(&self) -> &Vec<CleanupBranch> {
        match self {
            App::Cleanup(app) => &app.cleanup_branches,
            _ => &EMPTY_CLEANUP_BRANCHES,
        }
    }

    /// Returns a mutable reference to cleanup branches (cleanup mode only).
    /// Panics if called in non-cleanup mode.
    pub fn cleanup_branches_mut(&mut self) -> &mut Vec<CleanupBranch> {
        match self {
            App::Cleanup(app) => &mut app.cleanup_branches,
            _ => panic!("cleanup_branches_mut() called in non-cleanup mode"),
        }
    }

    // ========================================================================
    // Mode Checking
    // ========================================================================

    /// Returns true if this is merge mode.
    pub fn is_merge_mode(&self) -> bool {
        matches!(self, App::Merge(_))
    }

    /// Returns true if this is migration mode.
    pub fn is_migration_mode(&self) -> bool {
        matches!(self, App::Migration(_))
    }

    /// Returns true if this is cleanup mode.
    pub fn is_cleanup_mode(&self) -> bool {
        matches!(self, App::Cleanup(_))
    }

    /// Cleans up the migration worktree if one was created.
    pub fn cleanup_worktree(&mut self) {
        match self {
            App::Merge(app) => app.worktree.cleanup(),
            App::Migration(app) => app.worktree.cleanup(),
            App::Cleanup(app) => app.worktree.cleanup(),
        }
    }

    /// Cleans up the migration worktree if one was created.
    /// This is an alias for cleanup_worktree() for backward compatibility.
    pub fn cleanup_migration_worktree(&mut self) {
        self.cleanup_worktree();
    }
}

// Static empty vec for non-cleanup mode
static EMPTY_CLEANUP_BRANCHES: Vec<CleanupBranch> = Vec::new();

impl Drop for App {
    fn drop(&mut self) {
        // Clean up worktree when App is dropped
        self.cleanup_worktree();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::AzureDevOpsClient,
        models::{CleanupModeConfig, DefaultModeConfig, MigrationModeConfig, SharedConfig},
        parsed_property::ParsedProperty,
    };

    fn create_shared_config() -> SharedConfig {
        SharedConfig {
            organization: ParsedProperty::Default("test_org".to_string()),
            project: ParsedProperty::Default("test_project".to_string()),
            repository: ParsedProperty::Default("test_repo".to_string()),
            pat: ParsedProperty::Default("test_pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("next".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None,
            skip_confirmation: false,
        }
    }

    fn create_merge_config() -> Arc<MergeConfig> {
        Arc::new(MergeConfig {
            shared: create_shared_config(),
            work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            run_hooks: ParsedProperty::Default(false),
        })
    }

    fn create_migration_config() -> Arc<MigrationConfig> {
        Arc::new(MigrationConfig {
            shared: create_shared_config(),
            terminal_states: ParsedProperty::Default(vec![
                "Closed".to_string(),
                "Done".to_string(),
            ]),
        })
    }

    fn create_cleanup_config() -> Arc<CleanupConfig> {
        Arc::new(CleanupConfig {
            shared: create_shared_config(),
            target: ParsedProperty::Default("main".to_string()),
        })
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

    /// # App Enum Merge Mode Creation
    ///
    /// Tests that App::new_merge creates a merge mode app.
    ///
    /// ## Test Scenario
    /// - Creates App using new_merge constructor
    /// - Verifies mode and field access
    ///
    /// ## Expected Outcome
    /// - App is in merge mode
    /// - Merge-specific fields are accessible
    #[test]
    fn test_app_new_merge() {
        let config = create_merge_config();
        let client = create_test_client();

        let app = App::new_merge(config, client);

        assert!(app.is_merge_mode());
        assert!(!app.is_migration_mode());
        assert!(!app.is_cleanup_mode());
        assert!(app.cherry_pick_items().is_empty());
        assert_eq!(app.current_cherry_pick_index(), 0);
    }

    /// # App Enum Migration Mode Creation
    ///
    /// Tests that App::new_migration creates a migration mode app.
    ///
    /// ## Test Scenario
    /// - Creates App using new_migration constructor
    /// - Verifies mode and field access
    ///
    /// ## Expected Outcome
    /// - App is in migration mode
    /// - Migration-specific fields are accessible
    #[test]
    fn test_app_new_migration() {
        let config = create_migration_config();
        let client = create_test_client();

        let app = App::new_migration(config, client);

        assert!(!app.is_merge_mode());
        assert!(app.is_migration_mode());
        assert!(!app.is_cleanup_mode());
        assert!(app.migration_analysis().is_none());
    }

    /// # App Enum Cleanup Mode Creation
    ///
    /// Tests that App::new_cleanup creates a cleanup mode app.
    ///
    /// ## Test Scenario
    /// - Creates App using new_cleanup constructor
    /// - Verifies mode and field access
    ///
    /// ## Expected Outcome
    /// - App is in cleanup mode
    /// - Cleanup-specific fields are accessible
    #[test]
    fn test_app_new_cleanup() {
        let config = create_cleanup_config();
        let client = create_test_client();

        let app = App::new_cleanup(config, client);

        assert!(!app.is_merge_mode());
        assert!(!app.is_migration_mode());
        assert!(app.is_cleanup_mode());
        assert!(app.cleanup_branches().is_empty());
    }

    /// # App Enum from_config Auto Detection
    ///
    /// Tests that App::from_config creates the correct mode based on config.
    ///
    /// ## Test Scenario
    /// - Creates various configs and uses from_config
    /// - Verifies correct mode is selected
    ///
    /// ## Expected Outcome
    /// - Mode matches the config type
    #[test]
    fn test_app_from_config() {
        let client = create_test_client();

        // Default config -> Merge mode
        let default_config = Arc::new(AppConfig::Default {
            shared: create_shared_config(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
                run_hooks: ParsedProperty::Default(false),
            },
        });
        let app = App::from_config(default_config, client.clone());
        assert!(app.is_merge_mode());

        // Migration config -> Migration mode
        let migration_config = Arc::new(AppConfig::Migration {
            shared: create_shared_config(),
            migration: MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec!["Closed".to_string()]),
            },
        });
        let app = App::from_config(migration_config, client.clone());
        assert!(app.is_migration_mode());

        // Cleanup config -> Cleanup mode
        let cleanup_config = Arc::new(AppConfig::Cleanup {
            shared: create_shared_config(),
            cleanup: CleanupModeConfig {
                target: ParsedProperty::Default("main".to_string()),
            },
        });
        let app = App::from_config(cleanup_config, client);
        assert!(app.is_cleanup_mode());
    }

    /// # App Configuration Property Accessors
    ///
    /// Tests that configuration properties are accessible through App.
    ///
    /// ## Test Scenario
    /// - Creates App and accesses configuration properties
    ///
    /// ## Expected Outcome
    /// - All config properties return expected values
    #[test]
    fn test_app_config_accessors() {
        let config = create_merge_config();
        let client = create_test_client();
        let app = App::new_merge(config, client);

        assert_eq!(app.organization(), "test_org");
        assert_eq!(app.project(), "test_project");
        assert_eq!(app.repository(), "test_repo");
        assert_eq!(app.dev_branch(), "dev");
        assert_eq!(app.target_branch(), "next");
        assert_eq!(app.tag_prefix(), "merged-");
        assert_eq!(app.max_concurrent_network(), 100);
        assert_eq!(app.max_concurrent_processing(), 10);
        assert!(app.local_repo().is_none());
        assert!(app.since().is_none());
    }

    /// # App Field Access
    ///
    /// Tests that shared fields are accessible through App.
    ///
    /// ## Test Scenario
    /// - Creates App and accesses/modifies shared fields
    ///
    /// ## Expected Outcome
    /// - Can read and write to shared fields
    #[test]
    fn test_app_field_access() {
        let config = create_merge_config();
        let client = create_test_client();
        let mut app = App::new_merge(config, client);

        // Read pull_requests
        assert!(app.pull_requests().is_empty());

        // Write version
        app.set_version(Some("1.0.0".to_string()));
        assert_eq!(app.version(), Some("1.0.0"));
    }

    /// # App Merge Mode Cherry Pick Operations
    ///
    /// Tests cherry pick field access and mutation in merge mode.
    ///
    /// ## Test Scenario
    /// - Creates merge mode App
    /// - Accesses and modifies cherry pick fields
    ///
    /// ## Expected Outcome
    /// - Cherry pick fields work correctly
    #[test]
    fn test_app_merge_cherry_pick_operations() {
        use crate::models::CherryPickStatus;

        let config = create_merge_config();
        let client = create_test_client();
        let mut app = App::new_merge(config, client);

        // Add cherry pick items
        app.cherry_pick_items_mut().push(CherryPickItem {
            pr_id: 123,
            commit_id: "abc".to_string(),
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Pending,
        });

        assert_eq!(app.cherry_pick_items().len(), 1);
        assert_eq!(app.cherry_pick_items()[0].pr_id, 123);

        // Set index
        app.set_current_cherry_pick_index(1);
        assert_eq!(app.current_cherry_pick_index(), 1);
    }

    /// # App Migration Mode Operations
    ///
    /// Tests migration-specific operations.
    ///
    /// ## Test Scenario
    /// - Creates migration mode App
    /// - Tests migration analysis access
    ///
    /// ## Expected Outcome
    /// - Migration fields accessible in migration mode
    #[test]
    fn test_app_migration_operations() {
        let config = create_migration_config();
        let client = create_test_client();
        let mut app = App::new_migration(config, client);

        // Initially no analysis
        assert!(app.migration_analysis().is_none());

        // These should not panic in migration mode
        app.mark_pr_as_eligible(123);
        assert!(app.has_manual_override(123).is_none()); // No analysis yet
    }

    /// # App Cleanup Mode Operations
    ///
    /// Tests cleanup-specific operations.
    ///
    /// ## Test Scenario
    /// - Creates cleanup mode App
    /// - Tests cleanup branch access
    ///
    /// ## Expected Outcome
    /// - Cleanup fields accessible in cleanup mode
    #[test]
    fn test_app_cleanup_operations() {
        use crate::models::CleanupStatus;

        let config = create_cleanup_config();
        let client = create_test_client();
        let mut app = App::new_cleanup(config, client);

        // Add cleanup branches
        app.cleanup_branches_mut().push(CleanupBranch {
            name: "feature/test".to_string(),
            target: "main".to_string(),
            version: "1.0".to_string(),
            is_merged: true,
            selected: false,
            status: CleanupStatus::Pending,
        });

        assert_eq!(app.cleanup_branches().len(), 1);
        assert_eq!(app.cleanup_branches()[0].name, "feature/test");
    }

    /// # App Mode Mismatch - Cleanup in Non-Cleanup Mode
    ///
    /// Tests that cleanup_branches() returns empty in non-cleanup mode.
    ///
    /// ## Test Scenario
    /// - Creates merge mode App
    /// - Calls cleanup_branches()
    ///
    /// ## Expected Outcome
    /// - Returns empty slice (no panic)
    #[test]
    fn test_app_cleanup_branches_in_merge_mode() {
        let config = create_merge_config();
        let client = create_test_client();
        let app = App::new_merge(config, client);

        // Should return empty, not panic
        assert!(app.cleanup_branches().is_empty());
    }

    /// # App Work Item State by Mode
    ///
    /// Tests that work_item_state returns appropriate values per mode.
    ///
    /// ## Test Scenario
    /// - Creates apps in different modes
    /// - Tests work_item_state()
    ///
    /// ## Expected Outcome
    /// - Merge mode returns configured value
    /// - Other modes return fallback
    #[test]
    fn test_app_work_item_state_by_mode() {
        let client = create_test_client();

        // Merge mode with custom state
        let merge_config = Arc::new(MergeConfig {
            shared: create_shared_config(),
            work_item_state: ParsedProperty::Default("Custom State".to_string()),
            run_hooks: ParsedProperty::Default(false),
        });
        let merge_app = App::new_merge(merge_config, client.clone());
        assert_eq!(merge_app.work_item_state(), "Custom State");

        // Migration mode - fallback
        let migration_config = create_migration_config();
        let migration_app = App::new_migration(migration_config, client.clone());
        assert_eq!(migration_app.work_item_state(), "Next Merged");

        // Cleanup mode - fallback
        let cleanup_config = create_cleanup_config();
        let cleanup_app = App::new_cleanup(cleanup_config, client);
        assert_eq!(cleanup_app.work_item_state(), "Next Merged");
    }

    /// # App Error Message Access
    ///
    /// Tests error message getter and setter.
    ///
    /// ## Test Scenario
    /// - Creates App
    /// - Gets and sets error message
    ///
    /// ## Expected Outcome
    /// - Error message can be read and written
    #[test]
    fn test_app_error_message() {
        let config = create_merge_config();
        let client = create_test_client();
        let mut app = App::new_merge(config, client);

        // Initially None
        assert!(app.error_message().is_none());

        // Set error
        app.set_error_message(Some("Test error".to_string()));
        assert_eq!(app.error_message(), Some("Test error"));

        // Clear error
        app.set_error_message(None);
        assert!(app.error_message().is_none());
    }

    /// # App Version Access
    ///
    /// Tests version getter and setter.
    ///
    /// ## Test Scenario
    /// - Creates App
    /// - Gets and sets version
    ///
    /// ## Expected Outcome
    /// - Version can be read and written
    #[test]
    fn test_app_version() {
        let config = create_merge_config();
        let client = create_test_client();
        let mut app = App::new_merge(config, client);

        // Initially None
        assert!(app.version().is_none());

        // Set version
        app.set_version(Some("1.2.3".to_string()));
        assert_eq!(app.version(), Some("1.2.3"));
    }
}
