//! State file management for merge operations.
//!
//! This module provides persistent state storage for merge operations,
//! enabling resume after conflicts and cross-mode (TUI ↔ CLI) handoffs.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current schema version for state files.
/// Increment when making breaking changes to the state file format.
pub const SCHEMA_VERSION: u32 = 1;

/// Environment variable to override the state directory.
pub const STATE_DIR_ENV: &str = "MERGERS_STATE_DIR";

/// Merge phase representing the current stage of the merge operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergePhase {
    /// Loading data from Azure DevOps.
    Loading,
    /// Repository setup (worktree/clone).
    Setup,
    /// Cherry-picking in progress.
    CherryPicking,
    /// Waiting for conflict resolution.
    AwaitingConflictResolution,
    /// Cherry-picks done, awaiting 'complete'.
    ReadyForCompletion,
    /// Running post-merge tasks (tagging, work item updates).
    Completing,
    /// All done.
    Completed,
    /// Aborted by user.
    Aborted,
}

impl MergePhase {
    /// Returns a human-readable description of the phase.
    pub fn description(&self) -> &'static str {
        match self {
            MergePhase::Loading => "Loading data from Azure DevOps",
            MergePhase::Setup => "Setting up repository",
            MergePhase::CherryPicking => "Cherry-picking commits",
            MergePhase::AwaitingConflictResolution => "Awaiting conflict resolution",
            MergePhase::ReadyForCompletion => "Ready for completion",
            MergePhase::Completing => "Running post-merge tasks",
            MergePhase::Completed => "Completed",
            MergePhase::Aborted => "Aborted",
        }
    }

    /// Returns true if this phase indicates the merge is finished (completed or aborted).
    pub fn is_terminal(&self) -> bool {
        matches!(self, MergePhase::Completed | MergePhase::Aborted)
    }
}

impl std::fmt::Display for MergePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// Final status of a completed merge operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStatus {
    /// All operations completed successfully.
    Success,
    /// Some operations succeeded, some failed or were skipped.
    PartialSuccess,
    /// Operation was aborted by user.
    Aborted,
    /// Operation failed with an error.
    Failed,
}

impl std::fmt::Display for MergeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeStatus::Success => write!(f, "Success"),
            MergeStatus::PartialSuccess => write!(f, "Partial Success"),
            MergeStatus::Aborted => write!(f, "Aborted"),
            MergeStatus::Failed => write!(f, "Failed"),
        }
    }
}

/// Status of a single cherry-pick item in the state file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateItemStatus {
    /// Not yet processed.
    Pending,
    /// Successfully cherry-picked.
    Success,
    /// Conflict occurred, awaiting resolution.
    Conflict,
    /// Skipped by user.
    Skipped,
    /// Failed with an error message.
    Failed { message: String },
}

impl std::fmt::Display for StateItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateItemStatus::Pending => write!(f, "Pending"),
            StateItemStatus::Success => write!(f, "Success"),
            StateItemStatus::Conflict => write!(f, "Conflict"),
            StateItemStatus::Skipped => write!(f, "Skipped"),
            StateItemStatus::Failed { message } => write!(f, "Failed: {}", message),
        }
    }
}

/// A cherry-pick item stored in the state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCherryPickItem {
    /// The commit ID to cherry-pick.
    pub commit_id: String,
    /// The PR ID this commit belongs to.
    pub pr_id: i32,
    /// The PR title for display purposes.
    pub pr_title: String,
    /// Current status of this item.
    pub status: StateItemStatus,
    /// Work item IDs associated with this PR.
    #[serde(default)]
    pub work_item_ids: Vec<i32>,
}

/// Persistent state file for merge operations.
///
/// This structure is serialized to JSON and stored per-repository.
/// It enables resume after conflicts and cross-mode handoffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeStateFile {
    /// Schema version for forward compatibility.
    pub schema_version: u32,

    // Timestamps
    /// When the merge operation was started.
    pub created_at: DateTime<Utc>,
    /// When the state file was last updated.
    pub updated_at: DateTime<Utc>,

    // Repository Identity
    /// Path to the worktree or cloned repository.
    pub repo_path: PathBuf,
    /// Path to the base repository (for worktrees).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_repo_path: Option<PathBuf>,
    /// Whether this is a worktree or a clone.
    pub is_worktree: bool,

    // Azure DevOps Context
    /// Azure DevOps organization name.
    pub organization: String,
    /// Azure DevOps project name.
    pub project: String,
    /// Azure DevOps repository name.
    pub repository: String,

    // Branch Configuration
    /// Source branch for PRs.
    pub dev_branch: String,
    /// Target branch for cherry-picks.
    pub target_branch: String,
    /// Merge version string (e.g., "v1.2.3").
    pub merge_version: String,

    // Cherry-pick State
    /// List of items to cherry-pick.
    pub cherry_pick_items: Vec<StateCherryPickItem>,
    /// Current index in the cherry_pick_items list.
    pub current_index: usize,

    // Current Phase
    /// Current phase of the merge operation.
    pub phase: MergePhase,
    /// Files with conflicts (if in AwaitingConflictResolution phase).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicted_files: Option<Vec<String>>,

    // Settings
    /// State to set work items to after completion.
    pub work_item_state: String,
    /// Prefix for PR tags.
    pub tag_prefix: String,
    /// Whether git hooks are enabled for this merge.
    #[serde(default)]
    pub run_hooks: bool,

    // Completion Info
    /// When the merge was completed (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Final status of the merge (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_status: Option<MergeStatus>,
}

/// Builder for creating `MergeStateFile` instances.
///
/// This provides a fluent API for constructing state files with
/// better ergonomics than the 12-argument constructor.
///
/// # Example
///
/// ```ignore
/// let state = MergeStateFileBuilder::new()
///     .repo_path("/work/repo")
///     .organization("my-org")
///     .project("my-project")
///     .repository("my-repo")
///     .dev_branch("dev")
///     .target_branch("main")
///     .merge_version("v1.0.0")
///     .work_item_state("Done")
///     .tag_prefix("merged-")
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct MergeStateFileBuilder {
    repo_path: Option<PathBuf>,
    base_repo_path: Option<PathBuf>,
    is_worktree: bool,
    organization: Option<String>,
    project: Option<String>,
    repository: Option<String>,
    dev_branch: Option<String>,
    target_branch: Option<String>,
    merge_version: Option<String>,
    work_item_state: Option<String>,
    tag_prefix: Option<String>,
    run_hooks: bool,
}

impl MergeStateFileBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the path to the worktree or cloned repository.
    pub fn repo_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.repo_path = Some(path.into());
        self
    }

    /// Sets the path to the base repository (for worktrees).
    pub fn base_repo_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.base_repo_path = Some(path.into());
        self
    }

    /// Sets whether this is a worktree or a clone.
    pub fn is_worktree(mut self, is_worktree: bool) -> Self {
        self.is_worktree = is_worktree;
        self
    }

    /// Sets the Azure DevOps organization name.
    pub fn organization<S: Into<String>>(mut self, org: S) -> Self {
        self.organization = Some(org.into());
        self
    }

    /// Sets the Azure DevOps project name.
    pub fn project<S: Into<String>>(mut self, project: S) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets the Azure DevOps repository name.
    pub fn repository<S: Into<String>>(mut self, repo: S) -> Self {
        self.repository = Some(repo.into());
        self
    }

    /// Sets the source branch for PRs.
    pub fn dev_branch<S: Into<String>>(mut self, branch: S) -> Self {
        self.dev_branch = Some(branch.into());
        self
    }

    /// Sets the target branch for cherry-picks.
    pub fn target_branch<S: Into<String>>(mut self, branch: S) -> Self {
        self.target_branch = Some(branch.into());
        self
    }

    /// Sets the merge version string (e.g., "v1.2.3").
    pub fn merge_version<S: Into<String>>(mut self, version: S) -> Self {
        self.merge_version = Some(version.into());
        self
    }

    /// Sets the state to set work items to after completion.
    pub fn work_item_state<S: Into<String>>(mut self, state: S) -> Self {
        self.work_item_state = Some(state.into());
        self
    }

    /// Sets the prefix for PR tags.
    pub fn tag_prefix<S: Into<String>>(mut self, prefix: S) -> Self {
        self.tag_prefix = Some(prefix.into());
        self
    }

    /// Sets whether git hooks are enabled for this merge.
    pub fn run_hooks(mut self, run_hooks: bool) -> Self {
        self.run_hooks = run_hooks;
        self
    }

    /// Builds the `MergeStateFile`.
    ///
    /// # Panics
    ///
    /// Panics if required fields are not set:
    /// - `repo_path`
    /// - `organization`
    /// - `project`
    /// - `repository`
    /// - `dev_branch`
    /// - `target_branch`
    /// - `merge_version`
    /// - `work_item_state`
    /// - `tag_prefix`
    pub fn build(self) -> MergeStateFile {
        let now = Utc::now();
        MergeStateFile {
            schema_version: SCHEMA_VERSION,
            created_at: now,
            updated_at: now,
            repo_path: self.repo_path.expect("repo_path is required"),
            base_repo_path: self.base_repo_path,
            is_worktree: self.is_worktree,
            organization: self.organization.expect("organization is required"),
            project: self.project.expect("project is required"),
            repository: self.repository.expect("repository is required"),
            dev_branch: self.dev_branch.expect("dev_branch is required"),
            target_branch: self.target_branch.expect("target_branch is required"),
            merge_version: self.merge_version.expect("merge_version is required"),
            cherry_pick_items: Vec::new(),
            current_index: 0,
            phase: MergePhase::Loading,
            conflicted_files: None,
            work_item_state: self.work_item_state.expect("work_item_state is required"),
            tag_prefix: self.tag_prefix.expect("tag_prefix is required"),
            run_hooks: self.run_hooks,
            completed_at: None,
            final_status: None,
        }
    }

    /// Attempts to build the `MergeStateFile`, returning an error if required fields are missing.
    ///
    /// # Errors
    ///
    /// Returns an error if any required field is not set.
    pub fn try_build(self) -> Result<MergeStateFile> {
        let now = Utc::now();
        Ok(MergeStateFile {
            schema_version: SCHEMA_VERSION,
            created_at: now,
            updated_at: now,
            repo_path: self
                .repo_path
                .ok_or_else(|| anyhow::anyhow!("repo_path is required"))?,
            base_repo_path: self.base_repo_path,
            is_worktree: self.is_worktree,
            organization: self
                .organization
                .ok_or_else(|| anyhow::anyhow!("organization is required"))?,
            project: self
                .project
                .ok_or_else(|| anyhow::anyhow!("project is required"))?,
            repository: self
                .repository
                .ok_or_else(|| anyhow::anyhow!("repository is required"))?,
            dev_branch: self
                .dev_branch
                .ok_or_else(|| anyhow::anyhow!("dev_branch is required"))?,
            target_branch: self
                .target_branch
                .ok_or_else(|| anyhow::anyhow!("target_branch is required"))?,
            merge_version: self
                .merge_version
                .ok_or_else(|| anyhow::anyhow!("merge_version is required"))?,
            cherry_pick_items: Vec::new(),
            current_index: 0,
            phase: MergePhase::Loading,
            conflicted_files: None,
            work_item_state: self
                .work_item_state
                .ok_or_else(|| anyhow::anyhow!("work_item_state is required"))?,
            tag_prefix: self
                .tag_prefix
                .ok_or_else(|| anyhow::anyhow!("tag_prefix is required"))?,
            run_hooks: self.run_hooks,
            completed_at: None,
            final_status: None,
        })
    }
}

impl MergeStateFile {
    /// Creates a builder for constructing a `MergeStateFile`.
    ///
    /// This is the preferred way to create new state files.
    pub fn builder() -> MergeStateFileBuilder {
        MergeStateFileBuilder::new()
    }

    /// Creates a new state file with initial values.
    ///
    /// Consider using `MergeStateFile::builder()` for a more ergonomic API.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repo_path: PathBuf,
        base_repo_path: Option<PathBuf>,
        is_worktree: bool,
        organization: String,
        project: String,
        repository: String,
        dev_branch: String,
        target_branch: String,
        merge_version: String,
        work_item_state: String,
        tag_prefix: String,
        run_hooks: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            schema_version: SCHEMA_VERSION,
            created_at: now,
            updated_at: now,
            repo_path,
            base_repo_path,
            is_worktree,
            organization,
            project,
            repository,
            dev_branch,
            target_branch,
            merge_version,
            cherry_pick_items: Vec::new(),
            current_index: 0,
            phase: MergePhase::Loading,
            conflicted_files: None,
            work_item_state,
            tag_prefix,
            run_hooks,
            completed_at: None,
            final_status: None,
        }
    }

    /// Loads a state file from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read state file: {}", path.display()))?;
        let state: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse state file: {}", path.display()))?;
        Ok(state)
    }

    /// Loads a state file for a repository, if it exists.
    pub fn load_for_repo(repo_path: &Path) -> Result<Option<Self>> {
        let state_path = path_for_repo(repo_path)?;
        if state_path.exists() {
            Ok(Some(Self::load(&state_path)?))
        } else {
            Ok(None)
        }
    }

    /// Loads and validates a state file for a repository.
    ///
    /// Returns a detailed error if the state file is corrupted or invalid.
    /// The error message includes suggestions for recovery.
    pub fn load_and_validate_for_repo(repo_path: &Path) -> Result<Option<Self>> {
        let state_path = path_for_repo(repo_path)?;
        if !state_path.exists() {
            return Ok(None);
        }

        // Try to load the file
        let state = match Self::load(&state_path) {
            Ok(s) => s,
            Err(e) => {
                // Provide actionable error message for parse failures
                anyhow::bail!(
                    "State file corrupted: {}\n\n\
                     To recover, either:\n  \
                     1. Run 'mergers merge abort' to clean up, or\n  \
                     2. Manually delete the state file at: {}",
                    e,
                    state_path.display()
                );
            }
        };

        // Validate the loaded state
        if let Err(e) = state.validate() {
            anyhow::bail!(
                "{}\n\n\
                 To recover, either:\n  \
                 1. Run 'mergers merge abort' to clean up, or\n  \
                 2. Manually delete the state file at: {}",
                e,
                state_path.display()
            );
        }

        Ok(Some(state))
    }

    /// Saves the state file to disk atomically.
    ///
    /// Uses write-to-temp-then-rename pattern for atomicity.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.updated_at = Utc::now();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create state directory: {}", parent.display())
            })?;
        }

        // Write to temporary file first
        let temp_path = path.with_extension("json.tmp");
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize state file")?;

        let mut file = fs::File::create(&temp_path)
            .with_context(|| format!("Failed to create temp file: {}", temp_path.display()))?;
        file.write_all(content.as_bytes())
            .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to sync temp file: {}", temp_path.display()))?;

        // Atomically rename to final path
        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to rename temp file to: {}", path.display()))?;

        Ok(())
    }

    /// Saves the state file to the default location for this repository.
    pub fn save_for_repo(&mut self) -> Result<PathBuf> {
        let path = path_for_repo(&self.repo_path)?;
        self.save(&path)?;
        Ok(path)
    }

    /// Updates the phase and saves.
    pub fn set_phase(&mut self, phase: MergePhase) -> Result<PathBuf> {
        self.phase = phase;
        self.save_for_repo()
    }

    /// Marks the merge as completed with the given status.
    pub fn mark_completed(&mut self, status: MergeStatus) -> Result<PathBuf> {
        self.phase = MergePhase::Completed;
        self.completed_at = Some(Utc::now());
        self.final_status = Some(status);
        self.save_for_repo()
    }

    /// Returns the count of items by status.
    pub fn status_counts(&self) -> StatusCounts {
        let mut counts = StatusCounts::default();
        for item in &self.cherry_pick_items {
            match &item.status {
                StateItemStatus::Pending => counts.pending += 1,
                StateItemStatus::Success => counts.success += 1,
                StateItemStatus::Conflict => counts.conflict += 1,
                StateItemStatus::Skipped => counts.skipped += 1,
                StateItemStatus::Failed { .. } => counts.failed += 1,
            }
        }
        counts
    }

    /// Validates the state file for consistency and correctness.
    ///
    /// Checks:
    /// - Schema version is supported
    /// - Required fields are present and valid
    /// - Index is within bounds
    /// - Phase-specific invariants are met
    pub fn validate(&self) -> Result<()> {
        // Check schema version
        if self.schema_version != SCHEMA_VERSION {
            anyhow::bail!(
                "Unsupported schema version: {} (expected {}). \
                 The state file may have been created by a different version of mergers.",
                self.schema_version,
                SCHEMA_VERSION
            );
        }

        // Check required fields
        if self.repo_path.as_os_str().is_empty() {
            anyhow::bail!("State file corrupted: missing required field 'repo_path'");
        }

        if self.organization.is_empty() {
            anyhow::bail!("State file corrupted: missing required field 'organization'");
        }

        if self.project.is_empty() {
            anyhow::bail!("State file corrupted: missing required field 'project'");
        }

        if self.repository.is_empty() {
            anyhow::bail!("State file corrupted: missing required field 'repository'");
        }

        // Check index bounds
        if !self.cherry_pick_items.is_empty() && self.current_index > self.cherry_pick_items.len() {
            anyhow::bail!(
                "State file corrupted: current_index ({}) exceeds cherry_pick_items count ({})",
                self.current_index,
                self.cherry_pick_items.len()
            );
        }

        // Check phase-specific invariants
        if self.phase == MergePhase::AwaitingConflictResolution && self.conflicted_files.is_none() {
            anyhow::bail!(
                "State file corrupted: phase is 'AwaitingConflictResolution' but no conflicted_files recorded"
            );
        }

        Ok(())
    }
}

/// Counts of items by status.
#[derive(Debug, Default, Clone)]
pub struct StatusCounts {
    pub pending: usize,
    pub success: usize,
    pub conflict: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl StatusCounts {
    /// Returns the total number of items.
    pub fn total(&self) -> usize {
        self.pending + self.success + self.conflict + self.skipped + self.failed
    }

    /// Returns the number of completed items (success, skipped, or failed).
    pub fn completed(&self) -> usize {
        self.success + self.skipped + self.failed
    }
}

/// Returns the state directory path.
///
/// Uses `MERGERS_STATE_DIR` environment variable if set,
/// otherwise uses the XDG state directory.
pub fn state_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var(STATE_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }

    // Use XDG state directory
    let state_home = if cfg!(target_os = "macos") {
        // macOS doesn't have XDG by default, use Application Support
        dirs::data_local_dir().map(|p| p.join("mergers"))
    } else if cfg!(target_os = "windows") {
        // Windows: use Local AppData
        dirs::data_local_dir().map(|p| p.join("mergers").join("state"))
    } else {
        // Linux and others: use XDG state directory
        dirs::state_dir().map(|p| p.join("mergers")).or_else(|| {
            // Fallback to ~/.local/state/mergers
            dirs::home_dir().map(|p| p.join(".local").join("state").join("mergers"))
        })
    };

    state_home.context("Could not determine state directory")
}

/// Computes a hash of the repository path for unique file naming.
///
/// Returns the first 16 characters of the SHA-256 hash of the
/// canonicalized path.
pub fn compute_repo_hash(repo_path: &Path) -> Result<String> {
    let canonical = repo_path
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize path: {}", repo_path.display()))?;

    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let result = hasher.finalize();

    // Take first 16 characters of hex encoding
    Ok(hex::encode(&result[..8]))
}

/// Returns the state file path for a repository.
pub fn path_for_repo(repo_path: &Path) -> Result<PathBuf> {
    let hash = compute_repo_hash(repo_path)?;
    let dir = state_dir()?;
    Ok(dir.join(format!("merge-{}.json", hash)))
}

/// Returns the lock file path for a repository.
pub fn lock_path_for_repo(repo_path: &Path) -> Result<PathBuf> {
    let hash = compute_repo_hash(repo_path)?;
    let dir = state_dir()?;
    Ok(dir.join(format!("merge-{}.lock", hash)))
}

/// A lock guard that holds a lock on a merge operation.
///
/// The lock is automatically released when the guard is dropped.
/// Uses a simple PID-based locking mechanism with stale lock detection.
#[derive(Debug)]
pub struct LockGuard {
    path: PathBuf,
}

impl LockGuard {
    /// Checks if a lock exists for the given repository without acquiring it.
    ///
    /// Returns `true` if another process holds the lock, `false` otherwise.
    /// This is useful for early detection before loading state files.
    pub fn is_locked(repo_path: &Path) -> Result<bool> {
        let lock_path = lock_path_for_repo(repo_path)?;

        if !lock_path.exists() {
            return Ok(false);
        }

        // Check if the process holding the lock is still alive
        if let Ok(content) = fs::read_to_string(&lock_path)
            && let Ok(pid) = content.trim().parse::<u32>()
            && is_process_alive(pid)
        {
            return Ok(true);
        }

        // Lock file exists but is stale
        Ok(false)
    }

    /// Attempts to acquire a lock for the given repository.
    ///
    /// Returns `Ok(Some(guard))` if the lock was acquired,
    /// `Ok(None)` if another process holds the lock,
    /// or `Err` if an error occurred.
    pub fn acquire(repo_path: &Path) -> Result<Option<Self>> {
        let lock_path = lock_path_for_repo(repo_path)?;

        // Ensure parent directory exists
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create lock directory: {}", parent.display())
            })?;
        }

        // Check if lock file exists and if the process is still alive
        if lock_path.exists() {
            if let Ok(content) = fs::read_to_string(&lock_path)
                && let Ok(pid) = content.trim().parse::<u32>()
                && is_process_alive(pid)
            {
                // Another process holds the lock
                return Ok(None);
            }
            // Lock is stale or unreadable, remove it
            let _ = fs::remove_file(&lock_path);
        }

        // Try to create the lock file
        let pid = std::process::id();
        fs::write(&lock_path, pid.to_string())
            .with_context(|| format!("Failed to create lock file: {}", lock_path.display()))?;

        // Verify we own the lock (handle race condition)
        if let Ok(content) = fs::read_to_string(&lock_path)
            && content.trim() == pid.to_string()
        {
            return Ok(Some(LockGuard { path: lock_path }));
        }

        // Someone else won the race
        Ok(None)
    }

    /// Returns the path to the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Releases the lock (called automatically on drop).
    fn release(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Checks if a process with the given PID is still alive.
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    // On Unix, send signal 0 to check if process exists
    // SAFETY: signal 0 only checks process existence, no signal is actually delivered; pid cast is safe for valid PIDs
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // SAFETY: OpenProcess returns null on failure (process doesn't exist); handle is closed immediately after check
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == ptr::null_mut() {
            false
        } else {
            CloseHandle(handle);
            true
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn is_process_alive(_pid: u32) -> bool {
    // Conservative: assume process is alive on unknown platforms
    true
}

// Provide hex encoding since we don't want to add another dependency
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    /// # State File Serialization
    ///
    /// Verifies that MergeStateFile serializes to JSON correctly.
    ///
    /// ## Test Scenario
    /// - Creates a MergeStateFile with all fields populated
    /// - Serializes to JSON
    ///
    /// ## Expected Outcome
    /// - JSON is valid and contains expected fields
    #[test]
    fn test_state_file_serialization() {
        let state = MergeStateFile::new(
            PathBuf::from("/test/repo"),
            Some(PathBuf::from("/test/base")),
            true,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            "v1.0.0".to_string(),
            "Next Merged".to_string(),
            "merged-".to_string(),
            false,
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"organization\": \"org\""));
        assert!(json.contains("\"merge_version\": \"v1.0.0\""));
        assert!(json.contains("\"run_hooks\": false"));
    }

    /// # State File Deserialization
    ///
    /// Verifies that MergeStateFile deserializes from JSON correctly.
    ///
    /// ## Test Scenario
    /// - Creates JSON representing a state file
    /// - Deserializes to MergeStateFile
    ///
    /// ## Expected Outcome
    /// - All fields are correctly populated
    #[test]
    fn test_state_file_deserialization() {
        let json = r#"{
            "schema_version": 1,
            "created_at": "2024-01-15T10:00:00Z",
            "updated_at": "2024-01-15T10:30:00Z",
            "repo_path": "/test/repo",
            "is_worktree": true,
            "organization": "org",
            "project": "project",
            "repository": "repo",
            "dev_branch": "dev",
            "target_branch": "next",
            "merge_version": "v1.0.0",
            "cherry_pick_items": [],
            "current_index": 0,
            "phase": "cherry_picking",
            "work_item_state": "Next Merged",
            "tag_prefix": "merged-",
            "run_hooks": true
        }"#;

        let state: MergeStateFile = serde_json::from_str(json).unwrap();
        assert_eq!(state.schema_version, 1);
        assert_eq!(state.organization, "org");
        assert_eq!(state.phase, MergePhase::CherryPicking);
        assert!(state.run_hooks);
    }

    /// # State File Round Trip
    ///
    /// Verifies that serialize → deserialize produces identical struct.
    ///
    /// ## Test Scenario
    /// - Creates a MergeStateFile
    /// - Serializes and deserializes
    ///
    /// ## Expected Outcome
    /// - All fields match after round trip
    #[test]
    fn test_state_file_round_trip() {
        let mut state = MergeStateFile::new(
            PathBuf::from("/test/repo"),
            None,
            false,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            "v1.0.0".to_string(),
            "Done".to_string(),
            "merged-".to_string(),
            true,
        );

        state.cherry_pick_items.push(StateCherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 42,
            pr_title: "Test PR".to_string(),
            status: StateItemStatus::Success,
            work_item_ids: vec![1, 2, 3],
        });
        state.phase = MergePhase::ReadyForCompletion;

        let json = serde_json::to_string(&state).unwrap();
        let restored: MergeStateFile = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.organization, state.organization);
        assert_eq!(restored.phase, state.phase);
        assert_eq!(restored.run_hooks, state.run_hooks);
        assert_eq!(restored.cherry_pick_items.len(), 1);
        assert_eq!(restored.cherry_pick_items[0].pr_id, 42);
    }

    /// # Path Hashing Consistency
    ///
    /// Verifies that the same path always produces the same hash.
    ///
    /// ## Test Scenario
    /// - Creates a temporary directory
    /// - Computes hash multiple times
    ///
    /// ## Expected Outcome
    /// - All hash computations return the same value
    #[test]
    fn test_path_hashing_consistent() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        let hash1 = compute_repo_hash(path).unwrap();
        let hash2 = compute_repo_hash(path).unwrap();
        let hash3 = compute_repo_hash(path).unwrap();

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
        assert_eq!(hash1.len(), 16); // 8 bytes = 16 hex chars
    }

    /// # Path Hashing Different Paths
    ///
    /// Verifies that different paths produce different hashes.
    ///
    /// ## Test Scenario
    /// - Creates two different temporary directories
    /// - Computes hash for each
    ///
    /// ## Expected Outcome
    /// - Hashes are different
    #[test]
    fn test_path_hashing_different() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let hash1 = compute_repo_hash(temp_dir1.path()).unwrap();
        let hash2 = compute_repo_hash(temp_dir2.path()).unwrap();

        assert_ne!(hash1, hash2);
    }

    /// # State Directory Default
    ///
    /// Verifies that the default state directory is valid.
    ///
    /// ## Test Scenario
    /// - Calls state_dir() without env override
    ///
    /// ## Expected Outcome
    /// - Returns a valid path
    #[test]
    #[serial]
    fn test_state_dir_default() {
        // Temporarily unset the env var to test default behavior
        let old_val = std::env::var(STATE_DIR_ENV).ok();
        // SAFETY: Tests are run single-threaded, so env var mutation is safe
        unsafe { std::env::remove_var(STATE_DIR_ENV) };

        let result = state_dir();
        assert!(result.is_ok());
        let dir = result.unwrap();
        assert!(dir.to_string_lossy().contains("mergers"));

        // Restore env var if it was set
        if let Some(val) = old_val {
            // SAFETY: Tests are run single-threaded
            unsafe { std::env::set_var(STATE_DIR_ENV, val) };
        }
    }

    /// # State Directory Environment Override
    ///
    /// Verifies that MERGERS_STATE_DIR overrides the default.
    ///
    /// ## Test Scenario
    /// - Sets MERGERS_STATE_DIR to a custom path
    /// - Calls state_dir()
    ///
    /// ## Expected Outcome
    /// - Returns the custom path
    #[test]
    #[serial]
    fn test_state_dir_env_override() {
        let temp_dir = TempDir::new().unwrap();
        let custom_path = temp_dir.path().to_str().unwrap();

        let old_val = std::env::var(STATE_DIR_ENV).ok();
        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, custom_path) };

        let result = state_dir();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from(custom_path));

        // Restore env var
        // SAFETY: Tests are run single-threaded
        unsafe {
            match old_val {
                Some(val) => std::env::set_var(STATE_DIR_ENV, val),
                None => std::env::remove_var(STATE_DIR_ENV),
            }
        }
    }

    /// # Merge Phase Serialization
    ///
    /// Verifies that all MergePhase variants serialize correctly.
    ///
    /// ## Test Scenario
    /// - Serializes each phase variant
    ///
    /// ## Expected Outcome
    /// - All phases serialize to snake_case strings
    #[test]
    fn test_phase_serialization() {
        let phases = vec![
            (MergePhase::Loading, "\"loading\""),
            (MergePhase::Setup, "\"setup\""),
            (MergePhase::CherryPicking, "\"cherry_picking\""),
            (
                MergePhase::AwaitingConflictResolution,
                "\"awaiting_conflict_resolution\"",
            ),
            (MergePhase::ReadyForCompletion, "\"ready_for_completion\""),
            (MergePhase::Completing, "\"completing\""),
            (MergePhase::Completed, "\"completed\""),
            (MergePhase::Aborted, "\"aborted\""),
        ];

        for (phase, expected) in phases {
            let json = serde_json::to_string(&phase).unwrap();
            assert_eq!(json, expected, "Phase {:?} serialized incorrectly", phase);
        }
    }

    /// # Merge Status Serialization
    ///
    /// Verifies that all MergeStatus variants serialize correctly.
    ///
    /// ## Test Scenario
    /// - Serializes each status variant
    ///
    /// ## Expected Outcome
    /// - All statuses serialize to snake_case strings
    #[test]
    fn test_status_serialization() {
        let statuses = vec![
            (MergeStatus::Success, "\"success\""),
            (MergeStatus::PartialSuccess, "\"partial_success\""),
            (MergeStatus::Aborted, "\"aborted\""),
            (MergeStatus::Failed, "\"failed\""),
        ];

        for (status, expected) in statuses {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected, "Status {:?} serialized incorrectly", status);
        }
    }

    /// # Item Status Serialization
    ///
    /// Verifies that all StateItemStatus variants serialize correctly.
    ///
    /// ## Test Scenario
    /// - Serializes each item status variant
    ///
    /// ## Expected Outcome
    /// - All statuses serialize correctly including Failed with message
    #[test]
    fn test_item_status_serialization() {
        let pending = serde_json::to_string(&StateItemStatus::Pending).unwrap();
        assert_eq!(pending, "\"pending\"");

        let success = serde_json::to_string(&StateItemStatus::Success).unwrap();
        assert_eq!(success, "\"success\"");

        let conflict = serde_json::to_string(&StateItemStatus::Conflict).unwrap();
        assert_eq!(conflict, "\"conflict\"");

        let skipped = serde_json::to_string(&StateItemStatus::Skipped).unwrap();
        assert_eq!(skipped, "\"skipped\"");

        let failed = serde_json::to_string(&StateItemStatus::Failed {
            message: "test error".to_string(),
        })
        .unwrap();
        assert!(failed.contains("\"failed\""));
        assert!(failed.contains("test error"));
    }

    /// # State File Save and Load
    ///
    /// Verifies that state files can be saved and loaded.
    ///
    /// ## Test Scenario
    /// - Creates a state file
    /// - Saves to a temporary directory
    /// - Loads and verifies contents
    ///
    /// ## Expected Outcome
    /// - Loaded state matches saved state
    #[test]
    fn test_state_file_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let state_path = temp_dir.path().join("test-state.json");

        let mut state = MergeStateFile::new(
            PathBuf::from("/test/repo"),
            None,
            false,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            "v1.0.0".to_string(),
            "Done".to_string(),
            "merged-".to_string(),
            true,
        );

        state.save(&state_path).unwrap();
        let loaded = MergeStateFile::load(&state_path).unwrap();

        assert_eq!(loaded.organization, "org");
        assert_eq!(loaded.merge_version, "v1.0.0");
        assert!(loaded.run_hooks);
    }

    /// # Lock Acquisition and Release
    ///
    /// Verifies that locks can be acquired and are released on drop.
    ///
    /// ## Test Scenario
    /// - Acquires a lock
    /// - Verifies lock file exists
    /// - Drops the lock
    /// - Verifies lock file is removed
    ///
    /// ## Expected Outcome
    /// - Lock file created on acquire, removed on drop
    #[test]
    #[serial]
    fn test_lock_acquisition_and_release() {
        let temp_dir = TempDir::new().unwrap();
        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_dir.path()) };

        let repo_path = temp_dir.path().join("repo");
        fs::create_dir(&repo_path).unwrap();

        let lock_path = lock_path_for_repo(&repo_path).unwrap();

        // Acquire lock
        {
            let guard = LockGuard::acquire(&repo_path).unwrap();
            assert!(guard.is_some());
            assert!(lock_path.exists());
        }

        // Lock should be released after guard is dropped
        assert!(!lock_path.exists());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Second Lock Acquisition Blocked
    ///
    /// Verifies that a second lock acquisition fails when lock is held.
    ///
    /// ## Test Scenario
    /// - Acquires a lock
    /// - Attempts to acquire a second lock
    ///
    /// ## Expected Outcome
    /// - Second acquisition returns None
    #[test]
    #[serial]
    fn test_second_lock_acquisition_blocked() {
        let temp_dir = TempDir::new().unwrap();
        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_dir.path()) };

        let repo_path = temp_dir.path().join("repo");
        fs::create_dir(&repo_path).unwrap();

        // Acquire first lock
        let guard1 = LockGuard::acquire(&repo_path).unwrap();
        assert!(guard1.is_some());

        // Second acquisition should fail
        let guard2 = LockGuard::acquire(&repo_path).unwrap();
        assert!(guard2.is_none());

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Run Hooks Defaults False
    ///
    /// Verifies that run_hooks defaults to false when missing from JSON.
    ///
    /// ## Test Scenario
    /// - Deserializes JSON without run_hooks field
    ///
    /// ## Expected Outcome
    /// - run_hooks defaults to false
    #[test]
    fn test_run_hooks_defaults_false() {
        let json = r#"{
            "schema_version": 1,
            "created_at": "2024-01-15T10:00:00Z",
            "updated_at": "2024-01-15T10:30:00Z",
            "repo_path": "/test/repo",
            "is_worktree": true,
            "organization": "org",
            "project": "project",
            "repository": "repo",
            "dev_branch": "dev",
            "target_branch": "next",
            "merge_version": "v1.0.0",
            "cherry_pick_items": [],
            "current_index": 0,
            "phase": "loading",
            "work_item_state": "Done",
            "tag_prefix": "merged-"
        }"#;

        let state: MergeStateFile = serde_json::from_str(json).unwrap();
        assert!(!state.run_hooks);
    }

    /// # Status Counts
    ///
    /// Verifies that status counts are calculated correctly.
    ///
    /// ## Test Scenario
    /// - Creates a state file with mixed item statuses
    /// - Calculates status counts
    ///
    /// ## Expected Outcome
    /// - Counts match the number of items in each status
    #[test]
    fn test_status_counts() {
        let mut state = MergeStateFile::new(
            PathBuf::from("/test/repo"),
            None,
            false,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            "v1.0.0".to_string(),
            "Done".to_string(),
            "merged-".to_string(),
            false,
        );

        state.cherry_pick_items = vec![
            StateCherryPickItem {
                commit_id: "a".to_string(),
                pr_id: 1,
                pr_title: "PR 1".to_string(),
                status: StateItemStatus::Pending,
                work_item_ids: vec![],
            },
            StateCherryPickItem {
                commit_id: "b".to_string(),
                pr_id: 2,
                pr_title: "PR 2".to_string(),
                status: StateItemStatus::Success,
                work_item_ids: vec![],
            },
            StateCherryPickItem {
                commit_id: "c".to_string(),
                pr_id: 3,
                pr_title: "PR 3".to_string(),
                status: StateItemStatus::Success,
                work_item_ids: vec![],
            },
            StateCherryPickItem {
                commit_id: "d".to_string(),
                pr_id: 4,
                pr_title: "PR 4".to_string(),
                status: StateItemStatus::Skipped,
                work_item_ids: vec![],
            },
            StateCherryPickItem {
                commit_id: "e".to_string(),
                pr_id: 5,
                pr_title: "PR 5".to_string(),
                status: StateItemStatus::Failed {
                    message: "error".to_string(),
                },
                work_item_ids: vec![],
            },
        ];

        let counts = state.status_counts();
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.success, 2);
        assert_eq!(counts.conflict, 0);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.total(), 5);
        assert_eq!(counts.completed(), 4);
    }

    /// # Phase Terminal Check
    ///
    /// Verifies that is_terminal() correctly identifies terminal phases.
    ///
    /// ## Test Scenario
    /// - Checks is_terminal() for all phases
    ///
    /// ## Expected Outcome
    /// - Only Completed and Aborted return true
    #[test]
    fn test_phase_terminal_check() {
        assert!(!MergePhase::Loading.is_terminal());
        assert!(!MergePhase::Setup.is_terminal());
        assert!(!MergePhase::CherryPicking.is_terminal());
        assert!(!MergePhase::AwaitingConflictResolution.is_terminal());
        assert!(!MergePhase::ReadyForCompletion.is_terminal());
        assert!(!MergePhase::Completing.is_terminal());
        assert!(MergePhase::Completed.is_terminal());
        assert!(MergePhase::Aborted.is_terminal());
    }

    /// # Schema Version Constant
    ///
    /// Verifies schema version is set correctly in new state files.
    ///
    /// ## Test Scenario
    /// - Creates a new state file
    /// - Checks schema_version field
    ///
    /// ## Expected Outcome
    /// - Schema version matches SCHEMA_VERSION constant
    #[test]
    fn test_schema_version() {
        let state = MergeStateFile::new(
            PathBuf::from("/test/repo"),
            None,
            false,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            "v1.0.0".to_string(),
            "Done".to_string(),
            "merged-".to_string(),
            false,
        );
        assert_eq!(state.schema_version, SCHEMA_VERSION);
        assert_eq!(state.schema_version, 1);
    }

    /// # Path For Repo Generation
    ///
    /// Verifies state file path is generated correctly for a repo.
    ///
    /// ## Test Scenario
    /// - Creates temp dir and sets as state dir
    /// - Gets path for a repo
    ///
    /// ## Expected Outcome
    /// - Path is in state dir with merge- prefix and .json suffix
    #[test]
    #[serial]
    fn test_path_for_repo() {
        let temp_dir = TempDir::new().unwrap();
        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_dir.path()) };

        let repo_path = temp_dir.path().join("my-repo");
        fs::create_dir(&repo_path).unwrap();

        let state_path = path_for_repo(&repo_path).unwrap();

        assert!(state_path.starts_with(temp_dir.path()));
        assert!(
            state_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("merge-")
        );
        assert!(
            state_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(".json")
        );

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Lock Path For Repo Generation
    ///
    /// Verifies lock file path is generated correctly for a repo.
    ///
    /// ## Test Scenario
    /// - Creates temp dir and sets as state dir
    /// - Gets lock path for a repo
    ///
    /// ## Expected Outcome
    /// - Lock path is in state dir with merge- prefix and .lock suffix
    #[test]
    #[serial]
    fn test_lock_path_for_repo() {
        let temp_dir = TempDir::new().unwrap();
        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_dir.path()) };

        let repo_path = temp_dir.path().join("my-repo");
        fs::create_dir(&repo_path).unwrap();

        let lock_path = lock_path_for_repo(&repo_path).unwrap();

        assert!(lock_path.starts_with(temp_dir.path()));
        assert!(
            lock_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("merge-")
        );
        assert!(
            lock_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(".lock")
        );

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Run Hooks Serialization
    ///
    /// Verifies that run_hooks field serializes correctly.
    ///
    /// ## Test Scenario
    /// - Creates state with run_hooks=true
    /// - Serializes to JSON
    ///
    /// ## Expected Outcome
    /// - JSON contains "run_hooks": true
    #[test]
    fn test_run_hooks_serialization() {
        let state = MergeStateFile::new(
            PathBuf::from("/test/repo"),
            None,
            false,
            "org".to_string(),
            "project".to_string(),
            "repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            "v1.0.0".to_string(),
            "Done".to_string(),
            "merged-".to_string(),
            true, // run_hooks = true
        );

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"run_hooks\":true"));

        // Verify it deserializes correctly
        let deserialized: MergeStateFile = serde_json::from_str(&json).unwrap();
        assert!(deserialized.run_hooks);
    }

    /// # Lock Content Is PID
    ///
    /// Verifies that lock file contains the current process PID.
    ///
    /// ## Test Scenario
    /// - Acquires a lock
    /// - Reads lock file content
    ///
    /// ## Expected Outcome
    /// - Lock file contains current PID as text
    #[test]
    #[serial]
    fn test_lock_content_is_pid() {
        let temp_dir = TempDir::new().unwrap();
        // SAFETY: Tests are run single-threaded
        unsafe { std::env::set_var(STATE_DIR_ENV, temp_dir.path()) };

        let repo_path = temp_dir.path().join("repo");
        fs::create_dir(&repo_path).unwrap();

        let lock_path = lock_path_for_repo(&repo_path).unwrap();
        let expected_pid = std::process::id().to_string();

        // Acquire lock
        let _guard = LockGuard::acquire(&repo_path).unwrap();

        // Read lock file content
        let content = fs::read_to_string(&lock_path).unwrap();
        assert_eq!(content.trim(), expected_pid);

        // SAFETY: Tests are run single-threaded
        unsafe { std::env::remove_var(STATE_DIR_ENV) };
    }

    /// # Builder Pattern Basic Usage
    ///
    /// Verifies the builder pattern creates a valid state file.
    ///
    /// ## Test Scenario
    /// - Uses builder to create a state file
    /// - Verifies all fields are set correctly
    ///
    /// ## Expected Outcome
    /// - State file has correct values
    #[test]
    fn test_builder_basic_usage() {
        let state = MergeStateFile::builder()
            .repo_path("/test/repo")
            .organization("my-org")
            .project("my-project")
            .repository("my-repo")
            .dev_branch("dev")
            .target_branch("main")
            .merge_version("v1.0.0")
            .work_item_state("Done")
            .tag_prefix("merged-")
            .build();

        assert_eq!(state.repo_path, PathBuf::from("/test/repo"));
        assert_eq!(state.organization, "my-org");
        assert_eq!(state.project, "my-project");
        assert_eq!(state.repository, "my-repo");
        assert_eq!(state.dev_branch, "dev");
        assert_eq!(state.target_branch, "main");
        assert_eq!(state.merge_version, "v1.0.0");
        assert_eq!(state.work_item_state, "Done");
        assert_eq!(state.tag_prefix, "merged-");
        assert!(!state.run_hooks);
        assert!(!state.is_worktree);
        assert!(state.base_repo_path.is_none());
    }

    /// # Builder With Optional Fields
    ///
    /// Verifies the builder correctly sets optional fields.
    ///
    /// ## Test Scenario
    /// - Uses builder with optional fields set
    ///
    /// ## Expected Outcome
    /// - Optional fields are correctly set
    #[test]
    fn test_builder_with_optional_fields() {
        let state = MergeStateFile::builder()
            .repo_path("/work/repo")
            .base_repo_path("/base/repo")
            .is_worktree(true)
            .organization("org")
            .project("proj")
            .repository("repo")
            .dev_branch("develop")
            .target_branch("release")
            .merge_version("v2.0.0")
            .work_item_state("Released")
            .tag_prefix("release-")
            .run_hooks(true)
            .build();

        assert_eq!(state.base_repo_path, Some(PathBuf::from("/base/repo")));
        assert!(state.is_worktree);
        assert!(state.run_hooks);
    }

    /// # Builder Try Build Success
    ///
    /// Verifies try_build returns Ok when all required fields are set.
    ///
    /// ## Test Scenario
    /// - Uses try_build with all required fields
    ///
    /// ## Expected Outcome
    /// - Returns Ok with valid state file
    #[test]
    fn test_builder_try_build_success() {
        let result = MergeStateFile::builder()
            .repo_path("/test/repo")
            .organization("org")
            .project("proj")
            .repository("repo")
            .dev_branch("dev")
            .target_branch("main")
            .merge_version("v1.0.0")
            .work_item_state("Done")
            .tag_prefix("merged-")
            .try_build();

        assert!(result.is_ok());
        let state = result.unwrap();
        assert_eq!(state.organization, "org");
    }

    /// # Builder Try Build Missing Fields
    ///
    /// Verifies try_build returns Err when required fields are missing.
    ///
    /// ## Test Scenario
    /// - Uses try_build with missing required fields
    ///
    /// ## Expected Outcome
    /// - Returns Err with descriptive message
    #[test]
    fn test_builder_try_build_missing_fields() {
        // Missing repo_path
        let result = MergeStateFile::builder()
            .organization("org")
            .project("proj")
            .repository("repo")
            .dev_branch("dev")
            .target_branch("main")
            .merge_version("v1.0.0")
            .work_item_state("Done")
            .tag_prefix("merged-")
            .try_build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("repo_path"));
    }

    /// # Builder Default Values
    ///
    /// Verifies the builder uses correct defaults.
    ///
    /// ## Test Scenario
    /// - Creates builder and checks defaults
    ///
    /// ## Expected Outcome
    /// - Defaults are false for booleans, None for Options
    #[test]
    fn test_builder_default_values() {
        let builder = MergeStateFileBuilder::new();

        // Check that optional fields start as None/false
        assert!(builder.repo_path.is_none());
        assert!(builder.base_repo_path.is_none());
        assert!(!builder.is_worktree);
        assert!(!builder.run_hooks);
    }

    /// # Builder Fluent API Chain
    ///
    /// Verifies the builder methods can be chained.
    ///
    /// ## Test Scenario
    /// - Chains multiple builder methods
    ///
    /// ## Expected Outcome
    /// - Chain compiles and produces correct result
    #[test]
    fn test_builder_fluent_chain() {
        // This test verifies the fluent API compiles correctly
        let _state = MergeStateFileBuilder::new()
            .repo_path(PathBuf::from("/test"))
            .organization(String::from("org"))
            .project("proj".to_string())
            .repository("repo")
            .dev_branch("dev")
            .target_branch("main")
            .merge_version("v1.0.0")
            .work_item_state("Done")
            .tag_prefix("merged-")
            .is_worktree(false)
            .run_hooks(false)
            .build();
    }
}
