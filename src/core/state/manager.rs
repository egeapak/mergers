//! State manager for merge operations.
//!
//! This module provides [`StateManager`], a centralized manager for state file
//! and lock handling that can be shared across components via `Arc<Mutex<StateManager>>`.

use crate::core::state::{
    LockGuard, MergePhase, MergeStateFile, MergeStateFileBuilder, MergeStatus, StateCherryPickItem,
    StateItemStatus,
};
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuration for state file creation.
///
/// This struct contains all the configuration needed to create a state file,
/// extracted from the application config. It's passed as a parameter rather
/// than owned by [`StateManager`] to allow flexible usage patterns.
#[derive(Debug, Clone)]
pub struct StateCreateConfig {
    /// Azure DevOps organization name.
    pub organization: String,
    /// Azure DevOps project name.
    pub project: String,
    /// Azure DevOps repository name.
    pub repository: String,
    /// Source branch for PRs.
    pub dev_branch: String,
    /// Target branch for cherry-picks.
    pub target_branch: String,
    /// Prefix for PR tags.
    pub tag_prefix: String,
    /// State to set work items to after completion.
    pub work_item_state: String,
    /// Whether git hooks are enabled for this merge.
    pub run_hooks: bool,
}

/// Manages state file and lock for merge operations.
///
/// `StateManager` provides a centralized location for state file management
/// that can be shared via `Arc<Mutex<StateManager>>`. This allows background
/// tasks to create and update state files without requiring mutable access
/// to the entire application.
///
/// # Thread Safety
///
/// `StateManager` is designed to be used within a `Mutex`. All methods that
/// modify state require `&mut self`, ensuring proper synchronization when
/// wrapped in `Arc<Mutex<StateManager>>`.
///
/// # Example
///
/// ```ignore
/// use std::sync::{Arc, Mutex};
///
/// let manager = Arc::new(Mutex::new(StateManager::new()));
///
/// // In a background task
/// {
///     let mut mgr = manager.lock().unwrap();
///     mgr.create_state_file(repo_path, base_path, false, "v1.0.0", &config)?;
/// }
/// ```
#[derive(Debug)]
pub struct StateManager {
    /// State file for cross-mode resume support.
    state_file: Option<MergeStateFile>,
    /// Lock guard for exclusive merge access.
    lock_guard: Option<LockGuard>,
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StateManager {
    /// Creates a new empty StateManager.
    ///
    /// The manager starts with no state file or lock. Use [`create_state_file`]
    /// to initialize state or [`set_state_file`]/[`set_lock_guard`] to restore
    /// from an existing state.
    ///
    /// [`create_state_file`]: Self::create_state_file
    /// [`set_state_file`]: Self::set_state_file
    /// [`set_lock_guard`]: Self::set_lock_guard
    pub fn new() -> Self {
        Self {
            state_file: None,
            lock_guard: None,
        }
    }

    /// Creates a new state file and acquires a lock.
    ///
    /// This method:
    /// 1. Acquires a lock for exclusive merge access
    /// 2. Creates a new state file with the provided configuration
    /// 3. Saves the state file to disk
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the worktree or cloned repository
    /// * `base_repo_path` - Path to the base repository (for worktrees)
    /// * `is_worktree` - Whether this is a worktree or a clone
    /// * `version` - Merge version string (e.g., "v1.2.3")
    /// * `config` - Configuration for state file creation
    ///
    /// # Returns
    ///
    /// The path where the state file was saved.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Another merge operation is in progress (lock acquisition fails)
    /// - Failed to create or save the state file
    pub fn create_state_file(
        &mut self,
        repo_path: PathBuf,
        base_repo_path: Option<PathBuf>,
        is_worktree: bool,
        version: &str,
        config: &StateCreateConfig,
    ) -> Result<PathBuf> {
        // Acquire lock first to ensure exclusive access
        let guard = match LockGuard::acquire(&repo_path) {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                return Err(anyhow::anyhow!(
                    "Another merge operation is in progress for this repository"
                ));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to acquire lock: {}", e));
            }
        };

        // Build the state file using the builder pattern
        let mut builder = MergeStateFileBuilder::new()
            .repo_path(&repo_path)
            .is_worktree(is_worktree)
            .organization(&config.organization)
            .project(&config.project)
            .repository(&config.repository)
            .dev_branch(&config.dev_branch)
            .target_branch(&config.target_branch)
            .merge_version(version)
            .work_item_state(&config.work_item_state)
            .tag_prefix(&config.tag_prefix)
            .run_hooks(config.run_hooks);

        if let Some(base_path) = base_repo_path {
            builder = builder.base_repo_path(base_path);
        }

        let state_file = builder.build();
        self.state_file = Some(state_file);

        // Save - if it fails, clean up state_file but don't store the lock guard
        match self.save_state_file() {
            Ok(path) => {
                // Only store lock guard after successful save
                self.lock_guard = Some(guard);
                Ok(path)
            }
            Err(e) => {
                // Clean up state_file on failure
                self.state_file = None;
                Err(e)
            }
        }
    }

    /// Sets the state file (for resuming from existing state).
    ///
    /// Use this when loading a state file from disk to continue a merge operation.
    pub fn set_state_file(&mut self, state_file: MergeStateFile) {
        self.state_file = Some(state_file);
    }

    /// Sets the lock guard for exclusive merge access.
    ///
    /// Use this when restoring state from an existing merge operation.
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

    /// Returns the repo path from the state file, if any.
    pub fn state_repo_path(&self) -> Option<&Path> {
        self.state_file.as_ref().map(|s| s.repo_path.as_path())
    }

    /// Updates the phase in the state file and saves.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn update_phase(&mut self, phase: MergePhase) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            let path = state_file.set_phase(phase)?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Updates the status of a cherry-pick item in the state file.
    ///
    /// # Arguments
    ///
    /// * `index` - Index of the item in the cherry_pick_items list
    /// * `status` - New status for the item
    /// * `current_index` - Current cherry-pick index to sync
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn update_item_status(
        &mut self,
        index: usize,
        status: StateItemStatus,
        current_index: usize,
    ) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            if let Some(item) = state_file.cherry_pick_items.get_mut(index) {
                item.status = status;
            }
            state_file.current_index = current_index;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Syncs the current cherry-pick index to the state file.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn sync_current_index(&mut self, current_index: usize) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.current_index = current_index;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Sets the cherry-pick items in the state file.
    ///
    /// Converts TUI CherryPickItems to StateCherryPickItems.
    ///
    /// # Arguments
    ///
    /// * `items` - List of cherry-pick items to set
    /// * `work_items_map` - Map of PR ID to work item IDs
    /// * `current_index` - Current cherry-pick index
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn set_cherry_pick_items(
        &mut self,
        items: Vec<StateCherryPickItem>,
        work_items_map: &HashMap<i32, Vec<i32>>,
        current_index: usize,
    ) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            // Add work item IDs to items from the map
            let state_items: Vec<StateCherryPickItem> = items
                .into_iter()
                .map(|mut item| {
                    item.work_item_ids =
                        work_items_map.get(&item.pr_id).cloned().unwrap_or_default();
                    item
                })
                .collect();

            state_file.cherry_pick_items = state_items;
            state_file.current_index = current_index;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Sets conflicted files in the state file.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn set_conflicted_files(&mut self, files: Vec<String>) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.conflicted_files = Some(files);
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Clears conflicted files in the state file.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn clear_conflicted_files(&mut self) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.conflicted_files = None;
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Removes the state file from disk and releases the lock.
    ///
    /// This should be called when the merge operation is complete or aborted
    /// and the state file is no longer needed.
    pub fn cleanup(&mut self) -> Result<()> {
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

    // ==========================================================================
    // Convenience methods for MergeEngine integration
    // ==========================================================================

    /// Returns whether a state file is currently set.
    pub fn has_state_file(&self) -> bool {
        self.state_file.is_some()
    }

    /// Returns whether a lock is currently held.
    pub fn has_lock(&self) -> bool {
        self.lock_guard.is_some()
    }

    /// Creates a state file with cherry-pick items already populated.
    ///
    /// Used by non-interactive runner which has PRs ready at creation time.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the worktree or cloned repository
    /// * `base_repo_path` - Path to the base repository (for worktrees)
    /// * `is_worktree` - Whether this is a worktree or a clone
    /// * `version` - Merge version string (e.g., "v1.2.3")
    /// * `config` - Configuration for state file creation
    /// * `items` - Cherry-pick items to populate
    ///
    /// # Returns
    ///
    /// The path where the state file was saved.
    pub fn create_state_file_with_items(
        &mut self,
        repo_path: PathBuf,
        base_repo_path: Option<PathBuf>,
        is_worktree: bool,
        version: &str,
        config: &StateCreateConfig,
        items: Vec<StateCherryPickItem>,
    ) -> Result<PathBuf> {
        let path =
            self.create_state_file(repo_path, base_repo_path, is_worktree, version, config)?;

        if let Some(ref mut state_file) = self.state_file {
            state_file.cherry_pick_items = items;
            state_file.phase = MergePhase::CherryPicking;
            state_file.save_for_repo()?;
        }

        Ok(path)
    }

    /// Updates final status and completion timestamp.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn set_final_status(&mut self, status: MergeStatus) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            state_file.final_status = Some(status);
            state_file.completed_at = Some(chrono::Utc::now());
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    /// Saves the current state file to disk.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(path))` - The path where the state file was saved
    /// * `Ok(None)` - No state file is set (operation is a no-op)
    /// * `Err` - Failed to save the state file
    pub fn save(&mut self) -> Result<Option<PathBuf>> {
        if let Some(ref mut state_file) = self.state_file {
            let path = state_file.save_for_repo()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    // ==========================================================================
    // Internal methods
    // ==========================================================================

    /// Saves the current state file to disk (internal helper).
    fn save_state_file(&mut self) -> Result<PathBuf> {
        self.state_file
            .as_mut()
            .expect("state_file must be set before saving")
            .save_for_repo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{STATE_DIR_ENV, StateItemStatus};
    use serial_test::serial;
    use tempfile::TempDir;

    fn create_test_config() -> StateCreateConfig {
        StateCreateConfig {
            organization: "test-org".to_string(),
            project: "test-project".to_string(),
            repository: "test-repo".to_string(),
            dev_branch: "develop".to_string(),
            target_branch: "main".to_string(),
            tag_prefix: "merged/".to_string(),
            work_item_state: "Next Merged".to_string(),
            run_hooks: false,
        }
    }

    /// # StateManager New
    ///
    /// Verifies that StateManager::new() creates an empty manager.
    ///
    /// ## Test Scenario
    /// - Creates a new StateManager
    ///
    /// ## Expected Outcome
    /// - state_file is None
    /// - state_repo_path returns None
    #[test]
    fn test_state_manager_new() {
        let manager = StateManager::new();
        assert!(manager.state_file().is_none());
        assert!(manager.state_repo_path().is_none());
    }

    /// # StateManager Default
    ///
    /// Verifies that Default implementation works.
    ///
    /// ## Test Scenario
    /// - Creates StateManager using default()
    ///
    /// ## Expected Outcome
    /// - Same as new()
    #[test]
    fn test_state_manager_default() {
        let manager = StateManager::default();
        assert!(manager.state_file().is_none());
    }

    /// # Create State File
    ///
    /// Verifies that create_state_file creates and saves a state file.
    ///
    /// ## Test Scenario
    /// - Creates temp directory as state dir
    /// - Creates temp directory as repo
    /// - Calls create_state_file
    ///
    /// ## Expected Outcome
    /// - State file is created and saved
    /// - Lock is acquired
    /// - state_file() returns Some
    #[test]
    #[serial]
    fn test_create_state_file() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();

        let result = manager.create_state_file(
            temp_repo.path().to_path_buf(),
            None,
            false,
            "v1.0.0",
            &config,
        );

        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.exists());
        assert!(manager.state_file().is_some());
        assert_eq!(manager.state_repo_path().unwrap(), temp_repo.path());

        // Verify state file contents
        let state = manager.state_file().unwrap();
        assert_eq!(state.organization, "test-org");
        assert_eq!(state.project, "test-project");
        assert_eq!(state.merge_version, "v1.0.0");
        assert!(!state.run_hooks);

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Update Phase
    ///
    /// Verifies that update_phase updates and saves the state file.
    ///
    /// ## Test Scenario
    /// - Creates a state file
    /// - Updates the phase to CherryPicking
    ///
    /// ## Expected Outcome
    /// - Phase is updated
    /// - File is saved
    #[test]
    #[serial]
    fn test_update_phase() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        let result = manager.update_phase(MergePhase::CherryPicking);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
        assert_eq!(
            manager.state_file().unwrap().phase,
            MergePhase::CherryPicking
        );

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Update Phase No State
    ///
    /// Verifies that update_phase returns Ok(None) when no state file is set.
    ///
    /// ## Test Scenario
    /// - Creates manager without state file
    /// - Calls update_phase
    ///
    /// ## Expected Outcome
    /// - Returns Ok(None)
    #[test]
    fn test_update_phase_no_state() {
        let mut manager = StateManager::new();
        let result = manager.update_phase(MergePhase::CherryPicking);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// # Update Item Status
    ///
    /// Verifies that update_item_status updates the item and current index.
    ///
    /// ## Test Scenario
    /// - Creates a state file with cherry-pick items
    /// - Updates an item's status
    ///
    /// ## Expected Outcome
    /// - Item status is updated
    /// - Current index is updated
    #[test]
    #[serial]
    fn test_update_item_status() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        // Add a cherry-pick item
        manager
            .state_file_mut()
            .unwrap()
            .cherry_pick_items
            .push(StateCherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 42,
                pr_title: "Test PR".to_string(),
                status: StateItemStatus::Pending,
                work_item_ids: vec![],
            });

        let result = manager.update_item_status(0, StateItemStatus::Success, 1);
        assert!(result.is_ok());

        let state = manager.state_file().unwrap();
        assert_eq!(state.cherry_pick_items[0].status, StateItemStatus::Success);
        assert_eq!(state.current_index, 1);

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Cleanup
    ///
    /// Verifies that cleanup removes the state file and releases the lock.
    ///
    /// ## Test Scenario
    /// - Creates a state file
    /// - Calls cleanup
    ///
    /// ## Expected Outcome
    /// - State file is removed from disk
    /// - state_file() returns None
    #[test]
    #[serial]
    fn test_cleanup() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        let path = manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        assert!(path.exists());
        assert!(manager.state_file().is_some());

        manager.cleanup().unwrap();

        assert!(!path.exists());
        assert!(manager.state_file().is_none());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Thread Safety
    ///
    /// Verifies that StateManager can be used with Arc<Mutex<>>.
    ///
    /// ## Test Scenario
    /// - Creates StateManager wrapped in Arc<Mutex<>>
    /// - Accesses from multiple scopes
    ///
    /// ## Expected Outcome
    /// - Compiles and works correctly
    #[test]
    fn test_thread_safety() {
        use std::sync::{Arc, Mutex};

        let manager = Arc::new(Mutex::new(StateManager::new()));

        // Access from "thread 1"
        {
            let mgr = manager.lock().unwrap();
            assert!(mgr.state_file().is_none());
        }

        // Access from "thread 2"
        {
            let mgr = manager.lock().unwrap();
            assert!(mgr.state_repo_path().is_none());
        }
    }

    /// # Set State File
    ///
    /// Verifies that set_state_file correctly sets the state file.
    ///
    /// ## Test Scenario
    /// - Creates a state file externally
    /// - Sets it on the manager
    ///
    /// ## Expected Outcome
    /// - state_file() returns the set file
    #[test]
    fn test_set_state_file() {
        let mut manager = StateManager::new();

        let state_file = MergeStateFile::builder()
            .repo_path("/tmp/repo")
            .organization("org")
            .project("proj")
            .repository("repo")
            .dev_branch("dev")
            .target_branch("main")
            .merge_version("v1.0.0")
            .work_item_state("Done")
            .tag_prefix("merged/")
            .build();

        manager.set_state_file(state_file);

        assert!(manager.state_file().is_some());
        assert_eq!(manager.state_file().unwrap().organization, "org");
    }

    /// # Set Conflicted Files
    ///
    /// Verifies that set_conflicted_files and clear_conflicted_files work.
    ///
    /// ## Test Scenario
    /// - Creates a state file
    /// - Sets conflicted files
    /// - Clears conflicted files
    ///
    /// ## Expected Outcome
    /// - Conflicted files are set and cleared correctly
    #[test]
    #[serial]
    fn test_conflicted_files() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        // Set conflicted files
        let files = vec!["file1.rs".to_string(), "file2.rs".to_string()];
        manager.set_conflicted_files(files.clone()).unwrap();
        assert_eq!(manager.state_file().unwrap().conflicted_files, Some(files));

        // Clear conflicted files
        manager.clear_conflicted_files().unwrap();
        assert!(manager.state_file().unwrap().conflicted_files.is_none());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Sync Current Index
    ///
    /// Verifies that sync_current_index updates the current index.
    ///
    /// ## Test Scenario
    /// - Creates a state file
    /// - Syncs current index
    ///
    /// ## Expected Outcome
    /// - Current index is updated
    #[test]
    #[serial]
    fn test_sync_current_index() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        manager.sync_current_index(5).unwrap();
        assert_eq!(manager.state_file().unwrap().current_index, 5);

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Create State File With Worktree
    ///
    /// Verifies that create_state_file works with worktree mode.
    ///
    /// ## Test Scenario
    /// - Creates state file with base_repo_path and is_worktree=true
    ///
    /// ## Expected Outcome
    /// - State file has correct worktree settings
    #[test]
    #[serial]
    fn test_create_state_file_worktree() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();
        let temp_base = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();

        let result = manager.create_state_file(
            temp_repo.path().to_path_buf(),
            Some(temp_base.path().to_path_buf()),
            true,
            "v1.0.0",
            &config,
        );

        assert!(result.is_ok());
        let state = manager.state_file().unwrap();
        assert!(state.is_worktree);
        assert_eq!(state.base_repo_path, Some(temp_base.path().to_path_buf()));

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Lock Prevents Second State File
    ///
    /// Verifies that creating a second state file fails due to lock.
    ///
    /// ## Test Scenario
    /// - Creates first state file
    /// - Tries to create second state file for same repo
    ///
    /// ## Expected Outcome
    /// - Second creation fails with lock error
    #[test]
    #[serial]
    fn test_lock_prevents_second_state_file() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager1 = StateManager::new();
        let mut manager2 = StateManager::new();
        let config = create_test_config();

        // First creation succeeds
        let result1 = manager1.create_state_file(
            temp_repo.path().to_path_buf(),
            None,
            false,
            "v1.0.0",
            &config,
        );
        assert!(result1.is_ok());

        // Second creation fails
        let result2 = manager2.create_state_file(
            temp_repo.path().to_path_buf(),
            None,
            false,
            "v2.0.0",
            &config,
        );
        assert!(result2.is_err());
        assert!(
            result2
                .unwrap_err()
                .to_string()
                .contains("Another merge operation")
        );

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Has State File
    ///
    /// Verifies that has_state_file() returns correct value.
    ///
    /// ## Test Scenario
    /// - Creates manager without state file
    /// - Creates state file
    ///
    /// ## Expected Outcome
    /// - Returns false initially, true after creation
    #[test]
    #[serial]
    fn test_has_state_file() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        assert!(!manager.has_state_file());

        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        assert!(manager.has_state_file());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Has Lock
    ///
    /// Verifies that has_lock() returns correct value.
    ///
    /// ## Test Scenario
    /// - Creates manager without lock
    /// - Creates state file (which acquires lock)
    ///
    /// ## Expected Outcome
    /// - Returns false initially, true after creation
    #[test]
    #[serial]
    fn test_has_lock() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        assert!(!manager.has_lock());

        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        assert!(manager.has_lock());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Create State File With Items
    ///
    /// Verifies that create_state_file_with_items populates items.
    ///
    /// ## Test Scenario
    /// - Creates state file with pre-populated items
    ///
    /// ## Expected Outcome
    /// - Items are set in state file
    /// - Phase is set to CherryPicking
    #[test]
    #[serial]
    fn test_create_state_file_with_items() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();

        let items = vec![
            StateCherryPickItem {
                commit_id: "abc123".to_string(),
                pr_id: 1,
                pr_title: "PR 1".to_string(),
                status: StateItemStatus::Pending,
                work_item_ids: vec![100],
            },
            StateCherryPickItem {
                commit_id: "def456".to_string(),
                pr_id: 2,
                pr_title: "PR 2".to_string(),
                status: StateItemStatus::Pending,
                work_item_ids: vec![101, 102],
            },
        ];

        let result = manager.create_state_file_with_items(
            temp_repo.path().to_path_buf(),
            None,
            false,
            "v1.0.0",
            &config,
            items,
        );

        assert!(result.is_ok());
        let state = manager.state_file().unwrap();
        assert_eq!(state.cherry_pick_items.len(), 2);
        assert_eq!(state.cherry_pick_items[0].pr_id, 1);
        assert_eq!(state.cherry_pick_items[1].pr_id, 2);
        assert_eq!(state.phase, MergePhase::CherryPicking);

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Set Final Status
    ///
    /// Verifies that set_final_status updates status and completion time.
    ///
    /// ## Test Scenario
    /// - Creates state file
    /// - Sets final status
    ///
    /// ## Expected Outcome
    /// - Status is set
    /// - Completion timestamp is set
    #[test]
    #[serial]
    fn test_set_final_status() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        let result = manager.set_final_status(MergeStatus::Success);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());

        let state = manager.state_file().unwrap();
        assert_eq!(state.final_status, Some(MergeStatus::Success));
        assert!(state.completed_at.is_some());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Set Final Status No State
    ///
    /// Verifies that set_final_status returns Ok(None) when no state file.
    ///
    /// ## Test Scenario
    /// - Creates manager without state file
    /// - Calls set_final_status
    ///
    /// ## Expected Outcome
    /// - Returns Ok(None)
    #[test]
    fn test_set_final_status_no_state() {
        let mut manager = StateManager::new();
        let result = manager.set_final_status(MergeStatus::Success);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// # Save Method
    ///
    /// Verifies that save() persists the state file.
    ///
    /// ## Test Scenario
    /// - Creates state file
    /// - Modifies state
    /// - Calls save()
    ///
    /// ## Expected Outcome
    /// - Changes are persisted to disk
    #[test]
    #[serial]
    fn test_save_method() {
        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_state_dir.path()) };

        let mut manager = StateManager::new();
        let config = create_test_config();
        manager
            .create_state_file(
                temp_repo.path().to_path_buf(),
                None,
                false,
                "v1.0.0",
                &config,
            )
            .unwrap();

        // Modify state
        if let Some(ref mut state) = manager.state_file_mut() {
            state.current_index = 42;
        }

        // Save
        let result = manager.save();
        assert!(result.is_ok());
        let path = result.unwrap().unwrap();

        // Load and verify
        let loaded = MergeStateFile::load(&path).unwrap();
        assert_eq!(loaded.current_index, 42);

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Save Method No State
    ///
    /// Verifies that save() returns Ok(None) when no state file.
    ///
    /// ## Test Scenario
    /// - Creates manager without state file
    /// - Calls save()
    ///
    /// ## Expected Outcome
    /// - Returns Ok(None)
    #[test]
    fn test_save_no_state() {
        let mut manager = StateManager::new();
        let result = manager.save();
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
