use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use mergers::{
    Args, AzureDevOpsClient, Config,
    ui::{App, run_app},
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse_with_default_mode();

    // Handle --create-config flag
    if args.create_config {
        Config::create_sample_config()?;
        return Ok(());
    }

    // Resolve configuration from CLI args, environment variables, and config file
    let config = Arc::new(args.resolve_config()?);

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

    // Create app
    let mut app = App::new(pr_with_work_items, config.clone(), client);

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
