//! Traits and common types for merge runners.
//!
//! This module defines the interfaces that both interactive and
//! non-interactive runners implement.

use std::path::PathBuf;

use crate::core::ExitCode;
use crate::models::OutputFormat;

/// Configuration for a merge runner.
#[derive(Debug, Clone)]
pub struct MergeRunnerConfig {
    /// Azure DevOps organization.
    pub organization: String,
    /// Azure DevOps project.
    pub project: String,
    /// Azure DevOps repository.
    pub repository: String,
    /// Personal access token for Azure DevOps.
    pub pat: String,
    /// Development branch (source of PRs).
    pub dev_branch: String,
    /// Target branch for cherry-picks.
    pub target_branch: String,
    /// Version string (e.g., "v1.0.0").
    pub version: String,
    /// Tag prefix for PRs.
    pub tag_prefix: String,
    /// State for work items after completion.
    pub work_item_state: String,
    /// Work item states for PR selection (comma-separated).
    pub select_by_states: Option<String>,
    /// Local repository path for worktree creation.
    pub local_repo: Option<PathBuf>,
    /// Whether to run git hooks.
    pub run_hooks: bool,
    /// Output format (text, json, ndjson).
    pub output_format: OutputFormat,
    /// Whether to suppress progress output.
    pub quiet: bool,
    /// Maximum concurrent network operations.
    pub max_concurrent_network: usize,
    /// Maximum concurrent processing operations.
    pub max_concurrent_processing: usize,
    /// Filter PRs by date (e.g., "1mo", "2w", "2025-01-15").
    pub since: Option<String>,
}

/// Result of a merge operation.
#[derive(Debug)]
pub struct RunResult {
    /// Exit code for the operation.
    pub exit_code: ExitCode,
    /// Optional message to display.
    pub message: Option<String>,
    /// State file path if saved.
    pub state_file_path: Option<PathBuf>,
}

impl RunResult {
    /// Creates a successful result.
    pub fn success() -> Self {
        Self {
            exit_code: ExitCode::Success,
            message: None,
            state_file_path: None,
        }
    }

    /// Creates a success result with a message.
    pub fn success_with_message(message: impl Into<String>) -> Self {
        Self {
            exit_code: ExitCode::Success,
            message: Some(message.into()),
            state_file_path: None,
        }
    }

    /// Creates an error result.
    pub fn error(code: ExitCode, message: impl Into<String>) -> Self {
        Self {
            exit_code: code,
            message: Some(message.into()),
            state_file_path: None,
        }
    }

    /// Creates a conflict result.
    pub fn conflict(state_file_path: PathBuf) -> Self {
        Self {
            exit_code: ExitCode::Conflict,
            message: Some("Conflict detected - resolve and run 'merge continue'".into()),
            state_file_path: Some(state_file_path),
        }
    }

    /// Creates a partial success result.
    pub fn partial_success(message: impl Into<String>) -> Self {
        Self {
            exit_code: ExitCode::PartialSuccess,
            message: Some(message.into()),
            state_file_path: None,
        }
    }

    /// Sets the state file path.
    pub fn with_state_file(mut self, path: PathBuf) -> Self {
        self.state_file_path = Some(path);
        self
    }

    /// Returns true if the operation was successful.
    pub fn is_success(&self) -> bool {
        matches!(self.exit_code, ExitCode::Success)
    }

    /// Returns true if there was a conflict.
    pub fn is_conflict(&self) -> bool {
        matches!(self.exit_code, ExitCode::Conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Run Result Constructors
    ///
    /// Verifies RunResult constructor methods.
    ///
    /// ## Test Scenario
    /// - Creates results with different constructors
    ///
    /// ## Expected Outcome
    /// - Fields are set correctly
    #[test]
    fn test_run_result_constructors() {
        let success = RunResult::success();
        assert!(success.is_success());
        assert!(success.message.is_none());

        let with_msg = RunResult::success_with_message("Done");
        assert!(with_msg.is_success());
        assert_eq!(with_msg.message.as_deref(), Some("Done"));

        let error = RunResult::error(ExitCode::GeneralError, "Failed");
        assert!(!error.is_success());
        assert_eq!(error.exit_code, ExitCode::GeneralError);

        let conflict = RunResult::conflict(PathBuf::from("/tmp/state.json"));
        assert!(conflict.is_conflict());
        assert!(conflict.state_file_path.is_some());

        let partial = RunResult::partial_success("Some failed");
        assert_eq!(partial.exit_code, ExitCode::PartialSuccess);
    }

    /// # Run Result With State File
    ///
    /// Verifies the with_state_file builder method.
    ///
    /// ## Test Scenario
    /// - Creates a result and adds state file path
    ///
    /// ## Expected Outcome
    /// - State file path is set correctly
    #[test]
    fn test_run_result_with_state_file() {
        let result = RunResult::success().with_state_file(PathBuf::from("/tmp/state.json"));
        assert!(result.is_success());
        assert_eq!(
            result.state_file_path,
            Some(PathBuf::from("/tmp/state.json"))
        );
    }
}
