//! State management for merge operations.
//!
//! This module provides state persistence for merge operations, enabling:
//!
//! - Resume after conflicts
//! - Cross-mode resume (TUI ↔ CLI)
//! - Audit trail of merge operations
//!
//! # State File Location
//!
//! State files are stored per-repository using a hash of the repository path:
//!
//! ```text
//! Default: ~/.local/state/mergers/merge-{hash}.json
//! Override: $MERGERS_STATE_DIR/merge-{hash}.json
//! ```
//!
//! Where `{hash}` is the first 16 characters of SHA-256 of the canonical repository path.

mod file;
mod manager;

pub use file::{
    LockGuard, MergePhase, MergeStateFile, MergeStateFileBuilder, MergeStatus, STATE_DIR_ENV,
    StateCherryPickItem, StateItemStatus, compute_repo_hash, lock_path_for_repo, path_for_repo,
    state_dir,
};
pub use manager::{StateCreateConfig, StateManager};
