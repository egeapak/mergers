use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use mergers::{
    Args, AzureDevOpsClient, Commands, Config,
    config::Config as RawConfig,
    core::runner::{MergeRunnerConfig, NonInteractiveRunner, OutputFormat, RunResult},
    logging::{init_logging, parse_early_log_config},
    models::{
        MergeAbortArgs, MergeArgs, MergeCompleteArgs, MergeContinueArgs, MergeStatusArgs,
        MergeSubcommand,
    },
    parsed_property::ParsedProperty,
    ui::{App, run_app},
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging early (before any other operations)
    // Parse logging config from raw args and environment
    let raw_args: Vec<String> = std::env::args().collect();
    let log_config = parse_early_log_config(&raw_args);
    let _log_guard = init_logging(log_config);

    tracing::debug!("Mergers starting up");

    let args = Args::parse_with_default_mode();

    // Handle --create-config flag
    if args.create_config {
        Config::create_sample_config()?;
        return Ok(());
    }

    // Route based on command
    match &args.command {
        Some(Commands::Merge(merge_args)) => {
            // Check for subcommand
            match &merge_args.subcommand {
                Some(MergeSubcommand::Continue(cont_args)) => {
                    let result = run_continue(cont_args).await;
                    handle_run_result(result);
                }
                Some(MergeSubcommand::Abort(abort_args)) => {
                    let result = run_abort(abort_args);
                    handle_run_result(result);
                }
                Some(MergeSubcommand::Status(status_args)) => {
                    let result = run_status(status_args);
                    handle_run_result(result);
                }
                Some(MergeSubcommand::Complete(complete_args)) => {
                    let result = run_complete(complete_args).await;
                    handle_run_result(result);
                }
                // No subcommand with -n flag → non-interactive merge mode
                None if merge_args.ni.non_interactive => {
                    let result = run_non_interactive_merge(merge_args).await;
                    handle_run_result(result);
                }
                // No subcommand and no -n → TUI mode
                _ => {
                    run_interactive_tui(args).await?;
                }
            }
        }
        // Migrate, Cleanup, or no command → TUI mode
        _ => {
            run_interactive_tui(args).await?;
        }
    }

    Ok(())
}

/// Handles run result by printing messages and setting exit code.
fn handle_run_result(result: RunResult) {
    if let Some(ref msg) = result.message {
        if result.is_success() {
            eprintln!("{}", msg);
        } else {
            eprintln!("Error: {}", msg);
        }
    }

    if let Some(ref path) = result.state_file_path {
        eprintln!("State file: {}", path.display());
    }

    process::exit(result.exit_code as i32);
}

/// Runs the interactive TUI mode.
async fn run_interactive_tui(args: Args) -> Result<()> {
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

/// Runs a non-interactive merge operation.
async fn run_non_interactive_merge(args: &MergeArgs) -> RunResult {
    let config = match build_runner_config_from_merge_args(args) {
        Ok(c) => c,
        Err(e) => {
            return RunResult::error(
                mergers::core::ExitCode::GeneralError,
                format!("Configuration error: {}", e),
            );
        }
    };

    let mut runner = NonInteractiveRunner::new(config);
    runner.run().await
}

/// Continues a merge operation after conflict resolution.
async fn run_continue(args: &MergeContinueArgs) -> RunResult {
    let config = match build_minimal_runner_config(args.output, args.quiet) {
        Ok(c) => c,
        Err(e) => {
            return RunResult::error(
                mergers::core::ExitCode::GeneralError,
                format!("Configuration error: {}", e),
            );
        }
    };

    let repo_path = args.repo.as_ref().map(PathBuf::from);
    let mut runner = NonInteractiveRunner::new(config);
    runner.continue_merge(repo_path.as_deref()).await
}

/// Aborts a merge operation.
fn run_abort(args: &MergeAbortArgs) -> RunResult {
    let config = match build_minimal_runner_config(args.output, false) {
        Ok(c) => c,
        Err(e) => {
            return RunResult::error(
                mergers::core::ExitCode::GeneralError,
                format!("Configuration error: {}", e),
            );
        }
    };

    let repo_path = args.repo.as_ref().map(PathBuf::from);
    let mut runner = NonInteractiveRunner::new(config);
    runner.abort(repo_path.as_deref())
}

/// Shows merge status.
fn run_status(args: &MergeStatusArgs) -> RunResult {
    let config = match build_minimal_runner_config(args.output, false) {
        Ok(c) => c,
        Err(e) => {
            return RunResult::error(
                mergers::core::ExitCode::GeneralError,
                format!("Configuration error: {}", e),
            );
        }
    };

    let repo_path = args.repo.as_ref().map(PathBuf::from);
    let mut runner = NonInteractiveRunner::new(config);
    runner.status(repo_path.as_deref())
}

/// Completes a merge operation.
async fn run_complete(args: &MergeCompleteArgs) -> RunResult {
    let config = match build_minimal_runner_config(args.output, args.quiet) {
        Ok(c) => c,
        Err(e) => {
            return RunResult::error(
                mergers::core::ExitCode::GeneralError,
                format!("Configuration error: {}", e),
            );
        }
    };

    let repo_path = args.repo.as_ref().map(PathBuf::from);
    let mut runner = NonInteractiveRunner::new(config);
    runner
        .complete(repo_path.as_deref(), &args.next_state)
        .await
}

/// Builds MergeRunnerConfig from MergeArgs with full config resolution.
fn build_runner_config_from_merge_args(args: &MergeArgs) -> Result<MergeRunnerConfig> {
    let shared = &args.shared;

    // Determine local_repo path (positional arg takes precedence over --local-repo flag)
    let local_repo_path = shared.path.as_ref().or(shared.local_repo.as_ref());

    // Load from config file (lowest priority)
    let file_config = RawConfig::load_from_file()?;

    // Load from environment variables
    let env_config = RawConfig::load_from_env();

    // Try to detect from git remote if we have a local repo path
    let git_config = if let Some(repo_path) = local_repo_path {
        RawConfig::detect_from_git_remote(repo_path)
    } else {
        RawConfig::default()
    };

    // CLI config from args
    let cli_config = RawConfig {
        organization: shared
            .organization
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        project: shared
            .project
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        repository: shared
            .repository
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        pat: shared
            .pat
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        dev_branch: shared
            .dev_branch
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        target_branch: shared
            .target_branch
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        local_repo: local_repo_path.map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        work_item_state: args
            .work_item_state
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        parallel_limit: shared
            .parallel_limit
            .map(|v| ParsedProperty::Cli(v, v.to_string())),
        max_concurrent_network: shared
            .max_concurrent_network
            .map(|v| ParsedProperty::Cli(v, v.to_string())),
        max_concurrent_processing: shared
            .max_concurrent_processing
            .map(|v| ParsedProperty::Cli(v, v.to_string())),
        tag_prefix: shared
            .tag_prefix
            .as_ref()
            .map(|v| ParsedProperty::Cli(v.clone(), v.clone())),
        run_hooks: Some(ParsedProperty::Cli(
            args.run_hooks,
            args.run_hooks.to_string(),
        )),
        // UI settings are not set via CLI, only via config file
        show_dependency_highlights: None,
        show_work_item_highlights: None,
        // Hooks are not set via CLI, only via config file or env vars
        hooks: None,
    };

    // Merge configs: file < git_remote < env < cli
    let merged = file_config
        .merge(git_config)
        .merge(env_config)
        .merge(cli_config);

    // Extract required values
    let organization = merged
        .organization
        .ok_or_else(|| anyhow::anyhow!("organization is required"))?
        .value()
        .clone();
    let project = merged
        .project
        .ok_or_else(|| anyhow::anyhow!("project is required"))?
        .value()
        .clone();
    let repository = merged
        .repository
        .ok_or_else(|| anyhow::anyhow!("repository is required"))?
        .value()
        .clone();
    let pat = merged
        .pat
        .ok_or_else(|| anyhow::anyhow!("pat is required"))?
        .value()
        .clone();

    // Extract optional values with defaults
    let dev_branch = merged
        .dev_branch
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "dev".to_string());
    let target_branch = merged
        .target_branch
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "next".to_string());
    let tag_prefix = merged
        .tag_prefix
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "merged-".to_string());
    let work_item_state = merged
        .work_item_state
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "Next Merged".to_string());
    let local_repo = merged.local_repo.map(|p| PathBuf::from(p.value().clone()));
    let run_hooks = merged.run_hooks.map(|p| *p.value()).unwrap_or(false);
    let max_concurrent_network = merged
        .max_concurrent_network
        .map(|p| *p.value())
        .unwrap_or(100);
    let max_concurrent_processing = merged
        .max_concurrent_processing
        .map(|p| *p.value())
        .unwrap_or(10);
    let since = shared.since.clone();

    // Version is required for non-interactive mode
    let version = args
        .ni
        .version
        .clone()
        .ok_or_else(|| anyhow::anyhow!("version is required for non-interactive mode"))?;

    Ok(MergeRunnerConfig {
        organization,
        project,
        repository,
        pat,
        dev_branch,
        target_branch,
        version,
        tag_prefix,
        work_item_state,
        select_by_states: args.ni.select_by_state.clone(),
        local_repo,
        run_hooks,
        output_format: args.ni.output,
        quiet: args.ni.quiet,
        hooks_config: merged.hooks,
        max_concurrent_network,
        max_concurrent_processing,
        since,
    })
}

/// Builds a minimal MergeRunnerConfig for operations that don't need full config.
/// Used by continue, abort, status, and complete commands.
fn build_minimal_runner_config(output: OutputFormat, quiet: bool) -> Result<MergeRunnerConfig> {
    // Load from config file and environment for API operations
    let file_config = RawConfig::load_from_file()?;
    let env_config = RawConfig::load_from_env();
    let merged = file_config.merge(env_config);

    // Extract values, using empty strings for optional ones since these commands
    // will read the state file which has the actual values
    let organization = merged
        .organization
        .map(|p| p.value().clone())
        .unwrap_or_default();
    let project = merged
        .project
        .map(|p| p.value().clone())
        .unwrap_or_default();
    let repository = merged
        .repository
        .map(|p| p.value().clone())
        .unwrap_or_default();
    let pat = merged.pat.map(|p| p.value().clone()).unwrap_or_default();
    let dev_branch = merged
        .dev_branch
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "dev".to_string());
    let target_branch = merged
        .target_branch
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "next".to_string());
    let tag_prefix = merged
        .tag_prefix
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "merged-".to_string());
    let work_item_state = merged
        .work_item_state
        .map(|p| p.value().clone())
        .unwrap_or_else(|| "Next Merged".to_string());
    let local_repo = merged.local_repo.map(|p| PathBuf::from(p.value().clone()));
    let run_hooks = merged.run_hooks.map(|p| *p.value()).unwrap_or(false);
    let max_concurrent_network = merged
        .max_concurrent_network
        .map(|p| *p.value())
        .unwrap_or(100);
    let max_concurrent_processing = merged
        .max_concurrent_processing
        .map(|p| *p.value())
        .unwrap_or(10);

    Ok(MergeRunnerConfig {
        organization,
        project,
        repository,
        pat,
        dev_branch,
        target_branch,
        version: String::new(), // Not needed for continue/abort/status/complete
        tag_prefix,
        work_item_state,
        select_by_states: None,
        local_repo,
        run_hooks,
        output_format: output,
        quiet,
        hooks_config: merged.hooks,
        max_concurrent_network,
        max_concurrent_processing,
        since: None, // Not needed for continue/abort/status/complete
    })
}
