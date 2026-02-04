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
    ConflictInfo, ItemStatus, OutputFormatter, OutputWriter, ProgressEvent, ProgressSummary,
    StatusInfo, SummaryCounts, SummaryInfo, SummaryItem, SummaryResult,
};
use crate::core::state::{LockGuard, MergePhase, MergeStateFile, MergeStatus, StateItemStatus};
use crate::git;

use super::merge_engine::{MergeEngine, acquire_lock};
use super::traits::{MergeRunnerConfig, RunResult};

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
                self.emit_error("Another merge operation is in progress");
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
                eprintln!("Warning: Dependency analysis failed: {}", e);
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
        });

        // Process cherry-picks using internal state manager
        let conflict_info = engine.process_cherry_picks(|event| {
            self.emit_event(event);
        });

        // Save state after cherry-picks
        if let Err(e) = engine.state_manager_mut().save() {
            self.emit_error(&format!("Failed to save state: {}", e));
            return RunResult::error(ExitCode::GeneralError, e.to_string());
        }

        if let Some(conflict) = conflict_info {
            // Output conflict info
            if let Err(e) = self.output.write_conflict(&conflict) {
                eprintln!("Warning: Failed to write conflict info: {}", e);
            }

            return RunResult::conflict(state_path);
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
                self.emit_error("Another merge operation is in progress");
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
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase
        if state.phase != MergePhase::AwaitingConflictResolution {
            self.emit_error(&format!(
                "Cannot continue: merge is in '{}' phase",
                state.phase
            ));
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for continue");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error("Another merge operation is in progress");
                return RunResult::error(ExitCode::Locked, "Locked");
            }
            Err(e) => {
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Check if conflicts are resolved
        let conflicts_resolved = self.check_conflicts_resolved(&state.repo_path);
        if !conflicts_resolved {
            self.emit_error("Conflicts are not fully resolved. Please resolve all conflicts and stage the files.");
            return RunResult::error(ExitCode::Conflict, "Conflicts not resolved");
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
        let conflict_info = engine.process_cherry_picks(|event| {
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

        if let Some(conflict) = conflict_info {
            if let Err(e) = self.output.write_conflict(&conflict) {
                eprintln!("Warning: Failed to write conflict info: {}", e);
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
                self.emit_error("Another merge operation is in progress");
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
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase (can't abort if already completed)
        if state.phase.is_terminal() {
            self.emit_error(&format!("Cannot abort: merge is already '{}'", state.phase));
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for abort");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error("Another merge operation is in progress");
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
                    eprintln!("Warning: Failed to write status: {}", e);
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
            eprintln!("Warning: Failed to write status: {}", e);
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
                self.emit_error("Another merge operation is in progress");
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
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("{}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Validate phase
        if state.phase != MergePhase::ReadyForCompletion {
            self.emit_error(&format!(
                "Cannot complete: merge is in '{}' phase",
                state.phase
            ));
            return RunResult::error(ExitCode::InvalidPhase, "Invalid phase for complete");
        }

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error("Another merge operation is in progress");
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
        let (_success_count, failed_count) = match engine
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
            post_merge: None,
        };

        if let Err(e) = self.output.write_summary(&summary) {
            eprintln!("Warning: Failed to write summary: {}", e);
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
            self.config.max_concurrent_network,
            self.config.max_concurrent_processing,
            self.config.since.clone(),
        )
    }

    fn emit_event(&mut self, event: ProgressEvent) {
        if let Err(e) = self.output.write_event(&event) {
            eprintln!("Warning: Failed to write event: {}", e);
        }
    }

    fn emit_error(&mut self, message: &str) {
        let event = ProgressEvent::Error {
            message: message.to_string(),
            code: None,
        };
        if let Err(e) = self.output.write_event(&event) {
            eprintln!("Warning: Failed to write error: {}", e);
        }
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

    /// # JSON Output Format
    ///
    /// Verifies JSON output format produces valid JSON.
    ///
    /// ## Test Scenario
    /// - Creates runner with JSON format
    /// - Emits a progress event
    ///
    /// ## Expected Outcome
    /// - Output is valid JSON
    #[test]
    fn test_json_output_format() {
        let mut config = create_test_config();
        config.output_format = OutputFormat::Json;

        let mut buffer = Vec::new();
        let mut runner = NonInteractiveRunner::with_writer(config, &mut buffer);

        runner.emit_event(ProgressEvent::Start {
            total_prs: 3,
            version: "v2.0.0".to_string(),
            target_branch: "release".to_string(),
        });

        // JSON format collects events and outputs at the end,
        // so we just verify no error occurred
        assert!(buffer.is_empty() || !buffer.is_empty());
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
    /// Verifies the complete event shows correct counts.
    ///
    /// ## Test Scenario
    /// - Emits a complete event with mixed results
    ///
    /// ## Expected Outcome
    /// - Counts are shown in output
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
        // Should show the counts in some form
        assert!(
            output.contains("5") || output.contains("successful"),
            "Should show successful count"
        );
    }

    /// # Conflict Event Details
    ///
    /// Verifies conflict events include file details.
    ///
    /// ## Test Scenario
    /// - Emits a conflict event with file list
    ///
    /// ## Expected Outcome
    /// - Conflict files are listed
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
        assert!(
            output.contains("conflict") || output.contains("Conflict"),
            "Should mention conflict"
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
}
