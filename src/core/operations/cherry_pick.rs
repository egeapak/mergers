//! Cherry-pick operations for merge workflows.
//!
//! This module provides the core logic for cherry-picking commits,
//! handling conflicts, and managing the cherry-pick state.
//!
//! Note: The full implementation integrates with the existing git module.
//! This module provides types and interfaces for non-interactive mode.

use std::path::Path;

use anyhow::{Context, Result};

use crate::git::{self, CherryPickResult};
use crate::models::CherryPickStatus;

/// Outcome of a cherry-pick operation on a single commit.
#[derive(Debug, Clone)]
pub enum CherryPickOutcome {
    /// Cherry-pick succeeded.
    Success,
    /// Cherry-pick resulted in conflicts.
    Conflict {
        /// Files with conflicts.
        conflicted_files: Vec<String>,
    },
    /// Cherry-pick was skipped by user.
    Skipped,
    /// Cherry-pick failed with an error.
    Failed {
        /// Error message.
        message: String,
    },
}

impl From<CherryPickResult> for CherryPickOutcome {
    fn from(result: CherryPickResult) -> Self {
        match result {
            CherryPickResult::Success => CherryPickOutcome::Success,
            CherryPickResult::Conflict(files) => CherryPickOutcome::Conflict {
                conflicted_files: files,
            },
            CherryPickResult::Failed(msg) => CherryPickOutcome::Failed { message: msg },
        }
    }
}

impl From<CherryPickOutcome> for CherryPickStatus {
    fn from(outcome: CherryPickOutcome) -> Self {
        match outcome {
            CherryPickOutcome::Success => CherryPickStatus::Success,
            CherryPickOutcome::Conflict { .. } => CherryPickStatus::Conflict,
            CherryPickOutcome::Skipped => CherryPickStatus::Skipped,
            CherryPickOutcome::Failed { message } => CherryPickStatus::Failed(message),
        }
    }
}

/// Progress update for cherry-pick operations.
#[derive(Debug, Clone)]
pub enum CherryPickProgress {
    /// Starting cherry-pick of a commit.
    Starting {
        /// PR ID.
        pr_id: i32,
        /// Commit ID being cherry-picked.
        commit_id: String,
        /// Zero-based index of current item.
        index: usize,
        /// Total number of items.
        total: usize,
    },
    /// Cherry-pick completed for a commit.
    Completed {
        /// PR ID.
        pr_id: i32,
        /// Outcome of the cherry-pick.
        outcome: CherryPickOutcome,
    },
    /// All cherry-picks complete.
    AllComplete {
        /// Number of successful cherry-picks.
        success_count: usize,
        /// Number of failed cherry-picks.
        failed_count: usize,
        /// Number of skipped cherry-picks.
        skipped_count: usize,
    },
}

/// Configuration for cherry-pick operations.
#[derive(Debug, Clone)]
pub struct CherryPickConfig {
    /// Whether to run git hooks during cherry-pick.
    pub run_hooks: bool,
    /// Whether this is a worktree (vs a clone).
    pub is_worktree: bool,
}

impl Default for CherryPickConfig {
    fn default() -> Self {
        Self {
            run_hooks: false,
            is_worktree: true,
        }
    }
}

/// Item to be cherry-picked.
#[derive(Debug, Clone)]
pub struct CherryPickItem {
    /// The commit ID to cherry-pick.
    pub commit_id: String,
    /// The PR ID this commit belongs to.
    pub pr_id: i32,
    /// The PR title for display.
    pub pr_title: String,
    /// Current status of this item.
    pub status: CherryPickStatus,
    /// Work item IDs associated with this PR.
    pub work_item_ids: Vec<i32>,
}

/// Result of processing all cherry-pick items.
#[derive(Debug, Clone)]
pub struct CherryPickBatchResult {
    /// Updated items with their statuses.
    pub items: Vec<CherryPickItem>,
    /// Index of the item that caused a conflict (if any).
    pub conflict_index: Option<usize>,
    /// Whether all items were processed.
    pub all_complete: bool,
}

impl CherryPickBatchResult {
    /// Returns the count of successful cherry-picks.
    pub fn success_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.status, CherryPickStatus::Success))
            .count()
    }

    /// Returns the count of failed cherry-picks.
    pub fn failed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.status, CherryPickStatus::Failed(_)))
            .count()
    }

    /// Returns the count of skipped cherry-picks.
    pub fn skipped_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.status, CherryPickStatus::Skipped))
            .count()
    }
}

/// Core cherry-pick operation.
///
/// This struct encapsulates all the logic for cherry-picking commits
/// and handling the results.
pub struct CherryPickOperation {
    config: CherryPickConfig,
}

impl CherryPickOperation {
    /// Creates a new cherry-pick operation.
    pub fn new(config: CherryPickConfig) -> Self {
        Self { config }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &CherryPickConfig {
        &self.config
    }

    /// Returns whether hooks are enabled.
    pub fn run_hooks(&self) -> bool {
        self.config.run_hooks
    }

    /// Cherry-picks a single commit using the git module.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the repository
    /// * `commit_id` - The commit ID to cherry-pick
    ///
    /// # Returns
    ///
    /// The outcome of the cherry-pick operation.
    ///
    /// Note: The `run_hooks` config option is currently not implemented.
    /// Git hooks run based on the repository's configuration.
    pub fn cherry_pick_commit(&self, repo_path: &Path, commit_id: &str) -> CherryPickOutcome {
        match crate::git::cherry_pick_commit(repo_path, commit_id) {
            Ok(cp_result) => cp_result.into(),
            Err(e) => CherryPickOutcome::Failed {
                message: e.to_string(),
            },
        }
    }

    /// Continues cherry-picking after conflict resolution.
    ///
    /// This verifies that conflicts are resolved and continues with
    /// the remaining items.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the repository
    ///
    /// # Returns
    ///
    /// Ok(true) if ready to continue, Ok(false) if conflicts remain.
    pub fn continue_after_conflict(&self, repo_path: &Path) -> Result<bool> {
        // Delegate to the git module's implementation which uses `git ls-files -u`
        git::check_conflicts_resolved(repo_path)
    }
}

/// Checks if a git status porcelain line indicates a conflict.
///
/// Conflict markers in git status --porcelain output:
/// - `UU` - both modified (unmerged)
/// - `AA` - both added
/// - `DD` - both deleted
/// - `AU`, `UA`, `DU`, `UD` - various add/delete conflicts
fn is_conflict_status_line(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    chars.len() >= 2
        && ((chars[0] == 'U' || chars[1] == 'U')
            || (chars[0] == 'A' && chars[1] == 'A')
            || (chars[0] == 'D' && chars[1] == 'D'))
}

/// Gets the list of conflicted files in a repository.
pub fn get_conflicted_files(repo_path: &Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .context("Failed to run git status")?;

    let status = String::from_utf8_lossy(&output.stdout);

    let conflicted: Vec<String> = status
        .lines()
        .filter(|line| line.len() >= 3 && is_conflict_status_line(line))
        .map(|line| line[3..].to_string())
        .collect();

    Ok(conflicted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Cherry Pick Outcome From Result
    ///
    /// Verifies conversion from CherryPickResult to CherryPickOutcome.
    ///
    /// ## Test Scenario
    /// - Converts each CherryPickResult variant
    ///
    /// ## Expected Outcome
    /// - Correct mapping to CherryPickOutcome
    #[test]
    fn test_cherry_pick_outcome_from_result() {
        let success: CherryPickOutcome = CherryPickResult::Success.into();
        assert!(matches!(success, CherryPickOutcome::Success));

        let conflict: CherryPickOutcome =
            CherryPickResult::Conflict(vec!["file.rs".to_string()]).into();
        assert!(matches!(conflict, CherryPickOutcome::Conflict { .. }));

        let failed: CherryPickOutcome = CherryPickResult::Failed("error".to_string()).into();
        assert!(matches!(failed, CherryPickOutcome::Failed { .. }));
    }

    /// # Cherry Pick Outcome To Status
    ///
    /// Verifies conversion from CherryPickOutcome to CherryPickStatus.
    ///
    /// ## Test Scenario
    /// - Converts each CherryPickOutcome variant
    ///
    /// ## Expected Outcome
    /// - Correct mapping to CherryPickStatus
    #[test]
    fn test_cherry_pick_outcome_to_status() {
        let success: CherryPickStatus = CherryPickOutcome::Success.into();
        assert!(matches!(success, CherryPickStatus::Success));

        let conflict: CherryPickStatus = CherryPickOutcome::Conflict {
            conflicted_files: vec![],
        }
        .into();
        assert!(matches!(conflict, CherryPickStatus::Conflict));

        let skipped: CherryPickStatus = CherryPickOutcome::Skipped.into();
        assert!(matches!(skipped, CherryPickStatus::Skipped));

        let failed: CherryPickStatus = CherryPickOutcome::Failed {
            message: "error".to_string(),
        }
        .into();
        assert!(matches!(failed, CherryPickStatus::Failed(_)));
    }

    /// # Cherry Pick Config Default
    ///
    /// Verifies that default config has sensible values.
    ///
    /// ## Test Scenario
    /// - Creates default CherryPickConfig
    ///
    /// ## Expected Outcome
    /// - run_hooks is false, is_worktree is true
    #[test]
    fn test_cherry_pick_config_default() {
        let config = CherryPickConfig::default();
        assert!(!config.run_hooks);
        assert!(config.is_worktree);
    }

    /// # Cherry Pick Item Creation
    ///
    /// Verifies that CherryPickItem can be created.
    ///
    /// ## Test Scenario
    /// - Creates a CherryPickItem with sample values
    ///
    /// ## Expected Outcome
    /// - All fields are accessible
    #[test]
    fn test_cherry_pick_item_creation() {
        let item = CherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 42,
            pr_title: "Test PR".to_string(),
            status: CherryPickStatus::Pending,
            work_item_ids: vec![1, 2, 3],
        };

        assert_eq!(item.commit_id, "abc123");
        assert_eq!(item.pr_id, 42);
        assert_eq!(item.work_item_ids.len(), 3);
    }

    /// # Cherry Pick Progress Variants
    ///
    /// Verifies that all progress variants can be created.
    ///
    /// ## Test Scenario
    /// - Creates each progress variant
    ///
    /// ## Expected Outcome
    /// - All variants construct successfully
    #[test]
    fn test_cherry_pick_progress_variants() {
        let _p1 = CherryPickProgress::Starting {
            pr_id: 1,
            commit_id: "abc".to_string(),
            index: 0,
            total: 5,
        };
        let _p2 = CherryPickProgress::Completed {
            pr_id: 1,
            outcome: CherryPickOutcome::Success,
        };
        let _p3 = CherryPickProgress::AllComplete {
            success_count: 3,
            failed_count: 1,
            skipped_count: 1,
        };
    }

    /// # Cherry Pick Batch Result
    ///
    /// Verifies that CherryPickBatchResult can be created.
    ///
    /// ## Test Scenario
    /// - Creates a batch result with sample values
    ///
    /// ## Expected Outcome
    /// - All fields are accessible
    #[test]
    fn test_cherry_pick_batch_result() {
        let result = CherryPickBatchResult {
            items: vec![],
            conflict_index: None,
            all_complete: true,
        };

        assert!(result.items.is_empty());
        assert!(result.conflict_index.is_none());
        assert!(result.all_complete);
    }

    /// # Cherry Pick Batch Result Counts
    ///
    /// Verifies that count methods work correctly.
    ///
    /// ## Test Scenario
    /// - Creates a batch result with mixed item statuses
    ///
    /// ## Expected Outcome
    /// - Counts are calculated correctly
    #[test]
    fn test_cherry_pick_batch_result_counts() {
        let result = CherryPickBatchResult {
            items: vec![
                CherryPickItem {
                    commit_id: "a".to_string(),
                    pr_id: 1,
                    pr_title: "PR 1".to_string(),
                    status: CherryPickStatus::Success,
                    work_item_ids: vec![],
                },
                CherryPickItem {
                    commit_id: "b".to_string(),
                    pr_id: 2,
                    pr_title: "PR 2".to_string(),
                    status: CherryPickStatus::Success,
                    work_item_ids: vec![],
                },
                CherryPickItem {
                    commit_id: "c".to_string(),
                    pr_id: 3,
                    pr_title: "PR 3".to_string(),
                    status: CherryPickStatus::Failed("error".to_string()),
                    work_item_ids: vec![],
                },
                CherryPickItem {
                    commit_id: "d".to_string(),
                    pr_id: 4,
                    pr_title: "PR 4".to_string(),
                    status: CherryPickStatus::Skipped,
                    work_item_ids: vec![],
                },
            ],
            conflict_index: None,
            all_complete: true,
        };

        assert_eq!(result.success_count(), 2);
        assert_eq!(result.failed_count(), 1);
        assert_eq!(result.skipped_count(), 1);
    }

    /// # Cherry Pick Operation Creation
    ///
    /// Verifies that CherryPickOperation can be created.
    ///
    /// ## Test Scenario
    /// - Creates operation with default config
    ///
    /// ## Expected Outcome
    /// - Operation is created and config is accessible
    #[test]
    fn test_cherry_pick_operation_creation() {
        let operation = CherryPickOperation::new(CherryPickConfig::default());
        assert!(!operation.run_hooks());
    }

    /// # Conflict Status Line Detection
    ///
    /// Verifies that conflict status lines are correctly identified.
    ///
    /// ## Test Scenario
    /// - Tests various git status porcelain output lines
    /// - Includes conflict markers (UU, AA, DD, AU, UA, DU, UD)
    /// - Includes non-conflict markers (M, A, D, ??)
    ///
    /// ## Expected Outcome
    /// - Conflict lines return true
    /// - Non-conflict lines return false
    #[test]
    fn test_is_conflict_status_line() {
        // Conflict markers
        assert!(is_conflict_status_line("UU src/main.rs"));
        assert!(is_conflict_status_line("AA new_file.rs"));
        assert!(is_conflict_status_line("DD deleted.rs"));
        assert!(is_conflict_status_line("AU added_by_us.rs"));
        assert!(is_conflict_status_line("UA added_by_them.rs"));
        assert!(is_conflict_status_line("DU deleted_by_us.rs"));
        assert!(is_conflict_status_line("UD deleted_by_them.rs"));

        // Non-conflict markers
        assert!(!is_conflict_status_line("M  modified.rs"));
        assert!(!is_conflict_status_line(" M modified_unstaged.rs"));
        assert!(!is_conflict_status_line("A  added.rs"));
        assert!(!is_conflict_status_line("D  deleted.rs"));
        assert!(!is_conflict_status_line("?? untracked.rs"));
        assert!(!is_conflict_status_line("!! ignored.rs"));
        assert!(!is_conflict_status_line("R  renamed.rs"));

        // Edge cases
        assert!(!is_conflict_status_line(""));
        assert!(!is_conflict_status_line("X"));
        assert!(!is_conflict_status_line("  "));
    }
}
