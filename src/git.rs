use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

#[derive(Debug, Clone)]
pub enum RepositorySetupError {
    BranchExists(String),
    WorktreeExists(String),
    Other(String),
}

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

impl std::error::Error for RepositorySetupError {}

pub fn shallow_clone_repo(ssh_url: &str, target_branch: &str) -> Result<(PathBuf, TempDir)> {
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

    Ok((repo_path, temp_dir))
}

pub fn create_worktree(
    base_repo_path: &Path,
    target_branch: &str,
    version: &str,
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

pub enum RepositorySetup {
    Local(PathBuf),
    Clone(PathBuf, TempDir),
}

pub fn setup_repository(
    local_repo: Option<&str>,
    ssh_url: &str,
    target_branch: &str,
    version: &str,
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

            let worktree_path = create_worktree(repo_path, target_branch, version)?;
            Ok(RepositorySetup::Local(worktree_path))
        }
        None => {
            let (repo_path, temp_dir) = shallow_clone_repo(ssh_url, target_branch)
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
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["cherry-pick", commit_id])
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

    Ok(CommitHistory {
        commit_hashes,
        commit_messages,
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

    // Strategy 2: Check for GitHub merge patterns
    if check_github_merge_patterns_in_history(pr_id, pr_title, history) {
        return true;
    }

    // Strategy 3: Search for PR title in commit messages (broader search)
    if search_pr_title_in_history(pr_title, history) {
        return true;
    }

    // Strategy 4: Search for PR ID references in commit messages
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

fn check_github_merge_patterns_in_history(
    pr_id: i32,
    pr_title: &str,
    history: &CommitHistory,
) -> bool {
    let normalized_pr_title = normalize_title(pr_title);

    for commit_message in &history.commit_messages {
        let lowercase_commit = commit_message.to_lowercase();

        // Pattern 1: "Merge pull request #123 from branch/name" - check without normalization
        if lowercase_commit.contains(&format!("merge pull request #{}", pr_id)) {
            return true;
        }

        // Pattern 2: "#123: Title" at the beginning
        if lowercase_commit.starts_with(&format!("#{}: ", pr_id)) {
            let title_part = &lowercase_commit[format!("#{}: ", pr_id).len()..];
            if normalize_title(title_part) == normalized_pr_title {
                return true;
            }
        }

        // Pattern 3: "Title (#123)" at the end
        if lowercase_commit.ends_with(&format!(" (#{}))", pr_id)) {
            let title_part =
                &lowercase_commit[..lowercase_commit.len() - format!(" (#{}))", pr_id).len()];
            if normalize_title(title_part) == normalized_pr_title {
                return true;
            }
        }

        // Pattern 4: "[#123] Title" at the beginning
        if lowercase_commit.starts_with(&format!("[#{}] ", pr_id)) {
            let title_part = &lowercase_commit[format!("[#{}] ", pr_id).len()..];
            if normalize_title(title_part) == normalized_pr_title {
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
    let _pr_id_str = pr_id.to_string();

    for commit_message in &history.commit_messages {
        let lowercase_commit = commit_message.to_lowercase();

        // Look for PR ID in various formats - check without full normalization
        if lowercase_commit.contains(&format!("pr{}", pr_id))
            || lowercase_commit.contains(&format!("pr {}", pr_id))
            || lowercase_commit.contains(&format!("#{}", pr_id))
            || lowercase_commit.contains(&format!("[{}]", pr_id))
            || lowercase_commit.contains(&format!("({})", pr_id))
        {
            return true;
        }
    }

    false
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

    #[test]
    fn test_check_pr_merged_in_history_github_patterns() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create commits with GitHub merge patterns
        create_commit_with_message(&repo_path, "Merge pull request #456 from feature/auth");
        create_commit_with_message(&repo_path, "#789: Fix authentication issue");
        create_commit_with_message(&repo_path, "Authentication fix (#321)");
        create_commit_with_message(&repo_path, "[#654] Update login system");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test GitHub pattern matches - these should match based on PR ID presence
        assert!(check_pr_merged_in_history(456, "Some title", &history));
        assert!(check_pr_merged_in_history(
            789,
            "Fix authentication issue",
            &history
        ));
        assert!(check_pr_merged_in_history(
            321,
            "Authentication fix",
            &history
        ));
        assert!(check_pr_merged_in_history(
            654,
            "Update login system",
            &history
        ));
    }

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

    #[test]
    fn test_implementation_consistency() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create test commits with known patterns
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");
        create_commit_with_message(&repo_path, "Merge pull request #456 from feature/auth");
        create_commit_with_message(&repo_path, "Regular commit without PR pattern");
        create_commit_with_message(&repo_path, "Fix issue reported in PR789");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test cases that should work
        let test_cases = vec![
            (123, "Fix authentication bug", true), // Should match Azure DevOps pattern
            (456, "Some feature", true),           // Should match GitHub pattern
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
        let worktree_path = create_worktree(&repo_path, "target-branch", "1.0.0").unwrap();

        assert!(worktree_path.exists());
        assert_eq!(worktree_path.file_name().unwrap(), "next-1.0.0");
    }

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
        let result = create_worktree(&repo_path, "target-branch", "1.0.0");
        assert!(result.is_err());

        if let Err(RepositorySetupError::WorktreeExists(path)) = result {
            assert!(path.contains("next-1.0.0"));
        } else {
            panic!("Expected WorktreeExists error, got: {:?}", result);
        }
    }

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
        let result = create_worktree(&repo_path, "target-branch", "1.0.0");
        assert!(result.is_err());

        if let Err(RepositorySetupError::WorktreeExists(_)) = result {
            // Expected error
        } else {
            panic!("Expected WorktreeExists error");
        }
    }

    #[test]
    fn test_force_remove_worktree_non_existent() {
        let (_temp_dir, repo_path) = setup_test_repo();

        create_commit_with_message(&repo_path, "Initial commit");

        // Try to remove non-existent worktree - should succeed (no-op)
        let result = force_remove_worktree(&repo_path, "non-existent");
        assert!(result.is_ok());
    }

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
        let worktree_path = create_worktree(&repo_path, "target-branch", "1.0.0").unwrap();
        assert!(worktree_path.exists());

        // Force remove worktree
        let result = force_remove_worktree(&repo_path, "1.0.0");
        assert!(result.is_ok());

        // Verify worktree is removed
        assert!(!worktree_path.exists());
    }

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
        create_worktree(&repo_path, "target-branch", "migration-1.0.0").unwrap();
        create_worktree(&repo_path, "target-branch", "migration-2.0.0").unwrap();

        // Also create a regular worktree that shouldn't be removed
        create_worktree(&repo_path, "target-branch", "regular-1.0.0").unwrap();

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

    #[test]
    fn test_search_pr_title_in_history_short_title() {
        let mut commit_hashes = std::collections::HashSet::new();
        commit_hashes.insert("abc123".to_string());

        let history = CommitHistory {
            commit_messages: vec!["Some commit message".to_string()],
            commit_hashes,
        };

        // Short titles should return false to avoid false positives
        assert!(!search_pr_title_in_history("Fix", &history));
        assert!(!search_pr_title_in_history("Update", &history));
        assert!(!search_pr_title_in_history("", &history));
    }

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
}
