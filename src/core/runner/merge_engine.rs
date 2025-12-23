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
}
