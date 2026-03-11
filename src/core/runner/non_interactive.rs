//! Non-interactive merge runner for CLI/CI usage.
//!
//! This module provides the runner implementation for non-interactive mode,
//! designed for use by AI agents and CI systems.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::api::AzureDevOpsClient;
use crate::core::ExitCode;
use crate::core::output::{
    ConflictInfo, ItemStatus, OutputFormatter, OutputWriter, PostMergeSummary, ProgressEvent,
    ProgressSummary, StatusInfo, SummaryCounts, SummaryInfo, SummaryItem, SummaryResult,
};
use crate::core::state::{LockGuard, MergePhase, MergeStateFile, MergeStatus, StateItemStatus};
use crate::git;

use super::merge_engine::{CherryPickProcessResult, MergeEngine, acquire_lock};
use super::traits::{MergeRunnerConfig, RunResult};
use crate::core::operations::hooks::HookOutcome;

/// Non-interactive merge runner.
///
/// This runner executes merge operations without user interaction,
/// designed for AI agents and CI systems.
pub struct NonInteractiveRunner<W: Write = io::Stdout> {
    config: MergeRunnerConfig,
    output: OutputWriter<W>,
}

impl NonInteractiveRunner<io::Stdout> {
    /// Creates a new non-interactive runner with stdout output.
    pub fn new(config: MergeRunnerConfig) -> Self {
        let output = OutputWriter::new(io::stdout(), config.output_format, config.quiet);
        Self { config, output }
    }
}

impl<W: Write> NonInteractiveRunner<W> {
    /// Creates a new runner with a custom writer.
    pub fn with_writer(config: MergeRunnerConfig, writer: W) -> Self {
        let output = OutputWriter::new(writer, config.output_format, config.quiet);
        Self { config, output }
    }

    /// Runs a new merge operation.
    ///
    /// This is the main entry point for starting a merge.
    pub async fn run(&mut self) -> RunResult {
        tracing::info!("Starting non-interactive merge");
        tracing::debug!(
            "Config: version={}, target_branch={}, dev_branch={}",
            self.config.version,
            self.config.target_branch,
            self.config.dev_branch
        );

        // Validate required fields
        if self.config.version.is_empty() {
            tracing::error!("Version is required but not provided");
            return RunResult::error(
                ExitCode::GeneralError,
                "Version is required for non-interactive mode",
            );
        }

        // Create the API client
        tracing::debug!("Creating Azure DevOps API client");
        let client = match self.create_client() {
            Ok(c) => {
                tracing::info!("API client created successfully");
                c
            }
            Err(e) => {
                tracing::error!("Failed to create API client: {}", e);
                return RunResult::error(
                    ExitCode::GeneralError,
                    format!("Failed to create API client: {}", e),
                );
            }
        };

        // Create the merge engine
        tracing::debug!("Creating merge engine");
        let mut engine = self.create_engine(Arc::clone(&client));

        // Load PRs
        tracing::info!("Loading pull requests from Azure DevOps...");
        let mut prs = match engine.load_pull_requests().await {
            Ok(prs) => {
                tracing::info!("Loaded {} pull requests", prs.len());
                tracing::debug!(
                    "PR IDs: {:?}",
                    prs.iter().map(|pr| pr.pr.id).collect::<Vec<_>>()
                );
                prs
            }
            Err(e) => {
                tracing::error!("Failed to load PRs: {}", e);
                self.emit_error(&format!("Failed to load PRs: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Select PRs by work item states if configured
        if let Some(ref states) = self.config.select_by_states {
            tracing::info!("Selecting PRs by work item states: {:?}", states);
            let count = engine.select_prs_by_states(&mut prs, states);
            tracing::debug!("{} PRs matched the specified states", count);
            if count == 0 {
                tracing::warn!("No PRs matched the specified work item states");
                self.emit_error("No PRs matched the specified work item states");
                return RunResult::error(
                    ExitCode::NoPRsMatched,
                    "No PRs matched the specified work item states",
                );
            }
        } else {
            tracing::debug!("Selecting all PRs with merge commits");
            // Select all PRs with merge commits
            for pr in &mut prs {
                pr.selected = pr.pr.last_merge_commit.is_some();
            }
        }

        let selected_count = prs.iter().filter(|pr| pr.selected).count();
        tracing::info!("{} PRs selected for merge", selected_count);
        if selected_count == 0 {
            tracing::warn!("No PRs selected for merge");
            self.emit_error("No PRs selected for merge");
            return RunResult::error(ExitCode::NoPRsMatched, "No PRs selected for merge");
        }

        // Set up the repository
        tracing::info!("Setting up repository...");
        tracing::debug!("local_repo={:?}", self.config.local_repo);
        let (repo_path, is_worktree) = match engine.setup_repository() {
            Ok((path, is_worktree)) => {
                tracing::info!(
                    "Repository set up successfully at {} (worktree={})",
                    path.display(),
                    is_worktree
                );
                (path, is_worktree)
            }
            Err(e) => {
                tracing::error!("Failed to set up repository: {}", e);
                self.emit_error(&format!("Failed to set up repository: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Acquire lock
        tracing::debug!("Acquiring repository lock");
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => {
                tracing::info!("Repository lock acquired");
                lock
            }
            Ok(None) => {
                tracing::warn!("Another merge operation is in progress");
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(
                    ExitCode::Locked,
                    "Another merge operation is in progress",
                );
            }
            Err(e) => {
                tracing::error!("Failed to acquire lock: {}", e);
                self.emit_error(&format!("Failed to acquire lock: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Run post-checkout hooks (defaults to Abort on failure)
        let hook_outcome = engine.run_hooks_with_events(
            crate::core::operations::HookTrigger::PostCheckout,
            &repo_path,
            &crate::core::operations::HookContext::new()
                .with_version(&self.config.version)
                .with_target_branch(&self.config.target_branch)
                .with_dev_branch(&self.config.dev_branch)
                .with_repo_path(repo_path.to_string_lossy()),
            &mut |event| self.emit_event(event),
        );

        if let HookOutcome::Abort { command, error, .. } = hook_outcome {
            self.emit_error(&format!(
                "Post-checkout hook failed: {} - {}",
                command, error
            ));
            return RunResult::error(
                ExitCode::HookFailed,
                format!("Post-checkout hook '{}' failed: {}", command, error),
            );
        }

        // Run dependency analysis
        tracing::info!("Starting dependency analysis for {} PRs", selected_count);
        self.emit_event(ProgressEvent::DependencyAnalysisStart {
            pr_count: selected_count,
        });

        match engine.analyze_dependencies(&prs, &repo_path) {
            Ok(analysis_result) => {
                // Emit summary
                let summary = analysis_result.graph.summary();
                self.emit_event(ProgressEvent::DependencyAnalysisComplete {
                    independent: summary.independent_relationships,
                    partial: summary.partial_relationships,
                    dependent: summary.dependent_relationships,
                });

                // Emit warnings
                for warning in &analysis_result.warnings {
                    if let crate::core::operations::DependencyWarning::UnselectedDependency {
                        selected_pr_id,
                        selected_pr_title,
                        unselected_pr_id,
                        unselected_pr_title,
                        category,
                    } = warning
                    {
                        self.emit_event(ProgressEvent::DependencyWarning {
                            selected_pr_id: *selected_pr_id,
                            selected_pr_title: selected_pr_title.clone(),
                            unselected_pr_id: *unselected_pr_id,
                            unselected_pr_title: unselected_pr_title.clone(),
                            is_critical: warning.is_critical(),
                            shared_files: category.shared_files().to_vec(),
                        });
                    }
                }
            }
            Err(e) => {
                // Dependency analysis failure is non-fatal, just log a warning
                tracing::warn!("Warning: Dependency analysis failed: {}", e);
            }
        }

        // Create state file using StateManager-backed method
        let base_repo_path = if is_worktree {
            self.config.local_repo.clone()
        } else {
            None
        };

        // Create state file - this stores state in engine's internal StateManager
        let state_path =
            match engine.create_state_file(repo_path.clone(), base_repo_path, is_worktree, &prs) {
                Ok(path) => path,
                Err(e) => {
                    self.emit_error(&format!("Failed to create state file: {}", e));
                    return RunResult::error(ExitCode::GeneralError, e.to_string());
                }
            };

        // Get total PRs from state manager for the start event
        let total_prs = engine
            .state_manager()
            .state_file()
            .map(|s| s.cherry_pick_items.len())
            .unwrap_or(0);

        // Emit start event
        self.emit_event(ProgressEvent::Start {
            total_prs,
            version: self.config.version.clone(),
            target_branch: self.config.target_branch.clone(),
            state_file_path: Some(state_path.clone()),
        });

        // Process cherry-picks using internal state manager
        let process_result = engine.process_cherry_picks(|event| {
            self.emit_event(event);
        });

        // Save state after cherry-picks
        if let Err(e) = engine.state_manager_mut().save() {
            self.emit_error(&format!("Failed to save state: {}", e));
            return RunResult::error(ExitCode::GeneralError, e.to_string());
        }

        // Handle process result
        match process_result {
            CherryPickProcessResult::Conflict(conflict) => {
                // Output conflict info
                if let Err(e) = self.output.write_conflict(&conflict) {
                    tracing::warn!("Failed to write conflict info: {}", e);
                }
                return RunResult::conflict(state_path);
            }
            CherryPickProcessResult::HookAbort { command, error, .. } => {
                self.emit_error(&format!("Hook aborted: {} - {}", command, error));
                return RunResult::error(
                    ExitCode::HookFailed,
                    format!("Hook '{}' failed: {}", command, error),
                )
                .with_state_file(state_path);
            }
            CherryPickProcessResult::Complete => {
                // Continue to completion
            }
        }

        // All cherry-picks complete - get counts from state manager
        let counts = engine
            .state_manager()
            .state_file()
            .map(|state| engine.create_summary_counts(state))
            .unwrap_or_else(|| SummaryCounts::new(0, 0, 0, 0));

        self.emit_event(ProgressEvent::Complete {
            successful: counts.successful,
            failed: counts.failed,
            skipped: counts.skipped,
        });

        // Determine result
        if counts.failed > 0 {
            RunResult::partial_success(format!(
                "{} successful, {} failed, {} skipped",
                counts.successful, counts.failed, counts.skipped
            ))
            .with_state_file(state_path)
        } else {
            RunResult::success().with_state_file(state_path)
        }
    }

    /// Continues a merge operation after conflict resolution.
    pub async fn continue_merge(&mut self, repo_path: Option<&Path>) -> RunResult {
        // Determine repo path
        let repo_path = match self.find_repo_path(repo_path) {
            Ok(path) => path,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Early lock check (before loading state)
        match LockGuard::is_locked(&repo_path) {
            Ok(true) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                self.emit_error(&format!("Failed to check lock: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
            Ok(false) => {}
        }

        // Load and validate state file
        let mut state = match MergeStateFile::load_and_validate_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error_with_code(
                    "No state file found for this repository",
                    Some("no_state_file"),
                );
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase
        if state.phase != MergePhase::AwaitingConflictResolution {
            self.emit_error_with_code(
                &format!("Cannot continue: merge is in '{}' phase", state.phase),
                Some("invalid_phase"),
            );
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for continue");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Check if conflicts are resolved
        let conflicts_resolved = self.check_conflicts_resolved(&state.repo_path);
        if !conflicts_resolved {
            self.emit_error_with_code("Conflicts are not fully resolved. Please resolve all conflicts and stage the files.", Some("conflicts_unresolved"));
            return RunResult::error(ExitCode::Conflict, "Conflicts not resolved");
        }

        // Finalize the cherry-pick commit
        if let Err(e) = git::continue_cherry_pick(&state.repo_path) {
            self.emit_error(&format!("Failed to finalize cherry-pick: {}", e));
            return RunResult::error(
                ExitCode::GeneralError,
                format!("Failed to finalize cherry-pick: {}", e),
            );
        }

        // Mark current item as success and advance
        state.cherry_pick_items[state.current_index].status = StateItemStatus::Success;
        state.current_index += 1;
        state.phase = MergePhase::CherryPicking;
        state.conflicted_files = None;

        // Create the engine
        let client = match self.create_client() {
            Ok(c) => c,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };
        let mut engine = self.create_engine(client);

        // Set the loaded state file on the engine's state manager
        // The lock guard remains local to ensure it stays alive for the operation
        engine.state_manager_mut().set_state_file(state);

        // Continue processing using internal state manager
        let process_result = engine.process_cherry_picks(|event| {
            self.emit_event(event);
        });

        // Save state via state manager
        let state_path = match engine.state_manager_mut().save() {
            Ok(Some(path)) => path,
            Ok(None) => {
                self.emit_error("No state file to save");
                return RunResult::error(ExitCode::GeneralError, "No state file to save");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Handle process result
        match process_result {
            CherryPickProcessResult::Conflict(conflict) => {
                if let Err(e) = self.output.write_conflict(&conflict) {
                    tracing::warn!("Failed to write conflict info: {}", e);
                }
                return RunResult::conflict(state_path);
            }
            CherryPickProcessResult::HookAbort { command, error, .. } => {
                self.emit_error(&format!("Hook aborted: {} - {}", command, error));
                return RunResult::error(
                    ExitCode::HookFailed,
                    format!("Hook '{}' failed: {}", command, error),
                )
                .with_state_file(state_path);
            }
            CherryPickProcessResult::Complete => {
                // Continue to completion
            }
        }

        // Get counts from state manager
        let counts = engine
            .state_manager()
            .state_file()
            .map(|state| engine.create_summary_counts(state))
            .unwrap_or_else(|| SummaryCounts::new(0, 0, 0, 0));

        self.emit_event(ProgressEvent::Complete {
            successful: counts.successful,
            failed: counts.failed,
            skipped: counts.skipped,
        });

        if counts.failed > 0 {
            RunResult::partial_success("Completed with some failures").with_state_file(state_path)
        } else {
            RunResult::success().with_state_file(state_path)
        }
    }

    /// Aborts the current merge operation.
    pub fn abort(&mut self, repo_path: Option<&Path>) -> RunResult {
        // Determine repo path
        let repo_path = match self.find_repo_path(repo_path) {
            Ok(path) => path,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Early lock check (before loading state)
        match LockGuard::is_locked(&repo_path) {
            Ok(true) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                self.emit_error(&format!("Failed to check lock: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
            Ok(false) => {}
        }

        // Load and validate state file
        let mut state = match MergeStateFile::load_and_validate_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error_with_code(
                    "No state file found for this repository",
                    Some("no_state_file"),
                );
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase (can't abort if already completed)
        if state.phase.is_terminal() {
            self.emit_error_with_code(
                &format!("Cannot abort: merge is already '{}'", state.phase),
                Some("invalid_phase"),
            );
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for abort");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Create engine for cleanup
        let client = match self.create_client() {
            Ok(c) => c,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };
        let engine = self.create_engine(client);

        // Cleanup
        if let Err(e) = engine.cleanup(&state) {
            self.emit_error(&format!("Cleanup failed: {}", e));
        }

        // Update state
        state.phase = MergePhase::Aborted;
        state.final_status = Some(MergeStatus::Aborted);

        if let Err(e) = state.save_for_repo() {
            self.emit_error(&format!("Failed to save state: {}", e));
        }

        self.emit_event(ProgressEvent::Aborted {
            success: true,
            message: None,
        });

        RunResult::success_with_message("Merge aborted")
    }

    /// Skips the current conflicting PR and continues with remaining.
    pub async fn skip(&mut self, repo_path: Option<&Path>) -> RunResult {
        // Determine repo path
        let repo_path = match self.find_repo_path(repo_path) {
            Ok(path) => path,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Early lock check
        match LockGuard::is_locked(&repo_path) {
            Ok(true) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                self.emit_error(&format!("Failed to check lock: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
            Ok(false) => {}
        }

        // Load and validate state file
        let mut state = match MergeStateFile::load_and_validate_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error_with_code(
                    "No state file found for this repository",
                    Some("no_state_file"),
                );
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase
        if state.phase != MergePhase::AwaitingConflictResolution {
            self.emit_error_with_code(
                &format!("Cannot skip: merge is in '{}' phase", state.phase),
                Some("invalid_phase"),
            );
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for skip");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Abort the current cherry-pick
        if let Err(e) = git::abort_cherry_pick(&state.repo_path) {
            self.emit_error(&format!("Failed to abort cherry-pick: {}", e));
            return RunResult::error(
                ExitCode::GeneralError,
                format!("Failed to abort cherry-pick: {}", e),
            );
        }

        // Emit skip event for the current item
        let current_item = &state.cherry_pick_items[state.current_index];
        self.emit_event(ProgressEvent::CherryPickSkipped {
            pr_id: current_item.pr_id,
            reason: Some("Skipped by user due to unresolvable conflict".to_string()),
        });

        // Mark current item as skipped and advance
        state.cherry_pick_items[state.current_index].status = StateItemStatus::Skipped;
        state.current_index += 1;
        state.phase = MergePhase::CherryPicking;
        state.conflicted_files = None;

        // Create the engine
        let client = match self.create_client() {
            Ok(c) => c,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };
        let mut engine = self.create_engine(client);

        // Set the loaded state file on the engine's state manager
        engine.state_manager_mut().set_state_file(state);

        // Continue processing remaining cherry-picks
        let conflict_info = engine.process_cherry_picks(|event| {
            self.emit_event(event);
        });

        // Save state
        let state_path = match engine.state_manager_mut().save() {
            Ok(Some(path)) => path,
            Ok(None) => {
                self.emit_error("No state file to save");
                return RunResult::error(ExitCode::GeneralError, "No state file to save");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        if let Some(conflict) = conflict_info {
            if let Err(e) = self.output.write_conflict(&conflict) {
                tracing::warn!("Warning: Failed to write conflict info: {}", e);
            }
            return RunResult::conflict(state_path);
        }

        // Get counts from state manager
        let counts = engine
            .state_manager()
            .state_file()
            .map(|state| engine.create_summary_counts(state))
            .unwrap_or_else(|| SummaryCounts::new(0, 0, 0, 0));

        self.emit_event(ProgressEvent::Complete {
            successful: counts.successful,
            failed: counts.failed,
            skipped: counts.skipped,
        });

        if counts.failed > 0 || counts.skipped > 0 {
            RunResult::partial_success("Completed with some skipped/failed items")
                .with_state_file(state_path)
        } else {
            RunResult::success().with_state_file(state_path)
        }
    }
    /// Shows the current merge status.
    pub fn status(&mut self, repo_path: Option<&Path>) -> RunResult {
        // Determine repo path
        let repo_path = match self.find_repo_path(repo_path) {
            Ok(path) => path,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Load and validate state file
        let state = match MergeStateFile::load_and_validate_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                // No merge in progress - return idle status (not an error)
                let status_info = StatusInfo {
                    phase: "idle".to_string(),
                    status: "idle".to_string(),
                    version: String::new(),
                    target_branch: String::new(),
                    repo_path: repo_path.clone(),
                    progress: ProgressSummary {
                        total: 0,
                        completed: 0,
                        pending: 0,
                        current_index: 0,
                    },
                    conflict: None,
                    items: None,
                };

                if let Err(e) = self.output.write_status(&status_info) {
                    tracing::warn!("Warning: Failed to write status: {}", e);
                }

                return RunResult::success();
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Build status info
        let counts = state.status_counts();
        let items: Vec<SummaryItem> = state
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
            .collect();

        let conflict = if state.phase == MergePhase::AwaitingConflictResolution {
            state
                .cherry_pick_items
                .get(state.current_index)
                .map(|item| {
                    ConflictInfo::new(
                        item.pr_id,
                        item.pr_title.clone(),
                        item.commit_id.clone(),
                        state.conflicted_files.clone().unwrap_or_default(),
                        state.repo_path.clone(),
                    )
                })
        } else {
            None
        };

        let status_info = StatusInfo {
            phase: state.phase.to_string(),
            status: match state.phase {
                MergePhase::Completed => "completed".to_string(),
                MergePhase::Aborted => "aborted".to_string(),
                MergePhase::AwaitingConflictResolution => "conflict".to_string(),
                MergePhase::ReadyForCompletion => "ready".to_string(),
                _ => "in_progress".to_string(),
            },
            version: state.merge_version.clone(),
            target_branch: state.target_branch.clone(),
            repo_path: state.repo_path.clone(),
            progress: ProgressSummary {
                total: counts.total(),
                completed: counts.completed(),
                pending: counts.pending,
                current_index: state.current_index,
            },
            conflict,
            items: Some(items),
        };

        if let Err(e) = self.output.write_status(&status_info) {
            tracing::warn!("Warning: Failed to write status: {}", e);
        }

        if state.phase == MergePhase::AwaitingConflictResolution {
            RunResult::error(ExitCode::Conflict, "Conflict pending")
        } else {
            RunResult::success()
        }
    }

    /// Completes the merge (tags PRs and updates work items).
    pub async fn complete(&mut self, repo_path: Option<&Path>, next_state: &str) -> RunResult {
        // Determine repo path
        let repo_path = match self.find_repo_path(repo_path) {
            Ok(path) => path,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Early lock check (before loading state)
        match LockGuard::is_locked(&repo_path) {
            Ok(true) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                self.emit_error(&format!("Failed to check lock: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
            Ok(false) => {}
        }

        // Load and validate state file
        let mut state = match MergeStateFile::load_and_validate_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error_with_code(
                    "No state file found for this repository",
                    Some("no_state_file"),
                );
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase
        if state.phase != MergePhase::ReadyForCompletion {
            self.emit_error_with_code(
                &format!("Cannot complete: merge is in '{}' phase", state.phase),
                Some("invalid_phase"),
            );
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for complete");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error_with_code("Another merge operation is in progress", Some("locked"));
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Update phase
        state.phase = MergePhase::Completing;
        if let Err(e) = state.save_for_repo() {
            return RunResult::error(ExitCode::GeneralError, e.to_string());
        }

        // Create engine
        let client = match self.create_client() {
            Ok(c) => c,
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };
        let engine = self.create_engine(client);

        // Run post-merge tasks
        let (success_count, failed_count) = match engine
            .run_post_merge(&state, next_state, |event| {
                self.emit_event(event);
            })
            .await
        {
            Ok((s, f)) => (s, f),
            Err(e) => {
                self.emit_error(&format!("Post-merge failed: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Run post-complete hooks (defaults to Continue, so we don't abort on failure)
        let _ = engine.run_hooks_with_events(
            crate::core::operations::HookTrigger::PostComplete,
            &state.repo_path,
            &crate::core::operations::HookContext::new()
                .with_version(&state.merge_version)
                .with_target_branch(&state.target_branch)
                .with_dev_branch(&state.dev_branch)
                .with_repo_path(state.repo_path.to_string_lossy()),
            &mut |event| self.emit_event(event),
        );

        // Mark as completed
        let final_status = engine.determine_final_status(&state);
        if let Err(e) = state.mark_completed(final_status) {
            return RunResult::error(ExitCode::GeneralError, e.to_string());
        }

        // Build summary
        let counts = engine.create_summary_counts(&state);
        let items = engine.create_summary_items(&state);

        let summary = SummaryInfo {
            result: if failed_count == 0 {
                SummaryResult::Success
            } else {
                SummaryResult::PartialSuccess
            },
            version: state.merge_version.clone(),
            target_branch: state.target_branch.clone(),
            counts,
            items: Some(items),
            post_merge: Some(PostMergeSummary {
                total_tasks: success_count + failed_count,
                successful: success_count,
                failed: failed_count,
                tasks: None, // Individual task details not tracked at this level
            }),
        };

        if let Err(e) = self.output.write_summary(&summary) {
            tracing::warn!("Warning: Failed to write summary: {}", e);
        }

        if failed_count > 0 {
            RunResult::partial_success(format!("Completed with {} task failures", failed_count))
        } else {
            RunResult::success_with_message("Merge completed successfully")
        }
    }

    // Helper methods

    fn create_client(&self) -> Result<Arc<AzureDevOpsClient>> {
        let client = AzureDevOpsClient::new(
            self.config.organization.clone(),
            self.config.project.clone(),
            self.config.repository.clone(),
            self.config.pat.clone(),
        )?;
        Ok(Arc::new(client))
    }

    fn create_engine(&self, client: Arc<AzureDevOpsClient>) -> MergeEngine {
        MergeEngine::new(
            client,
            self.config.organization.clone(),
            self.config.project.clone(),
            self.config.repository.clone(),
            self.config.dev_branch.clone(),
            self.config.target_branch.clone(),
            self.config.version.clone(),
            self.config.tag_prefix.clone(),
            self.config.work_item_state.clone(),
            self.config.run_hooks,
            self.config.local_repo.clone(),
            self.config.hooks_config.clone(),
            self.config.max_concurrent_network,
            self.config.max_concurrent_processing,
            self.config.since.clone(),
        )
    }

    fn emit_event(&mut self, event: ProgressEvent) {
        if let Err(e) = self.output.write_event(&event) {
            tracing::warn!("Warning: Failed to write event: {}", e);
        }
    }

    fn emit_error_with_code(&mut self, message: &str, code: Option<&str>) {
        let event = ProgressEvent::Error {
            message: message.to_string(),
            code: code.map(|c| c.to_string()),
        };
        if let Err(e) = self.output.write_event(&event) {
            tracing::warn!("Warning: Failed to write error: {}", e);
        }
    }

    fn emit_error(&mut self, message: &str) {
        self.emit_error_with_code(message, None);
    }

    fn find_repo_path(&self, provided: Option<&Path>) -> Result<PathBuf> {
        if let Some(path) = provided {
            return Ok(path.to_path_buf());
        }

        // Try to find repo from current directory
        let current_dir = std::env::current_dir().context("Failed to get current directory")?;

        // Check if we're in a git repo
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(&current_dir)
            .output()
            .context("Failed to run git")?;

        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout);
            return Ok(PathBuf::from(path.trim()));
        }

        bail!("Not in a git repository. Specify --repo path.")
    }

    fn check_conflicts_resolved(&self, repo_path: &Path) -> bool {
        // Delegate to the git module's implementation which uses `git ls-files -u`
        git::check_conflicts_resolved(repo_path).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OutputFormat;

    fn create_test_config() -> MergeRunnerConfig {
        MergeRunnerConfig {
            organization: "test-org".to_string(),
            project: "test-project".to_string(),
            repository: "test-repo".to_string(),
            pat: "test-pat".to_string(),
            dev_branch: "dev".to_string(),
            target_branch: "main".to_string(),
            version: "v1.0.0".to_string(),
            tag_prefix: "merged-".to_string(),
            work_item_state: "Done".to_string(),
            select_by_states: None,
            local_repo: None,
            run_hooks: false,
            output_format: OutputFormat::Text,
            quiet: false,
            hooks_config: None,
            max_concurrent_network: 100,
            max_concurrent_processing: 10,
            since: None,
        }
    }

    /// # Runner Creation
    ///
    /// Verifies that the runner can be created with a config.
    ///
    /// ## Test Scenario
    /// - Creates a runner with test config
    ///
    /// ## Expected Outcome
    /// - Runner is created without error
    #[test]
    fn test_runner_creation() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let _runner = NonInteractiveRunner::with_writer(config, &mut buffer);
        // Runner created successfully
    }

    /// # Runner With Custom Writer
    ///
    /// Verifies that the runner can use a custom writer.
    ///
    /// ## Test Scenario
    /// - Creates runner with Vec<u8> writer
    /// - Emits an event
    ///
    /// ## Expected Outcome
    /// - Event is written to the buffer
    #[test]
    fn test_runner_with_custom_writer() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Start {
            total_prs: 5,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            state_file_path: None,
        });

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Starting merge"));
    }

    /// # Output Format Handling
    ///
    /// Verifies different output formats work correctly.
    ///
    /// ## Test Scenario
    /// - Creates runners with different output formats
    ///
    /// ## Expected Outcome
    /// - All formats are accepted
    #[test]
    fn test_output_format_variations() {
        for format in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Ndjson] {
            let mut config = create_test_config();
            config.output_format = format;

            let mut buffer = Vec::new();
            let _runner = NonInteractiveRunner::with_writer(config, &mut buffer);
        }
    }

    /// # Find Repo Path With Provided Path
    ///
    /// Verifies find_repo_path returns the provided path.
    ///
    /// ## Test Scenario
    /// - Calls find_repo_path with a specific path
    ///
    /// ## Expected Outcome
    /// - Returns the provided path
    #[test]
    fn test_find_repo_path_with_provided() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let path = PathBuf::from("/test/repo");
        let result = runner.find_repo_path(Some(&path));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), path);
    }

    /// # Error Emission
    ///
    /// Verifies that errors are emitted correctly.
    ///
    /// ## Test Scenario
    /// - Emits an error message
    ///
    /// ## Expected Outcome
    /// - Error appears in output
    #[test]
    fn test_error_emission() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error("Test error message");

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("error") || output.contains("Error"));
    }

    /// # JSON Output Format Buffers Events
    ///
    /// Verifies that JSON output format buffers events instead of writing
    /// them immediately, since JSON mode collects all events and outputs
    /// a single JSON object at the end via `write_summary`.
    ///
    /// ## Test Scenario
    /// - Creates runner with JSON output format
    /// - Emits a Start event
    ///
    /// ## Expected Outcome
    /// - Buffer remains empty after emitting events (events are buffered internally)
    #[test]
    fn test_json_output_format_buffers_events() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Json;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Start {
            total_prs: 3,
            version: "v2.0.0".to_string(),
            target_branch: "release".to_string(),
            state_file_path: None,
        });

        // JSON format collects events internally and only writes output
        // when write_summary is called, so the buffer must be empty here.
        assert!(
            buffer.is_empty(),
            "JSON format should buffer events, not write them immediately"
        );
    }

    /// # NDJSON Output Format
    ///
    /// Verifies NDJSON output format produces valid newline-delimited JSON.
    ///
    /// ## Test Scenario
    /// - Creates runner with NDJSON format
    /// - Emits progress events
    ///
    /// ## Expected Outcome
    /// - Each line is valid JSON
    #[test]
    fn test_ndjson_output_format() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Start {
            total_prs: 2,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            state_file_path: None,
        });

        runner.emit_event(ProgressEvent::CherryPickStart {
            pr_id: 123,
            commit_id: "abc123".to_string(),
            index: 0,
            total: 2,
        });

        let output = String::from_utf8(buffer).unwrap();
        // NDJSON should have one JSON object per line
        for line in output.lines() {
            if !line.is_empty() {
                assert!(
                    serde_json::from_str::<serde_json::Value>(line).is_ok(),
                    "Line should be valid JSON: {}",
                    line
                );
            }
        }
    }

    /// # Quiet Mode Suppresses Output
    ///
    /// Verifies quiet mode suppresses regular output.
    ///
    /// ## Test Scenario
    /// - Creates runner with quiet=true
    /// - Emits events
    ///
    /// ## Expected Outcome
    /// - Output is empty or minimal
    #[test]
    fn test_quiet_mode() {
        let mut config = create_test_config();
        config.quiet = true;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::CherryPickStart {
            pr_id: 1,
            commit_id: "abc".to_string(),
            index: 0,
            total: 1,
        });

        // In quiet mode, output should be suppressed
        let output = String::from_utf8(buffer).unwrap();
        // Either empty or contains only essential info
        assert!(
            output.is_empty() || output.len() < 100,
            "Quiet mode should produce minimal output"
        );
    }

    /// # Status Info Construction
    ///
    /// Verifies StatusInfo can be constructed with proper values.
    ///
    /// ## Test Scenario
    /// - Manually constructs StatusInfo
    /// - Verifies all fields
    ///
    /// ## Expected Outcome
    /// - Status info contains correct values
    #[test]
    fn test_status_info_construction() {
        let status = StatusInfo {
            phase: "cherry_picking".to_string(),
            status: "in_progress".to_string(),
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            repo_path: PathBuf::from("/test/repo"),
            progress: ProgressSummary {
                total: 5,
                completed: 2,
                pending: 3,
                current_index: 2,
            },
            conflict: None,
            items: None,
        };

        assert_eq!(status.version, "v1.0.0");
        assert_eq!(status.target_branch, "main");
        assert_eq!(status.progress.total, 5);
        assert_eq!(status.progress.completed, 2);
        assert_eq!(status.progress.pending, 3);
    }

    /// # Multiple Cherry-Pick Events
    ///
    /// Verifies multiple cherry-pick events are emitted correctly.
    ///
    /// ## Test Scenario
    /// - Emits a series of cherry-pick events
    ///
    /// ## Expected Outcome
    /// - All events are recorded in output
    #[test]
    fn test_multiple_cherry_pick_events() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::CherryPickStart {
            pr_id: 1,
            commit_id: "abc".to_string(),
            index: 0,
            total: 3,
        });
        runner.emit_event(ProgressEvent::CherryPickSuccess {
            pr_id: 1,
            commit_id: "abc".to_string(),
        });
        runner.emit_event(ProgressEvent::CherryPickStart {
            pr_id: 2,
            commit_id: "def".to_string(),
            index: 1,
            total: 3,
        });
        runner.emit_event(ProgressEvent::CherryPickFailed {
            pr_id: 2,
            error: "Commit not found".to_string(),
        });

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("PR #1") || output.contains("[1/3]"));
        assert!(output.contains("PR #2") || output.contains("[2/3]"));
    }

    /// # Complete Event With Counts
    ///
    /// Verifies the complete event shows all three counts (successful, failed,
    /// skipped) in the text output so operators can see the merge outcome.
    ///
    /// ## Test Scenario
    /// - Emits a Complete event with 5 successful, 2 failed, 1 skipped
    ///
    /// ## Expected Outcome
    /// - Output contains all three numeric counts with their labels
    #[test]
    fn test_complete_event_with_counts() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Complete {
            successful: 5,
            failed: 2,
            skipped: 1,
        });

        let output = String::from_utf8(buffer).unwrap();
        assert!(
            output.contains("5 successful"),
            "Should show successful count: got '{output}'"
        );
        assert!(
            output.contains("2 failed"),
            "Should show failed count: got '{output}'"
        );
        assert!(
            output.contains("1 skipped"),
            "Should show skipped count: got '{output}'"
        );
    }

    /// # Conflict Event Details
    ///
    /// Verifies conflict events include the PR ID, conflicted file names,
    /// and repository path in the text output.
    ///
    /// ## Test Scenario
    /// - Emits a cherry-pick conflict event with PR #42 and two conflicted files
    ///
    /// ## Expected Outcome
    /// - Output contains the PR ID, each conflicted file name, and the repo path
    #[test]
    fn test_conflict_event_details() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::CherryPickConflict {
            pr_id: 42,
            conflicted_files: vec!["src/main.rs".to_string(), "Cargo.toml".to_string()],
            repo_path: PathBuf::from("/test/repo"),
        });

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("42"), "Should include the PR ID");
        assert!(
            output.contains("src/main.rs"),
            "Should list the first conflicted file"
        );
        assert!(
            output.contains("Cargo.toml"),
            "Should list the second conflicted file"
        );
        assert!(
            output.contains("/test/repo"),
            "Should include the repository path"
        );
    }

    /// # Run Result Success
    ///
    /// Verifies RunResult::success() creates correct result.
    ///
    /// ## Test Scenario
    /// - Creates a success result
    ///
    /// ## Expected Outcome
    /// - Exit code is Success, is_success() returns true
    #[test]
    fn test_run_result_success() {
        let result = RunResult::success();
        assert!(result.is_success());
        assert_eq!(result.exit_code, crate::core::ExitCode::Success);
        assert!(result.message.is_none());
    }

    /// # Run Result Error
    ///
    /// Verifies RunResult::error() creates correct result.
    ///
    /// ## Test Scenario
    /// - Creates an error result
    ///
    /// ## Expected Outcome
    /// - Exit code matches, message is set
    #[test]
    fn test_run_result_error() {
        let result = RunResult::error(crate::core::ExitCode::GeneralError, "Something went wrong");
        assert!(!result.is_success());
        assert_eq!(result.exit_code, crate::core::ExitCode::GeneralError);
        assert_eq!(result.message, Some("Something went wrong".to_string()));
    }

    /// # Run Result Conflict
    ///
    /// Verifies RunResult::conflict() creates correct result.
    ///
    /// ## Test Scenario
    /// - Creates a conflict result
    ///
    /// ## Expected Outcome
    /// - Exit code is Conflict, state file path is set
    #[test]
    fn test_run_result_conflict() {
        let result = RunResult::conflict(PathBuf::from("/state/file.json"));
        assert!(!result.is_success());
        assert_eq!(result.exit_code, crate::core::ExitCode::Conflict);
        assert_eq!(
            result.state_file_path,
            Some(PathBuf::from("/state/file.json"))
        );
    }

    /// # Run Result Partial Success
    ///
    /// Verifies RunResult::partial_success() creates correct result.
    ///
    /// ## Test Scenario
    /// - Creates a partial success result
    ///
    /// ## Expected Outcome
    /// - Exit code is PartialSuccess
    #[test]
    fn test_run_result_partial_success() {
        let result = RunResult::partial_success("3 of 5 succeeded");
        assert!(!result.is_success());
        assert_eq!(result.exit_code, crate::core::ExitCode::PartialSuccess);
        assert_eq!(result.message, Some("3 of 5 succeeded".to_string()));
    }

    /// # Status Returns Idle When No State File
    ///
    /// Verifies that status returns idle status (not error) when no state file exists.
    ///
    /// ## Test Scenario
    /// - Creates a runner with a temporary directory (no state file)
    /// - Calls status method
    ///
    /// ## Expected Outcome
    /// - Returns success (exit code 0)
    /// - Output contains "idle" phase and status
    #[test]
    fn test_status_returns_idle_when_no_state_file() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        // Use a temp directory that has no state file
        let temp_dir = tempfile::tempdir().unwrap();
        let result = runner.status(Some(temp_dir.path()));

        // Should return success, not an error
        assert!(result.is_success());
        assert_eq!(result.exit_code, crate::core::ExitCode::Success);

        // Output should contain idle status
        let output = String::from_utf8(buffer).unwrap();
        assert!(
            output.contains("idle"),
            "Output should contain 'idle' status: {}",
            output
        );
    }

    /// # Status Idle Output JSON Format
    ///
    /// Verifies that idle status is properly formatted in JSON output.
    ///
    /// ## Test Scenario
    /// - Creates a runner with JSON output format
    /// - Calls status with no state file
    ///
    /// ## Expected Outcome
    /// - JSON output contains phase: "idle" and status: "idle"
    #[test]
    fn test_status_idle_json_format() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Json;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let temp_dir = tempfile::tempdir().unwrap();
        let result = runner.status(Some(temp_dir.path()));

        assert!(result.is_success());

        let output = String::from_utf8(buffer).unwrap();
        // Parse as JSON and verify structure
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["phase"], "idle");
        assert_eq!(json["status"], "idle");
        assert_eq!(json["progress"]["total"], 0);
        assert_eq!(json["progress"]["completed"], 0);
    }

    /// # Error Emission With Code
    ///
    /// Verifies that errors with codes are emitted correctly in NDJSON.
    ///
    /// ## Test Scenario
    /// - Emits an error with a code using NDJSON format
    ///
    /// ## Expected Outcome
    /// - Error contains both message and code in JSON output
    #[test]
    fn test_error_emission_with_code() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error_with_code("Test locked error", Some("locked"));

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"locked\""));
        assert!(output.contains("Test locked error"));
    }

    /// # Error Emission Without Code
    ///
    /// Verifies that errors without codes omit the code field in NDJSON.
    ///
    /// ## Test Scenario
    /// - Emits an error without a code using NDJSON format
    ///
    /// ## Expected Outcome
    /// - Error contains message but no code field
    #[test]
    fn test_error_emission_without_code() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error("Generic error");

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Generic error"));
        assert!(!output.contains("\"code\""));
    }

    /// # Error Emission With All Code Variants
    ///
    /// Verifies all error code strings used across the runner
    /// are correctly emitted in NDJSON output.
    ///
    /// ## Test Scenario
    /// - Emits errors with each code used in the codebase:
    ///   locked, no_state_file, invalid_phase, conflicts_unresolved
    ///
    /// ## Expected Outcome
    /// - Each NDJSON line contains the correct code field
    #[test]
    fn test_error_emission_all_code_variants() {
        let codes = [
            "locked",
            "no_state_file",
            "invalid_phase",
            "conflicts_unresolved",
        ];

        for code in &codes {
            let mut config = create_test_config();
            config.output_format = OutputFormat::Ndjson;

            let mut buffer = Vec::new();
            let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

            runner.emit_error_with_code(&format!("Error: {}", code), Some(code));

            let output = String::from_utf8(buffer).unwrap();
            let json: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
            assert_eq!(json["event"], "error", "event type should be error");
            assert_eq!(
                json["code"], *code,
                "code should be '{}' but got '{}'",
                code, json["code"]
            );
            assert!(
                json["message"].as_str().unwrap().contains(code),
                "message should contain code context"
            );
        }
    }

    /// # Error Emission in Text Format With Code
    ///
    /// Verifies that error codes are rendered in text output too.
    ///
    /// ## Test Scenario
    /// - Emits an error with code "locked" in Text format
    ///
    /// ## Expected Outcome
    /// - Text output contains the error message
    #[test]
    fn test_error_emission_text_format_with_code() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error_with_code("Another merge operation is in progress", Some("locked"));

        let output = String::from_utf8(buffer).unwrap();
        assert!(
            output.contains("Another merge operation is in progress"),
            "Text format should display the error message: got '{output}'"
        );
    }

    /// # Start Event With State File Path
    ///
    /// Verifies the Start event correctly includes state_file_path in NDJSON output.
    ///
    /// ## Test Scenario
    /// - Emits a Start event with a state file path set
    ///
    /// ## Expected Outcome
    /// - NDJSON output includes the state_file_path field
    #[test]
    fn test_start_event_with_state_file_path() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Start {
            total_prs: 3,
            version: "v2.0.0".to_string(),
            target_branch: "release".to_string(),
            state_file_path: Some(PathBuf::from("/tmp/state/merge.json")),
        });

        let output = String::from_utf8(buffer).unwrap();
        let json: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(json["event"], "start");
        assert_eq!(json["total_prs"], 3);
        assert_eq!(json["state_file_path"], "/tmp/state/merge.json");
    }

    /// # Start Event Without State File Path Omits Field
    ///
    /// Verifies the Start event omits state_file_path when None in NDJSON output.
    ///
    /// ## Test Scenario
    /// - Emits a Start event with state_file_path = None
    ///
    /// ## Expected Outcome
    /// - NDJSON output does not include state_file_path field
    #[test]
    fn test_start_event_without_state_file_path() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Start {
            total_prs: 1,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            state_file_path: None,
        });

        let output = String::from_utf8(buffer).unwrap();
        let json: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(json["event"], "start");
        assert!(
            json.get("state_file_path").is_none(),
            "state_file_path should be omitted when None"
        );
    }

    /// # RunResult Success With Message
    ///
    /// Verifies RunResult::success_with_message() sets exit code and message.
    ///
    /// ## Test Scenario
    /// - Creates a success result with a message
    ///
    /// ## Expected Outcome
    /// - Exit code is Success, message is set
    #[test]
    fn test_run_result_success_with_message() {
        let result = RunResult::success_with_message("Merge aborted");
        assert!(result.is_success());
        assert_eq!(result.exit_code, crate::core::ExitCode::Success);
        assert_eq!(result.message, Some("Merge aborted".to_string()));
    }

    /// # RunResult With State File Builder
    ///
    /// Verifies RunResult::with_state_file() builder method chains correctly.
    ///
    /// ## Test Scenario
    /// - Creates a partial success result and chains with_state_file
    ///
    /// ## Expected Outcome
    /// - State file path is set, original fields preserved
    #[test]
    fn test_run_result_with_state_file() {
        let result = RunResult::partial_success("3 of 5 succeeded")
            .with_state_file(PathBuf::from("/state/merge.json"));
        assert_eq!(result.exit_code, crate::core::ExitCode::PartialSuccess);
        assert_eq!(result.message, Some("3 of 5 succeeded".to_string()));
        assert_eq!(
            result.state_file_path,
            Some(PathBuf::from("/state/merge.json"))
        );
    }

    /// # Quiet Mode Still Shows Errors
    ///
    /// Verifies quiet mode shows errors (since errors and conflicts
    /// are always displayed even in quiet mode).
    ///
    /// ## Test Scenario
    /// - Creates runner with quiet=true
    /// - Emits error events with and without codes
    ///
    /// ## Expected Outcome
    /// - Error output is still present (quiet only suppresses progress)
    #[test]
    fn test_quiet_mode_shows_errors() {
        let mut config = create_test_config();
        config.quiet = true;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error_with_code("Something locked", Some("locked"));
        runner.emit_error("Generic failure");

        let output = String::from_utf8(buffer).unwrap();
        assert!(
            output.contains("Something locked"),
            "Quiet mode should still show errors: got '{output}'"
        );
        assert!(
            output.contains("Generic failure"),
            "Quiet mode should still show errors: got '{output}'"
        );
    }

    /// # Full Merge Event Flow in NDJSON
    ///
    /// Simulates a realistic merge flow: start → cherry-pick → conflict →
    /// skip → cherry-pick → success → complete. Validates all events appear
    /// as valid NDJSON and in correct order.
    ///
    /// ## Test Scenario
    /// - Emits the sequence of events a real merge + skip flow would produce
    ///
    /// ## Expected Outcome
    /// - Each line is valid JSON with the expected event type
    /// - Events appear in the correct order
    #[test]
    fn test_full_merge_flow_ndjson() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        // Start
        runner.emit_event(ProgressEvent::Start {
            total_prs: 2,
            version: "v1.0.0".to_string(),
            target_branch: "main".to_string(),
            state_file_path: Some(PathBuf::from("/tmp/state.json")),
        });

        // First PR conflicts
        runner.emit_event(ProgressEvent::CherryPickStart {
            pr_id: 100,
            commit_id: "aaa111".to_string(),
            index: 0,
            total: 2,
        });
        runner.emit_event(ProgressEvent::CherryPickSkipped {
            pr_id: 100,
            reason: Some("Skipped by user due to unresolvable conflict".to_string()),
        });

        // Second PR succeeds
        runner.emit_event(ProgressEvent::CherryPickStart {
            pr_id: 200,
            commit_id: "bbb222".to_string(),
            index: 1,
            total: 2,
        });
        runner.emit_event(ProgressEvent::CherryPickSuccess {
            pr_id: 200,
            commit_id: "bbb222".to_string(),
        });

        // Complete
        runner.emit_event(ProgressEvent::Complete {
            successful: 1,
            failed: 0,
            skipped: 1,
        });

        let output = String::from_utf8(buffer).unwrap();
        let lines: Vec<serde_json::Value> = output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap_or_else(|_| panic!("Invalid JSON line: {}", l)))
            .collect();

        assert_eq!(lines.len(), 6, "Should have 6 NDJSON events");
        assert_eq!(lines[0]["event"], "start");
        assert_eq!(lines[0]["state_file_path"], "/tmp/state.json");
        assert_eq!(lines[1]["event"], "cherry_pick_start");
        assert_eq!(lines[1]["pr_id"], 100);
        assert_eq!(lines[2]["event"], "cherry_pick_skipped");
        assert_eq!(lines[2]["pr_id"], 100);
        assert_eq!(lines[3]["event"], "cherry_pick_start");
        assert_eq!(lines[3]["pr_id"], 200);
        assert_eq!(lines[4]["event"], "cherry_pick_success");
        assert_eq!(lines[4]["pr_id"], 200);
        assert_eq!(lines[5]["event"], "complete");
        assert_eq!(lines[5]["successful"], 1);
        assert_eq!(lines[5]["skipped"], 1);
    }

    /// # Error Event in JSON Format Is Buffered
    ///
    /// Verifies that errors are also buffered in JSON mode (not written immediately).
    ///
    /// ## Test Scenario
    /// - Emits error events in JSON format
    ///
    /// ## Expected Outcome
    /// - Buffer remains empty since JSON buffers all events
    #[test]
    fn test_error_event_buffered_in_json_mode() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Json;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error_with_code("Locked", Some("locked"));
        runner.emit_error("Generic error");

        assert!(
            buffer.is_empty(),
            "JSON format should buffer error events too"
        );
    }

    /// # Skip Event in Text Format
    ///
    /// Verifies CherryPickSkipped events display correctly in text output
    /// including the PR ID and skip reason.
    ///
    /// ## Test Scenario
    /// - Emits a CherryPickSkipped event with a reason
    ///
    /// ## Expected Outcome
    /// - Output mentions the PR being skipped
    #[test]
    fn test_skip_event_text_format() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::CherryPickSkipped {
            pr_id: 42,
            reason: Some("Skipped by user due to unresolvable conflict".to_string()),
        });

        let output = String::from_utf8(buffer).unwrap();
        assert!(
            output.contains("42"),
            "Should mention the PR ID: got '{output}'"
        );
    }

    /// # Multiple Errors With Different Codes in NDJSON
    ///
    /// Verifies that emitting multiple errors produces separate NDJSON lines,
    /// each with the correct code.
    ///
    /// ## Test Scenario
    /// - Emits three errors: locked (with code), generic (no code), invalid_phase (with code)
    ///
    /// ## Expected Outcome
    /// - Three NDJSON lines, each valid JSON with correct fields
    #[test]
    fn test_multiple_errors_ndjson() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_error_with_code("Locked", Some("locked"));
        runner.emit_error("Something went wrong");
        runner.emit_error_with_code("Wrong phase", Some("invalid_phase"));

        let output = String::from_utf8(buffer).unwrap();
        let lines: Vec<serde_json::Value> = output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["code"], "locked");
        assert!(lines[1].get("code").is_none());
        assert_eq!(lines[2]["code"], "invalid_phase");
    }

    /// # Status Idle in NDJSON Format
    ///
    /// Verifies that idle status is properly formatted in NDJSON output.
    ///
    /// ## Test Scenario
    /// - Creates a runner with NDJSON output format
    /// - Calls status with no state file
    ///
    /// ## Expected Outcome
    /// - NDJSON output is valid JSON with idle fields
    #[test]
    fn test_status_idle_ndjson_format() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let temp_dir = tempfile::tempdir().unwrap();
        let result = runner.status(Some(temp_dir.path()));

        assert!(result.is_success());

        let output = String::from_utf8(buffer).unwrap();
        let json: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(json["phase"], "idle");
        assert_eq!(json["status"], "idle");
    }

    /// # Dependency Analysis Events in NDJSON
    ///
    /// Verifies dependency analysis events serialize correctly.
    ///
    /// ## Test Scenario
    /// - Emits DependencyAnalysisStart, DependencyAnalysisComplete,
    ///   and DependencyWarning events
    ///
    /// ## Expected Outcome
    /// - All events produce valid NDJSON with correct fields
    #[test]
    fn test_dependency_analysis_events_ndjson() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::DependencyAnalysisStart { pr_count: 5 });
        runner.emit_event(ProgressEvent::DependencyAnalysisComplete {
            independent: 3,
            partial: 1,
            dependent: 1,
        });
        runner.emit_event(ProgressEvent::DependencyWarning {
            selected_pr_id: 100,
            selected_pr_title: "Feature A".to_string(),
            unselected_pr_id: 200,
            unselected_pr_title: "Feature B".to_string(),
            is_critical: true,
            shared_files: vec!["src/lib.rs".to_string()],
        });

        let output = String::from_utf8(buffer).unwrap();
        let lines: Vec<serde_json::Value> = output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["event"], "dependency_analysis_start");
        assert_eq!(lines[0]["pr_count"], 5);
        assert_eq!(lines[1]["event"], "dependency_analysis_complete");
        assert_eq!(lines[1]["independent"], 3);
        assert_eq!(lines[2]["event"], "dependency_warning");
        assert!(lines[2]["is_critical"].as_bool().unwrap());
    }

    /// # Aborted Event in NDJSON
    ///
    /// Verifies Aborted event serializes correctly with and without message.
    ///
    /// ## Test Scenario
    /// - Emits Aborted events with success=true and success=false
    ///
    /// ## Expected Outcome
    /// - NDJSON contains correct success and message fields
    #[test]
    fn test_aborted_event_ndjson() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Aborted {
            success: true,
            message: None,
        });
        runner.emit_event(ProgressEvent::Aborted {
            success: false,
            message: Some("Failed to clean up".to_string()),
        });

        let output = String::from_utf8(buffer).unwrap();
        let lines: Vec<serde_json::Value> = output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["event"], "aborted");
        assert!(lines[0]["success"].as_bool().unwrap());
        assert!(lines[0].get("message").is_none());
        assert_eq!(lines[1]["event"], "aborted");
        assert!(!lines[1]["success"].as_bool().unwrap());
        assert_eq!(lines[1]["message"], "Failed to clean up");
    }

    /// # Cherry-pick Failed Event in Text Format
    ///
    /// Verifies CherryPickFailed events display the error in text output.
    ///
    /// ## Test Scenario
    /// - Emits a CherryPickFailed event
    ///
    /// ## Expected Outcome
    /// - Output includes the PR ID and error message
    #[test]
    fn test_cherry_pick_failed_event_text() {
        let config = create_test_config();
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::CherryPickFailed {
            pr_id: 99,
            error: "Commit abc123 not found".to_string(),
        });

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("99"), "Should include PR ID");
        assert!(
            output.contains("not found") || output.contains("abc123"),
            "Should include error details: got '{output}'"
        );
    }

    // -----------------------------------------------------------------------
    // Stateful tests (require MERGERS_STATE_DIR env, serialized execution)
    // -----------------------------------------------------------------------

    use crate::core::state::{LockGuard, MergePhase, MergeStateFile, STATE_DIR_ENV};
    use serial_test::file_serial;
    use std::fs;

    /// Helper: sets up a temp state dir + repo dir, sets MERGERS_STATE_DIR env var.
    /// Returns (state_dir, repo_dir) as TempDir handles (kept alive for RAII).
    fn setup_state_env() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = temp.path().join("state");
        let repo_dir = temp.path().join("repo");
        fs::create_dir_all(&state_dir).unwrap();
        fs::create_dir_all(&repo_dir).unwrap();
        unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };
        (temp, repo_dir)
    }

    fn teardown_state_env() {
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// Creates and saves a state file for the given repo_dir with the given phase.
    fn create_state_file_with_phase(repo_dir: &Path, phase: MergePhase) {
        let mut state = MergeStateFile::new(
            repo_dir.to_path_buf(),
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
        state.phase = phase;
        state.save_for_repo().unwrap();
    }

    /// # Abort Returns NoStateFile When No State Exists
    ///
    /// Verifies abort returns the correct error when no state file is found.
    ///
    /// ## Test Scenario
    /// - Sets up temp state dir with no state file
    /// - Calls abort with a repo path
    ///
    /// ## Expected Outcome
    /// - Exit code is NoStateFile
    /// - NDJSON output contains "no_state_file" code
    #[test]
    #[file_serial(state_env)]
    fn test_abort_no_state_file() {
        let (_temp, repo_dir) = setup_state_env();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.abort(Some(&repo_dir));

        assert_eq!(result.exit_code, ExitCode::NoStateFile);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"no_state_file\""));

        teardown_state_env();
    }

    /// # Abort Returns InvalidPhase When Already Completed
    ///
    /// Verifies abort returns invalid_phase when the merge is already terminal.
    ///
    /// ## Test Scenario
    /// - Creates a state file with Completed phase
    /// - Calls abort
    ///
    /// ## Expected Outcome
    /// - Exit code is InvalidPhase
    /// - NDJSON output contains "invalid_phase" code
    #[test]
    #[file_serial(state_env)]
    fn test_abort_invalid_phase() {
        let (_temp, repo_dir) = setup_state_env();
        create_state_file_with_phase(&repo_dir, MergePhase::Completed);

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.abort(Some(&repo_dir));

        assert_eq!(result.exit_code, ExitCode::InvalidPhase);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"invalid_phase\""));
        assert!(output.contains("Completed"));

        teardown_state_env();
    }

    /// # Abort Returns Locked When Lock Is Held
    ///
    /// Verifies abort returns locked error when another operation holds the lock.
    ///
    /// ## Test Scenario
    /// - Acquires a lock on the repo
    /// - Calls abort
    ///
    /// ## Expected Outcome
    /// - Exit code is Locked
    /// - NDJSON output contains "locked" code
    #[test]
    #[file_serial(state_env)]
    fn test_abort_locked() {
        let (_temp, repo_dir) = setup_state_env();

        // Acquire lock before calling abort
        let _lock = LockGuard::acquire(&repo_dir).unwrap();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.abort(Some(&repo_dir));

        assert_eq!(result.exit_code, ExitCode::Locked);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"locked\""));

        teardown_state_env();
    }

    /// # Skip Returns NoStateFile When No State Exists
    ///
    /// Verifies skip returns the correct error when no state file is found.
    ///
    /// ## Test Scenario
    /// - Sets up temp state dir with no state file
    /// - Calls skip with a repo path
    ///
    /// ## Expected Outcome
    /// - Exit code is NoStateFile
    /// - NDJSON output contains "no_state_file" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_skip_no_state_file() {
        let (_temp, repo_dir) = setup_state_env();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.skip(Some(&repo_dir)).await;

        assert_eq!(result.exit_code, ExitCode::NoStateFile);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"no_state_file\""));

        teardown_state_env();
    }

    /// # Skip Returns InvalidPhase When Not Awaiting Conflict
    ///
    /// Verifies skip returns invalid_phase when the merge is not in
    /// AwaitingConflictResolution phase.
    ///
    /// ## Test Scenario
    /// - Creates a state file with CherryPicking phase
    /// - Calls skip
    ///
    /// ## Expected Outcome
    /// - Exit code is InvalidPhase
    /// - NDJSON output contains "invalid_phase" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_skip_invalid_phase() {
        let (_temp, repo_dir) = setup_state_env();
        create_state_file_with_phase(&repo_dir, MergePhase::CherryPicking);

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.skip(Some(&repo_dir)).await;

        assert_eq!(result.exit_code, ExitCode::InvalidPhase);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"invalid_phase\""));

        teardown_state_env();
    }

    /// # Skip Returns Locked When Lock Is Held
    ///
    /// Verifies skip returns locked error when another operation holds the lock.
    ///
    /// ## Test Scenario
    /// - Acquires a lock on the repo
    /// - Calls skip
    ///
    /// ## Expected Outcome
    /// - Exit code is Locked
    /// - NDJSON output contains "locked" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_skip_locked() {
        let (_temp, repo_dir) = setup_state_env();

        let _lock = LockGuard::acquire(&repo_dir).unwrap();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.skip(Some(&repo_dir)).await;

        assert_eq!(result.exit_code, ExitCode::Locked);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"locked\""));

        teardown_state_env();
    }

    /// # Continue Returns NoStateFile When No State Exists
    ///
    /// Verifies continue_merge returns the correct error when no state file is found.
    ///
    /// ## Test Scenario
    /// - Sets up temp state dir with no state file
    /// - Calls continue_merge with a repo path
    ///
    /// ## Expected Outcome
    /// - Exit code is NoStateFile
    /// - NDJSON output contains "no_state_file" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_continue_no_state_file() {
        let (_temp, repo_dir) = setup_state_env();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.continue_merge(Some(&repo_dir)).await;

        assert_eq!(result.exit_code, ExitCode::NoStateFile);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"no_state_file\""));

        teardown_state_env();
    }

    /// # Continue Returns InvalidPhase When Not Awaiting Conflict
    ///
    /// Verifies continue_merge returns invalid_phase when the merge is not in
    /// AwaitingConflictResolution phase.
    ///
    /// ## Test Scenario
    /// - Creates a state file with CherryPicking phase
    /// - Calls continue_merge
    ///
    /// ## Expected Outcome
    /// - Exit code is InvalidPhase
    /// - NDJSON output contains "invalid_phase" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_continue_invalid_phase() {
        let (_temp, repo_dir) = setup_state_env();
        create_state_file_with_phase(&repo_dir, MergePhase::CherryPicking);

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.continue_merge(Some(&repo_dir)).await;

        assert_eq!(result.exit_code, ExitCode::InvalidPhase);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"invalid_phase\""));

        teardown_state_env();
    }

    /// # Continue Returns Locked When Lock Is Held
    ///
    /// Verifies continue_merge returns locked error when another operation
    /// holds the lock.
    ///
    /// ## Test Scenario
    /// - Acquires a lock on the repo
    /// - Calls continue_merge
    ///
    /// ## Expected Outcome
    /// - Exit code is Locked
    /// - NDJSON output contains "locked" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_continue_locked() {
        let (_temp, repo_dir) = setup_state_env();

        let _lock = LockGuard::acquire(&repo_dir).unwrap();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.continue_merge(Some(&repo_dir)).await;

        assert_eq!(result.exit_code, ExitCode::Locked);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"locked\""));

        teardown_state_env();
    }

    /// # Complete Returns NoStateFile When No State Exists
    ///
    /// Verifies complete returns the correct error when no state file is found.
    ///
    /// ## Test Scenario
    /// - Sets up temp state dir with no state file
    /// - Calls complete with a repo path
    ///
    /// ## Expected Outcome
    /// - Exit code is NoStateFile
    /// - NDJSON output contains "no_state_file" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_complete_no_state_file() {
        let (_temp, repo_dir) = setup_state_env();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.complete(Some(&repo_dir), "Done").await;

        assert_eq!(result.exit_code, ExitCode::NoStateFile);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"no_state_file\""));

        teardown_state_env();
    }

    /// # Complete Returns InvalidPhase When Not Ready
    ///
    /// Verifies complete returns invalid_phase when the merge is not in
    /// ReadyForCompletion phase.
    ///
    /// ## Test Scenario
    /// - Creates a state file with CherryPicking phase
    /// - Calls complete
    ///
    /// ## Expected Outcome
    /// - Exit code is InvalidPhase
    /// - NDJSON output contains "invalid_phase" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_complete_invalid_phase() {
        let (_temp, repo_dir) = setup_state_env();
        create_state_file_with_phase(&repo_dir, MergePhase::CherryPicking);

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.complete(Some(&repo_dir), "Done").await;

        assert_eq!(result.exit_code, ExitCode::InvalidPhase);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"invalid_phase\""));

        teardown_state_env();
    }

    /// # Complete Returns Locked When Lock Is Held
    ///
    /// Verifies complete returns locked error when another operation holds the lock.
    ///
    /// ## Test Scenario
    /// - Acquires a lock on the repo
    /// - Calls complete
    ///
    /// ## Expected Outcome
    /// - Exit code is Locked
    /// - NDJSON output contains "locked" code
    #[tokio::test]
    #[file_serial(state_env)]
    async fn test_complete_locked() {
        let (_temp, repo_dir) = setup_state_env();

        let _lock = LockGuard::acquire(&repo_dir).unwrap();

        let mut config = create_test_config();
        config.output_format = OutputFormat::Ndjson;
        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        let result = runner.complete(Some(&repo_dir), "Done").await;

        assert_eq!(result.exit_code, ExitCode::Locked);
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("\"code\":\"locked\""));

        teardown_state_env();
    }
}
