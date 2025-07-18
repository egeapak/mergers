mod api;
mod config;
mod git;
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
    migration::MigrationAnalyzer,
    models::{AppConfig, Args},
    ui::App,
    ui::state::{MigrationLoadingState, MigrationState},
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

    // Handle migration mode
    if config.is_migration_mode() {
        return run_migration_mode(config, client).await;
    }

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

async fn run_migration_mode(config: AppConfig, client: AzureDevOpsClient) -> Result<()> {
    use crate::api::AzureDevOpsClient;
    use crate::git::{cleanup_migration_worktrees, force_remove_worktree, setup_repository};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Setup terminal first
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(
        Vec::new(), // No pull requests for migration mode
        config.shared().organization.clone(),
        config.shared().project.clone(),
        config.shared().repository.clone(),
        config.shared().dev_branch.clone(),
        config.shared().target_branch.clone(),
        config.shared().local_repo.clone(),
        "Next Merged".to_string(), // Default for migration mode
        client.clone(),
    );

    // Create loading state
    let loading_state = Arc::new(Mutex::new(MigrationLoadingState::new()));
    let loading_state_clone = loading_state.clone();

    // Run analysis in background
    let mut analysis_task = {
        let config = config.clone();
        let client = client.clone();

        tokio::spawn(async move {
            // Generate unique ID for migration run
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let migration_id = format!("migration-{}", timestamp);

            // Setup repository for analysis
            loading_state_clone
                .lock()
                .unwrap()
                .update_status("Setting up repository...");
            let repo_details = client.fetch_repo_details().await?;

            // If using local repo, attempt to clean up any existing migration worktrees
            if let Some(local_repo) = &config.shared().local_repo {
                // Clean up the old hardcoded migration worktree
                let _ =
                    force_remove_worktree(std::path::Path::new(local_repo), "migration-analysis");
                // Clean up any timestamped migration worktrees from previous runs
                let _ = cleanup_migration_worktrees(std::path::Path::new(local_repo));
            }

            let repo_setup = setup_repository(
                config.shared().local_repo.as_deref(),
                &repo_details.ssh_url,
                &config.shared().target_branch,
                &migration_id,
            )
            .map_err(|e| anyhow::anyhow!("Failed to setup repository: {}", e))?;

            let repo_path = match &repo_setup {
                crate::git::RepositorySetup::Local(path) => path.as_path(),
                crate::git::RepositorySetup::Clone(path, _) => path.as_path(),
            };

            // Parse terminal states
            let terminal_states = match &config {
                AppConfig::Migration { migration, .. } => {
                    AzureDevOpsClient::parse_terminal_states(&migration.terminal_states)
                }
                _ => unreachable!("Migration mode should have migration config"),
            };

            // Create migration analyzer
            let analyzer = MigrationAnalyzer::new(client.clone(), terminal_states);

            // Run migration analysis with progress callback
            let analysis = analyzer
                .analyze_prs_for_migration(
                    repo_path,
                    &config.shared().dev_branch,
                    &config.shared().target_branch,
                    |status| {
                        loading_state_clone.lock().unwrap().update_status(status);
                    },
                )
                .await?;

            // Clean up migration worktree
            if let Some(local_repo) = &config.shared().local_repo {
                let _ = force_remove_worktree(std::path::Path::new(local_repo), &migration_id);
            }

            loading_state_clone.lock().unwrap().complete();
            Ok::<_, anyhow::Error>(analysis)
        })
    };

    // Run loading UI
    let mut current_state: Box<dyn crate::ui::state::AppState> =
        Box::new(MigrationLoadingState::new());
    let mut analysis_result = None;

    loop {
        // Update current state from shared loading state
        if let Ok(shared_state) = loading_state.lock() {
            current_state = Box::new(shared_state.clone());
        }

        // Use tokio::select to handle both UI events and analysis completion
        tokio::select! {
            // Check if analysis is complete
            task_result = &mut analysis_task => {
                match task_result {
                    Ok(Ok(analysis)) => {
                        analysis_result = Some(analysis);
                        break;
                    }
                    Ok(Err(e)) => {
                        loading_state.lock().unwrap().set_error(e.to_string());
                    }
                    Err(e) => {
                        loading_state.lock().unwrap().set_error(format!("Task failed: {}", e));
                    }
                }
            }

            // Handle UI events
            _ = async {
                // Draw UI
                terminal.draw(|f| current_state.ui(f, &app))?;

                // Handle input
                if crossterm::event::poll(std::time::Duration::from_millis(50))? {
                    if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                        match current_state.process_key(key.code, &mut app).await {
                            crate::ui::state::StateChange::Keep => {}
                            crate::ui::state::StateChange::Change(_) => {}
                            crate::ui::state::StateChange::Exit => {
                                // Restore terminal
                                disable_raw_mode()?;
                                execute!(
                                    terminal.backend_mut(),
                                    LeaveAlternateScreen,
                                    DisableMouseCapture
                                )?;
                                terminal.show_cursor()?;
                                return Ok(());
                            }
                        }
                    }
                }
                Ok::<(), anyhow::Error>(())
            } => {
                // UI handling completed normally, continue loop
            }
        }
    }

    // If we have analysis results, show the migration UI
    if let Some(analysis) = analysis_result {
        app.migration_analysis = Some(analysis);

        // Create migration state
        let mut migration_state: Box<dyn crate::ui::state::AppState> =
            Box::new(MigrationState::new());

        // Run migration UI
        let result = async {
            loop {
                terminal.draw(|f| migration_state.ui(f, &app))?;

                if crossterm::event::poll(std::time::Duration::from_millis(50))? {
                    if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                        match migration_state.process_key(key.code, &mut app).await {
                            crate::ui::state::StateChange::Keep => {}
                            crate::ui::state::StateChange::Change(_) => {
                                // Migration state doesn't change to other states
                            }
                            crate::ui::state::StateChange::Exit => break,
                        }
                    }
                }
            }
            Ok(())
        }
        .await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    } else {
        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    }
}
