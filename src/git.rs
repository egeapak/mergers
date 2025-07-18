use anyhow::{Context, Result};
use std::{
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

use crate::models::SymmetricDiffResult;

pub fn get_symmetric_difference(
    repo_path: &Path,
    dev_branch: &str,
    target_branch: &str,
) -> Result<SymmetricDiffResult> {
    // Get commits in dev branch but not in target branch
    let dev_not_target = get_commits_in_branch_not_in_target(repo_path, dev_branch, target_branch)?;

    // Get commits in target branch but not in dev branch
    let target_not_dev = get_commits_in_branch_not_in_target(repo_path, target_branch, dev_branch)?;

    // Get common commits using merge-base
    let common_commits = get_common_commits(repo_path, dev_branch, target_branch)?;

    Ok(SymmetricDiffResult {
        commits_in_dev_not_target: dev_not_target,
        commits_in_target_not_dev: target_not_dev,
        common_commits,
    })
}

pub fn get_commits_in_branch_not_in_target(
    repo_path: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Result<Vec<String>> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "rev-list",
            "--reverse",
            &format!("{}..{}", target_branch, source_branch),
        ])
        .output()
        .context("Failed to get commits difference")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get commits difference: {}", stderr);
    }

    let commits = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(commits)
}

pub fn check_commit_exists_in_branch(
    repo_path: &Path,
    commit_id: &str,
    branch: &str,
) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["merge-base", "--is-ancestor", commit_id, branch])
        .output()
        .context("Failed to check if commit exists in branch")?;

    Ok(output.status.success())
}

pub fn get_common_commits(repo_path: &Path, branch1: &str, branch2: &str) -> Result<Vec<String>> {
    let merge_base_output = Command::new("git")
        .current_dir(repo_path)
        .args(["merge-base", branch1, branch2])
        .output()
        .context("Failed to get merge base")?;

    if !merge_base_output.status.success() {
        return Ok(Vec::new());
    }

    let merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
        .trim()
        .to_string();

    if merge_base.is_empty() {
        return Ok(Vec::new());
    }

    // Get all commits from merge base to both branches
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "rev-list",
            "--reverse",
            &format!("{}..{}", merge_base, branch1),
        ])
        .output()
        .context("Failed to get common commits")?;

    if !output.status.success() {
        return Ok(vec![merge_base]);
    }

    let mut commits = vec![merge_base];
    commits.extend(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty()),
    );

    Ok(commits)
}

pub fn check_pr_merged_in_branch(
    repo_path: &Path,
    pr_id: i32,
    pr_title: &str,
    branch: &str,
) -> Result<bool> {
    // Get all commit messages from the target branch (last 1000 commits for performance)
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", "-n", "1000", branch])
        .output()
        .context("Failed to get commit messages from branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to get commit messages from branch: {}", stderr);
    }

    let commit_messages = String::from_utf8_lossy(&output.stdout);

    // Check for the Azure DevOps merge pattern: "Merged PR <PR ID>: <Original PR title>"
    let expected_prefix = format!("Merged PR {}: ", pr_id);

    for commit_message in commit_messages.lines() {
        if commit_message.starts_with(&expected_prefix) {
            // Extract the title part after the prefix
            let commit_title_part = &commit_message[expected_prefix.len()..];

            // Normalize both titles for comparison
            let normalized_commit_title = normalize_title(commit_title_part);
            let normalized_pr_title = normalize_title(pr_title);

            // Check if the titles match (allowing for some normalization differences)
            if normalized_commit_title == normalized_pr_title {
                return Ok(true);
            }

            // Fallback to fuzzy match if exact match fails
            if fuzzy_title_match(&normalized_pr_title, &normalized_commit_title) {
                return Ok(true);
            }
        }
    }

    Ok(false)
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

fn fuzzy_title_match(pr_title: &str, commit_message: &str) -> bool {
    // Split into words and check if significant words match
    let pr_words: Vec<&str> = pr_title
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();
    let commit_words: Vec<&str> = commit_message
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();

    if pr_words.is_empty() || commit_words.is_empty() {
        return false;
    }

    // Check if at least 60% of significant words match
    let matching_words = pr_words
        .iter()
        .filter(|&&pr_word| {
            commit_words
                .iter()
                .any(|&commit_word| commit_word.contains(pr_word) || pr_word.contains(commit_word))
        })
        .count();

    let match_ratio = matching_words as f64 / pr_words.len() as f64;
    match_ratio >= 0.6
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
            if let Some(dir_name) = std::path::Path::new(path).file_name() {
                if let Some(name) = dir_name.to_str() {
                    if name.starts_with("next-migration-") {
                        migration_worktrees.push(name.strip_prefix("next-").unwrap().to_string());
                    }
                }
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
        Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .unwrap();

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

        (temp_dir, repo_path)
    }

    fn create_commit_with_message(repo_path: &Path, message: &str) {
        // Create a test file with unique content based on message
        let content = format!("test content for: {}", message);
        fs::write(repo_path.join("test.txt"), content).unwrap();

        // Add and commit
        Command::new("git")
            .current_dir(repo_path)
            .args(["add", "."])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["commit", "-m", message])
            .output()
            .unwrap();
    }

    #[test]
    fn test_check_pr_merged_in_branch_exact_match() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a commit with Azure DevOps merge pattern
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");

        // Test exact match
        let result =
            check_pr_merged_in_branch(&repo_path, 123, "Fix authentication bug", "HEAD").unwrap();

        assert!(result, "Should find PR with exact title match");
    }

    #[test]
    fn test_check_pr_merged_in_branch_normalized_match() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a commit with Azure DevOps merge pattern and different casing
        create_commit_with_message(&repo_path, "Merged PR 456: Fix: Authentication Bug");

        // Test with different normalization
        let result =
            check_pr_merged_in_branch(&repo_path, 456, "fix authentication bug", "HEAD").unwrap();

        assert!(result, "Should find PR with normalized title match");
    }

    #[test]
    fn test_check_pr_merged_in_branch_fuzzy_match() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a commit with Azure DevOps merge pattern and slightly different title
        create_commit_with_message(
            &repo_path,
            "Merged PR 789: Fix authentication issue in login module",
        );

        // Test with fuzzy match
        let result =
            check_pr_merged_in_branch(&repo_path, 789, "Fix authentication issue", "HEAD").unwrap();

        assert!(result, "Should find PR with fuzzy title match");
    }

    #[test]
    fn test_check_pr_merged_in_branch_wrong_id() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a commit with Azure DevOps merge pattern
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");

        // Test with wrong PR ID
        let result =
            check_pr_merged_in_branch(&repo_path, 456, "Fix authentication bug", "HEAD").unwrap();

        assert!(!result, "Should not find PR with wrong ID");
    }

    #[test]
    fn test_check_pr_merged_in_branch_no_merge_pattern() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create a commit without Azure DevOps merge pattern
        create_commit_with_message(&repo_path, "Fix authentication bug");

        // Test with no merge pattern
        let result =
            check_pr_merged_in_branch(&repo_path, 123, "Fix authentication bug", "HEAD").unwrap();

        assert!(!result, "Should not find PR without merge pattern");
    }

    #[test]
    fn test_check_pr_merged_in_branch_multiple_commits() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create multiple commits
        create_commit_with_message(&repo_path, "Initial commit");
        create_commit_with_message(&repo_path, "Merged PR 111: Add feature A");
        create_commit_with_message(&repo_path, "Merged PR 222: Fix bug B");
        create_commit_with_message(&repo_path, "Regular commit");

        // Test finding the right PR among multiple
        let result = check_pr_merged_in_branch(&repo_path, 222, "Fix bug B", "HEAD").unwrap();

        assert!(result, "Should find specific PR among multiple commits");
    }
}
