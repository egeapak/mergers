//! Azure DevOps API client module.
//!
//! This module provides a client for interacting with Azure DevOps APIs,
//! specifically for managing pull requests and work items in merge workflows.
//!
//! ## Features
//!
//! - Pull request fetching with pagination support
//! - Work item retrieval and state management
//! - Terminal state analysis for migration workflows
//! - PR labeling and tagging
//!
//! ## Example
//!
//! ```rust,no_run
//! use mergers::AzureDevOpsClient;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = AzureDevOpsClient::new(
//!     "my-org".to_string(),
//!     "my-project".to_string(),
//!     "my-repo".to_string(),
//!     "my-pat".to_string(),
//! )?;
//!
//! // Fetch pull requests from the main branch
//! let prs = client.fetch_pull_requests("main", None).await?;
//! println!("Found {} pull requests", prs.len());
//! # Ok(())
//! # }
//! ```

mod client;
mod credential;
mod mappers;

// Re-export the client and its public items
pub use client::{AzureDevOpsClient, filter_prs_without_merged_tag};
pub use credential::PatCredential;
