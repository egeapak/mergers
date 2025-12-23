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
    StatusInfo, SummaryInfo, SummaryItem, SummaryResult,
};
use crate::core::state::{MergePhase, MergeStateFile, MergeStatus, StateItemStatus};

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
        // Validate required fields
        if self.config.version.is_empty() {
            return RunResult::error(
                ExitCode::GeneralError,
                "Version is required for non-interactive mode",
            );
        }

        // Create the API client
        let client = match self.create_client() {
            Ok(c) => c,
            Err(e) => {
                return RunResult::error(
                    ExitCode::GeneralError,
                    format!("Failed to create API client: {}", e),
                );
            }
        };

        // Create the merge engine
        let engine = self.create_engine(Arc::clone(&client));

        // Load PRs
        let mut prs = match engine.load_pull_requests().await {
            Ok(prs) => prs,
            Err(e) => {
                self.emit_error(&format!("Failed to load PRs: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Select PRs by work item states if configured
        if let Some(ref states) = self.config.select_by_states {
            let count = engine.select_prs_by_states(&mut prs, states);
            if count == 0 {
                self.emit_error("No PRs matched the specified work item states");
                return RunResult::error(
                    ExitCode::NoPRsMatched,
                    "No PRs matched the specified work item states",
                );
            }
        } else {
            // Select all PRs with merge commits
            for pr in &mut prs {
                pr.selected = pr.pr.last_merge_commit.is_some();
            }
        }

        let selected_count = prs.iter().filter(|pr| pr.selected).count();
        if selected_count == 0 {
            self.emit_error("No PRs selected for merge");
            return RunResult::error(ExitCode::NoPRsMatched, "No PRs selected for merge");
        }

        // Set up the repository
        let (repo_path, is_worktree) = match engine.setup_repository() {
            Ok((path, is_worktree)) => (path, is_worktree),
            Err(e) => {
                self.emit_error(&format!("Failed to set up repository: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Acquire lock
        let _lock = match acquire_lock(&repo_path) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                self.emit_error("Another merge operation is in progress");
                return RunResult::error(
                    ExitCode::Locked,
                    "Another merge operation is in progress",
                );
            }
            Err(e) => {
                self.emit_error(&format!("Failed to acquire lock: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        // Create state file
        let base_repo_path = if is_worktree {
            self.config.local_repo.clone()
        } else {
            None
        };

        let mut state =
            engine.create_state_file(repo_path.clone(), base_repo_path, is_worktree, &prs);

        // Emit start event
        self.emit_event(ProgressEvent::Start {
            total_prs: state.cherry_pick_items.len(),
            version: self.config.version.clone(),
            target_branch: self.config.target_branch.clone(),
        });

        // Save initial state
        if let Err(e) = state.save_for_repo() {
            self.emit_error(&format!("Failed to save state: {}", e));
            return RunResult::error(ExitCode::GeneralError, e.to_string());
        }

        // Process cherry-picks
        let conflict_info = engine.process_cherry_picks(&mut state, |event| {
            self.emit_event(event);
        });

        // Save state after cherry-picks
        let state_path = match state.save_for_repo() {
            Ok(path) => path,
            Err(e) => {
                self.emit_error(&format!("Failed to save state: {}", e));
                return RunResult::error(ExitCode::GeneralError, e.to_string());
            }
        };

        if let Some(conflict) = conflict_info {
            // Output conflict info
            if let Err(e) = self.output.write_conflict(&conflict) {
                eprintln!("Warning: Failed to write conflict info: {}", e);
            }

            return RunResult::conflict(state_path);
        }

        // All cherry-picks complete
        let counts = engine.create_summary_counts(&state);
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

        // Load state file
        let mut state = match MergeStateFile::load_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
                self.emit_error(&format!("Failed to load state: {}", e));
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
        let engine = self.create_engine(client);

        // Continue processing
        let conflict_info = engine.process_cherry_picks(&mut state, |event| {
            self.emit_event(event);
        });

        // Save state
        let state_path = match state.save_for_repo() {
            Ok(path) => path,
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

        let counts = engine.create_summary_counts(&state);
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

        // Load state file
        let mut state = match MergeStateFile::load_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
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

        // Load state file
        let state = match MergeStateFile::load_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
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

        // Load state file
        let mut state = match MergeStateFile::load_for_repo(&repo_path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                self.emit_error("No state file found for this repository");
                return RunResult::error(ExitCode::NoStateFile, "No state file found");
            }
            Err(e) => {
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
        let output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(repo_path)
            .output();

        match output {
            Ok(out) => {
                let status = String::from_utf8_lossy(&out.stdout);
                // Check for conflict markers (UU, AA, DD)
                !status.lines().any(|line| {
                    let chars: Vec<char> = line.chars().collect();
                    chars.len() >= 2
                        && ((chars[0] == 'U' || chars[1] == 'U')
                            || (chars[0] == 'A' && chars[1] == 'A')
                            || (chars[0] == 'D' && chars[1] == 'D'))
                })
            }
            Err(_) => false,
        }
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
}
