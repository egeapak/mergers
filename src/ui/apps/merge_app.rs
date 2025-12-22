//! Merge mode application state.
//!
//! This module provides [`MergeApp`] which contains state specific to
//! the default merge mode (cherry-picking PRs from dev to target branch).

use crate::{
    api::AzureDevOpsClient,
    models::{CherryPickItem, MergeConfig},
    ui::{AppBase, AppMode, browser::BrowserOpener},
};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// Application state for merge (default) mode.
///
/// `MergeApp` handles the workflow of cherry-picking merged PRs from
/// the development branch to a target release branch. It tracks the
/// cherry-pick queue and current progress.
///
/// # Type Safety
///
/// `MergeApp` uses `MergeConfig` as its configuration type, providing
/// compile-time type safety. The work_item_state is accessed directly
/// without pattern matching.
///
/// # Field Access
///
/// Mode-specific fields are accessed directly on `MergeApp`:
/// ```ignore
/// let items = &app.cherry_pick_items;
/// let index = app.current_cherry_pick_index;
/// let state = app.work_item_state(); // Direct access, no pattern matching
/// ```
///
/// Shared fields are accessed via `Deref` to [`AppBase`]:
/// ```ignore
/// let org = app.organization();
/// let prs = &app.pull_requests;
/// ```
pub struct MergeApp {
    /// Shared application state with MergeConfig.
    base: AppBase<MergeConfig>,

    /// Queue of items to cherry-pick.
    pub cherry_pick_items: Vec<CherryPickItem>,

    /// Index of the currently processing cherry-pick item.
    pub current_cherry_pick_index: usize,
}

impl MergeApp {
    /// Creates a new MergeApp with the given configuration, client, and browser opener.
    pub fn new(
        config: Arc<MergeConfig>,
        client: AzureDevOpsClient,
        browser: Box<dyn BrowserOpener>,
    ) -> Self {
        Self {
            base: AppBase::new(config, client, browser),
            cherry_pick_items: Vec::new(),
            current_cherry_pick_index: 0,
        }
    }

    /// Returns the work item state to set after merging.
    ///
    /// This provides direct, type-safe access to the merge-specific
    /// work_item_state configuration without runtime pattern matching.
    pub fn work_item_state(&self) -> &str {
        self.config().work_item_state.value()
    }

    /// Returns the current cherry-pick item, if any.
    pub fn current_cherry_pick(&self) -> Option<&CherryPickItem> {
        self.cherry_pick_items.get(self.current_cherry_pick_index)
    }

    /// Advances to the next cherry-pick item.
    /// Returns true if there are more items to process.
    pub fn advance_cherry_pick(&mut self) -> bool {
        if self.current_cherry_pick_index < self.cherry_pick_items.len() {
            self.current_cherry_pick_index += 1;
            self.current_cherry_pick_index < self.cherry_pick_items.len()
        } else {
            false
        }
    }

    /// Returns the number of remaining cherry-pick items.
    pub fn remaining_cherry_picks(&self) -> usize {
        self.cherry_pick_items
            .len()
            .saturating_sub(self.current_cherry_pick_index)
    }

    /// Returns a reference to the cherry-pick items.
    pub fn cherry_pick_items(&self) -> &Vec<CherryPickItem> {
        &self.cherry_pick_items
    }

    /// Returns a mutable reference to the cherry-pick items.
    pub fn cherry_pick_items_mut(&mut self) -> &mut Vec<CherryPickItem> {
        &mut self.cherry_pick_items
    }

    /// Returns the current cherry-pick index.
    pub fn current_cherry_pick_index(&self) -> usize {
        self.current_cherry_pick_index
    }

    /// Sets the current cherry-pick index.
    pub fn set_current_cherry_pick_index(&mut self, idx: usize) {
        self.current_cherry_pick_index = idx;
    }
}

impl Deref for MergeApp {
    type Target = AppBase<MergeConfig>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for MergeApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl AppMode for MergeApp {
    type Config = MergeConfig;

    fn base(&self) -> &AppBase<MergeConfig> {
        &self.base
    }

    fn base_mut(&mut self) -> &mut AppBase<MergeConfig> {
        &mut self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::SharedConfig, parsed_property::ParsedProperty, ui::browser::MockBrowserOpener,
    };

    fn create_test_config() -> Arc<MergeConfig> {
        Arc::new(MergeConfig {
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
            work_item_state: ParsedProperty::Default("Next Merged".to_string()),
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

    /// # MergeApp Initialization
    ///
    /// Tests that MergeApp initializes correctly.
    ///
    /// ## Test Scenario
    /// - Creates MergeApp with test config
    /// - Verifies all fields are properly initialized
    ///
    /// ## Expected Outcome
    /// - cherry_pick_items is empty
    /// - current_cherry_pick_index is 0
    #[test]
    fn test_merge_app_initialization() {
        let app = MergeApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        assert!(app.cherry_pick_items.is_empty());
        assert_eq!(app.current_cherry_pick_index, 0);
    }

    /// # MergeApp Deref to AppBase
    ///
    /// Tests that Deref works for accessing AppBase fields.
    ///
    /// ## Test Scenario
    /// - Creates MergeApp and accesses AppBase methods via Deref
    ///
    /// ## Expected Outcome
    /// - Can call AppBase methods directly on MergeApp
    #[test]
    fn test_deref_to_app_base() {
        let app = MergeApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Access AppBase methods via Deref
        assert_eq!(app.organization(), "test_org");
        assert_eq!(app.project(), "test_project");
        assert_eq!(app.dev_branch(), "develop");
    }

    /// # MergeApp Work Item State
    ///
    /// Tests the work_item_state() getter.
    ///
    /// ## Test Scenario
    /// - Creates MergeApp with custom work_item_state
    /// - Verifies getter returns correct value
    ///
    /// ## Expected Outcome
    /// - Returns configured work_item_state value
    #[test]
    fn test_work_item_state() {
        let config = Arc::new(MergeConfig {
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
            work_item_state: ParsedProperty::Default("Custom State".to_string()),
        });

        let app = MergeApp::new(
            config,
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );
        assert_eq!(app.work_item_state(), "Custom State");
    }

    /// # MergeApp Cherry Pick Navigation
    ///
    /// Tests cherry-pick item navigation methods.
    ///
    /// ## Test Scenario
    /// - Creates MergeApp with multiple cherry-pick items
    /// - Tests current_cherry_pick, advance_cherry_pick, remaining_cherry_picks
    ///
    /// ## Expected Outcome
    /// - Navigation works correctly through the queue
    #[test]
    fn test_cherry_pick_navigation() {
        let mut app = MergeApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Initially empty
        assert!(app.current_cherry_pick().is_none());
        assert_eq!(app.remaining_cherry_picks(), 0);
        assert!(!app.advance_cherry_pick());

        // Add some items
        app.cherry_pick_items = vec![
            CherryPickItem {
                pr_id: 1,
                commit_id: "abc123".to_string(),
                pr_title: "PR 1".to_string(),
                status: crate::models::CherryPickStatus::Pending,
            },
            CherryPickItem {
                pr_id: 2,
                commit_id: "def456".to_string(),
                pr_title: "PR 2".to_string(),
                status: crate::models::CherryPickStatus::Pending,
            },
        ];

        // Check first item
        assert_eq!(app.current_cherry_pick().unwrap().pr_id, 1);
        assert_eq!(app.remaining_cherry_picks(), 2);

        // Advance
        assert!(app.advance_cherry_pick());
        assert_eq!(app.current_cherry_pick().unwrap().pr_id, 2);
        assert_eq!(app.remaining_cherry_picks(), 1);

        // Advance again (last item)
        assert!(!app.advance_cherry_pick());
        assert!(app.current_cherry_pick().is_none());
        assert_eq!(app.remaining_cherry_picks(), 0);
    }

    /// # MergeApp DerefMut
    ///
    /// Tests that DerefMut works for mutable access to AppBase.
    ///
    /// ## Test Scenario
    /// - Creates MergeApp and mutates AppBase fields
    ///
    /// ## Expected Outcome
    /// - Can mutate AppBase fields via DerefMut
    #[test]
    fn test_deref_mut() {
        let mut app = MergeApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Mutate AppBase field via DerefMut
        app.version = Some("1.0.0".to_string());
        assert_eq!(app.version, Some("1.0.0".to_string()));

        app.error_message = Some("Test error".to_string());
        assert_eq!(app.error_message, Some("Test error".to_string()));
    }

    /// # MergeApp AppMode Trait
    ///
    /// Tests AppMode trait implementation.
    ///
    /// ## Test Scenario
    /// - Creates MergeApp and uses trait methods
    ///
    /// ## Expected Outcome
    /// - base() and base_mut() work correctly
    #[test]
    fn test_app_mode_trait() {
        let mut app = MergeApp::new(
            create_test_config(),
            create_test_client(),
            Box::new(MockBrowserOpener::new()),
        );

        // Test base()
        assert_eq!(app.base().organization(), "test_org");

        // Test base_mut()
        app.base_mut().version = Some("2.0.0".to_string());
        assert_eq!(app.version, Some("2.0.0".to_string()));
    }
}
