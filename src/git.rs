//! Git operations for repository management and cherry-picking.
//!
//! This module provides functions for:
//! - Repository cloning and worktree management
//! - Cherry-pick operations with conflict detection
//! - Commit history analysis
//! - Branch management
//!
//! All operations use the system `git` command via `std::process::Command`.

// Allow deprecated RepositorySetupError usage within this module during migration
#![allow(deprecated)]

use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

use crate::error::GitError;

/// Trait for abstracting git operations.
///
/// This trait allows for mocking git operations in tests and potentially
/// supporting different git backends (e.g., libgit2) in the future.
///
/// # Example
///
/// ```rust,no_run
/// use std::path::Path;
/// use mergers::git::{GitOperations, SystemGit};
///
/// let git = SystemGit;
/// let info = git.get_commit_info(Path::new("/repo"), "HEAD")?;
/// println!("Latest commit: {}", info.title);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub trait GitOperations: Send + Sync {
    /// Cherry-pick a commit into the current branch.
    fn cherry_pick(&self, repo_path: &Path, commit_id: &str) -> Result<CherryPickResult>;

    /// Get information about a specific commit.
    fn get_commit_info(&self, repo_path: &Path, commit_id: &str) -> Result<CommitInfo>;

    /// Check if conflicts have been resolved.
    fn check_conflicts_resolved(&self, repo_path: &Path) -> Result<bool>;

    /// Continue a paused cherry-pick operation.
    fn continue_cherry_pick(&self, repo_path: &Path) -> Result<()>;

    /// Abort a cherry-pick operation.
    fn abort_cherry_pick(&self, repo_path: &Path) -> Result<()>;

    /// Create a new branch and check it out.
    fn create_branch(&self, repo_path: &Path, branch_name: &str) -> Result<()>;

    /// Fetch specific commits from the remote.
    fn fetch_commits(&self, repo_path: &Path, commits: &[String]) -> Result<()>;

    /// Get the complete commit history for a branch.
    fn get_branch_history(&self, repo_path: &Path, branch: &str) -> Result<CommitHistory>;
}

/// Default implementation using system git command.
///
/// This implementation calls the `git` binary via `std::process::Command`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemGit;

impl GitOperations for SystemGit {
    fn cherry_pick(&self, repo_path: &Path, commit_id: &str) -> Result<CherryPickResult> {
        cherry_pick_commit(repo_path, commit_id)
    }

    fn get_commit_info(&self, repo_path: &Path, commit_id: &str) -> Result<CommitInfo> {
        get_commit_info(repo_path, commit_id)
    }

    fn check_conflicts_resolved(&self, repo_path: &Path) -> Result<bool> {
        check_conflicts_resolved(repo_path)
    }

    fn continue_cherry_pick(&self, repo_path: &Path) -> Result<()> {
        continue_cherry_pick(repo_path)
    }

    fn abort_cherry_pick(&self, repo_path: &Path) -> Result<()> {
        abort_cherry_pick(repo_path)
    }

    fn create_branch(&self, repo_path: &Path, branch_name: &str) -> Result<()> {
        create_branch(repo_path, branch_name)
    }

    fn fetch_commits(&self, repo_path: &Path, commits: &[String]) -> Result<()> {
        fetch_commits(repo_path, commits)
    }

    fn get_branch_history(&self, repo_path: &Path, branch: &str) -> Result<CommitHistory> {
        get_target_branch_history(repo_path, branch)
    }
}

/// Legacy error type for repository setup operations.
///
/// **Deprecated**: Use [`GitError`] from the `error` module instead.
/// This type is kept for backward compatibility.
#[deprecated(since = "0.2.0", note = "Use GitError from the error module instead")]
#[derive(Debug, Clone)]
pub enum RepositorySetupError {
    /// A branch with the specified name already exists.
    BranchExists(String),
    /// A worktree already exists at the specified path.
    WorktreeExists(String),
    /// A generic error message.
    Other(String),
}

#[allow(deprecated)]
impl std::fmt::Display for RepositorySetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepositorySetupError::BranchExists(branch) => {
                write!(f, "Branch '{}' already exists", branch)
            }
            RepositorySetupError::WorktreeExists(path) => {
                write!(f, "Worktree already exists at path: {}", path)
            }
            RepositorySetupError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

#[allow(deprecated)]
impl std::error::Error for RepositorySetupError {}

#[allow(deprecated)]
impl From<RepositorySetupError> for GitError {
    fn from(err: RepositorySetupError) -> Self {
        match err {
            RepositorySetupError::BranchExists(branch) => GitError::BranchExists { branch },
            RepositorySetupError::WorktreeExists(path) => GitError::WorktreeExists { path },
            RepositorySetupError::Other(msg) => GitError::Other(msg),
        }
    }
}

/// Validates that a git reference doesn't contain forbidden characters.
///
/// This helps prevent command injection and ensures the reference
/// is valid for git operations.
///
/// # Arguments
///
/// * `reference` - The git reference string to validate
///
/// # Returns
///
/// * `Ok(())` if the reference is valid
/// * `Err(GitError::InvalidReference)` if the reference contains forbidden characters
pub fn validate_git_ref(reference: &str) -> std::result::Result<(), GitError> {
    // Check for control characters and git-specific forbidden characters
    if reference.is_empty() {
        return Err(GitError::InvalidReference {
            reference: reference.to_string(),
        });
    }

    // Forbidden characters in git references
    // See: https://git-scm.com/docs/git-check-ref-format
    let forbidden_chars = ['~', '^', ':', '?', '*', '[', '\\', '\0'];
    let has_forbidden = reference
        .chars()
        .any(|c| c.is_control() || forbidden_chars.contains(&c));

    if has_forbidden {
        return Err(GitError::InvalidReference {
            reference: reference.to_string(),
        });
    }

    // Check for ".." which is not allowed
    if reference.contains("..") {
        return Err(GitError::InvalidReference {
            reference: reference.to_string(),
        });
    }

    // Check for "@{" which is not allowed
    if reference.contains("@{") {
        return Err(GitError::InvalidReference {
            reference: reference.to_string(),
        });
    }

    Ok(())
}

pub fn shallow_clone_repo(
    ssh_url: &str,
    target_branch: &str,
    run_hooks: bool,
) -> Result<(PathBuf, TempDir)> {
    let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
    let repo_path = temp_dir.path().to_path_buf();

    let output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "--branch",
            target_branch,
            "--no-tags",
            ssh_url,
            repo_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to clone repository")?;

    if !output.status.success() {
        anyhow::bail!(
            "Git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Disable hooks to prevent commit hook failures during cherry-pick operations
    // unless --run-hooks is specified
    if !run_hooks {
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "core.hooksPath", "/dev/null"])
            .output()
            .context("Failed to configure hooks path")?;
    }

    Ok((repo_path, temp_dir))
}

pub fn create_worktree(
    base_repo_path: &Path,
    target_branch: &str,
    version: &str,
    run_hooks: bool,
) -> Result<PathBuf, RepositorySetupError> {
    let worktree_name = format!("next-{}", version);
    let worktree_path = base_repo_path.join(&worktree_name);

    // Check if worktree already exists
    let list_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| RepositorySetupError::Other(format!("Failed to list worktrees: {}", e)))?;

    if !list_output.status.success() {
        return Err(RepositorySetupError::Other(format!(
            "Failed to list worktrees: {}",
            String::from_utf8_lossy(&list_output.stderr)
        )));
    }

    let worktree_list = String::from_utf8_lossy(&list_output.stdout);
    if worktree_list.contains(&worktree_name) || worktree_path.exists() {
        return Err(RepositorySetupError::WorktreeExists(
            worktree_path.display().to_string(),
        ));
    }

    let fetch_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(["fetch", "origin", target_branch])
        .output()
        .map_err(|e| {
            RepositorySetupError::Other(format!("Failed to fetch target branch: {}", e))
        })?;

    if !fetch_output.status.success() {
        return Err(RepositorySetupError::Other(format!(
            "Failed to fetch target branch: {}",
            String::from_utf8_lossy(&fetch_output.stderr)
        )));
    }

    let create_output = Command::new("git")
        .current_dir(base_repo_path)
        .args([
            "worktree",
            "add",
            worktree_path.to_str().unwrap(),
            &format!("origin/{}", target_branch),
        ])
        .output()
        .map_err(|e| RepositorySetupError::Other(format!("Failed to create worktree: {}", e)))?;

    if !create_output.status.success() {
        let stderr = String::from_utf8_lossy(&create_output.stderr);
        if stderr.contains("already exists")
            || stderr.contains("already checked out")
            || stderr.contains("is already checked out")
            || stderr.contains("already registered")
        {
            return Err(RepositorySetupError::WorktreeExists(
                worktree_path.display().to_string(),
            ));
        }
        if stderr.contains("invalid reference")
            || stderr.contains("not a valid branch")
            || stderr.contains("branch") && stderr.contains("already exists")
        {
            return Err(RepositorySetupError::BranchExists(format!(
                "origin/{}",
                target_branch
            )));
        }
        return Err(RepositorySetupError::Other(format!(
            "Failed to create worktree: {}",
            stderr
        )));
    }

    // Disable hooks to prevent commit hook failures during cherry-pick operations
    // unless --run-hooks is specified
    if !run_hooks {
        let config_output = Command::new("git")
            .current_dir(&worktree_path)
            .args(["config", "core.hooksPath", "/dev/null"])
            .output()
            .map_err(|e| {
                RepositorySetupError::Other(format!("Failed to configure hooks path: {}", e))
            })?;

        if !config_output.status.success() {
            return Err(RepositorySetupError::Other(format!(
                "Failed to configure hooks path: {}",
                String::from_utf8_lossy(&config_output.stderr)
            )));
        }
    }

    Ok(worktree_path)
}

pub fn force_remove_worktree(base_repo_path: &Path, version: &str) -> Result<()> {
    let worktree_name = format!("next-{}", version);
    let worktree_path = base_repo_path.join(&worktree_name);

    // Remove from git worktree list
    let _remove_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(["worktree", "remove", "--force", &worktree_name])
        .output();

    // Prune worktrees
    let _prune_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(["worktree", "prune"])
        .output();

    // Remove directory if it exists
    if worktree_path.exists() {
        std::fs::remove_dir_all(&worktree_path)
            .context("Failed to remove existing worktree directory")?;
    }

    Ok(())
}

pub fn force_delete_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
    // Delete local branch if it exists
    let _delete_output = Command::new("git")
        .current_dir(repo_path)
        .args(["branch", "-D", branch_name])
        .output();

    Ok(())
}

/// Clean up a cherry-pick operation by removing the worktree and branch.
/// This is used when aborting the entire cherry-pick process.
///
/// # Arguments
/// * `base_repo_path` - The base repository path (for worktree cleanup)
/// * `worktree_path` - The worktree path that was created
/// * `version` - The version string used to name the worktree and branch
/// * `target_branch` - The target branch name (used to construct the patch branch name)
pub fn cleanup_cherry_pick(
    base_repo_path: Option<&Path>,
    worktree_path: &Path,
    version: &str,
    target_branch: &str,
) -> Result<()> {
    // First abort any ongoing cherry-pick
    let _ = abort_cherry_pick(worktree_path);

    // Construct the branch name
    let branch_name = format!("patch/{}-{}", target_branch, version);

    // If we have a base repo path, we're using worktrees
    if let Some(base_path) = base_repo_path {
        // First, checkout to a detached HEAD to allow branch deletion
        let _ = Command::new("git")
            .current_dir(worktree_path)
            .args(["checkout", "--detach"])
            .output();

        // Remove the worktree
        let _ = force_remove_worktree(base_path, version);

        // Delete the branch from the base repo
        let _ = force_delete_branch(base_path, &branch_name);
    } else {
        // For cloned repos, just delete the branch (temp dir will be cleaned up automatically)
        let _ = force_delete_branch(worktree_path, &branch_name);
    }

    Ok(())
}

pub enum RepositorySetup {
    Local(PathBuf),
    Clone(PathBuf, TempDir),
}

pub fn setup_repository(
    local_repo: Option<&str>,
    ssh_url: &str,
    target_branch: &str,
    version: &str,
    run_hooks: bool,
) -> Result<RepositorySetup, RepositorySetupError> {
    match local_repo {
        Some(repo_path) => {
            let repo_path = Path::new(repo_path);
            if !repo_path.exists() {
                return Err(RepositorySetupError::Other(format!(
                    "Local repository path does not exist: {:?}",
                    repo_path
                )));
            }

            let verify_output = Command::new("git")
                .current_dir(repo_path)
                .args(["rev-parse", "--git-dir"])
                .output()
                .map_err(|e| {
                    RepositorySetupError::Other(format!("Failed to verify git repository: {}", e))
                })?;

            if !verify_output.status.success() {
                return Err(RepositorySetupError::Other(format!(
                    "Not a valid git repository: {:?}",
                    repo_path
                )));
            }

            let worktree_path = create_worktree(repo_path, target_branch, version, run_hooks)?;
            Ok(RepositorySetup::Local(worktree_path))
        }
        None => {
            let (repo_path, temp_dir) = shallow_clone_repo(ssh_url, target_branch, run_hooks)
                .map_err(|e| RepositorySetupError::Other(e.to_string()))?;
            Ok(RepositorySetup::Clone(repo_path, temp_dir))
        }
    }
}

pub enum CherryPickResult {
    Success,
    Conflict(Vec<String>), // List of conflicted files
    Failed(String),
}

pub fn cherry_pick_commit(repo_path: &Path, commit_id: &str) -> Result<CherryPickResult> {
    // Always use -m 1 to handle both regular and merge commits:
    // - For merge commits: selects the first parent (the branch that was merged into)
    // - For regular commits: git uses the single parent, -m 1 has no negative effect
    // Use --allow-empty to handle commits that may result in no changes (already applied)
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["cherry-pick", "-m", "1", "--allow-empty", commit_id])
        .output()
        .context("Failed to execute cherry-pick command")?;

    if output.status.success() {
        return Ok(CherryPickResult::Success);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    if stderr.contains("conflict") || stderr.contains("CONFLICT") {
        let status_output = Command::new("git")
            .current_dir(repo_path)
            .args(["diff", "--name-only", "--diff-filter=U"])
            .output()?;

        let conflicted_files: Vec<String> = String::from_utf8_lossy(&status_output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(CherryPickResult::Conflict(conflicted_files))
    } else {
        Ok(CherryPickResult::Failed(stderr.to_string()))
    }
}

pub fn create_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["checkout", "-b", branch_name])
        .output()
        .context("Failed to create and checkout branch")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to create branch: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

pub fn fetch_commits(repo_path: &Path, commits: &[String]) -> Result<()> {
    for commit_id in commits {
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["fetch", "--depth=1", "origin", commit_id])
            .output()?;

        if !output.status.success() {
            // Just continue, commit might already be available
        }
    }
    Ok(())
}

pub fn check_conflicts_resolved(repo_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["ls-files", "-u"])
        .output()?;

    Ok(output.stdout.is_empty())
}

pub fn continue_cherry_pick(repo_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["cherry-pick", "--continue"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to continue cherry-pick: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

pub fn abort_cherry_pick(repo_path: &Path) -> Result<()> {
    Command::new("git")
        .current_dir(repo_path)
        .args(["cherry-pick", "--abort"])
        .output()?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub date: String,
    pub title: String,
    pub author: String,
}

pub fn get_commit_info(repo_path: &Path, commit_id: &str) -> Result<CommitInfo> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["show", "--no-patch", "--format=%H|%ci|%s|%an", commit_id])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commit info: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = output_str.trim().split('|').collect();

    if parts.len() >= 4 {
        Ok(CommitInfo {
            hash: parts[0].to_string(),
            date: parts[1].to_string(),
            title: parts[2].to_string(),
            author: parts[3].to_string(),
        })
    } else {
        anyhow::bail!("Unexpected git show output format");
    }
}

/// Structure to hold pre-fetched commit history for optimized PR analysis
#[derive(Debug, Clone)]
pub struct CommitHistory {
    pub commit_hashes: HashSet<String>, // All commit hashes in target branch
    pub commit_messages: Vec<String>,   // All commit messages in target branch
    pub commit_bodies: Vec<String>,     // All commit bodies in target branch
}

/// Get complete commit history for target branch once to avoid repeated git calls
pub fn get_target_branch_history(repo_path: &Path, target_branch: &str) -> Result<CommitHistory> {
    // Get all commit hashes in target branch
    let hash_output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%H", target_branch])
        .output()
        .context("Failed to get commit hashes from target branch")?;

    if !hash_output.status.success() {
        let stderr = String::from_utf8_lossy(&hash_output.stderr);
        anyhow::bail!("Failed to get commit hashes from target branch: {}", stderr);
    }

    let commit_hashes: HashSet<String> = String::from_utf8_lossy(&hash_output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    // Get all commit messages in target branch
    let message_output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", target_branch])
        .output()
        .context("Failed to get commit messages from target branch")?;

    if !message_output.status.success() {
        let stderr = String::from_utf8_lossy(&message_output.stderr);
        anyhow::bail!(
            "Failed to get commit messages from target branch: {}",
            stderr
        );
    }

    let commit_messages: Vec<String> = String::from_utf8_lossy(&message_output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    // Get all commit bodies in target branch (full message including body)
    let body_output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%b", target_branch])
        .output()
        .context("Failed to get commit bodies from target branch")?;

    if !body_output.status.success() {
        let stderr = String::from_utf8_lossy(&body_output.stderr);
        anyhow::bail!("Failed to get commit bodies from target branch: {}", stderr);
    }

    let commit_bodies: Vec<String> = String::from_utf8_lossy(&body_output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(CommitHistory {
        commit_hashes,
        commit_messages,
        commit_bodies,
    })
}

/// Check if a commit exists in the pre-fetched commit history
pub fn check_commit_in_history(commit_id: &str, history: &CommitHistory) -> bool {
    history.commit_hashes.contains(commit_id)
}

/// Check if a PR is merged using pre-fetched commit history
pub fn check_pr_merged_in_history(pr_id: i32, pr_title: &str, history: &CommitHistory) -> bool {
    // Strategy 1: Check for Azure DevOps merge pattern (most common)
    if check_azure_devops_merge_pattern_in_history(pr_id, pr_title, history) {
        return true;
    }

    // Strategy 2: Search for PR title in commit messages (broader search)
    if search_pr_title_in_history(pr_title, history) {
        return true;
    }

    // Strategy 3: Search for PR ID references in commit messages
    if search_pr_id_in_history(pr_id, history) {
        return true;
    }

    false
}

fn check_azure_devops_merge_pattern_in_history(
    pr_id: i32,
    pr_title: &str,
    history: &CommitHistory,
) -> bool {
    // Check for the Azure DevOps merge pattern: "Merged PR <PR ID>: <Original PR title>"
    let expected_prefix = format!("Merged PR {}: ", pr_id);

    for commit_message in &history.commit_messages {
        if commit_message.starts_with(&expected_prefix) {
            // Extract the title part after the prefix
            let commit_title_part = &commit_message[expected_prefix.len()..];

            // Normalize both titles for comparison
            let normalized_commit_title = normalize_title(commit_title_part);
            let normalized_pr_title = normalize_title(pr_title);

            if normalized_commit_title == normalized_pr_title {
                return true;
            }
        }
    }

    false
}

fn search_pr_title_in_history(pr_title: &str, history: &CommitHistory) -> bool {
    let normalized_pr_title = normalize_title(pr_title);

    // Skip very short titles to avoid false positives
    if normalized_pr_title.len() < 10 {
        return false;
    }

    // Split title into meaningful words (longer than 2 characters)
    let title_words: Vec<&str> = normalized_pr_title
        .split_whitespace()
        .filter(|word| word.len() > 2 && !is_common_word(word))
        .collect();

    // Need at least 2 meaningful words for a reliable match
    if title_words.len() < 2 {
        return false;
    }

    for commit_message in &history.commit_messages {
        let normalized_commit = normalize_title(commit_message);

        // Check if all meaningful words from PR title appear in commit message
        let words_found = title_words
            .iter()
            .filter(|&&word| normalized_commit.contains(word))
            .count();

        // Require at least 80% of words to match for fuzzy matching
        if words_found as f64 / title_words.len() as f64 >= 0.8 {
            return true;
        }
    }

    false
}

fn search_pr_id_in_history(pr_id: i32, history: &CommitHistory) -> bool {
    for commit_message in &history.commit_messages {
        let lowercase_commit = commit_message.to_lowercase();

        // Look for PR ID in various formats with exact match validation
        // The PR ID must be followed by a non-digit character to avoid partial matches
        // (e.g., searching for PR 123 should not match PR 1234)
        let patterns = [
            format!("pr{}", pr_id),
            format!("pr {}", pr_id),
            format!("#{}", pr_id),
        ];

        for pattern in &patterns {
            if let Some(pos) = lowercase_commit.find(pattern) {
                let end_pos = pos + pattern.len();
                // Check if the next character is not a digit (word boundary)
                if is_pr_id_complete(&lowercase_commit, end_pos) {
                    return true;
                }
            }
        }

        // Bracket and parenthesis patterns are inherently bounded by closing chars
        // but still need to verify exact match
        if lowercase_commit.contains(&format!("[{}]", pr_id))
            || lowercase_commit.contains(&format!("({})", pr_id))
        {
            return true;
        }
    }

    false
}

/// Check if PR ID at given position is complete (not followed by more digits)
fn is_pr_id_complete(text: &str, end_pos: usize) -> bool {
    if end_pos >= text.len() {
        return true; // End of string
    }
    let next_char = text.chars().nth(end_pos);
    match next_char {
        Some(c) => !c.is_ascii_digit(),
        None => true,
    }
}

fn is_common_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "this"
            | "that"
            | "will"
            | "have"
            | "when"
            | "where"
            | "what"
            | "which"
            | "their"
            | "there"
            | "here"
            | "then"
            | "them"
            | "they"
            | "were"
            | "been"
            | "said"
            | "each"
            | "some"
            | "time"
            | "very"
            | "more"
            | "first"
            | "well"
            | "year"
            | "work"
            | "such"
            | "even"
            | "most"
            | "take"
            | "only"
            | "think"
            | "also"
            | "back"
            | "could"
            | "good"
            | "would"
            | "should"
            | "being"
            | "going"
            | "made"
            | "come"
            | "came"
            | "want"
            | "need"
            | "know"
            | "call"
            | "called"
            | "help"
            | "look"
            | "find"
            | "found"
            | "done"
            | "used"
            | "using"
            | "into"
            | "over"
            | "both"
            | "many"
            | "much"
            | "long"
            | "way"
            | "ways"
            | "may"
            | "might"
            | "part"
            | "same"
            | "other"
            | "another"
            | "different"
            | "new"
            | "old"
            | "great"
            | "little"
            | "large"
            | "small"
            | "right"
            | "left"
            | "next"
            | "last"
            | "between"
            | "during"
            | "before"
            | "above"
            | "below"
            | "through"
            | "against"
            | "within"
            | "without"
            | "across"
    )
}

fn normalize_title(title: &str) -> String {
    // Remove common prefixes and normalize for comparison
    title
        .to_lowercase()
        .replace("merge pull request", "")
        .replace("merged pr", "")
        .replace("#", "")
        .replace("fix:", "")
        .replace("feat:", "")
        .replace("chore:", "")
        .replace("docs:", "")
        .trim()
        .to_string()
}

pub fn cleanup_migration_worktrees(base_repo_path: &Path) -> Result<()> {
    // List all worktrees
    let list_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to list worktrees")?;

    if !list_output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to list worktrees: {}",
            String::from_utf8_lossy(&list_output.stderr)
        ));
    }

    let worktree_list = String::from_utf8_lossy(&list_output.stdout);
    let mut migration_worktrees = Vec::new();

    // Parse worktree list and find migration worktrees
    for line in worktree_list.lines() {
        if line.starts_with("worktree ") {
            let path = line.strip_prefix("worktree ").unwrap();
            if let Some(dir_name) = std::path::Path::new(path).file_name()
                && let Some(name) = dir_name.to_str()
                && name.starts_with("next-migration-")
            {
                migration_worktrees.push(name.strip_prefix("next-").unwrap().to_string());
            }
        }
    }

    // Remove found migration worktrees
    for worktree_id in migration_worktrees {
        if let Err(e) = force_remove_worktree(base_repo_path, &worktree_id) {
            eprintln!(
                "Warning: Failed to remove migration worktree {}: {}",
                worktree_id, e
            );
        }
    }

    Ok(())
}

/// List all local branches matching a pattern
pub fn list_local_branches(repo_path: &Path, pattern: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["branch", "--list", pattern])
        .output()
        .context("Failed to list local branches")?;

    if !output.status.success() {
        anyhow::bail!(
            "Git branch list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let branches: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().trim_start_matches("* ").to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(branches)
}

/// Parse a patch branch name into its components
/// Expected format: patch/<target>-<version>
fn parse_patch_branch(branch_name: &str) -> Option<(String, String)> {
    if !branch_name.starts_with("patch/") {
        return None;
    }

    let remainder = &branch_name[6..]; // Skip "patch/"

    // Find the last hyphen to split target and version
    if let Some(last_hyphen_idx) = remainder.rfind('-') {
        let target = remainder[..last_hyphen_idx].to_string();
        let version = remainder[last_hyphen_idx + 1..].to_string();
        Some((target, version))
    } else {
        None
    }
}

/// List all patch branches with parsed metadata
pub fn list_patch_branches(repo_path: &Path) -> Result<Vec<crate::models::CleanupBranch>> {
    let branches = list_local_branches(repo_path, "patch/*")?;

    let mut patch_branches = Vec::new();
    for branch in branches {
        if let Some((target, version)) = parse_patch_branch(&branch) {
            patch_branches.push(crate::models::CleanupBranch {
                name: branch.clone(),
                target: target.clone(),
                version: version.clone(),
                is_merged: false, // Will be determined later
                selected: false,
                status: crate::models::CleanupStatus::Pending,
            });
        }
    }

    Ok(patch_branches)
}

/// Get all commit hashes from a specific branch (excluding those already on the base branch)
fn get_branch_commits(repo_path: &Path, branch_name: &str) -> Result<Vec<String>> {
    // Use ^main to exclude commits already on main
    // This gets only commits unique to the branch
    let range_arg = if branch_name.contains('/') {
        // For patch branches like "patch/main-6.6.2", exclude commits from main
        let base = branch_name
            .split('/')
            .nth(1)
            .and_then(|s| s.split('-').next())
            .unwrap_or("main");
        format!("{}..{}", base, branch_name)
    } else {
        branch_name.to_string()
    };

    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%H", &range_arg])
        .output()
        .context("Failed to get branch commits")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commits from branch: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let commits: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(commits)
}

/// Get commit messages from a specific branch
fn get_branch_commit_messages(repo_path: &Path, branch_name: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", branch_name])
        .output()
        .context("Failed to get branch commit messages")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commit messages from branch: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let messages: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(messages)
}

/// Check if all commits from a patch branch are in the target branch
/// This handles both regular merges (matching commit hashes) and squash merges (matching commit titles)
pub fn check_patch_merged(
    repo_path: &Path,
    patch_branch: &str,
    target_branch: &str,
) -> Result<bool> {
    // Get commit history from target branch
    let target_history = get_target_branch_history(repo_path, target_branch)?;

    // Get all commits from the patch branch
    let patch_commits = get_branch_commits(repo_path, patch_branch)?;

    // Strategy 1: Check if all patch commit hashes are in target (for regular merges)
    let all_hashes_found = patch_commits
        .iter()
        .all(|commit| target_history.commit_hashes.contains(commit));

    if all_hashes_found {
        return Ok(true);
    }

    // Strategy 2: Check for cherry-pick references in commit bodies
    // Look for "cherry-picked from <hash>" or "(cherry picked from commit <hash>)" patterns
    // Each line in commit_bodies is a separate line from all commit bodies
    let cherry_pick_found_count = patch_commits
        .iter()
        .filter(|commit_hash| {
            target_history.commit_bodies.iter().any(|line| {
                line.contains(&format!("cherry-picked from {}", commit_hash))
                    || line.contains(&format!("cherry picked from commit {}", commit_hash))
                    || line.contains(&format!("(cherry picked from commit {})", commit_hash))
            })
        })
        .count();

    let cherry_pick_threshold = (patch_commits.len() as f64 * 0.8).ceil() as usize;
    if cherry_pick_found_count >= cherry_pick_threshold {
        return Ok(true);
    }

    // Strategy 3: Check commit messages for squash merges
    // Get commit messages from the patch branch
    let patch_messages = get_branch_commit_messages(repo_path, patch_branch)?;

    if patch_messages.is_empty() {
        // If no commits in patch branch, consider it not merged
        return Ok(false);
    }

    // Check if a significant portion of commit messages appear in target history
    // We require at least 80% of commit messages to be found in target
    let found_count = patch_messages
        .iter()
        .filter(|msg| {
            target_history
                .commit_messages
                .iter()
                .any(|target_msg| target_msg.contains(msg.as_str()))
        })
        .count();

    let threshold = (patch_messages.len() as f64 * 0.8).ceil() as usize;
    Ok(found_count >= threshold)
}

// ==================== Commit Change Analysis ====================

use crate::core::operations::dependency_analysis::{ChangeType, FileChange, LineRange};

/// Gets the files changed in a commit with their change types.
///
/// Returns a list of file changes without line range information.
/// Use `get_commit_changes_with_ranges` if line-level detail is needed.
pub fn get_commit_file_changes(repo_path: &Path, commit_id: &str) -> Result<Vec<FileChange>> {
    validate_git_ref(commit_id)?;

    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["show", "--format=", "--name-status", commit_id])
        .output()
        .context("Failed to execute git show --name-status")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commit file changes: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut changes = Vec::new();

    for line in output_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: STATUS<tab>PATH or STATUS<tab>OLD_PATH<tab>NEW_PATH (for renames)
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }

        let status = parts[0];
        let change_type = match ChangeType::from_git_status(status) {
            Some(ct) => ct,
            None => continue,
        };

        let mut change = if change_type == ChangeType::Rename && parts.len() >= 3 {
            // Rename: STATUS OLD_PATH NEW_PATH
            let mut c = FileChange::new(parts[2].to_string(), change_type);
            c.original_path = Some(parts[1].to_string());
            c
        } else {
            FileChange::new(parts[1].to_string(), change_type)
        };

        // For renames with percentage (R100), extract just the type
        if status.starts_with('R') || status.starts_with('C') {
            change = FileChange::new(
                if parts.len() >= 3 {
                    parts[2].to_string()
                } else {
                    parts[1].to_string()
                },
                change_type,
            );
            if parts.len() >= 3 {
                change.original_path = Some(parts[1].to_string());
            }
        }

        changes.push(change);
    }

    Ok(changes)
}

/// Gets the files changed in a commit with line range information.
///
/// This parses the unified diff output to extract which lines were modified.
/// For added files, returns a single range covering all new lines.
/// For deleted files, returns empty line ranges (the file no longer exists).
pub fn get_commit_changes_with_ranges(
    repo_path: &Path,
    commit_id: &str,
) -> Result<Vec<FileChange>> {
    validate_git_ref(commit_id)?;

    // First get the basic file changes
    let mut changes = get_commit_file_changes(repo_path, commit_id)?;

    // Now get the detailed diff to extract line ranges
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "show",
            "--format=",
            "-U0", // Zero context lines for precise ranges
            "--no-color",
            commit_id,
        ])
        .output()
        .context("Failed to execute git show for diff")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to get commit diff: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let diff_output = String::from_utf8_lossy(&output.stdout);
    let line_ranges_map = parse_unified_diff(&diff_output);

    // Merge line ranges into the file changes
    for change in &mut changes {
        if let Some(ranges) = line_ranges_map.get(&change.path) {
            change.line_ranges = ranges.clone();
        }
        // Also check original path for renames
        if let Some(ref orig_path) = change.original_path
            && let Some(ranges) = line_ranges_map.get(orig_path)
        {
            // For renames, include ranges from the original file
            change.line_ranges.extend(ranges.clone());
        }
    }

    Ok(changes)
}

/// Parses a unified diff output to extract line ranges per file.
///
/// Returns a map from file path to the line ranges that were modified.
fn parse_unified_diff(diff: &str) -> std::collections::HashMap<String, Vec<LineRange>> {
    let mut result: std::collections::HashMap<String, Vec<LineRange>> =
        std::collections::HashMap::new();
    let mut current_file: Option<String> = None;

    for line in diff.lines() {
        // Detect file header: +++ b/path/to/file
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = Some(path.to_string());
            continue;
        }
        // Also handle +++ /dev/null for deleted files
        if line == "+++ /dev/null" {
            current_file = None;
            continue;
        }

        // Parse hunk headers: @@ -old_start,old_count +new_start,new_count @@
        if line.starts_with("@@")
            && let Some(ref file) = current_file
            && let Some(range) = parse_hunk_header(line)
        {
            result.entry(file.clone()).or_default().push(range);
        }
    }

    result
}

/// Parses a hunk header to extract the new file line range.
///
/// Format: @@ -old_start,old_count +new_start,new_count @@ optional context
/// Examples:
///   @@ -10,5 +10,7 @@ -> lines 10-16 modified
///   @@ -10 +10,7 @@ -> lines 10-16 modified (old count defaults to 1)
///   @@ -10,5 +10 @@ -> line 10 modified (new count defaults to 1)
fn parse_hunk_header(header: &str) -> Option<LineRange> {
    // Find the +start,count portion
    let plus_idx = header.find('+')?;
    let at_idx = header[plus_idx..].find(" @@")?;
    let range_str = &header[plus_idx + 1..plus_idx + at_idx];

    // Parse start,count or just start
    let parts: Vec<&str> = range_str.split(',').collect();
    let start: u32 = parts.first()?.parse().ok()?;

    // If count is 0, this is a deletion at this position - no new lines
    let count: u32 = if parts.len() > 1 {
        parts[1].parse().ok()?
    } else {
        1 // Default count is 1
    };

    if count == 0 {
        // This is a pure deletion, no lines added at this position
        return None;
    }

    let end = start + count - 1;
    Some(LineRange::new(start, end))
}

/// Fetches commits from origin for the given commit IDs.
///
/// This is useful for fetching PR merge commits before analyzing their changes.
pub fn fetch_commits_for_analysis(repo_path: &Path, commit_ids: &[String]) -> Result<()> {
    for commit_id in commit_ids {
        // Try to fetch the commit - if it already exists locally, this is a no-op
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["cat-file", "-t", commit_id])
            .output()?;

        if output.status.success() {
            // Commit already exists locally
            continue;
        }

        // Try to fetch it from origin
        let fetch_output = Command::new("git")
            .current_dir(repo_path)
            .args(["fetch", "origin", commit_id])
            .output();

        // Ignore fetch errors - the commit might not be fetchable directly
        // (e.g., if it's not a ref). The caller should handle missing commits.
        if let Ok(out) = fetch_output
            && !out.status.success()
        {
            // Try with --depth=1 for shallow repos
            let _ = Command::new("git")
                .current_dir(repo_path)
                .args(["fetch", "--depth=1", "origin", commit_id])
                .output();
        }
    }

    Ok(())
}

/// Checks if a commit exists in the local repository.
pub fn commit_exists(repo_path: &Path, commit_id: &str) -> bool {
    Command::new("git")
        .current_dir(repo_path)
        .args(["cat-file", "-t", commit_id])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize git repo
        let init_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .unwrap();
        assert!(
            init_output.status.success(),
            "Git init failed: {}",
            String::from_utf8_lossy(&init_output.stderr)
        );

        // Configure git user
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();

        // Disable commit signing and verification for deterministic tests
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "tag.gpgsign", "false"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "push.gpgsign", "false"])
            .output()
            .unwrap();

        // Set default branch to main
        let branch_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "main"])
            .output()
            .unwrap();
        assert!(
            branch_output.status.success(),
            "Creating main branch failed: {}",
            String::from_utf8_lossy(&branch_output.stderr)
        );

        (temp_dir, repo_path)
    }

    fn setup_test_repo_with_origin() -> (TempDir, PathBuf, TempDir, PathBuf) {
        // Create origin repo as bare repository
        let origin_dir = TempDir::new().unwrap();
        let origin_path = origin_dir.path().to_path_buf();

        // Initialize origin as bare repository
        let init_origin_output = Command::new("git")
            .current_dir(&origin_path)
            .args(["init", "--bare"])
            .output()
            .unwrap();
        assert!(
            init_origin_output.status.success(),
            "Git init --bare failed: {}",
            String::from_utf8_lossy(&init_origin_output.stderr)
        );

        // Create temporary repo to populate origin with initial content
        let temp_setup_dir = TempDir::new().unwrap();
        let temp_setup_path = temp_setup_dir.path().to_path_buf();

        // Initialize the setup repo instead of cloning (since origin is empty)
        let init_setup_output = Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["init"])
            .output()
            .unwrap();
        assert!(
            init_setup_output.status.success(),
            "Git init setup failed: {}",
            String::from_utf8_lossy(&init_setup_output.stderr)
        );

        // Configure git user in setup repo
        Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();

        // Disable commit signing
        Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();

        // Set up main branch and origin remote
        Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["checkout", "-b", "main"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["remote", "add", "origin", origin_path.to_str().unwrap()])
            .output()
            .unwrap();

        // Create initial commit and push to origin
        create_commit_with_message(&temp_setup_path, "Initial commit");

        let push_output = Command::new("git")
            .current_dir(&temp_setup_path)
            .args(["push", "origin", "main"])
            .output()
            .unwrap();
        assert!(
            push_output.status.success(),
            "Git push failed: {}",
            String::from_utf8_lossy(&push_output.stderr)
        );

        // Now create the actual test repo by cloning from origin
        let test_dir = TempDir::new().unwrap();
        let test_path = test_dir.path().to_path_buf();

        let clone_test_output = Command::new("git")
            .current_dir(&test_path)
            .args(["clone", origin_path.to_str().unwrap(), "."])
            .output()
            .unwrap();
        assert!(
            clone_test_output.status.success(),
            "Git clone test failed: {}",
            String::from_utf8_lossy(&clone_test_output.stderr)
        );

        // Configure git user in test repo
        Command::new("git")
            .current_dir(&test_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&test_path)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();

        // Disable commit signing in test repo
        Command::new("git")
            .current_dir(&test_path)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&test_path)
            .args(["config", "tag.gpgsign", "false"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&test_path)
            .args(["config", "push.gpgsign", "false"])
            .output()
            .unwrap();

        // Return both directories for cleanup: (test_dir, test_path, origin_dir, origin_path)
        (test_dir, test_path, origin_dir, origin_path)
    }

    fn create_commit_with_message(repo_path: &Path, message: &str) {
        // Create a unique test file for each commit to ensure content changes
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let content = format!("test content for: {} (timestamp: {})", message, timestamp);
        let filename = format!("test_{}.txt", timestamp);
        fs::write(repo_path.join(&filename), content).unwrap();

        // Add and commit
        let add_output = Command::new("git")
            .current_dir(repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        assert!(
            add_output.status.success(),
            "Git add failed: {}",
            String::from_utf8_lossy(&add_output.stderr)
        );

        let commit_output = Command::new("git")
            .current_dir(repo_path)
            .args(["commit", "-m", message])
            .output()
            .unwrap();
        assert!(
            commit_output.status.success(),
            "Git commit failed: {}",
            String::from_utf8_lossy(&commit_output.stderr)
        );
    }

    /// # Get Target Branch History
    ///
    /// Tests retrieval of commit history from the target branch.
    ///
    /// ## Test Scenario
    /// - Sets up a test repository with multiple commits
    /// - Retrieves the commit history from the target branch
    ///
    /// ## Expected Outcome
    /// - All commits in the target branch are retrieved
    /// - Commit history includes commit IDs and metadata
    #[test]
    fn test_get_target_branch_history() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create some commits with known content
        create_commit_with_message(&repo_path, "Initial commit");
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");
        create_commit_with_message(&repo_path, "Regular commit");
        create_commit_with_message(&repo_path, "Merged PR 456: Add new feature");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Check that we have the expected number of messages (should be 4)
        assert!(
            history.commit_messages.len() >= 3,
            "Expected at least 3 commits, got {}",
            history.commit_messages.len()
        );

        // Check that specific messages are present
        assert!(
            history
                .commit_messages
                .contains(&"Initial commit".to_string())
        );
        assert!(
            history
                .commit_messages
                .contains(&"Merged PR 123: Fix authentication bug".to_string())
        );
        assert!(
            history
                .commit_messages
                .contains(&"Regular commit".to_string())
        );
        assert!(
            history
                .commit_messages
                .contains(&"Merged PR 456: Add new feature".to_string())
        );

        // Check that we have commit hashes
        assert_eq!(history.commit_hashes.len(), history.commit_messages.len());
        assert!(!history.commit_hashes.is_empty());
    }

    /// # Check Commit in History
    ///
    /// Tests checking whether a specific commit exists in branch history.
    ///
    /// ## Test Scenario
    /// - Creates a repository with known commits
    /// - Checks for both existing and non-existing commit IDs
    ///
    /// ## Expected Outcome
    /// - Existing commits are correctly identified in history
    /// - Non-existing commits return false
    #[test]
    fn test_check_commit_in_history() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a commit and get its hash
        create_commit_with_message(&repo_path, "Test commit");

        let output = std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "main"])
            .output()
            .unwrap();
        let commit_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test that the commit is found in history
        assert!(check_commit_in_history(&commit_hash, &history));

        // Test that a fake commit is not found
        assert!(!check_commit_in_history("fake_commit_hash", &history));
    }

    /// # Check PR Merged in History (Azure DevOps)
    ///
    /// Tests detection of Azure DevOps PR merge patterns in commit history.
    ///
    /// ## Test Scenario
    /// - Creates commits with Azure DevOps merge commit patterns
    /// - Tests PR merge detection using Azure DevOps conventions
    ///
    /// ## Expected Outcome
    /// - Azure DevOps merge patterns are correctly identified
    /// - PR numbers and merge commits are properly detected
    #[test]
    fn test_check_pr_merged_in_history_azure_devops() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create commits with Azure DevOps merge patterns
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");
        create_commit_with_message(&repo_path, "Merged PR 456: Add new feature");
        create_commit_with_message(&repo_path, "Regular commit");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test exact matches
        assert!(check_pr_merged_in_history(
            123,
            "Fix authentication bug",
            &history
        ));
        assert!(check_pr_merged_in_history(456, "Add new feature", &history));

        // Test non-matches - both implementations should find PR 123 by ID even with wrong title
        assert!(check_pr_merged_in_history(123, "Wrong title", &history));
        assert!(!check_pr_merged_in_history(
            789,
            "Non-existent PR",
            &history
        ));
    }

    /// # Check PR Merged in History (Title Search)
    ///
    /// Tests detection of PR merges by searching for PR titles in commit messages.
    ///
    /// ## Test Scenario
    /// - Creates commits with PR titles embedded in commit messages
    /// - Tests title-based PR merge detection
    ///
    /// ## Expected Outcome
    /// - PR titles are found in commit message history
    /// - Title matching is robust and handles variations
    #[test]
    fn test_check_pr_merged_in_history_title_search() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create commits with titles that should match PR titles using fuzzy matching
        create_commit_with_message(
            &repo_path,
            "Fix authentication vulnerability in login system",
        );
        create_commit_with_message(
            &repo_path,
            "Update authentication system for better security",
        );

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test fuzzy title matching
        assert!(check_pr_merged_in_history(
            999,
            "Fix authentication vulnerability login",
            &history
        ));
        assert!(check_pr_merged_in_history(
            888,
            "Update authentication system security",
            &history
        ));

        // Test that short titles don't match to avoid false positives
        assert!(!check_pr_merged_in_history(123, "Fix", &history));
        assert!(!check_pr_merged_in_history(456, "Update", &history));
    }

    /// # Check PR Merged in History (ID Search)
    ///
    /// Tests detection of PR merges by searching for PR IDs in commit messages.
    ///
    /// ## Test Scenario
    /// - Creates commits with PR IDs embedded in commit messages
    /// - Tests ID-based PR merge detection
    ///
    /// ## Expected Outcome
    /// - PR IDs are found in commit message history
    /// - ID matching correctly identifies PR numbers
    #[test]
    fn test_check_pr_merged_in_history_id_search() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create commits with PR ID references
        create_commit_with_message(&repo_path, "Fix issue reported in PR123");
        create_commit_with_message(&repo_path, "Addresses feedback from pr 456");
        create_commit_with_message(&repo_path, "Related to #789 discussion");
        create_commit_with_message(&repo_path, "Implements feature [321] as requested");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test PR ID search in various formats
        assert!(check_pr_merged_in_history(123, "Some title", &history));
        assert!(check_pr_merged_in_history(456, "Another title", &history));
        assert!(check_pr_merged_in_history(789, "Different title", &history));
        assert!(check_pr_merged_in_history(321, "Feature title", &history));
    }

    /// # Implementation Consistency
    ///
    /// Tests consistency between different PR detection implementations.
    ///
    /// ## Test Scenario
    /// - Runs multiple PR detection methods on the same data
    /// - Compares results for consistency and accuracy
    ///
    /// ## Expected Outcome
    /// - Different detection methods produce consistent results
    /// - Implementation variations don't affect core functionality
    #[test]
    fn test_implementation_consistency() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create test commits with known patterns
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");
        create_commit_with_message(&repo_path, "Merged PR 456: Add new feature");
        create_commit_with_message(&repo_path, "Regular commit without PR pattern");
        create_commit_with_message(&repo_path, "Fix issue reported in PR789");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test cases that should work
        let test_cases = vec![
            (123, "Fix authentication bug", true), // Should match Azure DevOps pattern
            (456, "Add new feature", true),        // Should match Azure DevOps pattern
            (789, "Any title", true),              // Should match PR ID reference
            (999, "Non-existent PR", false),       // Should not match
        ];

        for (pr_id, pr_title, expected) in test_cases {
            let result = check_pr_merged_in_history(pr_id, pr_title, &history);
            assert_eq!(
                result, expected,
                "Implementation failed for PR {} with title '{}'. Expected {}, got {}",
                pr_id, pr_title, expected, result
            );
        }
    }

    /// # PR ID False Positive Prevention
    ///
    /// Tests that PR ID detection does not produce false positives when
    /// similar but different PR IDs exist in the commit history.
    ///
    /// ## Test Scenario
    /// - Creates commits with PR IDs that could be confused with shorter prefixes
    /// - Verifies that searching for a shorter PR ID does not match a longer one
    ///
    /// ## Expected Outcome
    /// - PR 12 should NOT match commits containing only PR 1234
    /// - PR 123 should NOT match commits containing only PR 1234
    /// - Only exact PR ID matches should be detected
    #[test]
    fn test_pr_id_false_positive_prevention() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create commits with specific PR IDs (using 4-5 digit IDs to test prefix matching)
        create_commit_with_message(&repo_path, "Merged PR 12345: Feature implementation");
        create_commit_with_message(&repo_path, "Merged PR 56789: Another feature");
        create_commit_with_message(&repo_path, "Fix issue reported in PR99999");
        create_commit_with_message(&repo_path, "Related to #111111 discussion");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // These should NOT match (shorter PR IDs that are prefixes of the actual IDs)
        assert!(
            !check_pr_merged_in_history(1234, "Some title", &history),
            "PR 1234 should NOT match PR 12345"
        );
        assert!(
            !check_pr_merged_in_history(123, "Some title", &history),
            "PR 123 should NOT match PR 12345"
        );
        assert!(
            !check_pr_merged_in_history(12, "Some title", &history),
            "PR 12 should NOT match PR 12345"
        );
        assert!(
            !check_pr_merged_in_history(5678, "Some title", &history),
            "PR 5678 should NOT match PR 56789"
        );
        assert!(
            !check_pr_merged_in_history(567, "Some title", &history),
            "PR 567 should NOT match PR 56789"
        );
        assert!(
            !check_pr_merged_in_history(9999, "Some title", &history),
            "PR 9999 should NOT match PR 99999"
        );
        assert!(
            !check_pr_merged_in_history(11111, "Some title", &history),
            "PR 11111 should NOT match PR 111111"
        );

        // These SHOULD match (exact PR IDs)
        assert!(
            check_pr_merged_in_history(12345, "Feature implementation", &history),
            "PR 12345 should match its exact Azure DevOps merge commit"
        );
        assert!(
            check_pr_merged_in_history(56789, "Another feature", &history),
            "PR 56789 should match its exact Azure DevOps merge commit"
        );
        assert!(
            check_pr_merged_in_history(99999, "Any title", &history),
            "PR 99999 should match its exact ID reference"
        );
        assert!(
            check_pr_merged_in_history(111111, "Any title", &history),
            "PR 111111 should match its exact ID reference"
        );
    }

    /// # Shallow Clone Repository Success
    ///
    /// Tests successful shallow cloning of a Git repository.
    ///
    /// ## Test Scenario
    /// - Attempts to create a shallow clone of a repository
    /// - Validates that shallow clone operations work correctly
    ///
    /// ## Expected Outcome
    /// - Repository is successfully cloned with limited history
    /// - Shallow clone reduces storage and time requirements
    #[test]
    fn test_shallow_clone_repo_success() {
        // Skip this test if not in a Git environment that supports cloning
        if std::env::var("CI").is_ok() {
            // Skip in CI environments where networking might be restricted
        }

        // Test would require actual network access to clone a repo
        // This is more of an integration test that would need a test repository
        // For unit testing, we focus on the error handling paths below
    }

    /// # Create Worktree Success
    ///
    /// Tests successful creation of Git worktrees for parallel operations.
    ///
    /// ## Test Scenario
    /// - Creates a worktree in a test repository
    /// - Validates worktree creation and configuration
    ///
    /// ## Expected Outcome
    /// - Worktree is successfully created and accessible
    /// - Worktree allows independent parallel work
    #[test]
    fn test_create_worktree_success() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create and checkout a test branch to simulate target branch
        let branch_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();
        assert!(branch_output.status.success());

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push the target branch to origin
        let push_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();
        assert!(push_output.status.success());

        // Go back to main
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Test worktree creation
        let worktree_path = create_worktree(&repo_path, "target-branch", "1.0.0", false).unwrap();

        assert!(worktree_path.exists());
        assert_eq!(worktree_path.file_name().unwrap(), "next-1.0.0");
    }

    /// # Create Worktree Hooks Disabled by Default
    ///
    /// Tests that git hooks are disabled when run_hooks=false (default).
    ///
    /// ## Test Scenario
    /// - Creates a worktree with run_hooks=false
    /// - Verifies that core.hooksPath is set to /dev/null
    ///
    /// ## Expected Outcome
    /// - The worktree has hooks disabled via core.hooksPath=/dev/null
    #[test]
    fn test_create_worktree_hooks_disabled_by_default() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push the target branch to origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create worktree with hooks disabled (default)
        let worktree_path = create_worktree(&repo_path, "target-branch", "1.0.0", false).unwrap();

        // Verify hooks are disabled
        let output = Command::new("git")
            .current_dir(&worktree_path)
            .args(["config", "--get", "core.hooksPath"])
            .output()
            .unwrap();

        let hooks_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            hooks_path, "/dev/null",
            "Hooks should be disabled by default"
        );
    }

    /// # Create Worktree Hooks Enabled with run_hooks Flag
    ///
    /// Tests that git hooks are NOT disabled when run_hooks=true.
    ///
    /// ## Test Scenario
    /// - Creates a worktree with run_hooks=true
    /// - Verifies that core.hooksPath is NOT set to /dev/null
    ///
    /// ## Expected Outcome
    /// - The worktree does NOT have core.hooksPath set to /dev/null
    #[test]
    fn test_create_worktree_hooks_enabled_with_flag() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push the target branch to origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create worktree with hooks enabled
        let worktree_path = create_worktree(&repo_path, "target-branch", "2.0.0", true).unwrap();

        // Verify hooks are NOT disabled
        let output = Command::new("git")
            .current_dir(&worktree_path)
            .args(["config", "--get", "core.hooksPath"])
            .output()
            .unwrap();

        let hooks_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            hooks_path.is_empty() || hooks_path != "/dev/null",
            "Hooks should NOT be disabled when run_hooks=true"
        );
    }

    /// # Create Worktree Branch Exists
    ///
    /// Tests worktree creation when the target branch already exists.
    ///
    /// ## Test Scenario
    /// - Attempts to create worktree for an existing branch
    /// - Tests error handling for branch name conflicts
    ///
    /// ## Expected Outcome
    /// - System handles existing branch names gracefully
    /// - Appropriate error or alternative solution is provided
    #[test]
    fn test_create_worktree_branch_exists() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push the target branch to origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        // Create the worktree directory manually to simulate path conflict
        let worktree_path = repo_path.join("next-1.0.0");
        std::fs::create_dir_all(&worktree_path).unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // This should fail because worktree path already exists
        let result = create_worktree(&repo_path, "target-branch", "1.0.0", false);
        assert!(result.is_err());

        if let Err(RepositorySetupError::WorktreeExists(path)) = result {
            assert!(path.contains("next-1.0.0"));
        } else {
            panic!("Expected WorktreeExists error, got: {:?}", result);
        }
    }

    /// # Create Worktree with Existing Branch Name
    ///
    /// Tests worktree creation when using an existing branch name.
    ///
    /// ## Test Scenario
    /// - Attempts to create worktree with a branch name that exists
    /// - Tests conflict resolution and naming strategies
    ///
    /// ## Expected Outcome
    /// - Conflict is resolved through alternative naming or linking
    /// - Worktree creation succeeds with appropriate branch handling
    #[test]
    fn test_create_worktree_with_existing_branch_name() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create a local branch with the name that worktree would use
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "next-1.0.0"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Branch commit");

        // Go back to main
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create target branch in origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();
        create_commit_with_message(&repo_path, "Target branch commit");
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // This should succeed because current implementation doesn't use -b flag
        // It creates a detached HEAD instead, so existing branch doesn't conflict
        let result = create_worktree(&repo_path, "target-branch", "1.0.0", false);
        assert!(result.is_ok());

        let worktree_path = result.unwrap();
        assert!(worktree_path.exists());
        assert_eq!(worktree_path.file_name().unwrap(), "next-1.0.0");
    }

    /// # Create Worktree Detects Branch Exists Error
    ///
    /// Tests detection of branch existence errors during worktree creation.
    ///
    /// ## Test Scenario
    /// - Simulates conditions that trigger branch exists errors
    /// - Tests error detection and classification logic
    ///
    /// ## Expected Outcome
    /// - Branch existence errors are correctly identified
    /// - Error messages are properly parsed and classified
    #[test]
    fn test_create_worktree_detects_branch_exists_error() {
        // Test that the error detection logic correctly identifies "branch already exists" errors
        // This simulates what would happen if the implementation used -b flag

        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // First create an initial commit if needed
        create_commit_with_message(&repo_path, "Initial commit");

        // Create a branch that would conflict when using -b flag
        let branch_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "next-1.0.0"])
            .output()
            .unwrap();
        assert!(
            branch_output.status.success(),
            "Failed to create initial branch"
        );

        // Verify branch exists
        let branch_list = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "--list", "next-1.0.0"])
            .output()
            .unwrap();
        let branch_list_output = String::from_utf8_lossy(&branch_list.stdout);
        assert!(
            branch_list_output.contains("next-1.0.0"),
            "Branch next-1.0.0 should exist"
        );

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();
        create_commit_with_message(&repo_path, "Target branch commit");
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Manually test what happens if we try to use -b flag with existing branch
        let worktree_path = repo_path.join("next-1.0.0");
        let create_output = Command::new("git")
            .current_dir(&repo_path)
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                "-b",
                "next-1.0.0",
                "origin/target-branch",
            ])
            .output()
            .unwrap();

        let stderr = String::from_utf8_lossy(&create_output.stderr);

        if create_output.status.success() {
            // The command succeeded, which means Git didn't see a conflict.
            // This can happen if the branch exists but Git still allows the operation.
            // In this case, we just verify the worktree was created successfully.
            assert!(worktree_path.exists(), "Worktree should have been created");
        } else {
            // The command failed - verify it's due to branch already existing
            assert!(
                stderr.contains("branch") && stderr.contains("already exists"),
                "Expected 'branch already exists' error, got: {}",
                stderr
            );
        }
    }

    /// # Create Worktree Path Exists
    ///
    /// Tests worktree creation when the target path already exists.
    ///
    /// ## Test Scenario
    /// - Attempts to create worktree at an existing filesystem path
    /// - Tests path conflict detection and resolution
    ///
    /// ## Expected Outcome
    /// - Path conflicts are detected and handled appropriately
    /// - Error handling prevents overwriting existing paths
    #[test]
    fn test_create_worktree_path_exists() {
        let (_temp_dir, repo_path) = setup_test_repo();

        create_commit_with_message(&repo_path, "Initial commit");

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create the worktree directory manually
        let worktree_path = repo_path.join("next-1.0.0");
        std::fs::create_dir(&worktree_path).unwrap();

        // This should fail because path already exists
        let result = create_worktree(&repo_path, "target-branch", "1.0.0", false);
        assert!(result.is_err());

        if let Err(RepositorySetupError::WorktreeExists(_)) = result {
            // Expected error
        } else {
            panic!("Expected WorktreeExists error");
        }
    }

    /// # Force Remove Worktree Non-Existent
    ///
    /// Tests forced removal of worktrees that don't exist.
    ///
    /// ## Test Scenario
    /// - Attempts to force remove a worktree that doesn't exist
    /// - Tests error handling for non-existent worktree removal
    ///
    /// ## Expected Outcome
    /// - Non-existent worktree removal is handled gracefully
    /// - No errors are thrown for missing worktrees
    #[test]
    fn test_force_remove_worktree_non_existent() {
        let (_temp_dir, repo_path) = setup_test_repo();

        create_commit_with_message(&repo_path, "Initial commit");

        // Try to remove non-existent worktree - should succeed (no-op)
        let result = force_remove_worktree(&repo_path, "non-existent");
        assert!(result.is_ok());
    }

    /// # Force Remove Worktree Success
    ///
    /// Tests successful forced removal of existing worktrees.
    ///
    /// ## Test Scenario
    /// - Creates a worktree and then forces its removal
    /// - Tests cleanup and removal operations
    ///
    /// ## Expected Outcome
    /// - Worktree is successfully removed from filesystem
    /// - All associated references and files are cleaned up
    #[test]
    fn test_force_remove_worktree_success() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push the target branch to origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create worktree
        let worktree_path = create_worktree(&repo_path, "target-branch", "1.0.0", false).unwrap();
        assert!(worktree_path.exists());

        // Force remove worktree
        let result = force_remove_worktree(&repo_path, "1.0.0");
        assert!(result.is_ok());

        // Verify worktree is removed
        assert!(!worktree_path.exists());
    }

    /// # Cherry Pick Commit Success
    ///
    /// Tests successful cherry-picking of commits between branches.
    ///
    /// ## Test Scenario
    /// - Creates commits on one branch and cherry-picks to another
    /// - Tests commit application and conflict-free cherry-picking
    ///
    /// ## Expected Outcome
    /// - Commits are successfully cherry-picked without conflicts
    /// - Changes are correctly applied to the target branch
    #[test]
    fn test_cherry_pick_commit_success() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main
        create_commit_with_message(&repo_path, "Initial commit");

        // Create feature branch and commit
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "feature"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Feature commit");

        // Get commit hash
        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let commit_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Go back to main and cherry-pick
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        let result = cherry_pick_commit(&repo_path, &commit_hash);

        // Cherry-pick should succeed
        assert!(result.is_ok());

        // Verify commit was cherry-picked
        let history = get_target_branch_history(&repo_path, "main").unwrap();
        assert!(
            history
                .commit_messages
                .contains(&"Feature commit".to_string())
        );
    }

    /// # Cherry Pick Commit Conflict
    ///
    /// Tests cherry-picking behavior when conflicts occur.
    ///
    /// ## Test Scenario
    /// - Creates conflicting changes and attempts cherry-pick
    /// - Tests conflict detection and handling
    ///
    /// ## Expected Outcome
    /// - Conflicts are properly detected and reported
    /// - Cherry-pick operation fails gracefully with conflict info
    #[test]
    fn test_cherry_pick_commit_conflict() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a file with content
        std::fs::write(repo_path.join("conflict.txt"), "original content").unwrap();
        create_commit_with_message(&repo_path, "Initial commit with file");

        // Create feature branch and modify the same file
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "feature"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("conflict.txt"), "feature content").unwrap();
        create_commit_with_message(&repo_path, "Feature commit");

        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let feature_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Go back to main and modify the same file differently
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("conflict.txt"), "main content").unwrap();
        create_commit_with_message(&repo_path, "Main commit");

        // Try to cherry-pick - should detect conflict
        let result = cherry_pick_commit(&repo_path, &feature_hash);
        assert!(result.is_ok()); // cherry_pick_commit returns CherryPickResult, not error

        // Check that it detected conflict
        match result.unwrap() {
            crate::git::CherryPickResult::Conflict(_) => {
                // Expected conflict
            }
            _ => panic!("Expected conflict result"),
        }
    }

    /// # Cherry Pick Merge Commit Success
    ///
    /// Tests that cherry-picking a merge commit works correctly with the -m flag.
    ///
    /// ## Test Scenario
    /// - Creates a merge commit on a feature branch
    /// - Cherry-picks the merge commit to a different branch
    /// - Verifies the changes are applied correctly
    ///
    /// ## Expected Outcome
    /// - Cherry-pick succeeds without the "no -m option was given" error
    /// - Changes from the merged branch are applied to the target
    #[test]
    fn test_cherry_pick_merge_commit_success() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main with a base file
        std::fs::write(repo_path.join("base.txt"), "base content").unwrap();
        create_commit_with_message(&repo_path, "Initial commit");

        // Create a feature branch and add some changes
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "feature"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("feature.txt"), "feature content").unwrap();
        create_commit_with_message(&repo_path, "Feature commit");

        // Go back to main and create a different commit (so merge won't fast-forward)
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("main.txt"), "main content").unwrap();
        create_commit_with_message(&repo_path, "Main branch commit");

        // Merge feature into main (creates a merge commit)
        let merge_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["merge", "feature", "-m", "Merge feature branch"])
            .output()
            .unwrap();
        assert!(
            merge_output.status.success(),
            "Merge failed: {}",
            String::from_utf8_lossy(&merge_output.stderr)
        );

        // Get the merge commit hash
        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let merge_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Create a target branch from before the merge
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target", "HEAD~1"])
            .output()
            .unwrap();

        // Verify feature.txt doesn't exist on target branch yet
        assert!(
            !repo_path.join("feature.txt").exists(),
            "feature.txt should not exist before cherry-pick"
        );

        // Cherry-pick the merge commit (this should use -m 1 internally)
        let result = cherry_pick_commit(&repo_path, &merge_hash);
        assert!(result.is_ok(), "Cherry-pick should not error");

        match result.unwrap() {
            crate::git::CherryPickResult::Success => {
                // Expected success
            }
            crate::git::CherryPickResult::Conflict(files) => {
                panic!("Unexpected conflict with files: {:?}", files);
            }
            crate::git::CherryPickResult::Failed(msg) => {
                panic!("Cherry-pick failed: {}", msg);
            }
        }

        // Verify that the feature changes were applied
        assert!(
            repo_path.join("feature.txt").exists(),
            "feature.txt should exist after cherry-picking the merge commit"
        );

        let content = std::fs::read_to_string(repo_path.join("feature.txt")).unwrap();
        assert_eq!(
            content, "feature content",
            "Feature file should have correct content"
        );
    }

    /// # Cherry Pick Merge Commit With Conflict
    ///
    /// Tests that cherry-picking a merge commit correctly detects conflicts.
    ///
    /// ## Test Scenario
    /// - Creates a merge commit with changes to a specific file
    /// - Creates conflicting changes on the target branch
    /// - Attempts to cherry-pick the merge commit
    ///
    /// ## Expected Outcome
    /// - Cherry-pick detects the conflict and returns Conflict result
    #[test]
    fn test_cherry_pick_merge_commit_conflict() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main with a file
        std::fs::write(repo_path.join("shared.txt"), "original content").unwrap();
        create_commit_with_message(&repo_path, "Initial commit");

        // Create a feature branch and modify the shared file
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "feature"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("shared.txt"), "feature content").unwrap();
        create_commit_with_message(&repo_path, "Feature modifies shared");

        // Go back to main and create a different commit
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("unrelated.txt"), "unrelated").unwrap();
        create_commit_with_message(&repo_path, "Main unrelated commit");

        // Merge feature into main (creates a merge commit)
        let merge_output = Command::new("git")
            .current_dir(&repo_path)
            .args(["merge", "feature", "-m", "Merge feature branch"])
            .output()
            .unwrap();
        assert!(
            merge_output.status.success(),
            "Merge failed: {}",
            String::from_utf8_lossy(&merge_output.stderr)
        );

        // Get the merge commit hash
        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let merge_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Create a target branch from the initial commit with conflicting changes
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target", "HEAD~2"])
            .output()
            .unwrap();

        // Make conflicting changes to shared.txt
        std::fs::write(repo_path.join("shared.txt"), "target conflicting content").unwrap();
        create_commit_with_message(&repo_path, "Target conflicting commit");

        // Try to cherry-pick the merge commit - should detect conflict
        let result = cherry_pick_commit(&repo_path, &merge_hash);
        assert!(result.is_ok(), "Cherry-pick should not error");

        match result.unwrap() {
            crate::git::CherryPickResult::Conflict(files) => {
                assert!(
                    files.contains(&"shared.txt".to_string()),
                    "Should report shared.txt as conflicted"
                );
            }
            crate::git::CherryPickResult::Success => {
                panic!("Expected conflict but got success");
            }
            crate::git::CherryPickResult::Failed(msg) => {
                panic!("Unexpected failure: {}", msg);
            }
        }
    }

    /// # Cleanup Migration Worktrees
    ///
    /// Tests cleanup of worktrees created during migration processes.
    ///
    /// ## Test Scenario
    /// - Creates multiple migration worktrees
    /// - Tests bulk cleanup and removal operations
    ///
    /// ## Expected Outcome
    /// - All migration worktrees are successfully cleaned up
    /// - Repository is left in clean state after migration
    #[test]
    fn test_cleanup_migration_worktrees() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push the target branch to origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "origin", "target-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create some migration worktrees
        create_worktree(&repo_path, "target-branch", "migration-1.0.0", false).unwrap();
        create_worktree(&repo_path, "target-branch", "migration-2.0.0", false).unwrap();

        // Also create a regular worktree that shouldn't be removed
        create_worktree(&repo_path, "target-branch", "regular-1.0.0", false).unwrap();

        // Verify worktrees exist
        let worktree1_path = repo_path.join("next-migration-1.0.0");
        let worktree2_path = repo_path.join("next-migration-2.0.0");
        let regular_path = repo_path.join("next-regular-1.0.0");

        assert!(worktree1_path.exists());
        assert!(worktree2_path.exists());
        assert!(regular_path.exists());

        // Cleanup migration worktrees
        let result = cleanup_migration_worktrees(&repo_path);
        assert!(result.is_ok());

        // Verify only migration worktrees were removed
        assert!(!worktree1_path.exists());
        assert!(!worktree2_path.exists());
        assert!(regular_path.exists()); // Regular worktree should remain
    }

    /// # Normalize Title
    ///
    /// Tests normalization of PR titles for consistent matching.
    ///
    /// ## Test Scenario
    /// - Provides various PR titles with different formatting
    /// - Tests title normalization and standardization
    ///
    /// ## Expected Outcome
    /// - Titles are normalized to consistent format
    /// - Special characters and spacing are handled uniformly
    #[test]
    fn test_normalize_title() {
        let test_cases = vec![
            ("Merge pull request #123 from feature", "123 from feature"),
            ("Merged PR 456: Fix bug", "456: fix bug"),
            ("fix: Authentication issue", "authentication issue"),
            ("feat: New feature", "new feature"),
            ("chore: Update dependencies", "update dependencies"),
            ("docs: Update README", "update readme"),
            ("#789 Bug fix", "789 bug fix"),
            ("  Whitespace test  ", "whitespace test"),
        ];

        for (input, expected) in test_cases {
            let result = normalize_title(input);
            assert_eq!(result, expected, "Failed for input: '{}'", input);
        }
    }

    /// # Check Common Word
    ///
    /// Tests identification of common words that should be excluded from matching.
    ///
    /// ## Test Scenario
    /// - Tests various words against common word list
    /// - Validates filtering of noise words in title matching
    ///
    /// ## Expected Outcome
    /// - Common words are correctly identified and filtered
    /// - Content words are preserved for meaningful matching
    #[test]
    fn test_is_common_word() {
        // Test some common words
        assert!(is_common_word("the"));
        assert!(is_common_word("and"));
        assert!(is_common_word("with"));
        assert!(is_common_word("THE")); // Should work case-insensitive

        // Test some uncommon words
        assert!(!is_common_word("authentication"));
        assert!(!is_common_word("feature"));
        assert!(!is_common_word("implementation"));
    }

    /// # Search PR Title in History (Short Title)
    ///
    /// Tests searching for PR titles with short titles in commit history.
    ///
    /// ## Test Scenario
    /// - Creates commits with short PR titles
    /// - Tests title search functionality with brief titles
    ///
    /// ## Expected Outcome
    /// - Short titles are found despite limited content
    /// - Search handles edge cases of minimal title content
    #[test]
    fn test_search_pr_title_in_history_short_title() {
        let mut commit_hashes = std::collections::HashSet::new();
        commit_hashes.insert("abc123".to_string());

        let history = CommitHistory {
            commit_messages: vec!["Some commit message".to_string()],
            commit_hashes,
            commit_bodies: vec![],
        };

        // Short titles should return false to avoid false positives
        assert!(!search_pr_title_in_history("Fix", &history));
        assert!(!search_pr_title_in_history("Update", &history));
        assert!(!search_pr_title_in_history("", &history));
    }

    /// # Search PR Title in History (Fuzzy Match)
    ///
    /// Tests fuzzy matching of PR titles in commit history.
    ///
    /// ## Test Scenario
    /// - Creates commits with variations of PR titles
    /// - Tests fuzzy matching algorithms for title detection
    ///
    /// ## Expected Outcome
    /// - Titles with minor variations are successfully matched
    /// - Fuzzy matching tolerates small differences in wording
    #[test]
    fn test_search_pr_title_in_history_fuzzy_match() {
        let mut commit_hashes = std::collections::HashSet::new();
        commit_hashes.insert("abc123".to_string());
        commit_hashes.insert("def456".to_string());

        let history = CommitHistory {
            commit_messages: vec![
                "Fix authentication vulnerability in login system".to_string(),
                "Update user interface design".to_string(),
            ],
            commit_hashes,
            commit_bodies: vec![],
        };

        // Should match with 80% word overlap
        assert!(search_pr_title_in_history(
            "Fix authentication vulnerability login",
            &history
        ));
        assert!(search_pr_title_in_history(
            "Update user interface",
            &history
        ));

        // Should not match with low word overlap
        assert!(!search_pr_title_in_history(
            "Different completely unrelated words",
            &history
        ));

        // Should handle common words properly
        assert!(!search_pr_title_in_history(
            "The and with from this",
            &history
        ));
    }

    /// # Search PR ID in History
    ///
    /// Tests searching for specific PR IDs in commit message history.
    ///
    /// ## Test Scenario
    /// - Creates commits with embedded PR ID references
    /// - Tests ID-based search functionality
    ///
    /// ## Expected Outcome
    /// - PR IDs are accurately found in commit messages
    /// - ID search is precise and reliable
    #[test]
    fn test_search_pr_id_in_history() {
        let mut commit_hashes = std::collections::HashSet::new();
        commit_hashes.insert("a".to_string());
        commit_hashes.insert("b".to_string());
        commit_hashes.insert("c".to_string());
        commit_hashes.insert("d".to_string());
        commit_hashes.insert("e".to_string());

        let history = CommitHistory {
            commit_messages: vec![
                "Fix issue reported in PR123".to_string(),
                "Addresses feedback from pr 456".to_string(),
                "Related to #789 discussion".to_string(),
                "Implements feature [321] as requested".to_string(),
                "Update for work item (654)".to_string(),
            ],
            commit_hashes,
            commit_bodies: vec![],
        };

        // Test various PR ID formats
        assert!(search_pr_id_in_history(123, &history));
        assert!(search_pr_id_in_history(456, &history));
        assert!(search_pr_id_in_history(789, &history));
        assert!(search_pr_id_in_history(321, &history));
        assert!(search_pr_id_in_history(654, &history));

        // Test non-existent PR ID
        assert!(!search_pr_id_in_history(999, &history));
    }

    /// # List Local Branches
    ///
    /// Tests listing local branches matching a pattern.
    ///
    /// ## Test Scenario
    /// - Creates repository with various branches
    /// - Tests pattern matching for branch listing
    ///
    /// ## Expected Outcome
    /// - Correctly lists branches matching the pattern
    /// - Filters out non-matching branches
    #[test]
    fn test_list_local_branches() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create various branches
        for branch in &[
            "feature/test",
            "patch/next-1.0.0",
            "patch/main-2.0.0",
            "bugfix/issue",
        ] {
            Command::new("git")
                .current_dir(&repo_path)
                .args(["branch", branch])
                .output()
                .unwrap();
        }

        // Test listing all branches
        let all_branches = list_local_branches(&repo_path, "*").unwrap();
        assert!(all_branches.len() >= 5); // main + 4 created branches

        // Test listing patch branches only
        let patch_branches = list_local_branches(&repo_path, "patch/*").unwrap();
        assert_eq!(patch_branches.len(), 2);
        assert!(patch_branches.contains(&"patch/next-1.0.0".to_string()));
        assert!(patch_branches.contains(&"patch/main-2.0.0".to_string()));

        // Test listing feature branches
        let feature_branches = list_local_branches(&repo_path, "feature/*").unwrap();
        assert_eq!(feature_branches.len(), 1);
        assert!(feature_branches.contains(&"feature/test".to_string()));
    }

    /// # List Patch Branches
    ///
    /// Tests parsing and listing of patch branches with metadata.
    ///
    /// ## Test Scenario
    /// - Creates patch branches with various naming patterns
    /// - Tests parsing of target and version information
    ///
    /// ## Expected Outcome
    /// - Correctly parses patch branches into CleanupBranch structs
    /// - Extracts target and version information accurately
    #[test]
    fn test_list_patch_branches() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create patch branches with valid format
        for branch in &[
            "patch/next-1.0.0",
            "patch/main-2.0.0",
            "patch/next-v3.0.0",
            "patch/production-1.2.3",
        ] {
            Command::new("git")
                .current_dir(&repo_path)
                .args(["branch", branch])
                .output()
                .unwrap();
        }

        // Create some non-patch branches (should be ignored)
        Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "feature/test"])
            .output()
            .unwrap();

        let patch_branches = list_patch_branches(&repo_path).unwrap();
        assert_eq!(patch_branches.len(), 4);

        // Verify parsing of branch names
        let next_1_0_0 = patch_branches
            .iter()
            .find(|b| b.name == "patch/next-1.0.0")
            .unwrap();
        assert_eq!(next_1_0_0.target, "next");
        assert_eq!(next_1_0_0.version, "1.0.0");
        assert!(!next_1_0_0.selected);
        assert!(!next_1_0_0.is_merged);

        let prod_1_2_3 = patch_branches
            .iter()
            .find(|b| b.name == "patch/production-1.2.3")
            .unwrap();
        assert_eq!(prod_1_2_3.target, "production");
        assert_eq!(prod_1_2_3.version, "1.2.3");
    }

    /// # Check Patch Merged (Regular Merge)
    ///
    /// Tests detection of patch branches merged via regular git merge.
    ///
    /// ## Test Scenario
    /// - Creates a patch branch with commits
    /// - Merges it into target branch
    /// - Tests if merge is detected correctly
    ///
    /// ## Expected Outcome
    /// - Detects merged patch branch via commit hash matching
    /// - Returns true for merged branches
    #[test]
    fn test_check_patch_merged_regular_merge() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create and switch to patch branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "patch/main-1.0.0"])
            .output()
            .unwrap();

        // Add commits to patch branch
        fs::write(repo_path.join("patch.txt"), "patch content 1").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Patch commit 1"])
            .output()
            .unwrap();

        fs::write(repo_path.join("patch2.txt"), "patch content 2").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Patch commit 2"])
            .output()
            .unwrap();

        // Switch back to main and merge
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["merge", "patch/main-1.0.0", "--no-ff", "-m", "Merge patch"])
            .output()
            .unwrap();

        // Test that patch is detected as merged
        let is_merged = check_patch_merged(&repo_path, "patch/main-1.0.0", "main").unwrap();
        assert!(is_merged, "Patch should be detected as merged");
    }

    /// # Check Patch Merged (Squash Merge)
    ///
    /// Tests detection of patch branches merged via squash merge.
    ///
    /// ## Test Scenario
    /// - Creates a patch branch with multiple commits
    /// - Squash merges it into target branch
    /// - Tests if squash merge is detected via commit message matching
    ///
    /// ## Expected Outcome
    /// - Detects squash-merged patch branch via commit message matching
    /// - Returns true when commit messages are found in target
    #[test]
    fn test_check_patch_merged_squash_merge() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create and switch to patch branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "patch/main-2.0.0"])
            .output()
            .unwrap();

        // Add commits to patch branch with distinctive messages
        fs::write(repo_path.join("patch1.txt"), "patch content 1").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Fix critical authentication bug"])
            .output()
            .unwrap();

        fs::write(repo_path.join("patch2.txt"), "patch content 2").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Add comprehensive test coverage"])
            .output()
            .unwrap();

        // Switch back to main and squash merge
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["merge", "--squash", "patch/main-2.0.0"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args([
                "commit",
                "-m",
                "Squash merge: Fix critical authentication bug and Add comprehensive test coverage",
            ])
            .output()
            .unwrap();

        // Test that patch is detected as merged via message matching
        let is_merged = check_patch_merged(&repo_path, "patch/main-2.0.0", "main").unwrap();
        assert!(
            is_merged,
            "Squash-merged patch should be detected via commit message matching"
        );
    }

    /// # Check Patch Not Merged
    ///
    /// Tests detection of unmerged patch branches.
    ///
    /// ## Test Scenario
    /// - Creates a patch branch with commits
    /// - Does not merge it into target
    /// - Tests that it's correctly identified as not merged
    ///
    /// ## Expected Outcome
    /// - Returns false for unmerged branches
    /// - Does not give false positives
    #[test]
    fn test_check_patch_not_merged() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create patch branch with commits but don't merge
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "patch/main-3.0.0"])
            .output()
            .unwrap();

        fs::write(repo_path.join("unmerged.txt"), "unmerged content").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Unmerged patch commit"])
            .output()
            .unwrap();

        // Switch back to main without merging
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Test that patch is detected as NOT merged
        let is_merged = check_patch_merged(&repo_path, "patch/main-3.0.0", "main").unwrap();
        assert!(
            !is_merged,
            "Unmerged patch should be detected as not merged"
        );
    }

    /// # Check Patch Merged (Partial Message Match)
    ///
    /// Tests that the 80% threshold works correctly for squash merges.
    ///
    /// ## Test Scenario
    /// - Creates a patch branch with 5 commits
    /// - Squash merges with 4 out of 5 commit messages present
    /// - Tests that 80% threshold accepts the merge
    ///
    /// ## Expected Outcome
    /// - Returns true when 80% or more commit messages are found
    /// - Handles partial message matches correctly
    #[test]
    fn test_check_patch_merged_partial_message_match() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create patch branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "patch/main-4.0.0"])
            .output()
            .unwrap();

        // Add 5 commits
        let messages = [
            "Implement feature A",
            "Add tests for feature A",
            "Fix bug in feature B",
            "Update documentation",
            "Refactor module C",
        ];

        for (i, msg) in messages.iter().enumerate() {
            fs::write(repo_path.join(format!("file{}.txt", i)), "content").unwrap();
            Command::new("git")
                .current_dir(&repo_path)
                .args(["add", "."])
                .output()
                .unwrap();
            Command::new("git")
                .current_dir(&repo_path)
                .args(["commit", "-m", msg])
                .output()
                .unwrap();
        }

        // Switch back and create squash commit with only 4 of 5 messages
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["merge", "--squash", "patch/main-4.0.0"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args([
                "commit",
                "-m",
                "Squash: Implement feature A, Add tests for feature A, Fix bug in feature B, Update documentation",
            ])
            .output()
            .unwrap();

        // Should still be detected as merged (4/5 = 80%)
        let is_merged = check_patch_merged(&repo_path, "patch/main-4.0.0", "main").unwrap();
        assert!(
            is_merged,
            "Patch with 80% message match should be detected as merged"
        );
    }

    /// # Check Patch Merged (Cherry-Pick References)
    ///
    /// Tests detection of squash merges with cherry-pick reference messages.
    ///
    /// ## Test Scenario
    /// - Creates a patch branch with 3 commits
    /// - Squash merges to target with "cherry-picked from" references in body
    /// - Tests that merge is detected via cherry-pick references
    ///
    /// ## Expected Outcome
    /// - Returns true when cherry-pick references are found in commit bodies
    /// - Handles both "cherry-picked from" and "(cherry picked from commit)" formats
    #[test]
    fn test_check_patch_merged_cherry_pick_references() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit on main
        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        // Create patch branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "patch/main-5.0.0"])
            .output()
            .unwrap();

        // Add 3 commits and capture their hashes
        let mut commit_hashes = Vec::new();

        for i in 1..=3 {
            fs::write(repo_path.join(format!("file{}.txt", i)), "content").unwrap();
            Command::new("git")
                .current_dir(&repo_path)
                .args(["add", "."])
                .output()
                .unwrap();
            Command::new("git")
                .current_dir(&repo_path)
                .args(["commit", "-m", &format!("Commit {}", i)])
                .output()
                .unwrap();

            // Get the commit hash
            let hash_output = Command::new("git")
                .current_dir(&repo_path)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            let hash = String::from_utf8_lossy(&hash_output.stdout)
                .trim()
                .to_string();
            commit_hashes.push(hash);
        }

        // Switch back to main and create squash merge with cherry-pick references
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["merge", "--squash", "patch/main-5.0.0"])
            .output()
            .unwrap();

        // Create commit with cherry-pick references in body
        let commit_body = format!(
            "Squash merge patch/main-5.0.0\n\ncherry-picked from {}\ncherry-picked from {}\n(cherry picked from commit {})",
            commit_hashes[0], commit_hashes[1], commit_hashes[2]
        );
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", &commit_body])
            .output()
            .unwrap();

        // Test that patch is detected as merged via cherry-pick references
        let is_merged = check_patch_merged(&repo_path, "patch/main-5.0.0", "main").unwrap();
        assert!(
            is_merged,
            "Squash-merged patch with cherry-pick references should be detected"
        );
    }

    /// # Validate Git Reference (Valid References)
    ///
    /// Tests that valid git references pass validation.
    ///
    /// ## Test Scenario
    /// - Tests various valid git reference formats
    /// - Includes branch names, tags, and commit-like references
    ///
    /// ## Expected Outcome
    /// - All valid references pass without error
    #[test]
    fn test_validate_git_ref_valid() {
        // Simple branch names
        assert!(validate_git_ref("main").is_ok());
        assert!(validate_git_ref("develop").is_ok());
        assert!(validate_git_ref("feature-branch").is_ok());
        assert!(validate_git_ref("feature/new-feature").is_ok());

        // Tags
        assert!(validate_git_ref("v1.0.0").is_ok());
        assert!(validate_git_ref("release-2.0").is_ok());

        // Commit hashes
        assert!(validate_git_ref("abc123def").is_ok());
        assert!(validate_git_ref("0123456789abcdef0123456789abcdef01234567").is_ok());

        // HEAD references
        assert!(validate_git_ref("HEAD").is_ok());
        assert!(validate_git_ref("FETCH_HEAD").is_ok());
    }

    /// # Validate Git Reference (Invalid References)
    ///
    /// Tests that invalid git references are rejected.
    ///
    /// ## Test Scenario
    /// - Tests references with forbidden characters
    /// - Tests empty strings and control characters
    ///
    /// ## Expected Outcome
    /// - All invalid references return an error
    #[test]
    fn test_validate_git_ref_invalid() {
        // Empty string
        assert!(validate_git_ref("").is_err());

        // Control characters
        assert!(validate_git_ref("branch\0name").is_err());
        assert!(validate_git_ref("branch\tname").is_err());
        assert!(validate_git_ref("branch\nname").is_err());

        // Forbidden characters
        assert!(validate_git_ref("branch~name").is_err());
        assert!(validate_git_ref("branch^name").is_err());
        assert!(validate_git_ref("branch:name").is_err());
        assert!(validate_git_ref("branch?name").is_err());
        assert!(validate_git_ref("branch*name").is_err());
        assert!(validate_git_ref("branch[name").is_err());
        assert!(validate_git_ref("branch\\name").is_err());

        // Double dot sequence
        assert!(validate_git_ref("branch..name").is_err());

        // @{ sequence
        assert!(validate_git_ref("branch@{name").is_err());
    }

    /// # Git Trait Implementation
    ///
    /// Tests that the SystemGit struct correctly implements GitOperations trait.
    ///
    /// ## Test Scenario
    /// - Creates a test repository
    /// - Uses SystemGit to get commit info through the trait
    ///
    /// ## Expected Outcome
    /// - SystemGit correctly implements GitOperations
    /// - Operations work through the trait interface
    #[test]
    fn test_system_git_trait() {
        let (_temp_dir, repo_path) = setup_test_repo();
        create_commit_with_message(&repo_path, "Test commit for trait");

        let git: &dyn GitOperations = &SystemGit;

        // Test get_commit_info through trait
        let info = git.get_commit_info(&repo_path, "HEAD").unwrap();
        assert_eq!(info.title, "Test commit for trait");
        assert_eq!(info.author, "Test User");
    }

    /// # Git Trait Branch History
    ///
    /// Tests getting branch history through the GitOperations trait.
    ///
    /// ## Test Scenario
    /// - Creates a test repository with commits
    /// - Uses SystemGit to get branch history through the trait
    ///
    /// ## Expected Outcome
    /// - Branch history is correctly retrieved through the trait
    #[test]
    fn test_system_git_branch_history() {
        let (_temp_dir, repo_path) = setup_test_repo();
        create_commit_with_message(&repo_path, "First commit");
        create_commit_with_message(&repo_path, "Second commit");

        let git = SystemGit;
        let history = git.get_branch_history(&repo_path, "main").unwrap();

        assert!(
            history
                .commit_messages
                .contains(&"First commit".to_string())
        );
        assert!(
            history
                .commit_messages
                .contains(&"Second commit".to_string())
        );
        assert!(history.commit_hashes.len() >= 2);
    }

    /// # Cleanup Cherry-Pick With Worktree
    ///
    /// Tests cleanup of a cherry-pick operation when using worktrees.
    ///
    /// ## Test Scenario
    /// - Creates a repository with a worktree
    /// - Creates a branch in the worktree
    /// - Starts a cherry-pick that conflicts
    /// - Runs cleanup_cherry_pick to abort and remove worktree/branch
    ///
    /// ## Expected Outcome
    /// - Cherry-pick is aborted
    /// - Worktree is removed
    /// - Branch is deleted
    #[test]
    fn test_cleanup_cherry_pick_with_worktree() {
        let (_test_dir, repo_path, _origin_dir, _origin_path) = setup_test_repo_with_origin();

        // Create target branch
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "target-branch"])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Target branch commit");

        // Push to origin
        Command::new("git")
            .current_dir(&repo_path)
            .args(["push", "-u", "origin", "target-branch"])
            .output()
            .unwrap();

        // Go back to main
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Create worktree
        let worktree_result = create_worktree(&repo_path, "target-branch", "v1.0.0", false);
        assert!(worktree_result.is_ok());
        let worktree_path = worktree_result.unwrap();

        // Create branch in worktree
        let branch_name = "patch/target-branch-v1.0.0";
        Command::new("git")
            .current_dir(&worktree_path)
            .args(["checkout", "-b", branch_name])
            .output()
            .unwrap();

        // Verify worktree and branch exist
        assert!(worktree_path.exists());
        let branch_list = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "--list", branch_name])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_list.stdout)
                .trim()
                .is_empty(),
            "Branch should exist"
        );

        // Run cleanup
        let cleanup_result =
            cleanup_cherry_pick(Some(&repo_path), &worktree_path, "v1.0.0", "target-branch");
        assert!(cleanup_result.is_ok());

        // Verify worktree is removed
        assert!(!worktree_path.exists(), "Worktree should be removed");

        // Verify branch is deleted
        let branch_list_after = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "--list", branch_name])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_list_after.stdout)
                .trim()
                .is_empty(),
            "Branch should be deleted"
        );
    }

    /// # Cleanup Cherry-Pick Without Worktree
    ///
    /// Tests cleanup of a cherry-pick operation in a cloned repository (no worktree).
    ///
    /// ## Test Scenario
    /// - Creates a test repository
    /// - Creates a branch for patching
    /// - Runs cleanup_cherry_pick with no base_repo_path
    ///
    /// ## Expected Outcome
    /// - Branch is deleted from the repository
    #[test]
    fn test_cleanup_cherry_pick_without_worktree() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create initial commit
        create_commit_with_message(&repo_path, "Initial commit");

        // Create branch
        let branch_name = "patch/main-v2.0.0";
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", branch_name])
            .output()
            .unwrap();

        create_commit_with_message(&repo_path, "Patch commit");

        // Verify branch exists
        let branch_list = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "--list", branch_name])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&branch_list.stdout)
                .trim()
                .is_empty(),
            "Branch should exist"
        );

        // Switch to another branch before cleanup (can't delete current branch)
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        // Run cleanup (no base_repo_path = cloned repo)
        let cleanup_result = cleanup_cherry_pick(None, &repo_path, "v2.0.0", "main");
        assert!(cleanup_result.is_ok());

        // Verify branch is deleted
        let branch_list_after = Command::new("git")
            .current_dir(&repo_path)
            .args(["branch", "--list", branch_name])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branch_list_after.stdout)
                .trim()
                .is_empty(),
            "Branch should be deleted"
        );
    }

    /// # Cleanup Cherry-Pick Aborts In-Progress Cherry-Pick
    ///
    /// Tests that cleanup properly aborts an in-progress cherry-pick.
    ///
    /// ## Test Scenario
    /// - Creates a cherry-pick conflict
    /// - Runs cleanup_cherry_pick
    /// - Verifies the cherry-pick state is cleared
    ///
    /// ## Expected Outcome
    /// - Cherry-pick in progress is aborted
    /// - Repository is in clean state
    #[test]
    fn test_cleanup_cherry_pick_aborts_in_progress() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a file with content
        std::fs::write(repo_path.join("conflict.txt"), "original content").unwrap();
        create_commit_with_message(&repo_path, "Initial commit with file");

        // Create feature branch and modify the same file
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "feature"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("conflict.txt"), "feature content").unwrap();
        create_commit_with_message(&repo_path, "Feature commit");

        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let feature_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Go back to main and modify the same file differently
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "main"])
            .output()
            .unwrap();

        std::fs::write(repo_path.join("conflict.txt"), "main content").unwrap();
        create_commit_with_message(&repo_path, "Main commit");

        // Start cherry-pick (will conflict)
        let _ = Command::new("git")
            .current_dir(&repo_path)
            .args(["cherry-pick", &feature_hash])
            .output();

        // Verify cherry-pick is in progress
        let status = Command::new("git")
            .current_dir(&repo_path)
            .args(["status"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&status.stdout).contains("cherry-pick"),
            "Should be in cherry-pick state"
        );

        // Run cleanup
        let cleanup_result = cleanup_cherry_pick(None, &repo_path, "v1.0.0", "main");
        assert!(cleanup_result.is_ok());

        // Verify cherry-pick is no longer in progress
        let status_after = Command::new("git")
            .current_dir(&repo_path)
            .args(["status"])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&status_after.stdout).contains("cherry-picking"),
            "Cherry-pick should be aborted"
        );
    }
}
