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
use crate::core::operations::hooks::{
    HookContext, HookExecutor, HookFailureMode, HookOutcome, HookProgress, HookTrigger, HooksConfig,
};
use crate::core::operations::post_merge::{
    CompletedPRInfo, PostMergeConfig, PostMergeOperation, WorkItemInfo,
};
use crate::core::operations::pr_selection::{
    parse_work_item_states, select_prs_by_work_item_states,
};
use crate::core::output::{ConflictInfo, ItemStatus, ProgressEvent, SummaryCounts, SummaryItem};
use crate::core::state::{
    LockGuard, MergePhase, MergeStateFile, MergeStatus, StateCherryPickItem, StateCreateConfig,
    StateItemStatus, StateManager,
};
use crate::git;
use crate::models::PullRequestWithWorkItems;

/// Result of processing cherry-picks.
#[derive(Debug)]
pub enum CherryPickProcessResult {
    /// All cherry-picks completed successfully (may have skips/failures but no conflicts).
    Complete,
    /// A conflict was encountered that requires resolution.
    Conflict(ConflictInfo),
    /// A hook failed and was configured to abort the workflow.
    HookAbort {
        /// The hook trigger that failed.
        trigger: HookTrigger,
        /// The command that failed.
        command: String,
        /// Error message.
        error: String,
    },
}

impl CherryPickProcessResult {
    /// Returns true if processing should stop.
    pub fn should_stop(&self) -> bool {
        !matches!(self, CherryPickProcessResult::Complete)
    }

    /// Returns true if this is a hook abort.
    pub fn is_hook_abort(&self) -> bool {
        matches!(self, CherryPickProcessResult::HookAbort { .. })
    }

    /// Returns true if this is a conflict.
    pub fn is_conflict(&self) -> bool {
        matches!(self, CherryPickProcessResult::Conflict(_))
    }
}

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
    hooks_config: HooksConfig,
    /// Maximum concurrent network operations.
    max_concurrent_network: usize,
    /// Maximum concurrent processing operations.
    max_concurrent_processing: usize,
    /// Filter PRs by date (e.g., "1mo", "2w", "2025-01-15").
    since: Option<String>,
    /// State manager for state file operations.
    state_manager: StateManager,
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
        hooks_config: Option<HooksConfig>,
        max_concurrent_network: usize,
        max_concurrent_processing: usize,
        since: Option<String>,
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
            hooks_config: hooks_config.unwrap_or_default(),
            max_concurrent_network,
            max_concurrent_processing,
            since,
            state_manager: StateManager::new(),
        }
    }

    /// Returns the hooks configuration.
    pub fn hooks_config(&self) -> &HooksConfig {
        &self.hooks_config
    }

    /// Creates a base hook context with common fields populated.
    fn create_hook_context(&self, repo_path: &Path) -> HookContext {
        HookContext::new()
            .with_version(&self.version)
            .with_target_branch(&self.target_branch)
            .with_dev_branch(&self.dev_branch)
            .with_repo_path(repo_path.to_string_lossy())
    }

    /// Runs hooks for a given trigger point.
    ///
    /// # Arguments
    ///
    /// * `trigger` - The hook trigger point
    /// * `repo_path` - Path to the repository (used as working directory)
    /// * `context` - Hook context with environment variables
    /// * `progress_callback` - Optional callback for progress updates
    ///
    /// # Returns
    ///
    /// True if all hooks succeeded or no hooks were configured, false otherwise.
    pub fn run_hooks<F>(
        &self,
        trigger: HookTrigger,
        repo_path: &Path,
        context: &HookContext,
        progress_callback: Option<F>,
    ) -> bool
    where
        F: FnMut(HookProgress),
    {
        if !self.hooks_config.has_hooks_for(trigger) {
            return true;
        }

        let executor = HookExecutor::new(self.hooks_config.clone());
        let result = executor.run_hooks(trigger, repo_path, context, progress_callback);
        result.all_succeeded
    }

    /// Runs hooks for a given trigger without progress callbacks.
    pub fn run_hooks_simple(&self, trigger: HookTrigger, repo_path: &Path) -> bool {
        let context = self.create_hook_context(repo_path);
        self.run_hooks::<fn(HookProgress)>(trigger, repo_path, &context, None)
    }

    /// Runs hooks for a given trigger and emits ProgressEvents.
    ///
    /// This method is used during the merge workflow to run hooks and emit
    /// corresponding progress events for output formatting.
    ///
    /// Returns a `HookOutcome` that indicates whether the workflow should continue,
    /// abort, or if a warning was emitted and workflow continues.
    pub fn run_hooks_with_events<F>(
        &self,
        trigger: HookTrigger,
        repo_path: &Path,
        context: &HookContext,
        event_callback: &mut F,
    ) -> HookOutcome
    where
        F: FnMut(ProgressEvent),
    {
        if !self.hooks_config.has_hooks_for(trigger) {
            return HookOutcome::Success;
        }

        let trigger_name = trigger.description().to_string();
        let commands = self.hooks_config.commands_for(trigger);
        let trigger_config = self.hooks_config.config_for(trigger);

        event_callback(ProgressEvent::HookStart {
            trigger: trigger_name.clone(),
            command_count: commands.len(),
        });

        let executor = HookExecutor::new(self.hooks_config.clone());
        let result = executor.run_hooks(
            trigger,
            repo_path,
            context,
            Some(|progress: HookProgress| {
                // We can't emit events here since we don't have access to event_callback
                // The hook executor handles its own progress internally
                let _ = progress;
            }),
        );

        // Emit events for each command result
        for (index, cmd_result) in result.command_results.iter().enumerate() {
            event_callback(ProgressEvent::HookCommandComplete {
                trigger: trigger_name.clone(),
                command: cmd_result.command.clone(),
                success: cmd_result.success,
                index,
            });
        }

        event_callback(ProgressEvent::HookComplete {
            trigger: trigger_name.clone(),
            all_succeeded: result.all_succeeded,
        });

        // If all hooks succeeded, return success
        if result.all_succeeded {
            return HookOutcome::Success;
        }

        // Get failure details
        let failure = result.first_failure();
        let (command, error) = match failure {
            Some(f) => (
                f.command.clone(),
                if f.stderr.is_empty() {
                    format!("Command failed with exit code {:?}", f.exit_code)
                } else {
                    f.stderr.clone()
                },
            ),
            None => ("unknown".to_string(), "Unknown error".to_string()),
        };

        // Determine action based on failure mode
        let failure_mode = trigger_config.failure_mode(trigger);

        match failure_mode {
            HookFailureMode::Abort => {
                event_callback(ProgressEvent::HookFailed {
                    trigger: trigger_name,
                    command: command.clone(),
                    error: error.clone(),
                });
                HookOutcome::Abort {
                    trigger,
                    command,
                    error,
                }
            }
            HookFailureMode::Continue => {
                event_callback(ProgressEvent::HookFailed {
                    trigger: trigger_name,
                    command: command.clone(),
                    error: error.clone(),
                });
                HookOutcome::ContinuedAfterFailure {
                    trigger,
                    command,
                    error,
                }
            }
        }
    }

    /// Returns a mutable reference to the state manager.
    pub fn state_manager_mut(&mut self) -> &mut StateManager {
        &mut self.state_manager
    }

    /// Returns a reference to the state manager.
    pub fn state_manager(&self) -> &StateManager {
        &self.state_manager
    }

    /// Loads pull requests from Azure DevOps.
    pub async fn load_pull_requests(&self) -> Result<Vec<PullRequestWithWorkItems>> {
        use crate::api::filter_prs_without_merged_tag;
        use crate::utils::throttle::NetworkProcessor;
        use futures::stream::{self, StreamExt};

        tracing::info!("Fetching pull requests for branch: {}", self.dev_branch);
        if let Some(ref since) = self.since {
            tracing::info!("Filtering PRs since: {}", since);
        }

        // Fetch completed PRs from the dev branch
        let prs = self
            .client
            .fetch_pull_requests(&self.dev_branch, self.since.as_deref())
            .await
            .context("Failed to fetch pull requests")?;

        tracing::info!("Retrieved {} pull requests from Azure DevOps", prs.len());

        // Filter out PRs that already have "merged-" tags (same as TUI mode)
        let prs = filter_prs_without_merged_tag(prs);
        tracing::info!(
            "After filtering merged tags: {} pull requests remain",
            prs.len()
        );

        tracing::info!(
            "Fetching work items for PRs (max_concurrent_network={})",
            self.max_concurrent_network
        );

        // Use NetworkProcessor to throttle work item fetching (same approach as TUI)
        let network_processor = NetworkProcessor::new_with_limits(
            self.max_concurrent_network,
            self.max_concurrent_processing,
        );

        let total = prs.len();

        // Fetch work items for all PRs with proper throttling
        let prs_with_work_items: Vec<PullRequestWithWorkItems> =
            stream::iter(prs.into_iter().enumerate())
                .map(|(index, pr)| {
                    let client = self.client.clone();
                    let processor = network_processor.clone();
                    let pr_id = pr.id;
                    async move {
                        let work_items = processor
                            .execute_network_operation(|| async {
                                client.fetch_work_items_with_history_for_pr(pr_id).await
                            })
                            .await
                            .unwrap_or_default();

                        (index, pr, work_items)
                    }
                })
                .buffer_unordered(self.max_concurrent_network)
                .map(|(index, pr, work_items)| {
                    // Log progress periodically
                    if (index + 1) % 100 == 0 || index + 1 == total {
                        tracing::info!("Fetched work items for {}/{} PRs", index + 1, total);
                    }
                    PullRequestWithWorkItems {
                        pr,
                        work_items,
                        selected: false,
                    }
                })
                .collect()
                .await;

        tracing::info!(
            "Loaded {} PRs with work items successfully",
            prs_with_work_items.len()
        );

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
            tracing::info!(
                "Setting up worktree from existing repository at {}",
                local_repo.display()
            );
            // Create worktree
            // create_worktree(base_repo_path, target_branch, version, run_hooks)
            let worktree_path = git::create_worktree(
                local_repo,
                &self.target_branch,
                &self.version,
                !self.run_hooks,
            )
            .context("Failed to create worktree")?;

            tracing::info!("Worktree setup complete");
            Ok((worktree_path, true))
        } else {
            tracing::info!("Cloning repository (no local repo configured)");
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
    ///
    /// This method delegates to the internal StateManager and returns the path
    /// where the state file was saved. The state file is populated with cherry-pick
    /// items from the selected PRs.
    ///
    /// # Returns
    ///
    /// The path where the state file was saved.
    pub fn create_state_file(
        &mut self,
        repo_path: PathBuf,
        base_repo_path: Option<PathBuf>,
        is_worktree: bool,
        prs: &[PullRequestWithWorkItems],
    ) -> Result<PathBuf> {
        let config = StateCreateConfig {
            organization: self.organization.clone(),
            project: self.project.clone(),
            repository: self.repository.clone(),
            dev_branch: self.dev_branch.clone(),
            target_branch: self.target_branch.clone(),
            tag_prefix: self.tag_prefix.clone(),
            work_item_state: self.work_item_state.clone(),
            run_hooks: self.run_hooks,
        };

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

        self.state_manager.create_state_file_with_items(
            repo_path,
            base_repo_path,
            is_worktree,
            &self.version,
            &config,
            items,
        )
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

    /// Processes cherry-pick items using the internal StateManager.
    ///
    /// This method uses the state file stored in the engine's internal StateManager.
    /// The state file must have been created via `create_state_file` or set via
    /// `state_manager_mut().set_state_file()` before calling this method.
    ///
    /// Returns the result of processing:
    /// - `Complete` if all cherry-picks finished (with possible skips/failures)
    /// - `Conflict` if a conflict was encountered
    /// - `HookAbort` if a hook failed and was configured to abort
    ///
    /// # Panics
    ///
    /// Panics if no state file is set in the internal StateManager.
    pub fn process_cherry_picks<F>(&mut self, mut event_callback: F) -> CherryPickProcessResult
    where
        F: FnMut(ProgressEvent),
    {
        // Extract info from state file first to avoid borrow conflicts
        let (total, repo_path, current_start_index) = {
            let state_file = self
                .state_manager
                .state_file()
                .expect("state_file must be set before processing cherry-picks");
            (
                state_file.cherry_pick_items.len(),
                state_file.repo_path.clone(),
                state_file.current_index,
            )
        };

        // Run pre-cherry-pick hooks (only if starting from the beginning)
        if current_start_index == 0 && self.hooks_config.has_hooks_for(HookTrigger::PreCherryPick) {
            let outcome = self.run_hooks_with_events(
                HookTrigger::PreCherryPick,
                &repo_path,
                &self.create_hook_context(&repo_path),
                &mut event_callback,
            );

            if let HookOutcome::Abort {
                trigger,
                command,
                error,
            } = outcome
            {
                return CherryPickProcessResult::HookAbort {
                    trigger,
                    command,
                    error,
                };
            }
        }

        loop {
            // Get current index and item info
            let (current_index, commit_id, pr_id, pr_title) = {
                let state_file = self.state_manager.state_file().unwrap();
                if state_file.current_index >= total {
                    break;
                }
                let item = &state_file.cherry_pick_items[state_file.current_index];
                (
                    state_file.current_index,
                    item.commit_id.clone(),
                    item.pr_id,
                    item.pr_title.clone(),
                )
            };

            // Emit start event
            event_callback(ProgressEvent::CherryPickStart {
                pr_id,
                commit_id: commit_id.clone(),
                index: current_index,
                total,
            });

            // Perform cherry-pick (borrows self immutably)
            let (outcome, _conflicted_files) = self.cherry_pick_commit(&repo_path, &commit_id);

            // Update state based on outcome
            {
                let state_file = self.state_manager.state_file_mut().unwrap();
                let item = &mut state_file.cherry_pick_items[current_index];

                match outcome {
                    CherryPickOutcome::Success => {
                        item.status = StateItemStatus::Success;
                        event_callback(ProgressEvent::CherryPickSuccess {
                            pr_id,
                            commit_id: commit_id.clone(),
                        });
                    }
                    CherryPickOutcome::Conflict {
                        ref conflicted_files,
                    } => {
                        item.status = StateItemStatus::Conflict;
                        state_file.phase = MergePhase::AwaitingConflictResolution;
                        state_file.conflicted_files = Some(conflicted_files.clone());

                        event_callback(ProgressEvent::CherryPickConflict {
                            pr_id,
                            conflicted_files: conflicted_files.clone(),
                            repo_path: repo_path.clone(),
                        });

                        // Run on-conflict hooks (always continue regardless of failure)
                        if self.hooks_config.has_hooks_for(HookTrigger::OnConflict) {
                            let context = self
                                .create_hook_context(&repo_path)
                                .with_pr_id(pr_id)
                                .with_commit_id(&commit_id);
                            // OnConflict hooks default to Continue, so we don't check the outcome
                            let _ = self.run_hooks_with_events(
                                HookTrigger::OnConflict,
                                &repo_path,
                                &context,
                                &mut event_callback,
                            );
                        }

                        return CherryPickProcessResult::Conflict(ConflictInfo::new(
                            pr_id,
                            pr_title,
                            commit_id,
                            conflicted_files.clone(),
                            repo_path,
                        ));
                    }
                    CherryPickOutcome::Skipped => {
                        item.status = StateItemStatus::Skipped;
                        event_callback(ProgressEvent::CherryPickSkipped {
                            pr_id,
                            reason: None,
                        });
                    }
                    CherryPickOutcome::Failed { ref message } => {
                        item.status = StateItemStatus::Failed {
                            message: message.clone(),
                        };
                        event_callback(ProgressEvent::CherryPickFailed {
                            pr_id,
                            error: message.clone(),
                        });
                    }
                }

                state_file.current_index += 1;
            }

            // Run post-cherry-pick hooks after successful cherry-pick (outside the mutable borrow)
            if matches!(outcome, CherryPickOutcome::Success)
                && self.hooks_config.has_hooks_for(HookTrigger::PostCherryPick)
            {
                let context = self
                    .create_hook_context(&repo_path)
                    .with_pr_id(pr_id)
                    .with_commit_id(&commit_id);
                // PostCherryPick hooks default to Continue, so we don't check the outcome
                let _ = self.run_hooks_with_events(
                    HookTrigger::PostCherryPick,
                    &repo_path,
                    &context,
                    &mut event_callback,
                );
            }
        }

        // All cherry-picks complete
        if let Some(state_file) = self.state_manager.state_file_mut() {
            state_file.phase = MergePhase::ReadyForCompletion;
        }

        // Run post-merge hooks (defaults to Continue, so we don't abort on failure)
        if self.hooks_config.has_hooks_for(HookTrigger::PostMerge) {
            // PostMerge hooks default to Continue, so we don't check the outcome
            let _ = self.run_hooks_with_events(
                HookTrigger::PostMerge,
                &repo_path,
                &self.create_hook_context(&repo_path),
                &mut event_callback,
            );
        }

        CherryPickProcessResult::Complete
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

    /// Analyzes dependencies between PRs based on file changes.
    ///
    /// This method:
    /// 1. Extracts commit IDs from the PRs
    /// 2. Fetches commits if they don't exist locally
    /// 3. Parses file changes from each commit
    /// 4. Runs the dependency analyzer to categorize relationships
    ///
    /// # Arguments
    ///
    /// * `prs` - List of PRs to analyze (should be in chronological order)
    /// * `repo_path` - Path to the repository for git operations
    ///
    /// # Returns
    ///
    /// A `DependencyAnalysisResult` containing the dependency graph and warnings.
    pub fn analyze_dependencies(
        &self,
        prs: &[PullRequestWithWorkItems],
        repo_path: &Path,
    ) -> Result<crate::core::operations::DependencyAnalysisResult> {
        use crate::core::operations::{DependencyAnalyzer, FileChange, PRInfo};
        use std::collections::HashMap;

        // Convert PRs to PRInfo format
        let pr_infos: Vec<PRInfo> = prs
            .iter()
            .map(|pr| {
                PRInfo::new(
                    pr.pr.id,
                    pr.pr.title.clone(),
                    pr.selected,
                    pr.pr
                        .last_merge_commit
                        .as_ref()
                        .map(|c| c.commit_id.clone()),
                )
            })
            .collect();

        // Collect commit IDs for fetching
        let commit_ids: Vec<String> = pr_infos
            .iter()
            .filter_map(|pr| pr.commit_id.clone())
            .collect();

        // Try to fetch commits (best effort - some may already exist locally)
        if !commit_ids.is_empty() {
            let _ = git::fetch_commits_for_analysis(repo_path, &commit_ids);
        }

        // Get file changes for each PR
        let mut pr_changes: HashMap<i32, Vec<FileChange>> = HashMap::new();

        for pr in &pr_infos {
            if let Some(ref commit_id) = pr.commit_id {
                // Check if commit exists before trying to analyze
                if git::commit_exists(repo_path, commit_id) {
                    match git::get_commit_changes_with_ranges(repo_path, commit_id) {
                        Ok(changes) => {
                            pr_changes.insert(pr.id, changes);
                        }
                        Err(e) => {
                            // Log warning but continue - commit might not be fetchable
                            tracing::warn!(
                                "Warning: Could not analyze changes for PR #{}: {}",
                                pr.id,
                                e
                            );
                        }
                    }
                }
            }
        }

        // Run the dependency analyzer
        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&pr_infos, &pr_changes);

        Ok(result)
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
            None,
            100,
            10,
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
            None,
            100,
            10,
            None,
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
            None,
            100,
            10,
            None,
        );
    }

    /// # Create State File With StateManager
    ///
    /// Verifies state file creation using the StateManager-backed method.
    ///
    /// ## Test Scenario
    /// - Creates a state file using create_state_file which uses StateManager
    ///
    /// ## Expected Outcome
    /// - State file is persisted and accessible via state_manager
    #[test]
    #[serial_test::serial]
    fn test_create_state_file_with_state_manager() {
        use tempfile::TempDir;

        let temp_state_dir = TempDir::new().unwrap();
        let temp_repo = TempDir::new().unwrap();

        // Set state dir env var
        unsafe { std::env::set_var(crate::core::state::STATE_DIR_ENV, temp_state_dir.path()) };

        let mut engine = MergeEngine::new(
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
            None,
            100,
            10,
            None,
        );

        let result = engine.create_state_file(
            temp_repo.path().to_path_buf(),
            None,
            false,
            &[], // Empty PRs
        );

        assert!(result.is_ok());
        assert!(engine.state_manager().has_state_file());

        let state = engine.state_manager().state_file().unwrap();
        assert_eq!(state.organization, "test-org");
        assert_eq!(state.project, "test-project");
        assert_eq!(state.merge_version, "v2.0.0");
        assert!(state.run_hooks);

        // Cleanup
        unsafe { std::env::remove_var(crate::core::state::STATE_DIR_ENV) };
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

    /// # Filter PRs Without Merged Tag Integration
    ///
    /// Verifies that the filter_prs_without_merged_tag function works correctly
    /// when integrated with MergeEngine data structures.
    ///
    /// ## Test Scenario
    /// - Creates PRs with and without merged tags
    /// - Applies filter
    ///
    /// ## Expected Outcome
    /// - PRs with "merged-*" labels are excluded
    /// - PRs without such labels are retained
    #[test]
    fn test_filter_prs_without_merged_tag_integration() {
        use crate::api::filter_prs_without_merged_tag;
        use crate::models::{CreatedBy, Label, MergeCommit, PullRequest};

        fn create_pr_with_labels(id: i32, labels: Option<Vec<&str>>) -> PullRequest {
            PullRequest {
                id,
                title: format!("PR {}", id),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test User".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: format!("commit{}", id),
                }),
                labels: labels.map(|l| {
                    l.into_iter()
                        .map(|name| Label {
                            name: name.to_string(),
                        })
                        .collect()
                }),
            }
        }

        let prs = vec![
            create_pr_with_labels(1, None),                  // No labels - keep
            create_pr_with_labels(2, Some(vec!["feature"])), // Non-merged label - keep
            create_pr_with_labels(3, Some(vec!["merged-v1.0.0"])), // Merged tag - filter
            create_pr_with_labels(4, Some(vec!["bug", "merged-v2"])), // Has merged tag - filter
            create_pr_with_labels(5, Some(vec![])),          // Empty labels - keep
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].id, 1);
        assert_eq!(filtered[1].id, 2);
        assert_eq!(filtered[2].id, 5);
    }

    /// # Filter PRs All Have Merged Tags
    ///
    /// Verifies behavior when all PRs have merged tags.
    ///
    /// ## Test Scenario
    /// - All PRs have merged-* labels
    ///
    /// ## Expected Outcome
    /// - Returns empty vector
    #[test]
    fn test_filter_prs_all_merged() {
        use crate::api::filter_prs_without_merged_tag;
        use crate::models::{CreatedBy, Label, MergeCommit, PullRequest};

        let prs = vec![
            PullRequest {
                id: 1,
                title: "PR 1".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "a".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "merged-v1.0.0".to_string(),
                }]),
            },
            PullRequest {
                id: 2,
                title: "PR 2".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "b".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "merged-v2.0.0".to_string(),
                }]),
            },
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert!(filtered.is_empty());
    }

    /// # Filter PRs None Have Merged Tags
    ///
    /// Verifies that all PRs pass through when none have merged tags.
    ///
    /// ## Test Scenario
    /// - No PRs have merged-* labels
    ///
    /// ## Expected Outcome
    /// - All PRs retained
    #[test]
    fn test_filter_prs_none_merged() {
        use crate::api::filter_prs_without_merged_tag;
        use crate::models::{CreatedBy, Label, MergeCommit, PullRequest};

        let prs = vec![
            PullRequest {
                id: 1,
                title: "PR 1".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "a".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "feature".to_string(),
                }]),
            },
            PullRequest {
                id: 2,
                title: "PR 2".to_string(),
                closed_date: None,
                created_by: CreatedBy {
                    display_name: "Test".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "b".to_string(),
                }),
                labels: None,
            },
        ];

        let filtered = filter_prs_without_merged_tag(prs);

        assert_eq!(filtered.len(), 2);
    }

    /// # Non-Interactive Mode Filter Integration
    ///
    /// Verifies that PRs with merged tags would be filtered in the
    /// non-interactive workflow by testing the filter with the full
    /// PullRequestWithWorkItems structure.
    ///
    /// ## Test Scenario
    /// - Creates PullRequestWithWorkItems with various label configurations
    /// - Simulates the filtering that happens in load_pull_requests
    ///
    /// ## Expected Outcome
    /// - Only PRs without merged tags remain for processing
    #[test]
    fn test_non_interactive_workflow_filter_integration() {
        use crate::api::filter_prs_without_merged_tag;
        use crate::models::{
            CreatedBy, Label, MergeCommit, PullRequest, PullRequestWithWorkItems, WorkItem,
            WorkItemFields,
        };

        // Simulate what load_pull_requests does:
        // 1. Fetch PRs
        // 2. Filter merged tags
        // 3. Create PullRequestWithWorkItems

        let raw_prs = vec![
            PullRequest {
                id: 1,
                title: "Ready PR".to_string(),
                closed_date: Some("2024-01-01".to_string()),
                created_by: CreatedBy {
                    display_name: "Dev".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "abc".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "feature".to_string(),
                }]),
            },
            PullRequest {
                id: 2,
                title: "Already Merged PR".to_string(),
                closed_date: Some("2024-01-01".to_string()),
                created_by: CreatedBy {
                    display_name: "Dev".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "def".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "merged-v1.0.0".to_string(),
                }]),
            },
        ];

        // This is what load_pull_requests now does
        let filtered_prs = filter_prs_without_merged_tag(raw_prs);

        // Then converts to PullRequestWithWorkItems
        let prs_with_work_items: Vec<PullRequestWithWorkItems> = filtered_prs
            .into_iter()
            .map(|pr| PullRequestWithWorkItems {
                pr,
                work_items: vec![WorkItem {
                    id: 100,
                    fields: WorkItemFields {
                        title: Some("Work Item".to_string()),
                        state: Some("Ready".to_string()),
                        work_item_type: Some("Bug".to_string()),
                        assigned_to: None,
                        iteration_path: None,
                        description: None,
                        repro_steps: None,
                        state_color: None,
                    },
                    history: Vec::new(),
                }],
                selected: false,
            })
            .collect();

        // Verify only PR 1 remains
        assert_eq!(prs_with_work_items.len(), 1);
        assert_eq!(prs_with_work_items[0].pr.id, 1);
        assert_eq!(prs_with_work_items[0].pr.title, "Ready PR");
    }
}
