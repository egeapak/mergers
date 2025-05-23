mod api;
mod git;
mod models;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use dialoguer::Input;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, time::Duration};

use crate::{
    api::AzureDevOpsClient,
    git::{cherry_pick_commits, setup_repository},
    models::{Args, PullRequestWithWorkItems},
    ui::{App, run_app},
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Create Azure DevOps client
    let client = AzureDevOpsClient::new(
        args.organization.clone(),
        args.project.clone(),
        args.repository.clone(),
        args.pat.clone(),
    )?;

    println!("Fetching pull requests...");
    let mut prs = client.fetch_pull_requests(&args.dev_branch).await?;

    prs = api::filter_prs_without_merged_tag(prs);

    if prs.is_empty() {
        println!("No pull requests found without merged tags.");
        return Ok(());
    }

    println!("Fetching work items for {} pull requests...", prs.len());
    let mut pr_with_work_items = Vec::new();

    for pr in prs {
        let work_items = client
            .fetch_work_items_for_pr(pr.id)
            .await
            .unwrap_or_default();

        pr_with_work_items.push(PullRequestWithWorkItems {
            pr,
            work_items,
            selected: false,
        });
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let app = App::new(
        pr_with_work_items.clone(),
        args.organization.clone(),
        args.project.clone(),
        args.repository.clone(),
    );
    let selected_indices = run_app(&mut terminal, app).await?;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if selected_indices.is_empty() {
        println!("No pull requests selected.");
        return Ok(());
    }

    // Get selected PRs
    let selected_prs: Vec<&PullRequestWithWorkItems> = selected_indices
        .iter()
        .filter_map(|&i| {
            if i < pr_with_work_items.len() {
                Some(&pr_with_work_items[i])
            } else {
                None
            }
        })
        .collect();

    println!("\nSelected {} pull requests:", selected_prs.len());
    for pr in &selected_prs {
        println!("  - PR #{}: {}", pr.pr.id, pr.pr.title);
    }

    // Get repo details only if we need to clone
    let ssh_url = if args.local_repo.is_some() {
        String::new()
    } else {
        client.fetch_repo_details().await?.ssh_url
    };

    // Get version from user
    let version = Input::<String>::new()
        .with_prompt("Enter version number")
        .interact_text()?;

    // Setup repository (either clone or create worktree)
    let repo_path = setup_repository(
        args.local_repo.as_deref(),
        &ssh_url,
        &args.target_branch,
        &version,
    )?;

    // Collect commit IDs
    let commit_ids: Vec<String> = selected_prs
        .iter()
        .filter_map(|pr| {
            pr.pr
                .last_merge_commit
                .as_ref()
                .map(|mc| mc.commit_id.clone())
        })
        .collect();

    if commit_ids.is_empty() {
        println!("No merge commits found for selected PRs.");
        return Ok(());
    }

    // Cherry-pick commits
    cherry_pick_commits(
        &repo_path,
        commit_ids,
        &version,
        &args.target_branch,
        args.local_repo.is_some(),
    )?;

    println!(
        "\n✓ Successfully created branch patch/{}-{} with cherry-picked commits",
        args.target_branch, version
    );
    println!("Repository location: {:?}", repo_path);

    // Print clickable links
    ui::print_pr_links(
        &selected_prs,
        &args.organization,
        &args.project,
        &args.repository,
    );
    ui::print_work_item_links(&selected_prs, &args.organization, &args.project);

    Ok(())
}
