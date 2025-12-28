//! Merge mode application state.
//!
//! This module provides [`MergeApp`] which contains state specific to
//! the default merge mode (cherry-picking PRs from dev to target branch).

use crate::{
    api::AzureDevOpsClient,
    core::operations::PRDependencyGraph,
    core::state::{LockGuard, MergePhase, MergeStateFile, StateCherryPickItem, StateItemStatus},
    models::{CherryPickItem, CherryPickStatus, MergeConfig},
    ui::{AppBase, AppMode, browser::BrowserOpener},
};
use anyhow::Result;
use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
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

    /// State file for cross-mode resume support.
    /// Created during repo setup, updated during cherry-picks.
    state_file: Option<MergeStateFile>,

    /// Lock guard for exclusive merge access.
    /// Held for the duration of the TUI session.
    #[allow(dead_code)]
    lock_guard: Option<LockGuard>,

    /// Cached dependency analysis result.
    /// Populated during data loading, before PR selection.
    dependency_graph: Option<PRDependencyGraph>,
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
            state_file: None,
            lock_guard: None,
            dependency_graph: None,
        }
    }

    /// Returns the work item state to set after merging.
    ///
    /// This provides direct, type-safe access to the merge-specific
    /// work_item_state configuration without runtime pattern matching.
    pub fn work_item_state(&self) -> &str {
        self.config().work_item_state.value()
    }

    /// Returns whether to run git hooks during cherry-pick operations.
    ///
    /// This provides direct, type-safe access to the merge-specific
    /// run_hooks configuration without runtime pattern matching.
    /// When false (the default), hooks are disabled at repo initialization.
    pub fn run_hooks(&self) -> bool {
        *self.config().run_hooks.value()
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

    // ==========================================================================
    // State File Management (for cross-mode resume support)
    // ==========================================================================

    /// Creates a new state file for the merge operation.
    ///
    /// This is called during repo setup to establish the state file for
    /// potential cross-mode resume (TUI → CLI). Also acquires a lock for
    /// exclusive merge access.
    pub fn create_state_file(
        &mut self,
        repo_path: PathBuf,
        base_repo_path: Option<PathBuf>,
        is_worktree: bool,
        merge_version: &str,
    ) -> Result<PathBuf> {
        // Acquire lock first to ensure exclusive access
        match LockGuard::acquire(&repo_path) {
            Ok(Some(guard)) => {
                self.lock_guard = Some(guard);
            }
            Ok(None) => {
                return Err(anyhow::anyhow!(
                    "Another merge operation is in progress for this repository"
                ));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to acquire lock: {}", e));
            }
        }

        let config = self.config();
        let state_file = MergeStateFile::new(
            repo_path,
            base_repo_path,
            is_worktree,
            config.shared.organization.value().clone(),
            config.shared.project.value().clone(),
            config.shared.repository.value().clone(),
            config.shared.dev_branch.value().clone(),
            config.shared.target_branch.value().clone(),
            merge_version.to_string(),
            config.work_item_state.value().clone(),
            config.shared.tag_prefix.value().clone(),
            *config.run_hooks.value(),
        );
        self.state_file = Some(state_file);
        self.save_state_file()
    }

    /// Sets the state file (for resuming from existing state).
    pub fn set_state_file(&mut self, state_file: MergeStateFile) {
        self.state_file = Some(state_file);
    }

    /// Sets the lock guard for exclusive merge access.
    pub fn set_lock_guard(&mut self, lock_guard: LockGuard) {
        self.lock_guard = Some(lock_guard);
    }

    /// Returns a reference to the state file, if any.
    pub fn state_file(&self) -> Option<&MergeStateFile> {
        self.state_file.as_ref()
    }

    /// Returns a mutable reference to the state file, if any.
    pub fn state_file_mut(&mut self) -> Option<&mut MergeStateFile> {
        self.state_file.as_mut()
    }

    /// Updates the phase in the state file and saves.
    pub fn update_state_phase(&mut self, phase: MergePhase) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            let path = state_file.set_phase(phase)?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Sets the cherry-pick items in the state file.
    ///
    /// Converts TUI CherryPickItems to StateCherryPickItems.
    /// Work item IDs should be provided via the work_items_map.
    pub fn set_state_cherry_pick_items(
        &mut self,
        work_items_map: &std::collections::HashMap<i32, Vec<i32>>,
    ) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            let state_items: Vec<StateCherryPickItem> = self
                .cherry_pick_items
                .iter()
                .map(|item| StateCherryPickItem {
                    commit_id: item.commit_id.clone(),
                    pr_id: item.pr_id,
                    pr_title: item.pr_title.clone(),
                    status: cherry_pick_status_to_state(&item.status),
                    work_item_ids: work_items_map.get(&item.pr_id).cloned().unwrap_or_default(),
                })
                .collect();
            state_file.cherry_pick_items = state_items;
            state_file.current_index = self.current_cherry_pick_index;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Updates the status of a cherry-pick item in the state file.
    pub fn update_state_item_status(
        &mut self,
        index: usize,
        status: StateItemStatus,
    ) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            if let Some(item) = state_file.cherry_pick_items.get_mut(index) {
                item.status = status;
            }
            state_file.current_index = self.current_cherry_pick_index;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Syncs the current cherry-pick index to the state file.
    pub fn sync_state_current_index(&mut self) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.current_index = self.current_cherry_pick_index;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Sets conflicted files in the state file.
    pub fn set_state_conflicted_files(&mut self, files: Vec<String>) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.conflicted_files = Some(files);
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Clears conflicted files in the state file.
    pub fn clear_state_conflicted_files(&mut self) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.conflicted_files = None;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Saves the current state file to disk.
    fn save_state_file(&mut self) -> Result<PathBuf> {
        self.state_file
            .as_mut()
            .expect("state_file must be set before saving")
            .save_for_repo()
    }

    /// Removes the state file from disk and clears it from memory.
    pub fn cleanup_state_file(&mut self) -> Result<()> {
        if let Some(ref state_file) = self.state_file {
            let path = crate::core::state::path_for_repo(&state_file.repo_path)?;
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        self.state_file = None;
        self.lock_guard = None;
        Ok(())
    }

    /// Returns the repo path from the state file, if any.
    pub fn state_repo_path(&self) -> Option<&Path> {
        self.state_file.as_ref().map(|s| s.repo_path.as_path())
    }

    // ==========================================================================
    // Dependency Graph Management
    // ==========================================================================

    /// Returns a reference to the dependency graph, if computed.
    pub fn dependency_graph(&self) -> Option<&PRDependencyGraph> {
        self.dependency_graph.as_ref()
    }

    /// Sets the dependency graph after analysis.
    pub fn set_dependency_graph(&mut self, graph: PRDependencyGraph) {
        self.dependency_graph = Some(graph);
    }

    /// Clears the cached dependency graph.
    #[allow(dead_code)]
    pub fn clear_dependency_graph(&mut self) {
        self.dependency_graph = None;
    }
}

// ==========================================================================
// Conversion Functions
// ==========================================================================

/// Converts a TUI CherryPickStatus to a state file StateItemStatus.
fn cherry_pick_status_to_state(status: &CherryPickStatus) -> StateItemStatus {
    match status {
        CherryPickStatus::Pending => StateItemStatus::Pending,
        CherryPickStatus::InProgress => StateItemStatus::Pending, // In-progress maps to pending in state
        CherryPickStatus::Success => StateItemStatus::Success,
        CherryPickStatus::Conflict => StateItemStatus::Conflict,
        CherryPickStatus::Skipped => StateItemStatus::Skipped,
        CherryPickStatus::Failed(msg) => StateItemStatus::Failed {
            message: msg.clone(),
        },
    }
}

/// Converts a state file StateItemStatus to a TUI CherryPickStatus.
#[allow(dead_code)]
fn state_status_to_cherry_pick(status: &StateItemStatus) -> CherryPickStatus {
    match status {
        StateItemStatus::Pending => CherryPickStatus::Pending,
        StateItemStatus::Success => CherryPickStatus::Success,
        StateItemStatus::Conflict => CherryPickStatus::Conflict,
        StateItemStatus::Skipped => CherryPickStatus::Skipped,
        StateItemStatus::Failed { message } => CherryPickStatus::Failed(message.clone()),
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
            run_hooks: ParsedProperty::Default(false),
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
            run_hooks: ParsedProperty::Default(false),
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
