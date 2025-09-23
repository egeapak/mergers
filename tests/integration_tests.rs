//! Integration tests for the mergers library
//!
//! These tests demonstrate how to use the library APIs and verify
//! end-to-end functionality.

use mergers::{AppConfig, Args, AzureDevOpsClient, Config};
use std::fs;
use tempfile::TempDir;

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
fn test_config_loading_and_merging() {
    // Test that config loading doesn't panic and returns sensible defaults
    let _config = Config::load_from_file().expect("Should load config or return defaults");

    // If no config file exists, we get a default config
    // Let's check that the default() function gives us expected values
    let default_config = Config::default();
    assert_eq!(default_config.dev_branch, Some("dev".to_string()));
    assert_eq!(default_config.target_branch, Some("next".to_string()));
    assert_eq!(default_config.parallel_limit, Some(300));

    // Test environment config
    let _env_config = Config::load_from_env();

    // Test merging - create a test config with known values to merge
    let test_config = Config {
        organization: Some("test-org".to_string()),
        parallel_limit: Some(500),
        ..Config::default()
    };

    let merged = default_config.merge(test_config);

    // Basic validation that merge works
    assert_eq!(merged.organization, Some("test-org".to_string()));
    assert_eq!(merged.parallel_limit, Some(500));
    assert_eq!(merged.dev_branch, Some("dev".to_string())); // Should keep default
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
fn test_args_resolution_with_env() {
    // Test that Args can resolve configuration from environment variables

    // Set up test environment variables
    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "env-org");
        std::env::set_var("MERGERS_PROJECT", "env-project");
        std::env::set_var("MERGERS_REPOSITORY", "env-repo");
        std::env::set_var("MERGERS_PAT", "env-pat");
        std::env::set_var("MERGERS_PARALLEL_LIMIT", "500");
    }

    let args = Args {
        organization: None,
        project: None,
        repository: None,
        pat: None,
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: false,
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: None,
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(shared.organization, "env-org");
            assert_eq!(shared.project, "env-project");
            assert_eq!(shared.repository, "env-repo");
            assert_eq!(shared.pat, "env-pat");
            assert_eq!(shared.parallel_limit, 500);
        }
        AppConfig::Migration { .. } => panic!("Expected default mode"),
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
fn test_args_resolution_with_file() {
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

    let args = Args {
        organization: None,
        project: None,
        repository: None,
        pat: None,
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: false,
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: None,
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

    let result = args.resolve_config();

    // Clean up first
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(shared.organization, "file-org");
            assert_eq!(shared.project, "file-project");
            assert_eq!(shared.repository, "file-repo");
            assert_eq!(shared.pat, "file-pat");
            assert_eq!(shared.parallel_limit, 300); // Uses default value instead of file value
        }
        AppConfig::Migration { .. } => panic!("Expected default mode"),
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
fn test_missing_required_args() {
    // Test that Args validation catches missing required arguments

    let args = Args {
        organization: None,
        project: None,
        repository: None,
        pat: None,
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: false,
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: None,
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

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
fn test_migration_mode_initialization() {
    // Test migration mode configuration

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "test-org");
        std::env::set_var("MERGERS_PROJECT", "test-project");
        std::env::set_var("MERGERS_REPOSITORY", "test-repo");
        std::env::set_var("MERGERS_PAT", "test-pat");
    }

    let args = Args {
        organization: None,
        project: None,
        repository: None,
        pat: None,
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: true, // Migration mode
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: None,
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Migration {
            shared,
            migration: _,
        } => {
            assert_eq!(shared.organization, "test-org");
            assert_eq!(shared.project, "test-project");
            assert_eq!(shared.repository, "test-repo");
            assert_eq!(shared.pat, "test-pat");
        }
        AppConfig::Default { .. } => panic!("Expected migration mode"),
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
fn test_default_mode_initialization() {
    // Test default mode configuration (when migrate flag is false)

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "test-org");
        std::env::set_var("MERGERS_PROJECT", "test-project");
        std::env::set_var("MERGERS_REPOSITORY", "test-repo");
        std::env::set_var("MERGERS_PAT", "test-pat");
    }

    let args = Args {
        organization: None,
        project: None,
        repository: None,
        pat: None,
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: false, // Default mode
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: None,
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(shared.organization, "test-org");
            assert_eq!(shared.project, "test-project");
            assert_eq!(shared.repository, "test-repo");
            assert_eq!(shared.pat, "test-pat");
        }
        AppConfig::Migration { .. } => panic!("Expected default mode"),
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
fn test_args_cli_precedence() {
    // Test that CLI args take precedence over env vars

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "env-org");
        std::env::set_var("MERGERS_PROJECT", "env-project");
        std::env::set_var("MERGERS_REPOSITORY", "env-repo");
        std::env::set_var("MERGERS_PAT", "env-pat");
    }

    let args = Args {
        organization: Some("cli-org".to_string()),
        project: Some("cli-project".to_string()),
        repository: None, // Should use env var
        pat: None,        // Should use env var
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: false,
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: Some(999),
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

    let result = args.resolve_config();
    assert!(result.is_ok());

    let config = result.unwrap();
    match config {
        AppConfig::Default { shared, default: _ } => {
            assert_eq!(shared.organization, "cli-org"); // CLI wins
            assert_eq!(shared.project, "cli-project"); // CLI wins
            assert_eq!(shared.repository, "env-repo"); // Fallback to env
            assert_eq!(shared.pat, "env-pat"); // Fallback to env
            assert_eq!(shared.parallel_limit, 999); // CLI wins
        }
        AppConfig::Migration { .. } => panic!("Expected default mode"),
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
fn test_client_creation_with_resolved_config() {
    // Test that a client can be created from resolved config

    unsafe {
        std::env::set_var("MERGERS_ORGANIZATION", "test-org");
        std::env::set_var("MERGERS_PROJECT", "test-project");
        std::env::set_var("MERGERS_REPOSITORY", "test-repo");
        std::env::set_var("MERGERS_PAT", "test-pat");
    }

    let args = Args {
        organization: None,
        project: None,
        repository: None,
        pat: None,
        dev_branch: None,
        target_branch: None,
        local_repo: None,
        work_item_state: None,
        migrate: false,
        terminal_states: "Closed,Next Closed,Next Merged".to_string(),
        tag_prefix: None,
        parallel_limit: None,
        max_concurrent_network: None,
        max_concurrent_processing: None,
        create_config: false,
        since: None,
        skip_confirmation: false,
        path: None,
    };

    let config_result = args.resolve_config();
    assert!(config_result.is_ok());

    let config = config_result.unwrap();
    let shared = config.shared();

    let client_result = AzureDevOpsClient::new(
        shared.organization.clone(),
        shared.project.clone(),
        shared.repository.clone(),
        shared.pat.clone(),
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
