mod api;
mod git;
mod models;
mod ui;
mod utils;

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

    // Pull requests will be fetched by PullRequestSelectionState
    let pr_with_work_items = Vec::new();

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
        args.work_item_state.clone(),
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
