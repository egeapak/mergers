use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use mergers::{
    AppConfig, Args, AzureDevOpsClient, Config,
    ui::{App, AppConfiguration, run_app, state::create_initial_state},
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
        config.shared().organization.value().clone(),
        config.shared().project.value().clone(),
        config.shared().repository.value().clone(),
        config.shared().pat.value().clone(),
    )?;

    // Pull requests will be fetched by the appropriate loading state
    let pr_with_work_items = Vec::new();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app configuration
    let app_config = AppConfiguration {
        organization: config.shared().organization.value().clone(),
        project: config.shared().project.value().clone(),
        repository: config.shared().repository.value().clone(),
        dev_branch: config.shared().dev_branch.value().clone(),
        target_branch: config.shared().target_branch.value().clone(),
        local_repo: config
            .shared()
            .local_repo
            .as_ref()
            .map(|p| p.value().clone()),
        work_item_state: match &config {
            AppConfig::Default { default, .. } => default.work_item_state.value().clone(),
            AppConfig::Migration { .. } => "Next Merged".to_string(), // Default fallback for migration mode
        },
        max_concurrent_network: *config.shared().max_concurrent_network.value(),
        max_concurrent_processing: *config.shared().max_concurrent_processing.value(),
        tag_prefix: config.shared().tag_prefix.value().clone(),
        since: config
            .shared()
            .since
            .as_ref()
            .map(|d| d.original().unwrap_or("").to_string()),
    };

    // Create app
    let mut app = App::new(pr_with_work_items, app_config, client);

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
