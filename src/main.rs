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
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use ui::run_app;

use crate::{api::AzureDevOpsClient, models::Args, ui::App};

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

    // Fetch pull requests
    let mut prs = client.fetch_pull_requests(&args.dev_branch).await?;
    prs = api::filter_prs_without_merged_tag(prs);

    if prs.is_empty() {
        eprintln!("No pull requests found without merged tags.");
        return Ok(());
    }

    // Fetch work items
    let mut pr_with_work_items = Vec::new();
    for pr in prs {
        let work_items = client
            .fetch_work_items_for_pr(pr.id)
            .await
            .unwrap_or_default();
        pr_with_work_items.push(models::PullRequestWithWorkItems {
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

    // Create app
    let mut app = App::new(
        pr_with_work_items,
        args.organization.clone(),
        args.project.clone(),
        args.repository.clone(),
        args.dev_branch.clone(),
        args.target_branch.clone(),
        args.local_repo.clone(),
        client,
    );

    // Run app with state machine
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
