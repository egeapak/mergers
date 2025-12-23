//! Core module for non-interactive merge operations.
//!
//! This module provides the foundational abstractions for both interactive and
//! non-interactive merge workflows, including:
//!
//! - State file management for persisting merge progress
//! - Exit codes for CLI operations
//! - Core operations extracted from UI logic
//! - Output formatting for different display modes

pub mod operations;
pub mod output;
pub mod state;

/// Exit codes for non-interactive merge operations.
///
/// These codes are designed for consumption by CI systems and automation tools,
/// providing clear semantics for different outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    /// All operations completed successfully.
    Success = 0,

    /// General error (configuration, network, git, etc.).
    GeneralError = 1,

    /// Conflict detected - user must resolve and run 'continue'.
    Conflict = 2,

    /// Some PRs succeeded, some failed/skipped.
    PartialSuccess = 3,

    /// No state file found for the repository.
    NoStateFile = 4,

    /// State file exists but operation not valid for current phase.
    InvalidPhase = 5,

    /// No PRs matched selection criteria.
    NoPRsMatched = 6,

    /// Another merge is in progress (locked).
    Locked = 7,
}

impl ExitCode {
    /// Returns the numeric exit code value.
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Returns a human-readable description of the exit code.
    pub fn description(self) -> &'static str {
        match self {
            ExitCode::Success => "All operations completed successfully",
            ExitCode::GeneralError => "General error occurred",
            ExitCode::Conflict => "Conflict detected - resolve and run 'continue'",
            ExitCode::PartialSuccess => "Some operations succeeded, some failed or were skipped",
            ExitCode::NoStateFile => "No state file found for this repository",
            ExitCode::InvalidPhase => "Operation not valid for current merge phase",
            ExitCode::NoPRsMatched => "No pull requests matched the selection criteria",
            ExitCode::Locked => "Another merge operation is in progress",
        }
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        std::process::ExitCode::from(code.code())
    }
}

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Exit Code Values
    ///
    /// Verifies that all exit codes have the correct numeric values.
    ///
    /// ## Test Scenario
    /// - Checks each exit code variant against its expected value
    ///
    /// ## Expected Outcome
    /// - All exit codes map to their documented numeric values
    #[test]
    fn test_exit_code_values() {
        assert_eq!(ExitCode::Success.code(), 0);
        assert_eq!(ExitCode::GeneralError.code(), 1);
        assert_eq!(ExitCode::Conflict.code(), 2);
        assert_eq!(ExitCode::PartialSuccess.code(), 3);
        assert_eq!(ExitCode::NoStateFile.code(), 4);
        assert_eq!(ExitCode::InvalidPhase.code(), 5);
        assert_eq!(ExitCode::NoPRsMatched.code(), 6);
        assert_eq!(ExitCode::Locked.code(), 7);
    }

    /// # Exit Code Descriptions
    ///
    /// Verifies that all exit codes have meaningful descriptions.
    ///
    /// ## Test Scenario
    /// - Checks that each exit code has a non-empty description
    ///
    /// ## Expected Outcome
    /// - All exit codes return non-empty description strings
    #[test]
    fn test_exit_code_descriptions() {
        assert!(!ExitCode::Success.description().is_empty());
        assert!(!ExitCode::GeneralError.description().is_empty());
        assert!(!ExitCode::Conflict.description().is_empty());
        assert!(!ExitCode::PartialSuccess.description().is_empty());
        assert!(!ExitCode::NoStateFile.description().is_empty());
        assert!(!ExitCode::InvalidPhase.description().is_empty());
        assert!(!ExitCode::NoPRsMatched.description().is_empty());
        assert!(!ExitCode::Locked.description().is_empty());
    }

    /// # Exit Code Display
    ///
    /// Verifies that exit codes can be displayed as strings.
    ///
    /// ## Test Scenario
    /// - Uses Display trait to format exit codes
    ///
    /// ## Expected Outcome
    /// - Exit codes format to their description strings
    #[test]
    fn test_exit_code_display() {
        assert_eq!(
            format!("{}", ExitCode::Success),
            ExitCode::Success.description()
        );
        assert_eq!(
            format!("{}", ExitCode::Conflict),
            ExitCode::Conflict.description()
        );
    }

    /// # Exit Code Conversion to std::process::ExitCode
    ///
    /// Verifies that exit codes can be converted to std::process::ExitCode.
    ///
    /// ## Test Scenario
    /// - Converts ExitCode variants to std::process::ExitCode
    ///
    /// ## Expected Outcome
    /// - Conversion succeeds without panicking
    #[test]
    fn test_exit_code_conversion() {
        let _: std::process::ExitCode = ExitCode::Success.into();
        let _: std::process::ExitCode = ExitCode::GeneralError.into();
        let _: std::process::ExitCode = ExitCode::Conflict.into();
    }
}
