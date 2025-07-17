mod api;
mod config;
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

use crate::{api::AzureDevOpsClient, config::Config, models::Args, ui::App};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle --create-config flag
    if args.create_config {
        Config::create_sample_config()?;
        return Ok(());
    }

    // Resolve configuration from CLI args, environment variables, and config file
    let config = args.resolve_config()?;

    // Create Azure DevOps client
    let client = AzureDevOpsClient::new(
        config.organization.clone(),
        config.project.clone(),
        config.repository.clone(),
        config.pat.clone(),
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
        config.organization.clone(),
        config.project.clone(),
        config.repository.clone(),
        config.dev_branch.clone(),
        config.target_branch.clone(),
        config.local_repo.clone(),
        config.work_item_state.clone(),
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
