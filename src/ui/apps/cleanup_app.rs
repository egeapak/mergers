//! Cleanup mode application state.
//!
//! This module provides [`CleanupApp`] which contains state specific to
//! the branch cleanup mode.

use crate::{
    api::AzureDevOpsClient,
    models::{AppConfig, CleanupBranch},
    ui::{AppBase, AppMode, browser::BrowserOpener},
};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// Application state for cleanup mode.
///
/// `CleanupApp` handles the workflow of cleaning up merged branches
/// from the repository. It tracks which branches are candidates for
/// deletion and their selection state.
///
/// # Field Access
///
/// Mode-specific fields are accessed directly on `CleanupApp`:
/// ```ignore
/// for branch in &app.cleanup_branches {
///     // process branch...
/// }
/// ```
///
/// Shared fields are accessed via `Deref` to [`AppBase`]:
/// ```ignore
/// let org = app.organization();
/// let prs = &app.pull_requests;
/// ```
pub struct CleanupApp {
    /// Shared application state.
    base: AppBase,

    /// Branches that are candidates for cleanup.
    pub cleanup_branches: Vec<CleanupBranch>,
}

impl CleanupApp {
    /// Creates a new CleanupApp with the given configuration, client, and browser opener.
    pub fn new(
        config: Arc<AppConfig>,
        client: AzureDevOpsClient,
        browser: Box<dyn BrowserOpener>,
    ) -> Self {
        Self {
            base: AppBase::new(config, client, browser),
            cleanup_branches: Vec::new(),
        }
    }

    /// Returns the cleanup target branch.
    pub fn cleanup_target(&self) -> &str {
        match &*self.config {
            AppConfig::Cleanup { cleanup, .. } => cleanup.target.value(),
            _ => self.target_branch(), // fallback to shared target_branch
        }
    }

    /// Returns the number of branches selected for cleanup.
    pub fn selected_count(&self) -> usize {
        self.cleanup_branches.iter().filter(|b| b.selected).count()
    }

    /// Returns the total number of cleanup branch candidates.
    pub fn total_count(&self) -> usize {
        self.cleanup_branches.len()
    }

    /// Toggles selection state for a branch at the given index.
    pub fn toggle_branch(&mut self, index: usize) {
        if let Some(branch) = self.cleanup_branches.get_mut(index) {
            branch.selected = !branch.selected;
        }
    }

    /// Selects all branches for cleanup.
    pub fn select_all(&mut self) {
        for branch in &mut self.cleanup_branches {
            branch.selected = true;
        }
    }

    /// Deselects all branches.
    pub fn deselect_all(&mut self) {
        for branch in &mut self.cleanup_branches {
            branch.selected = false;
        }
    }

    /// Returns selected branches for cleanup.
    pub fn get_selected_branches(&self) -> Vec<&CleanupBranch> {
        self.cleanup_branches
            .iter()
            .filter(|b| b.selected)
            .collect()
    }

    // ========================================================================
    // Field Accessors (for AppState compatibility)
    // ========================================================================

    /// Returns a reference to the cleanup branches.
    pub fn cleanup_branches(&self) -> &Vec<CleanupBranch> {
        &self.cleanup_branches
    }

    /// Returns a mutable reference to the cleanup branches.
    pub fn cleanup_branches_mut(&mut self) -> &mut Vec<CleanupBranch> {
        &mut self.cleanup_branches
    }
}

impl Deref for CleanupApp {
    type Target = AppBase;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for CleanupApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl AppMode for CleanupApp {
    fn base(&self) -> &AppBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AppBase {
        &mut self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{CleanupModeConfig, SharedConfig},
        parsed_property::ParsedProperty,
        ui::browser::MockBrowserOpener,
    };

    fn create_test_config() -> Arc<AppConfig> {
        Arc::new(AppConfig::Cleanup {
            shared: SharedConfig {
                organization: ParsedProperty::Default("test_org".to_string()),
                project: ParsedProperty::Default("test_project".to_string()),
                repository: ParsedProperty::Default("test_repo".to_string()),
                pat: ParsedProperty::Default("test_pat".to_string()),
                dev_branch: ParsedProperty::Default("develop".to_string()),
                target_branch: ParsedProperty::Default("main".to_string()),
                local_repo: None,
                parallel_limit: ParsedProperty::Default(300),
                max_concurrent_network: ParsedProperty::Default(100),
                max_concurrent_processing: ParsedProperty::Default(10),
                tag_prefix: ParsedProperty::Default("merged/".to_string()),
                since: None,
                skip_confirmation: false,
            },
            cleanup: CleanupModeConfig {
                target: ParsedProperty::Default("release/1.0".to_string()),
            },
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

    /// # CleanupApp Initialization
    ///
    /// Tests that CleanupApp initializes correctly.
    ///
    /// ## Test Scenario
    /// - Creates CleanupApp with test config
    /// - Verifies all fields are properly initialized
    ///
    /// ## Expected Outcome
    /// - cleanup_branches is empty initially
    #[test]
    fn test_cleanup_app_initialization() {
        let app = CleanupApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        assert!(app.cleanup_branches.is_empty());
        assert_eq!(app.total_count(), 0);
        assert_eq!(app.selected_count(), 0);
    }

    /// # CleanupApp Deref to AppBase
    ///
    /// Tests that Deref works for accessing AppBase fields.
    ///
    /// ## Test Scenario
    /// - Creates CleanupApp and accesses AppBase methods via Deref
    ///
    /// ## Expected Outcome
    /// - Can call AppBase methods directly on CleanupApp
    #[test]
    fn test_deref_to_app_base() {
        let app = CleanupApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Access AppBase methods via Deref
        assert_eq!(app.organization(), "test_org");
        assert_eq!(app.project(), "test_project");
        assert_eq!(app.dev_branch(), "develop");
    }

    /// # CleanupApp Cleanup Target
    ///
    /// Tests the cleanup_target() getter.
    ///
    /// ## Test Scenario
    /// - Creates CleanupApp with configured cleanup target
    /// - Verifies getter returns correct value
    ///
    /// ## Expected Outcome
    /// - Returns configured cleanup target
    #[test]
    fn test_cleanup_target() {
        let app = CleanupApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );
        assert_eq!(app.cleanup_target(), "release/1.0");
    }

    /// # CleanupApp Branch Selection
    ///
    /// Tests branch selection methods.
    ///
    /// ## Test Scenario
    /// - Creates CleanupApp with branches
    /// - Tests toggle, select_all, deselect_all
    ///
    /// ## Expected Outcome
    /// - Selection state changes correctly
    #[test]
    fn test_branch_selection() {
        use crate::models::CleanupStatus;

        let mut app = CleanupApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Add some test branches
        app.cleanup_branches = vec![
            CleanupBranch {
                name: "feature/a".to_string(),
                target: "main".to_string(),
                version: "1.0".to_string(),
                is_merged: true,
                selected: false,
                status: CleanupStatus::Pending,
            },
            CleanupBranch {
                name: "feature/b".to_string(),
                target: "main".to_string(),
                version: "1.0".to_string(),
                is_merged: true,
                selected: false,
                status: CleanupStatus::Pending,
            },
            CleanupBranch {
                name: "feature/c".to_string(),
                target: "main".to_string(),
                version: "1.0".to_string(),
                is_merged: false,
                selected: false,
                status: CleanupStatus::Pending,
            },
        ];

        assert_eq!(app.total_count(), 3);
        assert_eq!(app.selected_count(), 0);

        // Toggle first branch
        app.toggle_branch(0);
        assert_eq!(app.selected_count(), 1);
        assert!(app.cleanup_branches[0].selected);

        // Select all
        app.select_all();
        assert_eq!(app.selected_count(), 3);

        // Deselect all
        app.deselect_all();
        assert_eq!(app.selected_count(), 0);
    }

    /// # CleanupApp Get Selected Branches
    ///
    /// Tests get_selected_branches() method.
    ///
    /// ## Test Scenario
    /// - Creates CleanupApp with mix of selected branches
    /// - Calls get_selected_branches()
    ///
    /// ## Expected Outcome
    /// - Only returns selected branches
    #[test]
    fn test_get_selected_branches() {
        use crate::models::CleanupStatus;

        let mut app = CleanupApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        app.cleanup_branches = vec![
            CleanupBranch {
                name: "feature/a".to_string(),
                target: "main".to_string(),
                version: "1.0".to_string(),
                is_merged: true,
                selected: true,
                status: CleanupStatus::Pending,
            },
            CleanupBranch {
                name: "feature/b".to_string(),
                target: "main".to_string(),
                version: "1.0".to_string(),
                is_merged: true,
                selected: false,
                status: CleanupStatus::Pending,
            },
            CleanupBranch {
                name: "feature/c".to_string(),
                target: "main".to_string(),
                version: "1.0".to_string(),
                is_merged: false,
                selected: true,
                status: CleanupStatus::Pending,
            },
        ];

        let selected = app.get_selected_branches();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].name, "feature/a");
        assert_eq!(selected[1].name, "feature/c");
    }

    /// # CleanupApp AppMode Trait
    ///
    /// Tests AppMode trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates CleanupApp and uses trait methods
    ///
    /// ## Expected Outcome
    /// - base() and base_mut() work correctly
    #[test]
    fn test_app_mode_trait() {
        let mut app = CleanupApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Test base()
        assert_eq!(app.base().organization(), "test_org");

        // Test base_mut()
        app.base_mut().version = Some("1.0.0".to_string());
        assert_eq!(app.version, Some("1.0.0".to_string()));
    }
}
