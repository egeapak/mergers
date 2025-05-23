use anyhow::{Context, Result};
use dialoguer::Confirm;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

pub fn shallow_clone_repo(ssh_url: &str, target_branch: &str) -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir().join(format!("azure-pr-cherry-pick-{}", Uuid::new_v4()));

    println!(
        "Shallow cloning repository from {} to {:?}...",
        ssh_url, temp_dir
    );

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

    println!("Clone completed successfully!");
    Ok(temp_dir)
}

pub fn create_worktree(
    base_repo_path: &Path,
    target_branch: &str,
    version: &str,
) -> Result<PathBuf> {
    let worktree_name = format!("next-{}", version);
    let worktree_path = base_repo_path.join(&worktree_name);

    println!("Creating git worktree at {:?}...", worktree_path);

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
        println!("Removing existing worktree: {}", worktree_name);

        // Remove the worktree
        let remove_output = Command::new("git")
            .current_dir(base_repo_path)
            .args(&["worktree", "remove", "--force", &worktree_name])
            .output()
            .context("Failed to remove existing worktree")?;

        if !remove_output.status.success() {
            // If removal failed, try to prune and remove directory manually
            let prune_output = Command::new("git")
                .current_dir(base_repo_path)
                .args(&["worktree", "prune"])
                .output()?;

            if !prune_output.status.success() {
                println!(
                    "Warning: worktree prune failed: {}",
                    String::from_utf8_lossy(&prune_output.stderr)
                );
            }

            if worktree_path.exists() {
                std::fs::remove_dir_all(&worktree_path)
                    .context("Failed to remove existing worktree directory")?;
            }
        }
    }

    // Ensure the directory doesn't exist
    if worktree_path.exists() {
        std::fs::remove_dir_all(&worktree_path).context("Failed to remove existing directory")?;
    }

    // Fetch the latest target branch
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

    // Create worktree
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

    println!("Worktree created successfully!");
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
            // Use existing repository with worktree
            let repo_path = Path::new(repo_path);
            if !repo_path.exists() {
                anyhow::bail!("Local repository path does not exist: {:?}", repo_path);
            }

            // Verify it's a valid git repository
            let verify_output = Command::new("git")
                .current_dir(repo_path)
                .args(&["rev-parse", "--git-dir"])
                .output()
                .context("Failed to verify git repository")?;

            if !verify_output.status.success() {
                anyhow::bail!("Not a valid git repository: {:?}", repo_path);
            }

            // Create worktree
            create_worktree(repo_path, target_branch, version)
        }
        None => {
            // Clone repository
            shallow_clone_repo(ssh_url, target_branch)
        }
    }
}

pub fn cherry_pick_commits(
    repo_path: &Path,
    commits: Vec<String>,
    version: &str,
    target_branch: &str,
    is_local_repo: bool,
) -> Result<()> {
    let branch_name = format!("patch/{}-{}", target_branch, version);

    // Only fetch commits if we're working with a cloned repo
    if !is_local_repo {
        println!("Fetching required commits...");
        for commit_id in &commits {
            let output = Command::new("git")
                .current_dir(repo_path)
                .args(&["fetch", "--depth=1", "origin", commit_id])
                .output()?;

            if !output.status.success() {
                println!(
                    "Warning: Could not fetch commit {}: {}",
                    commit_id,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    // Create and checkout new branch
    let checkout_output = Command::new("git")
        .current_dir(repo_path)
        .args(&["checkout", "-B", &branch_name])
        .output()
        .context("Failed to create and checkout branch")?;

    if !checkout_output.status.success() {
        anyhow::bail!(
            "Failed to create branch: {}",
            String::from_utf8_lossy(&checkout_output.stderr)
        );
    }

    println!("Created and checked out branch: {}", branch_name);

    // Cherry-pick each commit
    for (idx, commit_id) in commits.iter().enumerate() {
        println!(
            "\n[{}/{}] Cherry-picking commit: {}",
            idx + 1,
            commits.len(),
            commit_id
        );

        let output = Command::new("git")
            .current_dir(repo_path)
            .args(&["cherry-pick", commit_id])
            .output()
            .context("Failed to execute cherry-pick command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Check if it's a conflict
            if stderr.contains("conflict") || stderr.contains("CONFLICT") {
                println!(
                    "\n⚠️  Conflict detected while cherry-picking commit: {}",
                    commit_id
                );
                println!("Please resolve the conflicts manually in another terminal.");

                // Show the conflicted files
                let status_output = Command::new("git")
                    .current_dir(repo_path)
                    .args(&["status", "--short"])
                    .output()?;

                if status_output.status.success() {
                    println!("\nConflicted files:");
                    print!("{}", String::from_utf8_lossy(&status_output.stdout));
                }

                println!("\nRepository path: {:?}", repo_path);

                // Wait for user to resolve conflicts
                loop {
                    let response = dialoguer::Confirm::new()
                        .with_prompt("Have you resolved all conflicts and are ready to continue?")
                        .default(false)
                        .interact()?;

                    if response {
                        // Check if conflicts are actually resolved
                        let status_output = Command::new("git")
                            .current_dir(repo_path)
                            .args(&["diff", "--check"])
                            .output()?;

                        let diff_cached_output = Command::new("git")
                            .current_dir(repo_path)
                            .args(&["diff", "--cached", "--check"])
                            .output()?;

                        // Check for unmerged paths
                        let ls_files_output = Command::new("git")
                            .current_dir(repo_path)
                            .args(&["ls-files", "-u"])
                            .output()?;

                        if !ls_files_output.stdout.is_empty() {
                            println!(
                                "\n❌ There are still unmerged files. Please resolve all conflicts before continuing."
                            );
                            continue;
                        }

                        // Continue the cherry-pick
                        let continue_output = Command::new("git")
                            .current_dir(repo_path)
                            .args(&["cherry-pick", "--continue"])
                            .output()?;

                        if continue_output.status.success() {
                            println!(
                                "✅ Cherry-pick completed successfully for commit: {}",
                                commit_id
                            );
                            break;
                        } else {
                            println!(
                                "\n❌ Failed to continue cherry-pick: {}",
                                String::from_utf8_lossy(&continue_output.stderr)
                            );

                            let abort_response = dialoguer::Confirm::new()
                                .with_prompt(
                                    "Do you want to abort this cherry-pick and skip this commit?",
                                )
                                .default(false)
                                .interact()?;

                            if abort_response {
                                Command::new("git")
                                    .current_dir(repo_path)
                                    .args(&["cherry-pick", "--abort"])
                                    .output()?;
                                println!("⏭️  Skipped commit: {}", commit_id);
                                break;
                            }
                        }
                    } else {
                        let abort_response = dialoguer::Confirm::new()
                            .with_prompt(
                                "Do you want to abort this cherry-pick and skip this commit?",
                            )
                            .default(false)
                            .interact()?;

                        if abort_response {
                            Command::new("git")
                                .current_dir(repo_path)
                                .args(&["cherry-pick", "--abort"])
                                .output()?;
                            println!("⏭️  Skipped commit: {}", commit_id);
                            break;
                        }
                    }
                }
            } else {
                // Non-conflict error
                eprintln!(
                    "\n❌ Cherry-pick failed for commit {}: {}",
                    commit_id, stderr
                );

                // Check if we're in the middle of a cherry-pick
                let status_output = Command::new("git")
                    .current_dir(repo_path)
                    .args(&["status"])
                    .output()?;

                let status_str = String::from_utf8_lossy(&status_output.stdout);
                if status_str.contains("cherry-pick") {
                    // Abort the failed cherry-pick
                    Command::new("git")
                        .current_dir(repo_path)
                        .args(&["cherry-pick", "--abort"])
                        .output()?;
                }

                let continue_response = dialoguer::Confirm::new()
                    .with_prompt("Do you want to continue with the remaining commits?")
                    .default(true)
                    .interact()?;

                if !continue_response {
                    anyhow::bail!("Cherry-pick process aborted by user");
                }
            }
        } else {
            println!("✅ Successfully cherry-picked commit: {}", commit_id);
        }
    }

    // Show final status
    println!("\n🏁 Cherry-pick process completed!");
    let log_output = Command::new("git")
        .current_dir(repo_path)
        .args(&["log", "--oneline", "-5"])
        .output()?;

    if log_output.status.success() {
        println!("\nRecent commits on branch {}:", branch_name);
        print!("{}", String::from_utf8_lossy(&log_output.stdout));
    }

    Ok(())
}
