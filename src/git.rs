use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

pub fn shallow_clone_repo(ssh_url: &str, target_branch: &str) -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir().join(format!("azure-pr-cherry-pick-{}", Uuid::new_v4()));

    let output = Command::new("git")
        .args(&[
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "--branch",
            target_branch,
            "--no-tags",
            ssh_url,
            temp_dir.to_str().unwrap(),
        ])
        .output()
        .context("Failed to clone repository")?;

    if !output.status.success() {
        anyhow::bail!(
            "Git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(temp_dir)
}

pub fn create_worktree(
    base_repo_path: &Path,
    target_branch: &str,
    version: &str,
) -> Result<PathBuf> {
    let worktree_name = format!("next-{}", version);
    let worktree_path = base_repo_path.join(&worktree_name);

    // Check if worktree already exists and remove it
    let list_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(&["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to list worktrees")?;

    if !list_output.status.success() {
        anyhow::bail!(
            "Failed to list worktrees: {}",
            String::from_utf8_lossy(&list_output.stderr)
        );
    }

    let worktree_list = String::from_utf8_lossy(&list_output.stdout);
    if worktree_list.contains(&worktree_name) {
        let remove_output = Command::new("git")
            .current_dir(base_repo_path)
            .args(&["worktree", "remove", "--force", &worktree_name])
            .output()
            .context("Failed to remove existing worktree")?;

        if !remove_output.status.success() {
            let prune_output = Command::new("git")
                .current_dir(base_repo_path)
                .args(&["worktree", "prune"])
                .output()?;

            if !prune_output.status.success() {
                // Just continue
            }

            if worktree_path.exists() {
                std::fs::remove_dir_all(&worktree_path)
                    .context("Failed to remove existing worktree directory")?;
            }
        }
    }

    if worktree_path.exists() {
        std::fs::remove_dir_all(&worktree_path).context("Failed to remove existing directory")?;
    }

    let fetch_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(&["fetch", "origin", target_branch])
        .output()
        .context("Failed to fetch target branch")?;

    if !fetch_output.status.success() {
        anyhow::bail!(
            "Failed to fetch target branch: {}",
            String::from_utf8_lossy(&fetch_output.stderr)
        );
    }

    let create_output = Command::new("git")
        .current_dir(base_repo_path)
        .args(&[
            "worktree",
            "add",
            worktree_path.to_str().unwrap(),
            &format!("origin/{}", target_branch),
        ])
        .output()
        .context("Failed to create worktree")?;

    if !create_output.status.success() {
        anyhow::bail!(
            "Failed to create worktree: {}",
            String::from_utf8_lossy(&create_output.stderr)
        );
    }

    Ok(worktree_path)
}

pub fn setup_repository(
    local_repo: Option<&str>,
    ssh_url: &str,
    target_branch: &str,
    version: &str,
) -> Result<PathBuf> {
    match local_repo {
        Some(repo_path) => {
            let repo_path = Path::new(repo_path);
            if !repo_path.exists() {
                anyhow::bail!("Local repository path does not exist: {:?}", repo_path);
            }

            let verify_output = Command::new("git")
                .current_dir(repo_path)
                .args(&["rev-parse", "--git-dir"])
                .output()
                .context("Failed to verify git repository")?;

            if !verify_output.status.success() {
                anyhow::bail!("Not a valid git repository: {:?}", repo_path);
            }

            create_worktree(repo_path, target_branch, version)
        }
        None => shallow_clone_repo(ssh_url, target_branch),
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
        .args(&["cherry-pick", commit_id])
        .output()
        .context("Failed to execute cherry-pick command")?;

    if output.status.success() {
        return Ok(CherryPickResult::Success);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    if stderr.contains("conflict") || stderr.contains("CONFLICT") {
        let status_output = Command::new("git")
            .current_dir(repo_path)
            .args(&["diff", "--name-only", "--diff-filter=U"])
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
        .args(&["checkout", "-b", branch_name])
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
            .args(&["fetch", "--depth=1", "origin", commit_id])
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
        .args(&["ls-files", "-u"])
        .output()?;

    Ok(output.stdout.is_empty())
}

pub fn continue_cherry_pick(repo_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(&["cherry-pick", "--continue"])
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
        .args(&["cherry-pick", "--abort"])
        .output()?;

    Ok(())
}
