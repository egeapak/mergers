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

use crate::models::SymmetricDiffResult;

pub fn get_symmetric_difference(
    repo_path: &Path,
    dev_branch: &str,
    target_branch: &str,
) -> Result<SymmetricDiffResult> {
    // Get commits in dev branch but not in target branch using git log with cherry-pick
    let git_cmd = format!(
        "git log {}...{} --cherry-pick --right-only --oneline --no-decorate --no-merges",
        target_branch, dev_branch
    );
    let dev_not_target_output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "log",
            &format!("{}...{}", target_branch, dev_branch),
            "--cherry-pick",
            "--right-only",
            "--oneline",
            "--no-decorate",
            "--no-merges",
        ])
        .output()
        .context("Failed to get commits in dev not in target")?;

    if !dev_not_target_output.status.success() {
        let stderr = String::from_utf8_lossy(&dev_not_target_output.stderr);
        let stdout = String::from_utf8_lossy(&dev_not_target_output.stdout);
        anyhow::bail!(
            "Failed to get commits in '{}' not in '{}'. Command: '{}' (in {:?}). Git stderr: '{}'. Git stdout: '{}'",
            dev_branch,
            target_branch,
            git_cmd,
            repo_path,
            stderr,
            stdout
        );
    }

    let dev_not_target: Vec<String> = String::from_utf8_lossy(&dev_not_target_output.stdout)
        .lines()
        .map(|line| {
            // Extract commit hash from the oneline format (first part before space)
            line.split_whitespace().next().unwrap_or("").to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();

    // Get commits in target branch but not in dev branch
    let git_cmd2 = format!(
        "git log {}...{} --cherry-pick --right-only --oneline --no-decorate --no-merges",
        dev_branch, target_branch
    );
    let target_not_dev_output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "log",
            &format!("{}...{}", dev_branch, target_branch),
            "--cherry-pick",
            "--right-only",
            "--oneline",
            "--no-decorate",
            "--no-merges",
        ])
        .output()
        .context("Failed to get commits in target not in dev")?;

    if !target_not_dev_output.status.success() {
        let stderr = String::from_utf8_lossy(&target_not_dev_output.stderr);
        let stdout = String::from_utf8_lossy(&target_not_dev_output.stdout);
        anyhow::bail!(
            "Failed to get commits in '{}' not in '{}'. Command: '{}' (in {:?}). Git stderr: '{}'. Git stdout: '{}'",
            target_branch,
            dev_branch,
            git_cmd2,
            repo_path,
            stderr,
            stdout
        );
    }

    let target_not_dev: Vec<String> = String::from_utf8_lossy(&target_not_dev_output.stdout)
        .lines()
        .map(|line| {
            // Extract commit hash from the oneline format (first part before space)
            line.split_whitespace().next().unwrap_or("").to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();

    // Get common commits using merge-base
    let common_commits = get_common_commits(repo_path, dev_branch, target_branch)?;

    Ok(SymmetricDiffResult {
        commits_in_dev_not_target: dev_not_target,
        commits_in_target_not_dev: target_not_dev,
        common_commits,
    })
}

/// Check if a PR's commit is already present in the target branch
/// by getting commits that are in dev but not in target and checking if
/// the PR's last merge commit is in that list
pub fn is_pr_commit_already_in_target(
    repo_path: &Path,
    pr_commit_id: &str,
    dev_branch: &str,
    target_branch: &str,
) -> Result<bool> {
    let git_cmd = format!(
        "git log {}...{} --cherry-pick --right-only --oneline --no-decorate --no-merges",
        target_branch, dev_branch
    );
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "log",
            &format!("{}...{}", target_branch, dev_branch),
            "--cherry-pick",
            "--right-only",
            "--oneline",
            "--no-decorate",
            "--no-merges",
        ])
        .output()
        .context("Failed to get commits in dev not in target")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to check if PR commit is in target. Command: '{}' (in {:?}). Git stderr: '{}'. Git stdout: '{}'",
            git_cmd,
            repo_path,
            stderr,
            stdout
        );
    }

    let commits_only_in_dev: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            // Extract commit hash from the oneline format (first part before space)
            line.split_whitespace().next().unwrap_or("").to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();

    // If the PR's commit is NOT in the list of commits only in dev,
    // then it means it's already in target (or equivalent changes are in target)
    Ok(!commits_only_in_dev.contains(&pr_commit_id.to_string()))
}

/// Filter a list of PRs to only include those whose commits are not already in the target branch
/// This function takes a list of PRs and returns only those that haven't been merged into target yet
pub fn filter_prs_not_in_target(
    repo_path: &Path,
    prs: Vec<crate::models::PullRequest>,
    dev_branch: &str,
    target_branch: &str,
) -> Result<Vec<crate::models::PullRequest>> {
    // First, get all commits that are only in dev but not in target
    let git_cmd = format!(
        "git log {}...{} --cherry-pick --right-only --oneline --no-decorate --no-merges",
        target_branch, dev_branch
    );
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "log",
            &format!("{}...{}", target_branch, dev_branch),
            "--cherry-pick",
            "--right-only",
            "--oneline",
            "--no-decorate",
            "--no-merges",
        ])
        .output()
        .context("Failed to get commits in dev not in target for filtering")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to get commits for PR filtering. Command: '{}' (in {:?}). Git stderr: '{}'. Git stdout: '{}'",
            git_cmd,
            repo_path,
            stderr,
            stdout
        );
    }

    let commits_only_in_dev: std::collections::HashSet<String> =
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| {
                // Extract commit hash from the oneline format (first part before space)
                line.split_whitespace().next().unwrap_or("").to_string()
            })
            .filter(|line| !line.is_empty())
            .collect();

    // Filter PRs: only keep those whose last merge commit is in the commits_only_in_dev set
    let filtered_prs: Vec<crate::models::PullRequest> = prs
        .into_iter()
        .filter(|pr| {
            if let Some(ref last_merge_commit) = pr.last_merge_commit {
                // If the PR's commit is in the list of commits only in dev,
                // then it hasn't been merged to target yet
                commits_only_in_dev.contains(&last_merge_commit.commit_id)
            } else {
                // If there's no last merge commit, include it to be safe
                true
            }
        })
        .collect();

    Ok(filtered_prs)
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
    let git_cmd = format!("git merge-base {} {}", branch1, branch2);
    let merge_base_output = Command::new("git")
        .current_dir(repo_path)
        .args(["merge-base", branch1, branch2])
        .output()
        .context("Failed to get merge base")?;

    if !merge_base_output.status.success() {
        let stderr = String::from_utf8_lossy(&merge_base_output.stderr);
        let stdout = String::from_utf8_lossy(&merge_base_output.stdout);
        anyhow::bail!(
            "Failed to get merge base between '{}' and '{}'. Command: '{}' (in {:?}). Git stderr: '{}'. Git stdout: '{}'",
            branch1,
            branch2,
            git_cmd,
            repo_path,
            stderr,
            stdout
        );
    }

    let merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
        .trim()
        .to_string();

    if merge_base.is_empty() {
        return Ok(Vec::new());
    }

    // Get all commits from merge base to both branches
    let git_cmd2 = format!("git rev-list --reverse {}..{}", merge_base, branch1);
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
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to get commits from merge base '{}' to '{}'. Command: '{}' (in {:?}). Git stderr: '{}'. Git stdout: '{}'",
            merge_base,
            branch1,
            git_cmd2,
            repo_path,
            stderr,
            stdout
        );
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
    // Use comprehensive PR detection with multiple strategies
    check_pr_merged_comprehensive(repo_path, pr_id, pr_title, branch)
}

pub fn check_pr_merged_comprehensive(
    repo_path: &Path,
    pr_id: i32,
    pr_title: &str,
    branch: &str,
) -> Result<bool> {
    // Strategy 1: Check for Azure DevOps merge pattern (most common)
    if check_azure_devops_merge_pattern(repo_path, pr_id, pr_title, branch)? {
        return Ok(true);
    }

    // Strategy 2: Check for GitHub merge patterns
    if check_github_merge_patterns(repo_path, pr_id, pr_title, branch)? {
        return Ok(true);
    }

    // Strategy 3: Search for PR title in commit messages (broader search)
    if search_pr_title_in_commits(repo_path, pr_title, branch)? {
        return Ok(true);
    }

    // Strategy 4: Search for PR ID references in commit messages
    if search_pr_id_in_commits(repo_path, pr_id, branch)? {
        return Ok(true);
    }

    Ok(false)
}

fn check_azure_devops_merge_pattern(
    repo_path: &Path,
    pr_id: i32,
    pr_title: &str,
    branch: &str,
) -> Result<bool> {
    // Get all commit messages from the target branch (increased limit for better coverage)
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", "-n", "5000", branch])
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

fn check_github_merge_patterns(
    repo_path: &Path,
    pr_id: i32,
    pr_title: &str,
    branch: &str,
) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", "-n", "5000", branch])
        .output()
        .context("Failed to get commit messages from branch")?;

    if !output.status.success() {
        return Ok(false);
    }

    let commit_messages = String::from_utf8_lossy(&output.stdout);
    let normalized_pr_title = normalize_title(pr_title);

    // Common GitHub merge patterns
    let patterns = [
        format!("Merge pull request #{} from", pr_id),
        format!("#{}: {}", pr_id, pr_title),
        format!("(#{}) {}", pr_id, pr_title),
        format!("[{}] {}", pr_id, pr_title),
    ];

    for commit_message in commit_messages.lines() {
        let normalized_commit = normalize_title(commit_message);

        // Check for direct pattern matches
        for pattern in &patterns {
            if commit_message.contains(pattern)
                || commit_message.contains(&normalize_title(pattern))
            {
                return Ok(true);
            }
        }

        // Check if commit contains both PR ID and title
        if normalized_commit.contains(&pr_id.to_string())
            && fuzzy_title_match(&normalized_pr_title, &normalized_commit)
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn search_pr_title_in_commits(repo_path: &Path, pr_title: &str, branch: &str) -> Result<bool> {
    // If title is too short or generic, skip this search to avoid false positives
    if pr_title.len() < 10 {
        return Ok(false);
    }

    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", "-n", "5000", branch])
        .output()
        .context("Failed to get commit messages from branch")?;

    if !output.status.success() {
        return Ok(false);
    }

    let commit_messages = String::from_utf8_lossy(&output.stdout);
    let normalized_pr_title = normalize_title(pr_title);

    // Split title into significant words for better matching
    let pr_words: Vec<&str> = normalized_pr_title
        .split_whitespace()
        .filter(|w| w.len() > 3 && !is_common_word(w))
        .collect();

    if pr_words.len() < 2 {
        return Ok(false);
    }

    for commit_message in commit_messages.lines() {
        let normalized_commit = normalize_title(commit_message);

        // Check if commit contains most of the significant words from PR title
        let matching_words = pr_words
            .iter()
            .filter(|&&word| normalized_commit.contains(word))
            .count();

        // Require at least 70% of significant words to match
        if matching_words as f64 / pr_words.len() as f64 >= 0.7 {
            return Ok(true);
        }
    }

    Ok(false)
}

fn search_pr_id_in_commits(repo_path: &Path, pr_id: i32, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["log", "--format=%s", "-n", "5000", branch])
        .output()
        .context("Failed to get commit messages from branch")?;

    if !output.status.success() {
        return Ok(false);
    }

    let commit_messages = String::from_utf8_lossy(&output.stdout);
    let pr_id_str = pr_id.to_string();

    // Look for PR ID in various formats
    let id_patterns = [
        format!("#{}", pr_id),
        format!("PR{}", pr_id),
        format!("PR {}", pr_id),
        format!("pr{}", pr_id),
        format!("pr {}", pr_id),
        format!("({})", pr_id),
        format!("[{}]", pr_id),
    ];

    for commit_message in commit_messages.lines() {
        // Check for exact PR ID patterns
        for pattern in &id_patterns {
            if commit_message.contains(pattern) {
                return Ok(true);
            }
        }

        // Check for standalone PR ID (with word boundaries)
        if commit_message.contains(&pr_id_str) {
            // Ensure it's not part of a larger number
            let words: Vec<&str> = commit_message.split_whitespace().collect();
            for word in words {
                let clean_word = word.trim_matches(|c: char| !c.is_alphanumeric());
                if clean_word == pr_id_str {
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
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

        // Set default branch to main
        Command::new("git")
            .current_dir(&repo_path)
            .args(["checkout", "-b", "main"])
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

    #[test]
    fn test_check_pr_merged_github_patterns() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Test GitHub merge patterns
        create_commit_with_message(&repo_path, "Merge pull request #456 from feature/auth");
        create_commit_with_message(&repo_path, "#789: Fix authentication issue");
        create_commit_with_message(&repo_path, "Authentication fix (#321)");
        create_commit_with_message(&repo_path, "[#654] Update login system");

        assert!(check_pr_merged_in_branch(&repo_path, 456, "Add authentication", "HEAD").unwrap());
        assert!(
            check_pr_merged_in_branch(&repo_path, 789, "Fix authentication issue", "HEAD").unwrap()
        );
        assert!(check_pr_merged_in_branch(&repo_path, 321, "Authentication fix", "HEAD").unwrap());
        assert!(check_pr_merged_in_branch(&repo_path, 654, "Update login system", "HEAD").unwrap());
    }

    #[test]
    fn test_check_pr_merged_title_search() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Test title-based search without explicit PR patterns
        create_commit_with_message(
            &repo_path,
            "Fix critical authentication vulnerability in login module",
        );
        create_commit_with_message(
            &repo_path,
            "Update user authentication system for better security",
        );

        assert!(
            check_pr_merged_in_branch(
                &repo_path,
                999,
                "Fix authentication vulnerability login",
                "HEAD"
            )
            .unwrap()
        );
        assert!(
            check_pr_merged_in_branch(
                &repo_path,
                888,
                "Update authentication system security",
                "HEAD"
            )
            .unwrap()
        );
    }

    #[test]
    fn test_check_pr_merged_id_search() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Test PR ID search in various formats
        create_commit_with_message(&repo_path, "Fix issue reported in PR123");
        create_commit_with_message(&repo_path, "Addresses feedback from pr 456");
        create_commit_with_message(&repo_path, "Related to #789 discussion");
        create_commit_with_message(&repo_path, "Implements feature [321] as requested");

        assert!(check_pr_merged_in_branch(&repo_path, 123, "Some title", "HEAD").unwrap());
        assert!(check_pr_merged_in_branch(&repo_path, 456, "Another title", "HEAD").unwrap());
        assert!(check_pr_merged_in_branch(&repo_path, 789, "Different title", "HEAD").unwrap());
        assert!(check_pr_merged_in_branch(&repo_path, 321, "Feature title", "HEAD").unwrap());
    }

    #[test]
    fn test_check_pr_merged_false_positives() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Test that we don't get false positives with common words or short titles
        create_commit_with_message(&repo_path, "Fix the bug");
        create_commit_with_message(&repo_path, "Update");
        create_commit_with_message(&repo_path, "Version 1.2.3 release");

        assert!(!check_pr_merged_in_branch(&repo_path, 999, "Fix", "HEAD").unwrap());
        assert!(!check_pr_merged_in_branch(&repo_path, 888, "Update", "HEAD").unwrap());
        assert!(!check_pr_merged_in_branch(&repo_path, 123, "Bug", "HEAD").unwrap());
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
    fn test_old_vs_new_implementation_consistency() {
        let (_temp_dir, repo_path) = setup_test_repo();

        // Create test commits with known patterns
        create_commit_with_message(&repo_path, "Merged PR 123: Fix authentication bug");
        create_commit_with_message(&repo_path, "Merge pull request #456 from feature/auth");
        create_commit_with_message(&repo_path, "Regular commit without PR pattern");
        create_commit_with_message(&repo_path, "Fix issue reported in PR789");

        let history = get_target_branch_history(&repo_path, "main").unwrap();

        // Test cases that should work with both implementations
        let test_cases = vec![
            (123, "Fix authentication bug", true), // Should match Azure DevOps pattern
            (456, "Some feature", true),           // Should match GitHub pattern
            (789, "Any title", true),              // Should match PR ID reference
            (999, "Non-existent PR", false),       // Should not match
        ];

        for (pr_id, pr_title, expected) in test_cases {
            // Test new implementation
            let new_result = check_pr_merged_in_history(pr_id, pr_title, &history);

            // Test old implementation
            let old_result =
                check_pr_merged_in_branch(&repo_path, pr_id, pr_title, "main").unwrap_or(false);

            // Both should give the same result
            assert_eq!(
                new_result, expected,
                "New implementation failed for PR {} with title '{}'. Expected {}, got {}",
                pr_id, pr_title, expected, new_result
            );

            assert_eq!(
                old_result, expected,
                "Old implementation failed for PR {} with title '{}'. Expected {}, got {}",
                pr_id, pr_title, expected, old_result
            );

            // Most importantly, both implementations should agree
            assert_eq!(
                new_result, old_result,
                "Implementation mismatch for PR {} with title '{}'. New: {}, Old: {}",
                pr_id, pr_title, new_result, old_result
            );
        }
    }
}
