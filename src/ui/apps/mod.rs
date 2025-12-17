//! Mode-specific application types.
//!
//! This module provides the three app types for different operational modes:
//!
//! - [`MergeApp`] - Default mode for cherry-picking PRs
//! - [`MigrationApp`] - Migration analysis mode
//! - [`CleanupApp`] - Branch cleanup mode
//!
//! Each app type contains an [`AppBase`](super::AppBase) for shared state
//! and implements `Deref`/`DerefMut` for ergonomic access to shared fields.

mod cleanup_app;
mod merge_app;
mod migration_app;

pub use cleanup_app::CleanupApp;
pub use merge_app::MergeApp;
pub use migration_app::MigrationApp;
