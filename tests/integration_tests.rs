//! Integration tests for the merge-tool library
//!
//! These tests demonstrate how to use the library APIs and verify
//! end-to-end functionality.

use merge_tool::{AzureDevOpsClient, Config};

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

#[test]
fn test_library_version() {
    // Test that version constant is accessible
    let version = merge_tool::VERSION;
    assert!(!version.is_empty());
    assert!(version.contains('.'));
}

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
