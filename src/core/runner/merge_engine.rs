//! Core merge orchestration engine.
//!
//! This module provides the shared logic for running merge operations,
//! independent of whether the runner is interactive or non-interactive.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::api::AzureDevOpsClient;
use crate::core::operations::cherry_pick::{
    CherryPickConfig, CherryPickOperation, CherryPickOutcome,
};
use crate::core::operations::post_merge::{
    CompletedPRInfo, PostMergeConfig, PostMergeOperation, WorkItemInfo,
};
use crate::core::operations::pr_selection::{
    parse_work_item_states, select_prs_by_work_item_states,
};
use crate::core::output::{ConflictInfo, ItemStatus, ProgressEvent, SummaryCounts, SummaryItem};
use crate::core::state::{
    LockGuard, MergePhase, MergeStateFile, MergeStatus, StateCherryPickItem, StateItemStatus,
};
use crate::git;
use crate::models::PullRequestWithWorkItems;

/// Core merge engine that orchestrates the merge workflow.
///
/// This struct encapsulates the main merge logic and can be used by
/// both interactive and non-interactive runners.
pub struct MergeEngine {
    client: Arc<AzureDevOpsClient>,
    organization: String,
    project: String,
    repository: String,
    dev_branch: String,
    target_branch: String,
    version: String,
    tag_prefix: String,
    work_item_state: String,
    run_hooks: bool,
    local_repo: Option<PathBuf>,
}

impl MergeEngine {
    /// Creates a new merge engine.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Arc<AzureDevOpsClient>,
        organization: String,
        project: String,
        repository: String,
        dev_branch: String,
        target_branch: String,
        version: String,
        tag_prefix: String,
        work_item_state: String,
        run_hooks: bool,
        local_repo: Option<PathBuf>,
    ) -> Self {
        Self {
            client,
            organization,
            project,
            repository,
            dev_branch,
            target_branch,
            version,
            tag_prefix,
            work_item_state,
            run_hooks,
            local_repo,
        }
    }

    /// Loads pull requests from Azure DevOps.
    pub async fn load_pull_requests(&self) -> Result<Vec<PullRequestWithWorkItems>> {
        // Fetch completed PRs from the dev branch
        let prs = self
            .client
            .fetch_pull_requests(&self.dev_branch, None)
            .await
            .context("Failed to fetch pull requests")?;

        // Fetch work items for all PRs in parallel
        let prs_with_work_items = self
            .client
            .fetch_work_items_for_prs_parallel(&prs, 10, 5)
            .await;

        Ok(prs_with_work_items)
    }

    /// Selects PRs based on work item states.
    ///
    /// Returns the number of selected PRs.
    pub fn select_prs_by_states(
        &self,
        prs: &mut [PullRequestWithWorkItems],
        states_str: &str,
    ) -> usize {
        let states = parse_work_item_states(states_str);
        select_prs_by_work_item_states(prs, &states)
    }

    /// Sets up the repository for cherry-picking.
    ///
    /// Returns the path to the worktree/clone.
    pub fn setup_repository(&self) -> Result<(PathBuf, bool)> {
        // Check if we have a local repo configured
        if let Some(ref local_repo) = self.local_repo {
            // Create worktree
            // create_worktree(base_repo_path, target_branch, version, run_hooks)
            let worktree_path = git::create_worktree(
                local_repo,
                &self.target_branch,
                &self.version,
                !self.run_hooks,
            )
            .context("Failed to create worktree")?;

            Ok((worktree_path, true))
        } else {
            // Clone the repository
            // shallow_clone_repo(ssh_url, target_branch, run_hooks) -> (PathBuf, TempDir)
            let (clone_path, _temp_dir) = git::shallow_clone_repo(
                &format!(
                    "https://dev.azure.com/{}/{}/_git/{}",
                    self.organization, self.project, self.repository
                ),
                &self.target_branch,
                !self.run_hooks,
            )
            .context("Failed to clone repository")?;

            // Note: We intentionally drop _temp_dir which means the cloned repo
            // will be deleted when this function returns. For persistent clones,
            // use a worktree approach instead.
            Ok((clone_path, false))
        }
    }

    /// Creates a new state file for a merge operation.
    pub fn create_state_file(
        &self,
        repo_path: PathBuf,
        base_repo_path: Option<PathBuf>,
        is_worktree: bool,
        prs: &[PullRequestWithWorkItems],
    ) -> MergeStateFile {
        let mut state = MergeStateFile::new(
            repo_path,
            base_repo_path,
            is_worktree,
            self.organization.clone(),
            self.project.clone(),
            self.repository.clone(),
            self.dev_branch.clone(),
            self.target_branch.clone(),
            self.version.clone(),
            self.work_item_state.clone(),
            self.tag_prefix.clone(),
            self.run_hooks,
        );

        // Convert selected PRs to cherry-pick items
        let items: Vec<StateCherryPickItem> = prs
            .iter()
            .filter(|pr| pr.selected)
            .filter_map(|pr| {
                // Get the merge commit ID
                pr.pr
                    .last_merge_commit
                    .as_ref()
                    .map(|commit| StateCherryPickItem {
                        commit_id: commit.commit_id.clone(),
                        pr_id: pr.pr.id,
                        pr_title: pr.pr.title.clone(),
                        status: StateItemStatus::Pending,
                        work_item_ids: pr.work_items.iter().map(|wi| wi.id).collect(),
                    })
            })
            .collect();

        state.cherry_pick_items = items;
        state.phase = MergePhase::CherryPicking;
        state
    }

    /// Cherry-picks a single commit.
    ///
    /// Returns the outcome and optionally the list of conflicted files.
    pub fn cherry_pick_commit(
        &self,
        repo_path: &Path,
        commit_id: &str,
    ) -> (CherryPickOutcome, Option<Vec<String>>) {
        let config = CherryPickConfig {
            run_hooks: self.run_hooks,
            is_worktree: self.local_repo.is_some(),
        };
        let operation = CherryPickOperation::new(config);

        let outcome = operation.cherry_pick_commit(repo_path, commit_id);

        let conflicted_files = match &outcome {
            CherryPickOutcome::Conflict { conflicted_files } => Some(conflicted_files.clone()),
            _ => None,
        };

        (outcome, conflicted_files)
    }

    /// Processes cherry-pick items from the current index.
    ///
    /// Returns the progress events and optionally a conflict info.
    pub fn process_cherry_picks<F>(
        &self,
        state: &mut MergeStateFile,
        mut event_callback: F,
    ) -> Option<ConflictInfo>
    where
        F: FnMut(ProgressEvent),
    {
        let total = state.cherry_pick_items.len();

        while state.current_index < total {
            let item = &state.cherry_pick_items[state.current_index];

            // Emit start event
            event_callback(ProgressEvent::CherryPickStart {
                pr_id: item.pr_id,
                commit_id: item.commit_id.clone(),
                index: state.current_index,
                total,
            });

            let (outcome, _conflicted_files) =
                self.cherry_pick_commit(&state.repo_path, &item.commit_id);

            // Update state based on outcome
            let item = &mut state.cherry_pick_items[state.current_index];
            match outcome {
                CherryPickOutcome::Success => {
                    item.status = StateItemStatus::Success;
                    event_callback(ProgressEvent::CherryPickSuccess {
                        pr_id: item.pr_id,
                        commit_id: item.commit_id.clone(),
                    });
                }
                CherryPickOutcome::Conflict {
                    ref conflicted_files,
                } => {
                    item.status = StateItemStatus::Conflict;
                    state.phase = MergePhase::AwaitingConflictResolution;
                    state.conflicted_files = Some(conflicted_files.clone());

                    event_callback(ProgressEvent::CherryPickConflict {
                        pr_id: item.pr_id,
                        conflicted_files: conflicted_files.clone(),
                        repo_path: state.repo_path.clone(),
                    });

                    return Some(ConflictInfo::new(
                        item.pr_id,
                        item.pr_title.clone(),
                        item.commit_id.clone(),
                        conflicted_files.clone(),
                        state.repo_path.clone(),
                    ));
                }
                CherryPickOutcome::Skipped => {
                    item.status = StateItemStatus::Skipped;
                    event_callback(ProgressEvent::CherryPickSkipped {
                        pr_id: item.pr_id,
                        reason: None,
                    });
                }
                CherryPickOutcome::Failed { ref message } => {
                    item.status = StateItemStatus::Failed {
                        message: message.clone(),
                    };
                    event_callback(ProgressEvent::CherryPickFailed {
                        pr_id: item.pr_id,
                        error: message.clone(),
                    });
                }
            }

            state.current_index += 1;
        }

        // All cherry-picks complete
        state.phase = MergePhase::ReadyForCompletion;
        None
    }

    /// Executes post-merge tasks (tagging PRs and updating work items).
    pub async fn run_post_merge<F>(
        &self,
        state: &MergeStateFile,
        next_state: &str,
        mut event_callback: F,
    ) -> Result<(usize, usize)>
    where
        F: FnMut(ProgressEvent),
    {
        // Build completed PR info from successfully cherry-picked items
        let completed_prs: Vec<CompletedPRInfo> = state
            .cherry_pick_items
            .iter()
            .filter(|item| matches!(item.status, StateItemStatus::Success))
            .map(|item| CompletedPRInfo {
                pr_id: item.pr_id,
                pr_title: item.pr_title.clone(),
                work_items: item
                    .work_item_ids
                    .iter()
                    .map(|&id| WorkItemInfo {
                        id,
                        title: String::new(), // Title not stored in state
                    })
                    .collect(),
            })
            .collect();

        if completed_prs.is_empty() {
            return Ok((0, 0));
        }

        let config = PostMergeConfig {
            tag_prefix: state.tag_prefix.clone(),
            version: state.merge_version.clone(),
            work_item_state: next_state.to_string(),
        };

        let operation = PostMergeOperation::new(Arc::clone(&self.client), config);
        let mut tasks = operation.build_task_queue(&completed_prs);

        event_callback(ProgressEvent::PostMergeStart {
            task_count: tasks.len(),
        });

        let result = operation
            .execute_all(
                &mut tasks,
                Some(|_progress| {
                    // Could emit more detailed progress events here
                }),
            )
            .await;

        Ok((result.success_count, result.failed_count))
    }

    /// Creates summary items from the state file.
    pub fn create_summary_items(&self, state: &MergeStateFile) -> Vec<SummaryItem> {
        state
            .cherry_pick_items
            .iter()
            .map(|item| SummaryItem {
                pr_id: item.pr_id,
                pr_title: item.pr_title.clone(),
                commit_id: item.commit_id.clone(),
                status: match &item.status {
                    StateItemStatus::Pending => ItemStatus::Pending,
                    StateItemStatus::Success => ItemStatus::Success,
                    StateItemStatus::Conflict => ItemStatus::Conflict,
                    StateItemStatus::Skipped => ItemStatus::Skipped,
                    StateItemStatus::Failed { .. } => ItemStatus::Failed,
                },
                error: match &item.status {
                    StateItemStatus::Failed { message } => Some(message.clone()),
                    _ => None,
                },
            })
            .collect()
    }

    /// Creates summary counts from the state file.
    pub fn create_summary_counts(&self, state: &MergeStateFile) -> SummaryCounts {
        let counts = state.status_counts();
        SummaryCounts::new(
            counts.success,
            counts.failed,
            counts.skipped,
            counts.pending,
        )
    }

    /// Determines the final merge status based on item statuses.
    pub fn determine_final_status(&self, state: &MergeStateFile) -> MergeStatus {
        let counts = state.status_counts();

        if counts.failed == 0 && counts.skipped == 0 {
            MergeStatus::Success
        } else if counts.success > 0 {
            MergeStatus::PartialSuccess
        } else {
            MergeStatus::Failed
        }
    }

    /// Cleans up a merge operation (removes worktree, aborts cherry-pick).
    pub fn cleanup(&self, state: &MergeStateFile) -> Result<()> {
        // Abort any in-progress cherry-pick
        let _ = std::process::Command::new("git")
            .args(["cherry-pick", "--abort"])
            .current_dir(&state.repo_path)
            .output();

        // Remove worktree if applicable
        if state.is_worktree {
            // cleanup_cherry_pick(base_repo_path: Option<&Path>, worktree_path: &Path, version: &str, target_branch: &str)
            git::cleanup_cherry_pick(
                state.base_repo_path.as_deref(),
                &state.repo_path,
                &state.merge_version,
                &state.target_branch,
            )?;
        }

        Ok(())
    }
}

/// Acquires a lock for a repository.
pub fn acquire_lock(repo_path: &Path) -> Result<Option<LockGuard>> {
    LockGuard::acquire(repo_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::StateCherryPickItem;
    use std::sync::Arc;

    /// Creates a mock API client for testing (will error on actual API calls).
    fn create_mock_client() -> Arc<AzureDevOpsClient> {
        Arc::new(
            AzureDevOpsClient::new(
                "test-org".to_string(),
                "test-project".to_string(),
                "test-repo".to_string(),
                "test-pat".to_string(),
            )
            .unwrap(),
        )
    }

    /// Creates a test engine with mock client.
    fn create_test_engine() -> MergeEngine {
        MergeEngine::new(
            create_mock_client(),
            "test-org".to_string(),
            "test-project".to_string(),
            "test-repo".to_string(),
            "dev".to_string(),
            "main".to_string(),
            "v1.0.0".to_string(),
            "merged-".to_string(),
            "Done".to_string(),
            false,
            None,
        )
    }

    /// Creates a test state file with items in various statuses.
    fn create_test_state(items: Vec<(i32, StateItemStatus)>) -> MergeStateFile {
        let mut state = MergeStateFile::new(
            std::path::PathBuf::from("/test/repo"),
            None,
            false,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "main".to_string(),
            "v1.0.0".to_string(),
            "Done".to_string(),
            "merged-".to_string(),
            false,
        );

        state.cherry_pick_items = items
            .into_iter()
            .map(|(pr_id, status)| StateCherryPickItem {
                commit_id: format!("commit_{}", pr_id),
                pr_id,
                pr_title: format!("PR #{}", pr_id),
                status,
                work_item_ids: vec![],
            })
            .collect();

        state
    }

    /// # Summary Counts Creation
    ///
    /// Verifies SummaryCounts is correctly created.
    ///
    /// ## Test Scenario
    /// - Creates counts with specific values
    ///
    /// ## Expected Outcome
    /// - Total is calculated correctly
    #[test]
    fn test_summary_counts_calculation() {
        let counts = SummaryCounts::new(5, 2, 1, 0);
        assert_eq!(counts.total, 8);
        assert_eq!(counts.successful, 5);
        assert_eq!(counts.failed, 2);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.pending, 0);
    }

    /// # Summary Counts From State File
    ///
    /// Verifies summary counts are correctly extracted from a state file.
    ///
    /// ## Test Scenario
    /// - Creates a state file with mixed item statuses
    /// - Calls create_summary_counts
    ///
    /// ## Expected Outcome
    /// - Counts match the items in the state file
    #[test]
    fn test_create_summary_counts_from_state() {
        let engine = create_test_engine();
        let state = create_test_state(vec![
            (1, StateItemStatus::Success),
            (2, StateItemStatus::Success),
            (
                3,
                StateItemStatus::Failed {
                    message: "error".to_string(),
                },
            ),
            (4, StateItemStatus::Skipped),
            (5, StateItemStatus::Pending),
        ]);

        let counts = engine.create_summary_counts(&state);

        assert_eq!(counts.successful, 2);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.total, 5);
    }

    /// # Determine Final Status - All Success
    ///
    /// Verifies final status is Success when all items succeed.
    ///
    /// ## Test Scenario
    /// - Creates state with all successful items
    ///
    /// ## Expected Outcome
    /// - Returns MergeStatus::Success
    #[test]
    fn test_determine_final_status_all_success() {
        let engine = create_test_engine();
        let state = create_test_state(vec![
            (1, StateItemStatus::Success),
            (2, StateItemStatus::Success),
            (3, StateItemStatus::Success),
        ]);

        assert_eq!(engine.determine_final_status(&state), MergeStatus::Success);
    }

    /// # Determine Final Status - Partial Success
    ///
    /// Verifies final status is PartialSuccess when some items fail.
    ///
    /// ## Test Scenario
    /// - Creates state with mixed success and failure
    ///
    /// ## Expected Outcome
    /// - Returns MergeStatus::PartialSuccess
    #[test]
    fn test_determine_final_status_partial_success() {
        let engine = create_test_engine();

        // Some failed
        let state1 = create_test_state(vec![
            (1, StateItemStatus::Success),
            (
                2,
                StateItemStatus::Failed {
                    message: "error".to_string(),
                },
            ),
        ]);
        assert_eq!(
            engine.determine_final_status(&state1),
            MergeStatus::PartialSuccess
        );

        // Some skipped
        let state2 = create_test_state(vec![
            (1, StateItemStatus::Success),
            (2, StateItemStatus::Skipped),
        ]);
        assert_eq!(
            engine.determine_final_status(&state2),
            MergeStatus::PartialSuccess
        );
    }

    /// # Determine Final Status - All Failed
    ///
    /// Verifies final status is Failed when no items succeed.
    ///
    /// ## Test Scenario
    /// - Creates state with all failed/skipped items
    ///
    /// ## Expected Outcome
    /// - Returns MergeStatus::Failed
    #[test]
    fn test_determine_final_status_all_failed() {
        let engine = create_test_engine();
        let state = create_test_state(vec![
            (
                1,
                StateItemStatus::Failed {
                    message: "error1".to_string(),
                },
            ),
            (2, StateItemStatus::Skipped),
        ]);

        assert_eq!(engine.determine_final_status(&state), MergeStatus::Failed);
    }

    /// # Create Summary Items
    ///
    /// Verifies summary items are correctly created from state.
    ///
    /// ## Test Scenario
    /// - Creates state with various item statuses
    /// - Calls create_summary_items
    ///
    /// ## Expected Outcome
    /// - Items are correctly mapped with proper status and error info
    #[test]
    fn test_create_summary_items() {
        let engine = create_test_engine();
        let state = create_test_state(vec![
            (1, StateItemStatus::Success),
            (2, StateItemStatus::Conflict),
            (3, StateItemStatus::Skipped),
            (
                4,
                StateItemStatus::Failed {
                    message: "test error".to_string(),
                },
            ),
            (5, StateItemStatus::Pending),
        ]);

        let items = engine.create_summary_items(&state);

        assert_eq!(items.len(), 5);
        assert_eq!(items[0].pr_id, 1);
        assert_eq!(items[0].status, ItemStatus::Success);
        assert!(items[0].error.is_none());

        assert_eq!(items[1].pr_id, 2);
        assert_eq!(items[1].status, ItemStatus::Conflict);

        assert_eq!(items[2].pr_id, 3);
        assert_eq!(items[2].status, ItemStatus::Skipped);

        assert_eq!(items[3].pr_id, 4);
        assert_eq!(items[3].status, ItemStatus::Failed);
        assert_eq!(items[3].error, Some("test error".to_string()));

        assert_eq!(items[4].pr_id, 5);
        assert_eq!(items[4].status, ItemStatus::Pending);
    }

    /// # Engine Creation With Options
    ///
    /// Verifies engine can be created with various options.
    ///
    /// ## Test Scenario
    /// - Creates engines with different configurations
    ///
    /// ## Expected Outcome
    /// - Engines are created without errors
    #[test]
    fn test_engine_creation_with_options() {
        // With local repo
        let _engine1 = MergeEngine::new(
            create_mock_client(),
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "main".to_string(),
            "v1.0.0".to_string(),
            "merged-".to_string(),
            "Done".to_string(),
            false,
            Some(std::path::PathBuf::from("/path/to/repo")),
        );

        // With hooks enabled
        let _engine2 = MergeEngine::new(
            create_mock_client(),
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "main".to_string(),
            "v1.0.0".to_string(),
            "merged-".to_string(),
            "Done".to_string(),
            true,
            None,
        );
    }

    /// # Create State File From PRs
    ///
    /// Verifies state file is correctly created from PRs.
    ///
    /// ## Test Scenario
    /// - Creates a state file with test parameters
    ///
    /// ## Expected Outcome
    /// - State file has correct metadata
    #[test]
    fn test_create_state_file() {
        let engine = MergeEngine::new(
            create_mock_client(),
            "test-org".to_string(),
            "test-project".to_string(),
            "test-repo".to_string(),
            "develop".to_string(),
            "release".to_string(),
            "v2.0.0".to_string(),
            "release-".to_string(),
            "Released".to_string(),
            true,
            Some(std::path::PathBuf::from("/base/repo")),
        );

        let state = engine.create_state_file(
            std::path::PathBuf::from("/work/repo"),
            Some(std::path::PathBuf::from("/base/repo")),
            true,
            &[], // Empty PRs
        );

        assert_eq!(state.organization, "test-org");
        assert_eq!(state.project, "test-project");
        assert_eq!(state.repository, "test-repo");
        assert_eq!(state.dev_branch, "develop");
        assert_eq!(state.target_branch, "release");
        assert_eq!(state.merge_version, "v2.0.0");
        assert_eq!(state.tag_prefix, "release-");
        assert_eq!(state.work_item_state, "Released");
        assert!(state.run_hooks);
        assert!(state.is_worktree);
        assert_eq!(state.phase, MergePhase::CherryPicking);
        assert!(state.cherry_pick_items.is_empty());
    }

    /// # Acquire Lock Function
    ///
    /// Verifies the acquire_lock convenience function works.
    ///
    /// ## Test Scenario
    /// - Calls acquire_lock on a temp directory
    ///
    /// ## Expected Outcome
    /// - Returns Ok with Some lock guard
    #[test]
    fn test_acquire_lock_convenience_function() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        // Set state dir env var
        unsafe {
            std::env::set_var(
                crate::core::state::STATE_DIR_ENV,
                temp_dir.path().join("state"),
            )
        };

        let repo_dir = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();

        let result = acquire_lock(&repo_dir);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());

        // Cleanup
        unsafe { std::env::remove_var(crate::core::state::STATE_DIR_ENV) };
    }

    /// # Select PRs By States
    ///
    /// Verifies PR selection by work item states.
    ///
    /// ## Test Scenario
    /// - Creates PRs with different work item states
    /// - Selects by specific states
    ///
    /// ## Expected Outcome
    /// - Only PRs matching the states are selected
    #[test]
    fn test_select_prs_by_states() {
        use crate::models::{
            CreatedBy, PullRequest, PullRequestWithWorkItems, WorkItem, WorkItemFields,
        };

        let engine = create_test_engine();

        fn create_work_item(id: i32, state: &str) -> WorkItem {
            WorkItem {
                id,
                fields: WorkItemFields {
                    title: Some(format!("WI {}", id)),
                    state: Some(state.to_string()),
                    work_item_type: Some("Bug".to_string()),
                    assigned_to: None,
                    iteration_path: None,
                    description: None,
                    repro_steps: None,
                    state_color: None,
                },
                history: Vec::new(),
            }
        }

        fn create_pr(id: i32, commit_id: &str) -> PullRequest {
            PullRequest {
                id,
                title: format!("PR {}", id),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test User".to_string(),
                },
                last_merge_commit: Some(crate::models::MergeCommit {
                    commit_id: commit_id.to_string(),
                }),
                labels: None,
            }
        }

        let mut prs = vec![
            PullRequestWithWorkItems {
                pr: create_pr(1, "abc"),
                work_items: vec![create_work_item(100, "Ready")],
                selected: false,
            },
            PullRequestWithWorkItems {
                pr: create_pr(2, "def"),
                work_items: vec![create_work_item(101, "Done")],
                selected: false,
            },
        ];

        let count = engine.select_prs_by_states(&mut prs, "Ready");

        assert_eq!(count, 1);
        assert!(prs[0].selected);
        assert!(!prs[1].selected);
    }
}
