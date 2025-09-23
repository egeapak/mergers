//! # Merge Tool Library
//!
//! A comprehensive library for managing Azure DevOps pull request merging and migration workflows.
//! This library provides tools for:
//!
//! - Azure DevOps API integration
//! - Configuration management
//! - Git operations and analysis
//! - Pull request migration analysis
//! - Terminal UI for interactive workflows
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use merge_tool::{AzureDevOpsClient, Config};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a client
//! let client = AzureDevOpsClient::new(
//!     "my-org".to_string(),
//!     "my-project".to_string(),
//!     "my-repo".to_string(),
//!     "my-pat".to_string(),
//! )?;
//!
//! // Fetch pull requests
//! let prs = client.fetch_pull_requests("main", None).await?;
//! println!("Found {} pull requests", prs.len());
//! # Ok(())
//! # }
//! ```

pub mod api;
pub mod config;
pub mod git;
pub mod git_config;
pub mod migration;
pub mod models;
pub mod ui;
pub mod utils;

// Re-export commonly used types for convenience
pub use api::AzureDevOpsClient;
pub use config::Config;
pub use models::{AppConfig, Args, DefaultModeConfig, MigrationModeConfig, SharedConfig};

/// Core result type used throughout the library
pub type Result<T> = anyhow::Result<T>;

/// Library version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
