mod api;
mod config;
mod git;
mod git_config;
mod migration;
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

use crate::{
    api::AzureDevOpsClient,
    config::Config,
    models::{AppConfig, Args},
    ui::App,
    ui::state::create_initial_state,
};

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
        config.shared().organization.clone(),
        config.shared().project.clone(),
        config.shared().repository.clone(),
        config.shared().pat.clone(),
    )?;

    // Pull requests will be fetched by the appropriate loading state
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
        config.shared().organization.clone(),
        config.shared().project.clone(),
        config.shared().repository.clone(),
        config.shared().dev_branch.clone(),
        config.shared().target_branch.clone(),
        config.shared().local_repo.clone(),
        match &config {
            AppConfig::Default { default, .. } => default.work_item_state.clone(),
            AppConfig::Migration { .. } => "Next Merged".to_string(), // Default fallback for migration mode
        },
        config.shared().parallel_limit,
        config.shared().max_concurrent_network,
        config.shared().max_concurrent_processing,
        config.shared().tag_prefix.clone(),
        config.shared().since.clone(),
        client,
    );

    // Set the initial state based on the configuration
    app.initial_state = Some(create_initial_state(Some(config)));

    // Run app with unified state machine
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
