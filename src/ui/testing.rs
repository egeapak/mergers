use crate::{
    api::AzureDevOpsClient,
    models::{AppConfig, DefaultModeConfig, MigrationModeConfig, SharedConfig},
    parsed_property::ParsedProperty,
    ui::{App, state::AppState},
};
use ratatui::{Terminal, backend::TestBackend};
use std::{path::PathBuf, sync::Arc};

/// Fixed terminal dimensions for consistent snapshot testing
pub const TEST_TERMINAL_WIDTH: u16 = 80;
pub const TEST_TERMINAL_HEIGHT: u16 = 30;

/// Test harness for TUI components with fixed terminal size
pub struct TuiTestHarness {
    pub terminal: Terminal<TestBackend>,
    pub app: App,
}

impl Default for TuiTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiTestHarness {
    /// Create a new test harness with a minimal app configuration
    pub fn new() -> Self {
        let backend = TestBackend::new(TEST_TERMINAL_WIDTH, TEST_TERMINAL_HEIGHT);
        let terminal = Terminal::new(backend).unwrap();

        let config = create_test_config_default();
        let client = create_test_client();
        let app = App::new(Vec::new(), Arc::new(config), client);

        Self { terminal, app }
    }

    /// Create a test harness with a specific configuration
    pub fn with_config(config: AppConfig) -> Self {
        let backend = TestBackend::new(TEST_TERMINAL_WIDTH, TEST_TERMINAL_HEIGHT);
        let terminal = Terminal::new(backend).unwrap();

        let client = create_test_client();
        let app = App::new(Vec::new(), Arc::new(config), client);

        Self { terminal, app }
    }

    /// Set an error message on the app (for error state testing)
    pub fn with_error_message(mut self, message: impl Into<String>) -> Self {
        self.app.error_message = Some(message.into());
        self
    }

    /// Render a state to the terminal
    pub fn render_state(&mut self, mut state: Box<dyn AppState>) {
        self.terminal.draw(|f| state.ui(f, &self.app)).unwrap();
    }

    /// Get the terminal backend for snapshot testing
    pub fn backend(&self) -> &TestBackend {
        self.terminal.backend()
    }
}

/// Create a test Azure DevOps client (minimal implementation for testing)
fn create_test_client() -> AzureDevOpsClient {
    AzureDevOpsClient::new(
        "test-org".to_string(),
        "test-project".to_string(),
        "test-repo".to_string(),
        "test-pat".to_string(),
    )
    .unwrap()
}

/// Create a default test configuration with all fields populated
pub fn create_test_config_default() -> AppConfig {
    AppConfig::Default {
        shared: create_test_shared_config(),
        default: DefaultModeConfig {
            work_item_state: ParsedProperty::Default("Next Merged".to_string()),
        },
    }
}

/// Create a migration mode test configuration
pub fn create_test_config_migration() -> AppConfig {
    AppConfig::Migration {
        shared: create_test_shared_config(),
        migration: MigrationModeConfig {
            terminal_states: ParsedProperty::Default(vec![
                "Closed".to_string(),
                "Resolved".to_string(),
            ]),
        },
    }
}

/// Create a shared configuration with mixed sources for testing
fn create_test_shared_config() -> SharedConfig {
    SharedConfig {
        organization: ParsedProperty::Cli("test-org".to_string(), "test-org".to_string()),
        project: ParsedProperty::Env(
            "test-project".to_string(),
            "MERGERS_PROJECT=test-project".to_string(),
        ),
        repository: ParsedProperty::File(
            "test-repo".to_string(),
            PathBuf::from("/test/config.toml"),
            "repository = \"test-repo\"".to_string(),
        ),
        pat: ParsedProperty::Default("test-pat".to_string()),
        dev_branch: ParsedProperty::Git("develop".to_string(), "origin/develop".to_string()),
        target_branch: ParsedProperty::Default("main".to_string()),
        local_repo: Some(ParsedProperty::Cli(
            "/path/to/repo".to_string(),
            "/path/to/repo".to_string(),
        )),
        parallel_limit: ParsedProperty::Default(4),
        max_concurrent_network: ParsedProperty::Default(10),
        max_concurrent_processing: ParsedProperty::Default(5),
        tag_prefix: ParsedProperty::Default("merged/".to_string()),
        since: None,
        skip_confirmation: false,
    }
}

/// Create a configuration with all default values
pub fn create_test_config_all_defaults() -> AppConfig {
    AppConfig::Default {
        shared: SharedConfig {
            organization: ParsedProperty::Default("default-org".to_string()),
            project: ParsedProperty::Default("default-project".to_string()),
            repository: ParsedProperty::Default("default-repo".to_string()),
            pat: ParsedProperty::Default("default-pat".to_string()),
            dev_branch: ParsedProperty::Default("develop".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(4),
            max_concurrent_network: ParsedProperty::Default(10),
            max_concurrent_processing: ParsedProperty::Default(5),
            tag_prefix: ParsedProperty::Default("merged/".to_string()),
            since: None,
            skip_confirmation: false,
        },
        default: DefaultModeConfig {
            work_item_state: ParsedProperty::Default("Next Merged".to_string()),
        },
    }
}

/// Create a configuration with CLI-provided values
pub fn create_test_config_cli_values() -> AppConfig {
    AppConfig::Default {
        shared: SharedConfig {
            organization: ParsedProperty::Cli("cli-org".to_string(), "cli-org".to_string()),
            project: ParsedProperty::Cli("cli-project".to_string(), "cli-project".to_string()),
            repository: ParsedProperty::Cli("cli-repo".to_string(), "cli-repo".to_string()),
            pat: ParsedProperty::Cli("cli-pat".to_string(), "cli-pat".to_string()),
            dev_branch: ParsedProperty::Cli(
                "feature-branch".to_string(),
                "feature-branch".to_string(),
            ),
            target_branch: ParsedProperty::Cli(
                "release-branch".to_string(),
                "release-branch".to_string(),
            ),
            local_repo: Some(ParsedProperty::Cli(
                "/cli/path/to/repo".to_string(),
                "/cli/path/to/repo".to_string(),
            )),
            parallel_limit: ParsedProperty::Cli(8, "8".to_string()),
            max_concurrent_network: ParsedProperty::Cli(20, "20".to_string()),
            max_concurrent_processing: ParsedProperty::Cli(10, "10".to_string()),
            tag_prefix: ParsedProperty::Cli("cli-prefix/".to_string(), "cli-prefix/".to_string()),
            since: Some(ParsedProperty::Cli(
                chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .into(),
                "2024-01-01".to_string(),
            )),
            skip_confirmation: false,
        },
        default: DefaultModeConfig {
            work_item_state: ParsedProperty::Cli("Done".to_string(), "Done".to_string()),
        },
    }
}

/// Create a configuration with environment variable values
pub fn create_test_config_env_values() -> AppConfig {
    AppConfig::Default {
        shared: SharedConfig {
            organization: ParsedProperty::Env(
                "env-org".to_string(),
                "MERGERS_ORGANIZATION=env-org".to_string(),
            ),
            project: ParsedProperty::Env(
                "env-project".to_string(),
                "MERGERS_PROJECT=env-project".to_string(),
            ),
            repository: ParsedProperty::Env(
                "env-repo".to_string(),
                "MERGERS_REPOSITORY=env-repo".to_string(),
            ),
            pat: ParsedProperty::Env("env-pat".to_string(), "MERGERS_PAT=env-pat".to_string()),
            dev_branch: ParsedProperty::Env(
                "env-dev".to_string(),
                "MERGERS_DEV_BRANCH=env-dev".to_string(),
            ),
            target_branch: ParsedProperty::Env(
                "env-target".to_string(),
                "MERGERS_TARGET_BRANCH=env-target".to_string(),
            ),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(4),
            max_concurrent_network: ParsedProperty::Default(10),
            max_concurrent_processing: ParsedProperty::Default(5),
            tag_prefix: ParsedProperty::Default("merged/".to_string()),
            since: None,
            skip_confirmation: false,
        },
        default: DefaultModeConfig {
            work_item_state: ParsedProperty::Default("Next Merged".to_string()),
        },
    }
}

/// Create a configuration with file-based values
pub fn create_test_config_file_values() -> AppConfig {
    AppConfig::Default {
        shared: SharedConfig {
            organization: ParsedProperty::File(
                "file-org".to_string(),
                PathBuf::from("/home/user/.config/mergers/config.toml"),
                "organization = \"file-org\"".to_string(),
            ),
            project: ParsedProperty::File(
                "file-project".to_string(),
                PathBuf::from("/home/user/.config/mergers/config.toml"),
                "project = \"file-project\"".to_string(),
            ),
            repository: ParsedProperty::File(
                "file-repo".to_string(),
                PathBuf::from("/home/user/.config/mergers/config.toml"),
                "repository = \"file-repo\"".to_string(),
            ),
            pat: ParsedProperty::Default("default-pat".to_string()),
            dev_branch: ParsedProperty::Default("develop".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: Some(ParsedProperty::File(
                "/file/path/to/repo".to_string(),
                PathBuf::from("/home/user/.config/mergers/config.toml"),
                "local_repo = \"/file/path/to/repo\"".to_string(),
            )),
            parallel_limit: ParsedProperty::Default(4),
            max_concurrent_network: ParsedProperty::Default(10),
            max_concurrent_processing: ParsedProperty::Default(5),
            tag_prefix: ParsedProperty::Default("merged/".to_string()),
            since: None,
            skip_confirmation: false,
        },
        default: DefaultModeConfig {
            work_item_state: ParsedProperty::Default("Next Merged".to_string()),
        },
    }
}
