//! Runner module for merge operations.
//!
//! This module provides the execution engine for merge operations,
//! supporting both interactive and non-interactive modes.
//!
//! # Architecture
//!
//! - `traits.rs` - Defines the `MergeRunner` trait and common types
//! - `merge_engine.rs` - Core orchestration logic shared between runners
//! - `non_interactive.rs` - CLI runner for non-interactive mode

pub mod merge_engine;
pub mod non_interactive;
pub mod release_notes;
pub mod traits;

pub use merge_engine::{CherryPickProcessResult, MergeEngine};
pub use non_interactive::NonInteractiveRunner;
pub use release_notes::{ReleaseNotesRunner, ReleaseNotesRunnerConfig};
pub use traits::{MergeRunnerConfig, RunResult};

// Re-export OutputFormat from models for convenience
pub use crate::models::OutputFormat;
