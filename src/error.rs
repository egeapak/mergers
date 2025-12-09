//! Unified error handling for the mergers library.
//!
//! This module provides a comprehensive error hierarchy using `thiserror` for better
//! programmatic error handling and more informative error messages.
//!
//! ## Error Categories
//!
//! - [`ApiError`]: Errors from Azure DevOps API interactions
//! - [`GitError`]: Errors from git operations
//! - [`ConfigError`]: Errors from configuration loading and validation
//! - [`UiError`]: Errors from terminal UI operations
//!
//! ## Example
//!
//! ```rust,no_run
//! use mergers::error::{MergersError, ApiError};
//!
//! fn example() -> Result<(), MergersError> {
//!     // Errors are automatically converted via From trait
//!     Err(ApiError::Unauthorized)?;
//!     Ok(())
//! }
//! ```

use std::path::PathBuf;
use thiserror::Error;

/// The main error type for the mergers library.
///
/// This enum encompasses all possible errors that can occur during
/// merge operations, API calls, git operations, and UI interactions.
#[derive(Error, Debug)]
pub enum MergersError {
    /// An error occurred while interacting with the Azure DevOps API.
    #[error("API error: {0}")]
    Api(#[from] ApiError),

    /// An error occurred during a git operation.
    #[error("Git error: {0}")]
    Git(#[from] GitError),

    /// An error occurred while loading or validating configuration.
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// An error occurred in the terminal UI.
    #[error("UI error: {0}")]
    Ui(#[from] UiError),

    /// A generic error for cases not covered by specific error types.
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Errors that can occur when interacting with the Azure DevOps API.
#[derive(Error, Debug)]
pub enum ApiError {
    /// The API request was unauthorized (401).
    #[error("Unauthorized: invalid or expired Personal Access Token")]
    Unauthorized,

    /// The requested resource was not found (404).
    #[error("Resource not found: {resource}")]
    NotFound {
        /// Description of the resource that was not found.
        resource: String,
    },

    /// The API rate limit was exceeded (429).
    #[error("Rate limit exceeded, retry after {retry_after_seconds} seconds")]
    RateLimited {
        /// Number of seconds to wait before retrying.
        retry_after_seconds: u64,
    },

    /// The API returned an error response.
    #[error("API request failed with status {status}: {message}")]
    RequestFailed {
        /// HTTP status code.
        status: u16,
        /// Error message from the API.
        message: String,
    },

    /// Failed to parse the API response.
    #[error("Failed to parse API response: {message}")]
    ParseError {
        /// Description of the parse error.
        message: String,
    },

    /// A network error occurred.
    #[error("Network error: {0}")]
    Network(#[from] azure_core::Error),

    /// The pull request has no merge commit.
    #[error("Pull request {pr_id} has no merge commit")]
    NoMergeCommit {
        /// The PR ID that lacks a merge commit.
        pr_id: i32,
    },

    /// Exceeded maximum pagination requests.
    #[error("Exceeded maximum requests ({max}) while fetching data, retrieved {retrieved} items")]
    PaginationLimitExceeded {
        /// Maximum allowed requests.
        max: usize,
        /// Number of items retrieved before the limit was hit.
        retrieved: usize,
    },
}

/// Errors that can occur during git operations.
#[derive(Error, Debug, Clone)]
pub enum GitError {
    /// The specified branch already exists.
    #[error("Branch '{branch}' already exists")]
    BranchExists {
        /// Name of the existing branch.
        branch: String,
    },

    /// A worktree already exists at the specified path.
    #[error("Worktree already exists at path: {path}")]
    WorktreeExists {
        /// Path where the worktree exists.
        path: String,
    },

    /// The specified path is not a valid git repository.
    #[error("Not a valid git repository: {path}")]
    NotARepository {
        /// Path that was expected to be a repository.
        path: PathBuf,
    },

    /// The repository path does not exist.
    #[error("Repository path does not exist: {path}")]
    PathNotFound {
        /// Path that was not found.
        path: PathBuf,
    },

    /// A git clone operation failed.
    #[error("Failed to clone repository: {message}")]
    CloneFailed {
        /// Error message from git.
        message: String,
    },

    /// A cherry-pick operation resulted in conflicts.
    #[error("Cherry-pick conflict in {file_count} file(s)")]
    CherryPickConflict {
        /// Number of files with conflicts.
        file_count: usize,
        /// List of conflicted file paths.
        files: Vec<String>,
    },

    /// A cherry-pick operation failed.
    #[error("Cherry-pick failed: {message}")]
    CherryPickFailed {
        /// Error message from git.
        message: String,
    },

    /// Failed to fetch from remote.
    #[error("Failed to fetch from remote: {message}")]
    FetchFailed {
        /// Error message from git.
        message: String,
    },

    /// A git command execution failed.
    #[error("Git command failed: {command} - {message}")]
    CommandFailed {
        /// The git command that failed.
        command: String,
        /// Error message from git.
        message: String,
    },

    /// Invalid git reference (contains invalid characters).
    #[error("Invalid git reference '{reference}': contains forbidden characters")]
    InvalidReference {
        /// The invalid reference string.
        reference: String,
    },

    /// Generic git operation error.
    #[error("{0}")]
    Other(String),
}

/// Errors that can occur during configuration loading and validation.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// A required configuration field is missing.
    #[error("{field} is required (use --{field}, {env_var} env var, or config file)")]
    MissingRequired {
        /// Name of the missing field.
        field: String,
        /// Environment variable name for this field.
        env_var: String,
    },

    /// Failed to read the configuration file.
    #[error("Failed to read config file at {path}: {message}")]
    FileReadError {
        /// Path to the config file.
        path: PathBuf,
        /// Error message.
        message: String,
    },

    /// Failed to parse the configuration file.
    #[error("Failed to parse config file at {path}: {message}")]
    ParseError {
        /// Path to the config file.
        path: PathBuf,
        /// Parse error message.
        message: String,
    },

    /// An invalid value was provided for a configuration field.
    #[error("Invalid value for {field}: {message}")]
    InvalidValue {
        /// Name of the field with invalid value.
        field: String,
        /// Description of why the value is invalid.
        message: String,
    },

    /// Failed to parse a date string.
    #[error("Failed to parse date '{input}': {message}")]
    DateParseError {
        /// The input date string.
        input: String,
        /// Parse error message.
        message: String,
    },

    /// Failed to create config directory.
    #[error("Failed to create config directory at {path}: {message}")]
    DirectoryCreationError {
        /// Path where directory creation failed.
        path: PathBuf,
        /// Error message.
        message: String,
    },
}

/// Errors that can occur in the terminal UI.
#[derive(Error, Debug)]
pub enum UiError {
    /// Failed to initialize the terminal.
    #[error("Failed to initialize terminal: {0}")]
    TerminalInitError(String),

    /// Failed to render UI.
    #[error("Failed to render UI: {0}")]
    RenderError(String),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// User cancelled the operation.
    #[error("Operation cancelled by user")]
    Cancelled,
}

/// Type alias for Results using MergersError.
///
/// Note: This is not re-exported from the crate root to avoid shadowing `anyhow::Result`.
/// Use explicitly as `error::Result<T>` when needed.
pub type MergersResult<T> = std::result::Result<T, MergersError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// # API Error Display
    ///
    /// Tests that API errors display correctly formatted messages.
    ///
    /// ## Test Scenario
    /// - Creates various ApiError variants
    /// - Tests their Display implementation
    ///
    /// ## Expected Outcome
    /// - Each error variant produces a clear, informative message
    #[test]
    fn test_api_error_display() {
        let unauthorized = ApiError::Unauthorized;
        assert!(unauthorized.to_string().contains("Unauthorized"));

        let not_found = ApiError::NotFound {
            resource: "PR #123".to_string(),
        };
        assert!(not_found.to_string().contains("PR #123"));

        let rate_limited = ApiError::RateLimited {
            retry_after_seconds: 60,
        };
        assert!(rate_limited.to_string().contains("60 seconds"));

        let request_failed = ApiError::RequestFailed {
            status: 500,
            message: "Internal Server Error".to_string(),
        };
        assert!(request_failed.to_string().contains("500"));
        assert!(request_failed.to_string().contains("Internal Server Error"));
    }

    /// # Git Error Display
    ///
    /// Tests that Git errors display correctly formatted messages.
    ///
    /// ## Test Scenario
    /// - Creates various GitError variants
    /// - Tests their Display implementation
    ///
    /// ## Expected Outcome
    /// - Each error variant produces a clear, informative message
    #[test]
    fn test_git_error_display() {
        let branch_exists = GitError::BranchExists {
            branch: "feature/test".to_string(),
        };
        assert!(branch_exists.to_string().contains("feature/test"));

        let worktree_exists = GitError::WorktreeExists {
            path: "/tmp/worktree".to_string(),
        };
        assert!(worktree_exists.to_string().contains("/tmp/worktree"));

        let conflict = GitError::CherryPickConflict {
            file_count: 3,
            files: vec!["a.rs".to_string(), "b.rs".to_string(), "c.rs".to_string()],
        };
        assert!(conflict.to_string().contains("3 file(s)"));
    }

    /// # Config Error Display
    ///
    /// Tests that Config errors display correctly formatted messages.
    ///
    /// ## Test Scenario
    /// - Creates various ConfigError variants
    /// - Tests their Display implementation
    ///
    /// ## Expected Outcome
    /// - Each error variant produces a clear, informative message with hints
    #[test]
    fn test_config_error_display() {
        let missing = ConfigError::MissingRequired {
            field: "organization".to_string(),
            env_var: "MERGERS_ORGANIZATION".to_string(),
        };
        let msg = missing.to_string();
        assert!(msg.contains("organization"));
        assert!(msg.contains("MERGERS_ORGANIZATION"));
        assert!(msg.contains("--organization"));

        let invalid_date = ConfigError::DateParseError {
            input: "invalid".to_string(),
            message: "unrecognized format".to_string(),
        };
        assert!(invalid_date.to_string().contains("invalid"));
    }

    /// # Error Conversion
    ///
    /// Tests that errors convert correctly through the From trait.
    ///
    /// ## Test Scenario
    /// - Creates specific error types
    /// - Converts them to MergersError
    ///
    /// ## Expected Outcome
    /// - All error types convert seamlessly to MergersError
    #[test]
    fn test_error_conversion() {
        let api_error = ApiError::Unauthorized;
        let mergers_error: MergersError = api_error.into();
        assert!(matches!(mergers_error, MergersError::Api(_)));

        let git_error = GitError::BranchExists {
            branch: "test".to_string(),
        };
        let mergers_error: MergersError = git_error.into();
        assert!(matches!(mergers_error, MergersError::Git(_)));

        let config_error = ConfigError::MissingRequired {
            field: "pat".to_string(),
            env_var: "MERGERS_PAT".to_string(),
        };
        let mergers_error: MergersError = config_error.into();
        assert!(matches!(mergers_error, MergersError::Config(_)));
    }

    /// # Git Error Clone
    ///
    /// Tests that GitError implements Clone correctly.
    ///
    /// ## Test Scenario
    /// - Creates GitError instances
    /// - Clones them and verifies equality
    ///
    /// ## Expected Outcome
    /// - Cloned errors are equivalent to originals
    #[test]
    fn test_git_error_clone() {
        let original = GitError::CherryPickConflict {
            file_count: 2,
            files: vec!["a.rs".to_string(), "b.rs".to_string()],
        };
        let cloned = original.clone();

        match (&original, &cloned) {
            (
                GitError::CherryPickConflict {
                    file_count: c1,
                    files: f1,
                },
                GitError::CherryPickConflict {
                    file_count: c2,
                    files: f2,
                },
            ) => {
                assert_eq!(c1, c2);
                assert_eq!(f1, f2);
            }
            _ => panic!("Clone produced different variant"),
        }
    }
}
