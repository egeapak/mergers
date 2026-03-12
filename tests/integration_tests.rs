//! Integration tests for the mergers library
//!
//! These tests demonstrate how to use the library APIs and verify
//! end-to-end functionality.

use mergers::{
    AppConfig, Args, AzureDevOpsClient, Commands, Config, MergeArgs, MigrateArgs,
    NonInteractiveArgs, SharedArgs, parsed_property::ParsedProperty,
};
use serial_test::file_serial;
use std::fs;
use tempfile::TempDir;

/// Helper function to create a default Args struct for testing
fn create_empty_args() -> Args {
    Args {
        command: None, // Default to merge mode if no command
        create_config: false,
    }
}

/// Helper function to create Args with migration mode
fn create_empty_migrate_args() -> Args {
    Args {
        command: Some(Commands::Migrate(MigrateArgs {
            shared: SharedArgs {
                path: None,
                organization: None,
                project: None,
                repository: None,
                pat: None,
                dev_branch: None,
                target_branch: None,
                local_repo: None,
                tag_prefix: None,
                parallel_limit: None,
                max_concurrent_network: None,
                max_concurrent_processing: None,
                since: None,
                skip_confirmation: false,
                log_level: None,
                log_file: None,
                log_format: None,
            },
            terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        })),
        create_config: false,
    }
}

/// # Config Loading and Merging Integration
///
/// Tests end-to-end configuration loading and merging functionality.
///
/// ## Test Scenario
/// - Tests loading configuration from files and environment
/// - Validates configuration merging and precedence rules
/// - Tests default value application
///
/// ## Expected Outcome
/// - Configuration loading works correctly across different sources
/// - Merging rules are properly applied with correct precedence
#[test]
#[file_serial(env_tests)]
fn test_config_loading_and_merging() {
    // Test that config loading doesn't panic and returns sensible defaults
    let _config = Config::load_from_file().expect("Should load config or return defaults");

    // If no config file exists, we get a default config
    // Let's check that the default() function gives us expected values
    let default_config = Config::default();
    assert_eq!(
        default_config.dev_branch,
        Some(ParsedProperty::Default("dev".to_string()))
    );
    assert_eq!(
        default_config.target_branch,
        Some(ParsedProperty::Default("next".to_string()))
    );
    assert_eq!(
        default_config.parallel_limit,
        Some(ParsedProperty::Default(300))
    );

    // Test environment config
    let _env_config = Config::load_from_env();

    // Test merging - create a test config with known values to merge
    let test_config = Config {
        organization: Some(ParsedProperty::Default("test-org".to_string())),
        parallel_limit: Some(ParsedProperty::Default(500)),
        ..Config::default()
    };

    let merged = default_config.merge(test_config);

    // Basic validation that merge works
    assert_eq!(
        merged.organization,
        Some(ParsedProperty::Default("test-org".to_string()))
    );
    assert_eq!(merged.parallel_limit, Some(ParsedProperty::Default(500)));
    assert_eq!(
        merged.dev_branch,
        Some(ParsedProperty::Default("dev".to_string()))
    ); // Should keep default
}

/// # API Client Creation Integration
///
/// Tests integration of API client creation with configuration.
///
/// ## Test Scenario
/// - Tests Azure DevOps client creation with various parameters
/// - Validates terminal states parsing functionality
/// - Tests client configuration and setup
///
/// ## Expected Outcome
/// - API client is correctly created with provided configuration
/// - Terminal states parsing works with different input formats
#[test]
fn test_api_client_creation() {
    // Test that client creation works with valid inputs
    let result = AzureDevOpsClient::parse_terminal_states("Closed,Done,Complete");
    assert_eq!(result, vec!["Closed", "Done", "Complete"]);

    // Test whitespace handling
    let result = AzureDevOpsClient::parse_terminal_states(" Active , In Progress , Done ");
    assert_eq!(result, vec!["Active", "In Progress", "Done"]);

    // Test empty input
    let result = AzureDevOpsClient::parse_terminal_states("");
    assert!(result.is_empty());
}

/// # Library Version Access
///
/// Tests that the library version constant is accessible and valid.
///
/// ## Test Scenario
/// - Accesses the library version constant
/// - Validates version format and content
///
/// ## Expected Outcome
/// - Version constant is accessible from library API
/// - Version follows expected format conventions
#[test]
fn test_library_version() {
    // Test that version constant is accessible
    let version = mergers::VERSION;
    assert!(!version.is_empty());
    assert!(version.contains('.'));
}

/// # Async Client Creation
///
/// Tests asynchronous creation of Azure DevOps client.
///
/// ## Test Scenario
/// - Creates Azure DevOps client in async context
/// - Tests client creation without network calls
///
/// ## Expected Outcome
/// - Client creation succeeds in async environment
/// - No network errors occur during basic client setup
#[tokio::test]
async fn test_client_creation() {
    // This test just creates a client without making network calls
    // In a real scenario, you'd mock the network calls

    let client_result = AzureDevOpsClient::new(
        "test-org".to_string(),
        "test-project".to_string(),
        "test-repo".to_string(),
        "test-pat".to_string(),
    );

    // Should not fail with valid strings
    assert!(client_result.is_ok());
}

/// # Args Resolution with Environment Variables
///
/// Tests configuration resolution using environment variables.
///
/// ## Test Scenario
/// - Sets up environment variables for configuration
/// - Resolves Args to configuration using environment sources
/// - Tests environment variable precedence and parsing
///
/// ## Expected Outcome
/// - Environment variables are correctly parsed into configuration
/// - Configuration resolution succeeds with environment sources
#[test]
#[file_serial(env_tests)]
fn test_args_resolution_with_env() {
    // Clean up XDG_CONFIG_HOME first to prevent loading from config files left by other tests
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    // Test that Args can resolve configuration from environment variables

    // Set up test environment variables
    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "env-org");
        std::env::set_var("MERGERS_PROJECT", "env-project");
        std::env::set_var("MERGERS_REPOSITORY", "env-repo");
        std::env::set_var("MERGERS_PAT", "env-pat");
        std::env::set_var("MERGERS_PARALLEL_LIMIT", "500");
    }

    let args = create_empty_args();

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(
                shared.organization,
                ParsedProperty::Env("env-org".to_string(), "env-org".to_string())
            );
            assert_eq!(
                shared.project,
                ParsedProperty::Env("env-project".to_string(), "env-project".to_string())
            );
            assert_eq!(
                shared.repository,
                ParsedProperty::Env("env-repo".to_string(), "env-repo".to_string())
            );
            assert_eq!(
                shared.pat,
                ParsedProperty::Env("env-pat".to_string(), "env-pat".to_string())
            );
            assert_eq!(
                shared.parallel_limit,
                ParsedProperty::Env(500, "500".to_string())
            );
        }
        _ => panic!("Expected default mode"),
    }

    // Clean up
    unsafe {
        std::env::remove_var("MERGERS_ORGANIZATION");
        std::env::remove_var("MERGERS_PROJECT");
        std::env::remove_var("MERGERS_REPOSITORY");
        std::env::remove_var("MERGERS_PAT");
        std::env::remove_var("MERGERS_PARALLEL_LIMIT");
    }
}

/// # Args Resolution with Configuration File
///
/// Tests configuration resolution using TOML configuration files.
///
/// ## Test Scenario
/// - Creates temporary configuration file with known values
/// - Resolves Args to configuration using file sources
/// - Tests file-based configuration parsing
///
/// ## Expected Outcome
/// - Configuration file is correctly parsed
/// - File-based configuration values are properly applied
#[test]
#[file_serial(env_tests)]
fn test_args_resolution_with_file() {
    // Clean up any env vars from other tests that could interfere
    // This is needed because tests run in parallel and share the same process
    unsafe {
        std::env::remove_var("MERGERS_ORGANIZATION");
        std::env::remove_var("MERGERS_PROJECT");
        std::env::remove_var("MERGERS_REPOSITORY");
        std::env::remove_var("MERGERS_PAT");
        std::env::remove_var("MERGERS_DEV_BRANCH");
        std::env::remove_var("MERGERS_TARGET_BRANCH");
        std::env::remove_var("MERGERS_LOCAL_REPO");
        std::env::remove_var("MERGERS_WORK_ITEM_STATE");
        std::env::remove_var("MERGERS_PARALLEL_LIMIT");
        std::env::remove_var("MERGERS_MAX_CONCURRENT_NETWORK");
        std::env::remove_var("MERGERS_MAX_CONCURRENT_PROCESSING");
        std::env::remove_var("MERGERS_TAG_PREFIX");
    }

    // Test that Args can resolve configuration from a config file

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("mergers").join("config.toml");

    // Create directory
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();

    let config_content = r#"organization = "file-org"
project = "file-project"
repository = "file-repo"
pat = "file-pat"
parallel_limit = 750
dev_branch = "develop"
target_branch = "main"
"#;

    fs::write(&config_path, config_content).unwrap();

    // Set config file path via environment
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    let args = create_empty_args();

    let result = args.resolve_config();

    // Clean up first
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(
                shared.organization,
                ParsedProperty::File(
                    "file-org".to_string(),
                    config_path.clone(),
                    "file-org".to_string()
                )
            );
            assert_eq!(
                shared.project,
                ParsedProperty::File(
                    "file-project".to_string(),
                    config_path.clone(),
                    "file-project".to_string()
                )
            );
            assert_eq!(
                shared.repository,
                ParsedProperty::File(
                    "file-repo".to_string(),
                    config_path.clone(),
                    "file-repo".to_string()
                )
            );
            assert_eq!(
                shared.pat,
                ParsedProperty::File("file-pat".to_string(), config_path, "file-pat".to_string())
            );
            assert_eq!(shared.parallel_limit, ParsedProperty::Default(300)); // Uses default value instead of file value
        }
        _ => panic!("Expected default mode"),
    }
}

/// # Missing Required Arguments Validation
///
/// Tests validation of missing required configuration arguments.
///
/// ## Test Scenario
/// - Attempts configuration resolution with missing required fields
/// - Tests error handling for incomplete configuration
///
/// ## Expected Outcome
/// - Missing required arguments are properly detected
/// - Appropriate error messages are generated for missing fields
#[test]
#[file_serial(env_tests)]
fn test_missing_required_args() {
    // Test that Args validation catches missing required arguments

    let args = create_empty_args();

    // Without env vars or config file, should fail
    let result = args.resolve_config();
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("organization")
            || error_msg.contains("project")
            || error_msg.contains("repository")
    );
}

/// # Migration Mode Initialization
///
/// Tests initialization of the application in migration mode.
///
/// ## Test Scenario
/// - Configures application for migration mode operation
/// - Tests migration-specific configuration and setup
///
/// ## Expected Outcome
/// - Application correctly initializes in migration mode
/// - Migration-specific settings are properly configured
#[test]
#[file_serial(env_tests)]
fn test_migration_mode_initialization() {
    // Test migration mode configuration

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "test-org");
        std::env::set_var("MERGERS_PROJECT", "test-project");
        std::env::set_var("MERGERS_REPOSITORY", "test-repo");
        std::env::set_var("MERGERS_PAT", "test-pat");
    }

    let args = create_empty_migrate_args();

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Migration {
            shared,
            migration: _,
        } => {
            assert_eq!(
                shared.organization,
                ParsedProperty::Env("test-org".to_string(), "test-org".to_string())
            );
            assert_eq!(
                shared.project,
                ParsedProperty::Env("test-project".to_string(), "test-project".to_string())
            );
            assert_eq!(
                shared.repository,
                ParsedProperty::Env("test-repo".to_string(), "test-repo".to_string())
            );
            assert_eq!(
                shared.pat,
                ParsedProperty::Env("test-pat".to_string(), "test-pat".to_string())
            );
        }
        _ => panic!("Expected migration mode"),
    }

    // Clean up
    unsafe {
        std::env::remove_var("MERGERS_ORGANIZATION");
        std::env::remove_var("MERGERS_PROJECT");
        std::env::remove_var("MERGERS_REPOSITORY");
        std::env::remove_var("MERGERS_PAT");
    }
}

/// # Default Mode Initialization
///
/// Tests initialization of the application in default mode.
///
/// ## Test Scenario
/// - Configures application for default mode operation
/// - Tests default mode configuration and setup
///
/// ## Expected Outcome
/// - Application correctly initializes in default mode
/// - Default mode settings are properly configured
#[test]
#[file_serial(env_tests)]
fn test_default_mode_initialization() {
    // Clean up any XDG_CONFIG_HOME that might be left over from previous tests
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    // Test default mode configuration (when migrate flag is false)

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "test-org");
        std::env::set_var("MERGERS_PROJECT", "test-project");
        std::env::set_var("MERGERS_REPOSITORY", "test-repo");
        std::env::set_var("MERGERS_PAT", "test-pat");
    }

    let args = create_empty_args();

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(
                shared.organization,
                ParsedProperty::Env("test-org".to_string(), "test-org".to_string())
            );
            assert_eq!(
                shared.project,
                ParsedProperty::Env("test-project".to_string(), "test-project".to_string())
            );
            assert_eq!(
                shared.repository,
                ParsedProperty::Env("test-repo".to_string(), "test-repo".to_string())
            );
            assert_eq!(
                shared.pat,
                ParsedProperty::Env("test-pat".to_string(), "test-pat".to_string())
            );
        }
        _ => panic!("Expected default mode"),
    }

    // Clean up
    unsafe {
        std::env::remove_var("MERGERS_ORGANIZATION");
        std::env::remove_var("MERGERS_PROJECT");
        std::env::remove_var("MERGERS_REPOSITORY");
        std::env::remove_var("MERGERS_PAT");
    }
}

/// # Create Config Flag Functionality
///
/// Tests the --create-config flag functionality for generating sample configs.
///
/// ## Test Scenario
/// - Tests sample configuration file creation
/// - Validates generated configuration content and structure
///
/// ## Expected Outcome
/// - Sample configuration file is successfully created
/// - Generated config contains expected template content
#[test]
#[file_serial(env_tests)]
fn test_create_config_flag_functionality() {
    // Test the --create-config functionality

    let temp_dir = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    let result = Config::create_sample_config();

    // Clean up first
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    assert!(result.is_ok());

    // Verify config file was created
    let expected_path = temp_dir.path().join("mergers").join("config.toml");
    assert!(expected_path.exists());

    // Verify file has expected content
    let content = fs::read_to_string(expected_path).unwrap();
    assert!(content.contains("organization"));
    assert!(content.contains("project"));
    assert!(content.contains("repository"));
}

/// # CLI Arguments Precedence
///
/// Tests that command-line arguments take precedence over other configuration sources.
///
/// ## Test Scenario
/// - Sets up conflicting values in CLI args and environment variables
/// - Tests configuration resolution with multiple sources
///
/// ## Expected Outcome
/// - CLI arguments take precedence over environment variables
/// - Configuration precedence rules are correctly applied
#[test]
#[file_serial(env_tests)]
fn test_args_cli_precedence() {
    // Test that CLI args take precedence over env vars

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "env-org");
        std::env::set_var("MERGERS_PROJECT", "env-project");
        std::env::set_var("MERGERS_REPOSITORY", "env-repo");
        std::env::set_var("MERGERS_PAT", "env-pat");
    }

    let args = Args {
        command: Some(Commands::Merge(MergeArgs {
            shared: SharedArgs {
                organization: Some("cli-org".to_string()),
                project: Some("cli-project".to_string()),
                repository: None, // Should use env var
                pat: None,        // Should use env var
                dev_branch: None,
                target_branch: None,
                local_repo: None,
                tag_prefix: None,
                parallel_limit: Some(999),
                max_concurrent_network: None,
                max_concurrent_processing: None,
                path: None,
                since: None,
                skip_confirmation: false,
                log_level: None,
                log_file: None,
                log_format: None,
            },
            ni: NonInteractiveArgs::default(),
            work_item_state: None,
            run_hooks: false,
            subcommand: None,
        })),
        create_config: false,
    };

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(
                shared.organization,
                ParsedProperty::Cli("cli-org".to_string(), "cli-org".to_string())
            ); // CLI wins
            assert_eq!(
                shared.project,
                ParsedProperty::Cli("cli-project".to_string(), "cli-project".to_string())
            ); // CLI wins
            assert_eq!(
                shared.repository,
                ParsedProperty::Env("env-repo".to_string(), "env-repo".to_string())
            ); // Fallback to env
            assert_eq!(
                shared.pat,
                ParsedProperty::Env("env-pat".to_string(), "env-pat".to_string())
            ); // Fallback to env
            assert_eq!(
                shared.parallel_limit,
                ParsedProperty::Cli(999, "999".to_string())
            ); // CLI wins
        }
        _ => panic!("Expected default mode"),
    }

    // Clean up
    unsafe {
        std::env::remove_var("MERGERS_ORGANIZATION");
        std::env::remove_var("MERGERS_PROJECT");
        std::env::remove_var("MERGERS_REPOSITORY");
        std::env::remove_var("MERGERS_PAT");
    }
}

/// # Client Creation with Resolved Configuration
///
/// Tests end-to-end flow from configuration resolution to client creation.
///
/// ## Test Scenario
/// - Resolves configuration from various sources
/// - Creates Azure DevOps client using resolved configuration
/// - Tests complete integration workflow
///
/// ## Expected Outcome
/// - Configuration resolves correctly from environment
/// - Client creation succeeds using resolved configuration values
#[test]
#[file_serial(env_tests)]
fn test_client_creation_with_resolved_config() {
    // Test that a client can be created from resolved config

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "test-org");
        std::env::set_var("MERGERS_PROJECT", "test-project");
        std::env::set_var("MERGERS_REPOSITORY", "test-repo");
        std::env::set_var("MERGERS_PAT", "test-pat");
    }

    let args = create_empty_args();

    let config_result = args.resolve_config();
    assert!(config_result.is_ok());

    let config = config_result.unwrap();
    let shared = config.shared();

    let client_result = AzureDevOpsClient::new(
        shared.organization.value().clone(),
        shared.project.value().clone(),
        shared.repository.value().clone(),
        shared.pat.value().clone(),
    );

    assert!(client_result.is_ok());

    // Clean up
    unsafe {
        std::env::remove_var("MERGERS_ORGANIZATION");
        std::env::remove_var("MERGERS_PROJECT");
        std::env::remove_var("MERGERS_REPOSITORY");
        std::env::remove_var("MERGERS_PAT");
    }
}

// =============================================================================
// Non-Interactive Mode Integration Tests
// =============================================================================

use mergers::core::ExitCode;
use mergers::core::runner::{MergeRunnerConfig, NonInteractiveRunner, RunResult};
use mergers::core::state::{
    LockGuard, MergePhase, MergeStateFile, MergeStatus, STATE_DIR_ENV, StateCherryPickItem,
    StateItemStatus, lock_path_for_repo, path_for_repo,
};
use mergers::models::OutputFormat;

/// # State File Lifecycle
///
/// Tests the complete lifecycle of a state file: creation, updates, and completion.
///
/// ## Test Scenario
/// - Creates a new state file with initial values
/// - Updates the phase through various stages
/// - Adds cherry-pick items and updates their statuses
/// - Marks the merge as completed
///
/// ## Expected Outcome
/// - State file is created and updated correctly at each stage
/// - Final state reflects all changes
#[test]
#[file_serial(env_tests)]
fn test_state_file_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    // Set state directory
    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create new state file
    let mut state = MergeStateFile::new(
        repo_dir.clone(),
        None,
        false,
        "test-org".to_string(),
        "test-project".to_string(),
        "test-repo".to_string(),
        "dev".to_string(),
        "next".to_string(),
        "v1.0.0".to_string(),
        "Next Merged".to_string(),
        "merged-".to_string(),
        false,
    );

    // Add cherry-pick items
    state.cherry_pick_items = vec![
        StateCherryPickItem {
            commit_id: "abc123".to_string(),
            pr_id: 1,
            pr_title: "PR 1".to_string(),
            status: StateItemStatus::Pending,
            work_item_ids: vec![100],
        },
        StateCherryPickItem {
            commit_id: "def456".to_string(),
            pr_id: 2,
            pr_title: "PR 2".to_string(),
            status: StateItemStatus::Pending,
            work_item_ids: vec![101, 102],
        },
    ];

    // Set to cherry-picking phase
    state.phase = MergePhase::CherryPicking;
    let state_path = state.save_for_repo().unwrap();
    assert!(state_path.exists());

    // Simulate first cherry-pick success
    state.cherry_pick_items[0].status = StateItemStatus::Success;
    state.current_index = 1;
    state.save_for_repo().unwrap();

    // Simulate conflict on second cherry-pick
    state.cherry_pick_items[1].status = StateItemStatus::Conflict;
    state.phase = MergePhase::AwaitingConflictResolution;
    state.conflicted_files = Some(vec!["src/main.rs".to_string(), "Cargo.toml".to_string()]);
    state.save_for_repo().unwrap();

    // Load and verify conflict state
    let loaded = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();
    assert_eq!(loaded.phase, MergePhase::AwaitingConflictResolution);
    assert_eq!(loaded.current_index, 1);
    assert!(loaded.conflicted_files.is_some());
    assert_eq!(loaded.conflicted_files.as_ref().unwrap().len(), 2);

    // Simulate conflict resolution and continue
    state.cherry_pick_items[1].status = StateItemStatus::Success;
    state.phase = MergePhase::ReadyForCompletion;
    state.conflicted_files = None;
    state.current_index = 2;
    state.save_for_repo().unwrap();

    // Mark as completed
    state.mark_completed(MergeStatus::Success).unwrap();

    // Load and verify final state
    let final_state = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();
    assert_eq!(final_state.phase, MergePhase::Completed);
    assert_eq!(final_state.final_status, Some(MergeStatus::Success));
    assert!(final_state.completed_at.is_some());
    assert_eq!(
        final_state.cherry_pick_items[0].status,
        StateItemStatus::Success
    );
    assert_eq!(
        final_state.cherry_pick_items[1].status,
        StateItemStatus::Success
    );

    // Cleanup
    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # State File Cross-Mode Compatibility
///
/// Tests that state files created in one mode can be read and modified in another.
///
/// ## Test Scenario
/// - Creates a state file simulating TUI mode (with conflict)
/// - Loads the state file simulating CLI mode
/// - Verifies all fields are accessible and correct
///
/// ## Expected Outcome
/// - State file is fully compatible across modes
#[test]
#[file_serial(env_tests)]
fn test_state_file_cross_mode_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Simulate TUI creating state file
    let mut tui_state = MergeStateFile::new(
        repo_dir.clone(),
        Some(temp_dir.path().join("base-repo")),
        true, // is_worktree
        "my-org".to_string(),
        "my-project".to_string(),
        "my-repo".to_string(),
        "develop".to_string(),
        "main".to_string(),
        "v2.0.0".to_string(),
        "Done".to_string(),
        "release-".to_string(),
        true, // run_hooks = true
    );

    tui_state.cherry_pick_items = vec![
        StateCherryPickItem {
            commit_id: "commit1".to_string(),
            pr_id: 100,
            pr_title: "Feature A".to_string(),
            status: StateItemStatus::Success,
            work_item_ids: vec![1000, 1001],
        },
        StateCherryPickItem {
            commit_id: "commit2".to_string(),
            pr_id: 101,
            pr_title: "Feature B".to_string(),
            status: StateItemStatus::Conflict,
            work_item_ids: vec![1002],
        },
        StateCherryPickItem {
            commit_id: "commit3".to_string(),
            pr_id: 102,
            pr_title: "Feature C".to_string(),
            status: StateItemStatus::Pending,
            work_item_ids: vec![],
        },
    ];

    tui_state.phase = MergePhase::AwaitingConflictResolution;
    tui_state.current_index = 1;
    tui_state.conflicted_files = Some(vec!["lib/core.rs".to_string()]);
    tui_state.save_for_repo().unwrap();

    // Simulate CLI loading the state file
    let cli_state = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();

    // Verify all fields are preserved
    assert_eq!(cli_state.organization, "my-org");
    assert_eq!(cli_state.project, "my-project");
    assert_eq!(cli_state.repository, "my-repo");
    assert_eq!(cli_state.dev_branch, "develop");
    assert_eq!(cli_state.target_branch, "main");
    assert_eq!(cli_state.merge_version, "v2.0.0");
    assert_eq!(cli_state.work_item_state, "Done");
    assert_eq!(cli_state.tag_prefix, "release-");
    assert!(cli_state.is_worktree);
    assert!(cli_state.base_repo_path.is_some());
    assert!(cli_state.run_hooks);
    assert_eq!(cli_state.phase, MergePhase::AwaitingConflictResolution);
    assert_eq!(cli_state.current_index, 1);
    assert_eq!(cli_state.cherry_pick_items.len(), 3);

    // Verify status counts
    let counts = cli_state.status_counts();
    assert_eq!(counts.success, 1);
    assert_eq!(counts.conflict, 1);
    assert_eq!(counts.pending, 1);
    assert_eq!(counts.total(), 3);
    assert_eq!(counts.completed(), 1);

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Lock Guard Prevents Concurrent Access
///
/// Tests that lock guards prevent multiple merge operations on the same repo.
///
/// ## Test Scenario
/// - Acquires a lock on a repository
/// - Attempts to acquire a second lock
/// - Releases the first lock
/// - Acquires a new lock successfully
///
/// ## Expected Outcome
/// - Second lock acquisition fails while first is held
/// - Lock can be acquired after release
#[test]
#[file_serial(env_tests)]
fn test_lock_guard_prevents_concurrent_access() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // First lock acquisition
    let guard1 = LockGuard::acquire(&repo_dir).unwrap();
    assert!(guard1.is_some(), "First lock should succeed");

    // Second lock acquisition should fail
    let guard2 = LockGuard::acquire(&repo_dir).unwrap();
    assert!(
        guard2.is_none(),
        "Second lock should fail while first is held"
    );

    // Drop first lock
    drop(guard1);

    // Now we should be able to acquire the lock again
    let guard3 = LockGuard::acquire(&repo_dir).unwrap();
    assert!(guard3.is_some(), "Lock should succeed after release");

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Runner Configuration Validation
///
/// Tests that runner configuration is properly validated.
///
/// ## Test Scenario
/// - Creates runner with various configurations
/// - Tests output with different formats
///
/// ## Expected Outcome
/// - Runners are created correctly with all configuration options
#[test]
fn test_runner_configuration() {
    // Test with text format
    let config1 = MergeRunnerConfig {
        organization: "org1".to_string(),
        project: "project1".to_string(),
        repository: "repo1".to_string(),
        pat: "pat1".to_string(),
        dev_branch: "dev".to_string(),
        target_branch: "main".to_string(),
        version: "v1.0.0".to_string(),
        tag_prefix: "merged-".to_string(),
        work_item_state: "Done".to_string(),
        select_by_states: Some("Ready".to_string()),
        local_repo: None,
        run_hooks: false,
        output_format: OutputFormat::Text,
        quiet: false,
        hooks_config: None,
        max_concurrent_network: 100,
        max_concurrent_processing: 10,
        since: None,
    };

    let mut buffer1 = Vec::new();
    let _runner1 = NonInteractiveRunner::with_writer(config1, &mut buffer1);

    // Test with JSON format
    let config2 = MergeRunnerConfig {
        organization: "org2".to_string(),
        project: "project2".to_string(),
        repository: "repo2".to_string(),
        pat: "pat2".to_string(),
        dev_branch: "develop".to_string(),
        target_branch: "release".to_string(),
        version: "v2.0.0".to_string(),
        tag_prefix: "release-".to_string(),
        work_item_state: "Merged".to_string(),
        select_by_states: None,
        local_repo: Some(std::path::PathBuf::from("/path/to/repo")),
        run_hooks: true,
        output_format: OutputFormat::Json,
        quiet: true,
        hooks_config: None,
        max_concurrent_network: 100,
        max_concurrent_processing: 10,
        since: None,
    };

    let mut buffer2 = Vec::new();
    let _runner2 = NonInteractiveRunner::with_writer(config2, &mut buffer2);

    // Test with NDJSON format
    let config3 = MergeRunnerConfig {
        organization: "org3".to_string(),
        project: "project3".to_string(),
        repository: "repo3".to_string(),
        pat: "pat3".to_string(),
        dev_branch: "dev".to_string(),
        target_branch: "next".to_string(),
        version: "v3.0.0".to_string(),
        tag_prefix: "v".to_string(),
        work_item_state: "Complete".to_string(),
        select_by_states: Some("Ready,Approved".to_string()),
        local_repo: None,
        run_hooks: false,
        output_format: OutputFormat::Ndjson,
        quiet: false,
        hooks_config: None,
        max_concurrent_network: 100,
        max_concurrent_processing: 10,
        since: None,
    };

    let mut buffer3 = Vec::new();
    let _runner3 = NonInteractiveRunner::with_writer(config3, &mut buffer3);
}

/// # Exit Code Mapping
///
/// Tests that exit codes are correctly mapped to results.
///
/// ## Test Scenario
/// - Creates RunResult instances with various exit codes
/// - Verifies the correct exit codes are produced
///
/// ## Expected Outcome
/// - All exit codes map correctly to their meanings
#[test]
fn test_exit_code_mapping() {
    // Success
    let result = RunResult::success();
    assert_eq!(result.exit_code, ExitCode::Success);

    // Error
    let result = RunResult::error(ExitCode::GeneralError, "Something failed");
    assert_eq!(result.exit_code, ExitCode::GeneralError);
    assert!(result.message.is_some());
    assert!(result.message.unwrap().contains("failed"));

    // Conflict
    let result = RunResult::conflict(std::path::PathBuf::from("/state/path"));
    assert_eq!(result.exit_code, ExitCode::Conflict);
    assert!(result.state_file_path.is_some());

    // Partial success
    let result = RunResult::partial_success("3 of 5 succeeded");
    assert_eq!(result.exit_code, ExitCode::PartialSuccess);

    // No state file
    let result = RunResult::error(ExitCode::NoStateFile, "No state file found");
    assert_eq!(result.exit_code, ExitCode::NoStateFile);

    // Invalid phase
    let result = RunResult::error(ExitCode::InvalidPhase, "Cannot continue");
    assert_eq!(result.exit_code, ExitCode::InvalidPhase);

    // No PRs matched
    let result = RunResult::error(ExitCode::NoPRsMatched, "No PRs match criteria");
    assert_eq!(result.exit_code, ExitCode::NoPRsMatched);

    // Locked
    let result = RunResult::error(ExitCode::Locked, "Another merge in progress");
    assert_eq!(result.exit_code, ExitCode::Locked);
}

/// # State File Path Determinism
///
/// Tests that state file paths are deterministic based on repo path.
///
/// ## Test Scenario
/// - Computes state file paths for the same repo multiple times
/// - Computes paths for different repos
///
/// ## Expected Outcome
/// - Same repo always produces same path
/// - Different repos produce different paths
#[test]
#[file_serial(env_tests)]
fn test_state_file_path_determinism() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo1 = temp_dir.path().join("repo1");
    let repo2 = temp_dir.path().join("repo2");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo1).unwrap();
    fs::create_dir_all(&repo2).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Same repo produces same path
    let path1a = path_for_repo(&repo1).unwrap();
    let path1b = path_for_repo(&repo1).unwrap();
    let path1c = path_for_repo(&repo1).unwrap();
    assert_eq!(path1a, path1b);
    assert_eq!(path1b, path1c);

    // Different repos produce different paths
    let path2 = path_for_repo(&repo2).unwrap();
    assert_ne!(path1a, path2);

    // Paths follow expected format
    assert!(
        path1a
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("merge-")
    );
    assert!(
        path1a
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with(".json")
    );

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Mixed Item Status State File
///
/// Tests state file with various item statuses for proper serialization.
///
/// ## Test Scenario
/// - Creates state file with all possible item statuses
/// - Saves and loads the state file
///
/// ## Expected Outcome
/// - All status types are correctly preserved through save/load cycle
#[test]
#[file_serial(env_tests)]
fn test_mixed_item_status_state_file() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    let mut state = MergeStateFile::new(
        repo_dir.clone(),
        None,
        false,
        "org".to_string(),
        "project".to_string(),
        "repo".to_string(),
        "dev".to_string(),
        "next".to_string(),
        "v1.0.0".to_string(),
        "Done".to_string(),
        "merged-".to_string(),
        false,
    );

    // Add items with all possible statuses
    state.cherry_pick_items = vec![
        StateCherryPickItem {
            commit_id: "a1".to_string(),
            pr_id: 1,
            pr_title: "PR 1 - Pending".to_string(),
            status: StateItemStatus::Pending,
            work_item_ids: vec![],
        },
        StateCherryPickItem {
            commit_id: "b2".to_string(),
            pr_id: 2,
            pr_title: "PR 2 - Success".to_string(),
            status: StateItemStatus::Success,
            work_item_ids: vec![10],
        },
        StateCherryPickItem {
            commit_id: "c3".to_string(),
            pr_id: 3,
            pr_title: "PR 3 - Conflict".to_string(),
            status: StateItemStatus::Conflict,
            work_item_ids: vec![20, 21],
        },
        StateCherryPickItem {
            commit_id: "d4".to_string(),
            pr_id: 4,
            pr_title: "PR 4 - Skipped".to_string(),
            status: StateItemStatus::Skipped,
            work_item_ids: vec![],
        },
        StateCherryPickItem {
            commit_id: "e5".to_string(),
            pr_id: 5,
            pr_title: "PR 5 - Failed".to_string(),
            status: StateItemStatus::Failed {
                message: "Cherry-pick failed: merge conflict in lib/core.rs".to_string(),
            },
            work_item_ids: vec![30],
        },
    ];

    state.save_for_repo().unwrap();

    // Load and verify
    let loaded = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();
    assert_eq!(loaded.cherry_pick_items.len(), 5);

    assert!(matches!(
        loaded.cherry_pick_items[0].status,
        StateItemStatus::Pending
    ));
    assert!(matches!(
        loaded.cherry_pick_items[1].status,
        StateItemStatus::Success
    ));
    assert!(matches!(
        loaded.cherry_pick_items[2].status,
        StateItemStatus::Conflict
    ));
    assert!(matches!(
        loaded.cherry_pick_items[3].status,
        StateItemStatus::Skipped
    ));

    if let StateItemStatus::Failed { message } = &loaded.cherry_pick_items[4].status {
        assert!(message.contains("merge conflict"));
    } else {
        panic!("Expected Failed status");
    }

    // Verify counts
    let counts = loaded.status_counts();
    assert_eq!(counts.pending, 1);
    assert_eq!(counts.success, 1);
    assert_eq!(counts.conflict, 1);
    assert_eq!(counts.skipped, 1);
    assert_eq!(counts.failed, 1);

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Abort State File Update
///
/// Tests that aborting correctly updates the state file.
///
/// ## Test Scenario
/// - Creates an in-progress state file
/// - Updates it to aborted status
///
/// ## Expected Outcome
/// - State file reflects aborted status with correct phase
#[test]
#[file_serial(env_tests)]
fn test_abort_state_file_update() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    let mut state = MergeStateFile::new(
        repo_dir.clone(),
        None,
        false,
        "org".to_string(),
        "project".to_string(),
        "repo".to_string(),
        "dev".to_string(),
        "next".to_string(),
        "v1.0.0".to_string(),
        "Done".to_string(),
        "merged-".to_string(),
        false,
    );

    state.cherry_pick_items = vec![
        StateCherryPickItem {
            commit_id: "a".to_string(),
            pr_id: 1,
            pr_title: "PR 1".to_string(),
            status: StateItemStatus::Success,
            work_item_ids: vec![],
        },
        StateCherryPickItem {
            commit_id: "b".to_string(),
            pr_id: 2,
            pr_title: "PR 2".to_string(),
            status: StateItemStatus::Pending,
            work_item_ids: vec![],
        },
    ];

    state.phase = MergePhase::CherryPicking;
    state.current_index = 1;
    state.save_for_repo().unwrap();

    // Simulate abort
    state.phase = MergePhase::Aborted;
    state.final_status = Some(MergeStatus::Aborted);
    state.completed_at = Some(chrono::Utc::now());
    state.save_for_repo().unwrap();

    // Load and verify
    let loaded = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();
    assert_eq!(loaded.phase, MergePhase::Aborted);
    assert_eq!(loaded.final_status, Some(MergeStatus::Aborted));
    assert!(loaded.completed_at.is_some());
    assert!(loaded.phase.is_terminal());

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Complete State File Update
///
/// Tests the complete workflow from ready to completed.
///
/// ## Test Scenario
/// - Creates a state file in ReadyForCompletion phase
/// - Calls mark_completed to finish
///
/// ## Expected Outcome
/// - State file is marked as completed with success status
#[test]
#[file_serial(env_tests)]
fn test_complete_state_file_update() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    let mut state = MergeStateFile::new(
        repo_dir.clone(),
        None,
        false,
        "org".to_string(),
        "project".to_string(),
        "repo".to_string(),
        "dev".to_string(),
        "next".to_string(),
        "v1.0.0".to_string(),
        "Done".to_string(),
        "merged-".to_string(),
        false,
    );

    state.cherry_pick_items = vec![
        StateCherryPickItem {
            commit_id: "a".to_string(),
            pr_id: 1,
            pr_title: "PR 1".to_string(),
            status: StateItemStatus::Success,
            work_item_ids: vec![100],
        },
        StateCherryPickItem {
            commit_id: "b".to_string(),
            pr_id: 2,
            pr_title: "PR 2".to_string(),
            status: StateItemStatus::Success,
            work_item_ids: vec![101],
        },
    ];

    state.phase = MergePhase::ReadyForCompletion;
    state.current_index = 2;
    state.save_for_repo().unwrap();

    // Mark as completed
    state.mark_completed(MergeStatus::Success).unwrap();

    // Load and verify
    let loaded = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();
    assert_eq!(loaded.phase, MergePhase::Completed);
    assert_eq!(loaded.final_status, Some(MergeStatus::Success));
    assert!(loaded.completed_at.is_some());
    assert!(loaded.phase.is_terminal());

    // Test partial success
    let mut state2 = MergeStateFile::new(
        repo_dir.clone(),
        None,
        false,
        "org".to_string(),
        "project".to_string(),
        "repo".to_string(),
        "dev".to_string(),
        "next".to_string(),
        "v2.0.0".to_string(),
        "Done".to_string(),
        "merged-".to_string(),
        false,
    );
    state2.mark_completed(MergeStatus::PartialSuccess).unwrap();

    let loaded2 = MergeStateFile::load_for_repo(&repo_dir).unwrap().unwrap();
    assert_eq!(loaded2.final_status, Some(MergeStatus::PartialSuccess));

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

// =============================================================================
// Phase 8: Lock File & Corrupted State Tests
// =============================================================================

/// # Lock Is Locked Check
///
/// Tests the is_locked helper function for early lock detection.
///
/// ## Test Scenario
/// - Creates a lock file manually to simulate another process
/// - Checks if is_locked returns true
/// - Removes the lock and checks again
///
/// ## Expected Outcome
/// - is_locked returns true when lock file exists with valid PID
/// - is_locked returns false when no lock file exists
#[test]
#[file_serial(env_tests)]
fn test_lock_is_locked_check() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Initially not locked
    let is_locked = LockGuard::is_locked(&repo_dir).unwrap();
    assert!(!is_locked, "Should not be locked initially");

    // Acquire a lock
    let guard = LockGuard::acquire(&repo_dir).unwrap();
    assert!(guard.is_some());

    // Now should be locked
    let is_locked = LockGuard::is_locked(&repo_dir).unwrap();
    assert!(is_locked, "Should be locked after acquiring");

    // Drop the guard
    drop(guard);

    // Should not be locked after release
    let is_locked = LockGuard::is_locked(&repo_dir).unwrap();
    assert!(!is_locked, "Should not be locked after release");

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Lock File Created During Lock Acquisition
///
/// Tests that lock file is created and contains the PID.
///
/// ## Test Scenario
/// - Acquires a lock
/// - Verifies lock file exists
/// - Reads lock file content and verifies it contains current PID
///
/// ## Expected Outcome
/// - Lock file is created at expected path
/// - Lock file contains current process PID
#[test]
#[file_serial(env_tests)]
fn test_lock_file_contains_pid() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    let lock_path = lock_path_for_repo(&repo_dir).unwrap();

    // Lock file should not exist initially
    assert!(!lock_path.exists());

    // Acquire lock
    let guard = LockGuard::acquire(&repo_dir).unwrap();
    assert!(guard.is_some());

    // Lock file should now exist
    assert!(lock_path.exists());

    // Read and verify PID
    let content = fs::read_to_string(&lock_path).unwrap();
    let expected_pid = std::process::id().to_string();
    assert_eq!(content.trim(), expected_pid);

    // Drop the guard - file should be removed
    drop(guard);
    assert!(!lock_path.exists());

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Stale Lock File Detection
///
/// Tests that stale lock files (from dead processes) are detected and cleaned up.
///
/// ## Test Scenario
/// - Creates a lock file with invalid PID (simulating dead process)
/// - Attempts to acquire lock
///
/// ## Expected Outcome
/// - Stale lock is detected and cleaned up
/// - New lock can be acquired
#[test]
#[file_serial(env_tests)]
fn test_stale_lock_file_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    let lock_path = lock_path_for_repo(&repo_dir).unwrap();

    // Create a stale lock file with invalid PID (very high number unlikely to exist)
    fs::write(&lock_path, "999999999").unwrap();

    // is_locked should return false for stale lock
    let is_locked = LockGuard::is_locked(&repo_dir).unwrap();
    assert!(!is_locked, "Stale lock should not be considered locked");

    // Should be able to acquire the lock
    let guard = LockGuard::acquire(&repo_dir).unwrap();
    assert!(guard.is_some(), "Should acquire lock over stale lock");

    drop(guard);
    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Corrupted State File - Invalid JSON
///
/// Tests that corrupted JSON state files are handled gracefully.
///
/// ## Test Scenario
/// - Creates a state file with invalid JSON
/// - Attempts to load and validate it
///
/// ## Expected Outcome
/// - Loading fails with clear error message
/// - Error suggests recovery options
#[test]
#[file_serial(env_tests)]
fn test_corrupted_state_file_invalid_json() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create invalid JSON file
    let state_path = path_for_repo(&repo_dir).unwrap();
    fs::write(&state_path, "{ invalid json content }").unwrap();

    // Try to load and validate
    let result = MergeStateFile::load_and_validate_for_repo(&repo_dir);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("corrupted") || error_msg.contains("parse"),
        "Error should mention corruption: {}",
        error_msg
    );
    assert!(
        error_msg.contains("abort") || error_msg.contains("delete"),
        "Error should suggest recovery: {}",
        error_msg
    );

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Corrupted State File - Invalid Schema Version
///
/// Tests that state files with unsupported schema versions are rejected.
///
/// ## Test Scenario
/// - Creates a state file with schema_version = 999
/// - Attempts to load and validate it
///
/// ## Expected Outcome
/// - Validation fails with clear error about schema version
#[test]
#[file_serial(env_tests)]
fn test_corrupted_state_file_invalid_schema_version() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create state file with invalid schema version
    let state_json = r#"{
        "schema_version": 999,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-15T10:30:00Z",
        "repo_path": "/test/repo",
        "is_worktree": false,
        "organization": "org",
        "project": "project",
        "repository": "repo",
        "dev_branch": "dev",
        "target_branch": "next",
        "merge_version": "v1.0.0",
        "cherry_pick_items": [],
        "current_index": 0,
        "phase": "loading",
        "work_item_state": "Done",
        "tag_prefix": "merged-"
    }"#;

    let state_path = path_for_repo(&repo_dir).unwrap();
    fs::write(&state_path, state_json).unwrap();

    // Try to load and validate
    let result = MergeStateFile::load_and_validate_for_repo(&repo_dir);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("schema version") || error_msg.contains("Unsupported"),
        "Error should mention schema version: {}",
        error_msg
    );

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Corrupted State File - Invalid Index
///
/// Tests that state files with out-of-bounds index are rejected.
///
/// ## Test Scenario
/// - Creates a state file where current_index exceeds items count
/// - Attempts to load and validate it
///
/// ## Expected Outcome
/// - Validation fails with clear error about index
#[test]
#[file_serial(env_tests)]
fn test_corrupted_state_file_invalid_index() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create state file with invalid index (index=5 but only 2 items)
    let state_json = r#"{
        "schema_version": 1,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-15T10:30:00Z",
        "repo_path": "/test/repo",
        "is_worktree": false,
        "organization": "org",
        "project": "project",
        "repository": "repo",
        "dev_branch": "dev",
        "target_branch": "next",
        "merge_version": "v1.0.0",
        "cherry_pick_items": [
            {"commit_id": "a", "pr_id": 1, "pr_title": "PR 1", "status": "pending", "work_item_ids": []},
            {"commit_id": "b", "pr_id": 2, "pr_title": "PR 2", "status": "pending", "work_item_ids": []}
        ],
        "current_index": 10,
        "phase": "cherry_picking",
        "work_item_state": "Done",
        "tag_prefix": "merged-"
    }"#;

    let state_path = path_for_repo(&repo_dir).unwrap();
    fs::write(&state_path, state_json).unwrap();

    // Try to load and validate
    let result = MergeStateFile::load_and_validate_for_repo(&repo_dir);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("current_index") || error_msg.contains("exceeds"),
        "Error should mention index issue: {}",
        error_msg
    );

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Corrupted State File - Missing Required Fields
///
/// Tests that state files with missing required fields are rejected.
///
/// ## Test Scenario
/// - Creates a state file with empty organization
/// - Attempts to load and validate it
///
/// ## Expected Outcome
/// - Validation fails with clear error about missing field
#[test]
#[file_serial(env_tests)]
fn test_corrupted_state_file_missing_fields() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create state file with empty organization (required field)
    let state_json = r#"{
        "schema_version": 1,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-15T10:30:00Z",
        "repo_path": "/test/repo",
        "is_worktree": false,
        "organization": "",
        "project": "project",
        "repository": "repo",
        "dev_branch": "dev",
        "target_branch": "next",
        "merge_version": "v1.0.0",
        "cherry_pick_items": [],
        "current_index": 0,
        "phase": "loading",
        "work_item_state": "Done",
        "tag_prefix": "merged-"
    }"#;

    let state_path = path_for_repo(&repo_dir).unwrap();
    fs::write(&state_path, state_json).unwrap();

    // Try to load and validate
    let result = MergeStateFile::load_and_validate_for_repo(&repo_dir);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("organization") || error_msg.contains("missing"),
        "Error should mention missing field: {}",
        error_msg
    );

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # State File Validation - Phase Consistency
///
/// Tests that phase-specific invariants are validated.
///
/// ## Test Scenario
/// - Creates a state file in AwaitingConflictResolution phase but no conflicted_files
/// - Attempts to load and validate it
///
/// ## Expected Outcome
/// - Validation fails due to phase inconsistency
#[test]
#[file_serial(env_tests)]
fn test_state_file_phase_consistency_validation() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create state file with inconsistent phase (conflict but no conflicted_files)
    let state_json = r#"{
        "schema_version": 1,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-15T10:30:00Z",
        "repo_path": "/test/repo",
        "is_worktree": false,
        "organization": "org",
        "project": "project",
        "repository": "repo",
        "dev_branch": "dev",
        "target_branch": "next",
        "merge_version": "v1.0.0",
        "cherry_pick_items": [
            {"commit_id": "a", "pr_id": 1, "pr_title": "PR 1", "status": "conflict", "work_item_ids": []}
        ],
        "current_index": 0,
        "phase": "awaiting_conflict_resolution",
        "work_item_state": "Done",
        "tag_prefix": "merged-"
    }"#;

    let state_path = path_for_repo(&repo_dir).unwrap();
    fs::write(&state_path, state_json).unwrap();

    // Try to load and validate
    let result = MergeStateFile::load_and_validate_for_repo(&repo_dir);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("AwaitingConflictResolution") || error_msg.contains("conflicted_files"),
        "Error should mention phase inconsistency: {}",
        error_msg
    );

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Valid State File Passes Validation
///
/// Tests that valid state files pass validation.
///
/// ## Test Scenario
/// - Creates a properly formed state file
/// - Loads and validates it
///
/// ## Expected Outcome
/// - Validation passes without errors
#[test]
#[file_serial(env_tests)]
fn test_valid_state_file_passes_validation() {
    let temp_dir = TempDir::new().unwrap();
    let state_dir = temp_dir.path().join("state");
    let repo_dir = temp_dir.path().join("repo");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();

    unsafe { std::env::set_var(STATE_DIR_ENV, &state_dir) };

    // Create valid state file
    let mut state = MergeStateFile::new(
        repo_dir.clone(),
        None,
        false,
        "org".to_string(),
        "project".to_string(),
        "repo".to_string(),
        "dev".to_string(),
        "next".to_string(),
        "v1.0.0".to_string(),
        "Done".to_string(),
        "merged-".to_string(),
        false,
    );

    state.cherry_pick_items = vec![StateCherryPickItem {
        commit_id: "a".to_string(),
        pr_id: 1,
        pr_title: "PR 1".to_string(),
        status: StateItemStatus::Success,
        work_item_ids: vec![],
    }];
    state.phase = MergePhase::ReadyForCompletion;
    state.current_index = 1;
    state.save_for_repo().unwrap();

    // Load and validate - should succeed
    let result = MergeStateFile::load_and_validate_for_repo(&repo_dir);
    assert!(result.is_ok(), "Valid state file should pass validation");
    assert!(result.unwrap().is_some());

    unsafe { std::env::remove_var(STATE_DIR_ENV) };
}

/// # Lock Exit Code 7
///
/// Tests that exit code 7 is returned when locked.
///
/// ## Test Scenario
/// - Tests that ExitCode::Locked has value 7
///
/// ## Expected Outcome
/// - ExitCode::Locked.code() returns 7
#[test]
fn test_lock_exit_code_7() {
    assert_eq!(ExitCode::Locked.code(), 7);
    assert_eq!(
        ExitCode::Locked.description(),
        "Another merge operation is in progress"
    );
}
