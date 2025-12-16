//! Worktree context management for temporary git worktrees.
//!
//! This module provides [`WorktreeContext`] which tracks and manages temporary
//! git worktrees created during migration and merge operations. It ensures
//! automatic cleanup via the [`Drop`] trait.

use std::path::PathBuf;
use tempfile::TempDir;

/// Shared context for worktree management across app modes.
///
/// Tracks the working repository path, base repository (for worktree operations),
/// and ensures proper cleanup of temporary worktrees when dropped.
///
/// # Automatic Cleanup
///
/// When a `WorktreeContext` is dropped, it automatically removes any tracked
/// worktree from the base repository. This ensures temporary worktrees don't
/// accumulate on the filesystem.
///
/// # Example
///
/// ```ignore
/// let mut ctx = WorktreeContext::new();
/// ctx.base_repo_path = Some(PathBuf::from("/path/to/repo"));
/// ctx.worktree_id = Some("migration-123456".to_string());
/// ctx.repo_path = Some(PathBuf::from("/path/to/repo/.worktrees/migration-123456"));
///
/// // When ctx goes out of scope, the worktree is automatically removed
/// ```
#[derive(Debug, Default)]
pub struct WorktreeContext {
    /// Path to the working repository (worktree or cloned repo).
    pub repo_path: Option<PathBuf>,

    /// Base repository path (used for worktree cleanup).
    /// This is the original repository from which worktrees are created.
    pub base_repo_path: Option<PathBuf>,

    /// Temporary directory handle (keeps cloned repos alive).
    /// When this is dropped, the temporary directory is removed.
    pub _temp_dir: Option<TempDir>,

    /// Worktree ID for cleanup (e.g., "migration-123456" or version like "1.0.0").
    /// This is used to identify and remove the worktree on cleanup.
    pub worktree_id: Option<String>,
}

impl WorktreeContext {
    /// Creates a new empty worktree context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cleans up the tracked worktree if one exists.
    ///
    /// This method removes the worktree identified by `worktree_id` from
    /// the `base_repo_path` repository. After cleanup, `worktree_id` is
    /// set to `None` to prevent double-cleanup.
    ///
    /// # Errors
    ///
    /// Errors during cleanup are silently ignored as this is typically
    /// called during drop or exit paths where we cannot propagate errors.
    pub fn cleanup(&mut self) {
        if let (Some(base_repo), Some(worktree_id)) =
            (&self.base_repo_path, self.worktree_id.take())
        {
            // Use force_remove_worktree to clean up the worktree
            let _ = crate::git::force_remove_worktree(base_repo, &worktree_id);
        }
    }

    /// Returns true if this context has a worktree that needs cleanup.
    pub fn has_worktree(&self) -> bool {
        self.base_repo_path.is_some() && self.worktree_id.is_some()
    }

    /// Returns the current working repository path, if set.
    pub fn repo_path(&self) -> Option<&PathBuf> {
        self.repo_path.as_ref()
    }
}

impl Drop for WorktreeContext {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # WorktreeContext Default Initialization
    ///
    /// Tests that a new WorktreeContext is properly initialized with all None values.
    ///
    /// ## Test Scenario
    /// - Creates a new WorktreeContext using new()
    /// - Verifies all fields are None
    ///
    /// ## Expected Outcome
    /// - All optional fields should be None
    /// - has_worktree() should return false
    #[test]
    fn test_worktree_context_default() {
        let ctx = WorktreeContext::new();

        assert!(ctx.repo_path.is_none());
        assert!(ctx.base_repo_path.is_none());
        assert!(ctx._temp_dir.is_none());
        assert!(ctx.worktree_id.is_none());
        assert!(!ctx.has_worktree());
    }

    /// # WorktreeContext Has Worktree Detection
    ///
    /// Tests the has_worktree() method under various conditions.
    ///
    /// ## Test Scenario
    /// - Tests with no fields set
    /// - Tests with only base_repo_path set
    /// - Tests with only worktree_id set
    /// - Tests with both set
    ///
    /// ## Expected Outcome
    /// - Only returns true when both base_repo_path and worktree_id are set
    #[test]
    fn test_has_worktree() {
        let mut ctx = WorktreeContext::new();
        assert!(!ctx.has_worktree());

        // Only base_repo_path
        ctx.base_repo_path = Some(PathBuf::from("/repo"));
        assert!(!ctx.has_worktree());

        // Only worktree_id
        ctx.base_repo_path = None;
        ctx.worktree_id = Some("test-worktree".to_string());
        assert!(!ctx.has_worktree());

        // Both set
        ctx.base_repo_path = Some(PathBuf::from("/repo"));
        assert!(ctx.has_worktree());
    }

    /// # WorktreeContext Cleanup Clears Worktree ID
    ///
    /// Tests that cleanup() properly clears the worktree_id.
    ///
    /// ## Test Scenario
    /// - Creates context with worktree_id set but no base_repo_path
    /// - Calls cleanup()
    /// - Verifies worktree_id is cleared
    ///
    /// ## Expected Outcome
    /// - worktree_id should be None after cleanup
    /// - No errors should occur even without base_repo_path
    #[test]
    fn test_cleanup_clears_worktree_id() {
        let mut ctx = WorktreeContext::new();
        ctx.worktree_id = Some("migration-123".to_string());
        // Note: base_repo_path is None, so actual git cleanup won't happen

        ctx.cleanup();

        assert!(ctx.worktree_id.is_none());
    }

    /// # WorktreeContext Cleanup Without Tracking
    ///
    /// Tests that cleanup() works safely when no worktree is tracked.
    ///
    /// ## Test Scenario
    /// - Creates empty context
    /// - Calls cleanup()
    ///
    /// ## Expected Outcome
    /// - No panic or error should occur
    #[test]
    fn test_cleanup_without_tracking() {
        let mut ctx = WorktreeContext::new();

        // Should not panic
        ctx.cleanup();

        assert!(ctx.worktree_id.is_none());
        assert!(ctx.base_repo_path.is_none());
    }

    /// # WorktreeContext Repo Path Accessor
    ///
    /// Tests the repo_path() accessor method.
    ///
    /// ## Test Scenario
    /// - Tests with no repo_path
    /// - Tests with repo_path set
    ///
    /// ## Expected Outcome
    /// - Returns None when not set
    /// - Returns reference to path when set
    #[test]
    fn test_repo_path_accessor() {
        let mut ctx = WorktreeContext::new();
        assert!(ctx.repo_path().is_none());

        let path = PathBuf::from("/test/repo");
        ctx.repo_path = Some(path.clone());
        assert_eq!(ctx.repo_path(), Some(&path));
    }

    /// # WorktreeContext Drop Behavior
    ///
    /// Tests that Drop properly cleans up without panic.
    ///
    /// ## Test Scenario
    /// - Creates context with worktree tracking
    /// - Lets it go out of scope
    ///
    /// ## Expected Outcome
    /// - No panic during drop
    /// - (Actual cleanup would require a real git repo)
    #[test]
    fn test_drop_behavior() {
        {
            let mut ctx = WorktreeContext::new();
            ctx.worktree_id = Some("test-drop".to_string());
            // No base_repo_path, so no actual cleanup happens
            // Context dropped here
        }
        // If we get here, drop succeeded without panic
    }

    /// # WorktreeContext Double Cleanup Safety
    ///
    /// Tests that calling cleanup() multiple times is safe.
    ///
    /// ## Test Scenario
    /// - Creates context with worktree_id
    /// - Calls cleanup() twice
    ///
    /// ## Expected Outcome
    /// - No panic on second cleanup
    /// - worktree_id remains None
    #[test]
    fn test_double_cleanup_safety() {
        let mut ctx = WorktreeContext::new();
        ctx.worktree_id = Some("double-cleanup".to_string());

        ctx.cleanup();
        assert!(ctx.worktree_id.is_none());

        // Second cleanup should be safe
        ctx.cleanup();
        assert!(ctx.worktree_id.is_none());
    }
}
