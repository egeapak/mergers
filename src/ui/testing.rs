use crate::{
    api::AzureDevOpsClient,
    models::{
        AppConfig, CherryPickItem, CherryPickStatus, CleanupModeConfig, CreatedBy,
        DefaultModeConfig, Label, MergeCommit, MigrationAnalysis, MigrationModeConfig, PullRequest,
        PullRequestWithWorkItems, SharedConfig, WorkItem, WorkItemFields,
    },
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

    /// Set the initial state for the app
    pub fn with_initial_state(mut self, state: Box<dyn AppState>) -> Self {
        self.app.initial_state = Some(state);
        self
    }

    /// Run the app loop with mock events until exit or events exhausted.
    ///
    /// This method is useful for integration testing the full app loop.
    ///
    /// # Arguments
    ///
    /// * `event_source` - The mock event source containing events to process
    ///
    /// # Example
    ///
    /// ```ignore
    /// let events = MockEventSource::new()
    ///     .with_key(KeyCode::Down)
    ///     .with_key(KeyCode::Char('q'));
    /// harness.run_with_events(&events).await?;
    /// ```
    pub async fn run_with_events(
        &mut self,
        event_source: &crate::ui::MockEventSource,
    ) -> anyhow::Result<()> {
        crate::ui::run_app_with_events(&mut self.terminal, &mut self.app, event_source).await
    }

    /// Run the app with a sequence of key codes.
    ///
    /// Convenience method that creates a MockEventSource from a list of keys.
    ///
    /// # Arguments
    ///
    /// * `keys` - The sequence of key codes to process
    ///
    /// # Example
    ///
    /// ```ignore
    /// harness.run_with_keys(vec![
    ///     KeyCode::Down,
    ///     KeyCode::Enter,
    ///     KeyCode::Char('q'),
    /// ]).await?;
    /// ```
    pub async fn run_with_keys(
        &mut self,
        keys: Vec<crossterm::event::KeyCode>,
    ) -> anyhow::Result<()> {
        let events = crate::ui::MockEventSource::new();
        for key in keys {
            events.push_key(key);
        }
        self.run_with_events(&events).await
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

/// Create a cleanup mode test configuration
pub fn create_test_config_cleanup() -> AppConfig {
    AppConfig::Cleanup {
        shared: create_test_shared_config(),
        cleanup: CleanupModeConfig {
            target: ParsedProperty::Default("main".to_string()),
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

/// Create a sample pull request for testing
pub fn create_test_pull_request() -> PullRequest {
    PullRequest {
        id: 12345,
        title: "Add new feature for user authentication".to_string(),
        closed_date: Some("2024-01-15T10:30:00Z".to_string()),
        created_by: CreatedBy {
            display_name: "John Doe".to_string(),
        },
        last_merge_commit: Some(MergeCommit {
            commit_id: "abc123def456789".to_string(),
        }),
        labels: Some(vec![
            Label {
                name: "feature".to_string(),
            },
            Label {
                name: "enhancement".to_string(),
            },
        ]),
    }
}

/// Create a sample work item for testing
pub fn create_test_work_item() -> WorkItem {
    WorkItem {
        id: 67890,
        fields: WorkItemFields {
            title: Some("Implement OAuth2 authentication".to_string()),
            state: Some("Active".to_string()),
            work_item_type: Some("User Story".to_string()),
            assigned_to: Some(CreatedBy {
                display_name: "Jane Smith".to_string(),
            }),
            iteration_path: Some("Project\\Sprint 5".to_string()),
            description: Some(
                "<div>Implement OAuth2 authentication for the application</div>".to_string(),
            ),
            repro_steps: None,
        },
        history: vec![],
    }
}

/// Create a list of sample pull requests with work items for testing
pub fn create_test_pull_requests() -> Vec<PullRequestWithWorkItems> {
    vec![
        PullRequestWithWorkItems {
            pr: PullRequest {
                id: 100,
                title: "Fix login bug".to_string(),
                closed_date: Some("2024-01-10T09:00:00Z".to_string()),
                created_by: CreatedBy {
                    display_name: "Alice Johnson".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "fix123abc".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "bug".to_string(),
                }]),
            },
            work_items: vec![WorkItem {
                id: 1001,
                fields: WorkItemFields {
                    title: Some("Login button not responding".to_string()),
                    state: Some("Closed".to_string()),
                    work_item_type: Some("Bug".to_string()),
                    assigned_to: Some(CreatedBy {
                        display_name: "Alice Johnson".to_string(),
                    }),
                    iteration_path: Some("Project\\Sprint 4".to_string()),
                    description: Some("<div>Users unable to click login button</div>".to_string()),
                    repro_steps: Some("<div>1. Navigate to login page<br>2. Click login button<br>3. Nothing happens</div>".to_string()),
                },
                history: vec![],
            }],
            selected: false,
        },
        PullRequestWithWorkItems {
            pr: PullRequest {
                id: 101,
                title: "Update user profile page design".to_string(),
                closed_date: Some("2024-01-12T14:30:00Z".to_string()),
                created_by: CreatedBy {
                    display_name: "Bob Wilson".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "design456def".to_string(),
                }),
                labels: Some(vec![
                    Label {
                        name: "ui".to_string(),
                    },
                    Label {
                        name: "enhancement".to_string(),
                    },
                ]),
            },
            work_items: vec![WorkItem {
                id: 1002,
                fields: WorkItemFields {
                    title: Some("Redesign user profile page".to_string()),
                    state: Some("Active".to_string()),
                    work_item_type: Some("Task".to_string()),
                    assigned_to: Some(CreatedBy {
                        display_name: "Bob Wilson".to_string(),
                    }),
                    iteration_path: Some("Project\\Sprint 5".to_string()),
                    description: Some(
                        "<div>Update the user profile page with new design mockups</div>"
                            .to_string(),
                    ),
                    repro_steps: None,
                },
                history: vec![],
            }],
            selected: false,
        },
        PullRequestWithWorkItems {
            pr: PullRequest {
                id: 102,
                title: "Add analytics tracking".to_string(),
                closed_date: Some("2024-01-14T11:00:00Z".to_string()),
                created_by: CreatedBy {
                    display_name: "Carol Martinez".to_string(),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: "analytics789".to_string(),
                }),
                labels: Some(vec![Label {
                    name: "feature".to_string(),
                }]),
            },
            work_items: vec![
                WorkItem {
                    id: 1003,
                    fields: WorkItemFields {
                        title: Some("Implement Google Analytics".to_string()),
                        state: Some("Resolved".to_string()),
                        work_item_type: Some("User Story".to_string()),
                        assigned_to: Some(CreatedBy {
                            display_name: "Carol Martinez".to_string(),
                        }),
                        iteration_path: Some("Project\\Sprint 5".to_string()),
                        description: Some(
                            "<div>Add Google Analytics tracking to the application</div>"
                                .to_string(),
                        ),
                        repro_steps: None,
                    },
                    history: vec![],
                },
                WorkItem {
                    id: 1004,
                    fields: WorkItemFields {
                        title: Some("Add event tracking for user actions".to_string()),
                        state: Some("Active".to_string()),
                        work_item_type: Some("Task".to_string()),
                        assigned_to: Some(CreatedBy {
                            display_name: "Carol Martinez".to_string(),
                        }),
                        iteration_path: Some("Project\\Sprint 5".to_string()),
                        description: Some(
                            "<div>Track button clicks and page views</div>".to_string(),
                        ),
                        repro_steps: None,
                    },
                    history: vec![],
                },
            ],
            selected: false,
        },
    ]
}

/// Create a list of cherry-pick items for testing
pub fn create_test_cherry_pick_items() -> Vec<CherryPickItem> {
    vec![
        CherryPickItem {
            commit_id: "abc123def456".to_string(),
            pr_id: 100,
            pr_title: "Fix login bug".to_string(),
            status: CherryPickStatus::Success,
        },
        CherryPickItem {
            commit_id: "design456def".to_string(),
            pr_id: 101,
            pr_title: "Update user profile page design".to_string(),
            status: CherryPickStatus::InProgress,
        },
        CherryPickItem {
            commit_id: "analytics789".to_string(),
            pr_id: 102,
            pr_title: "Add analytics tracking".to_string(),
            status: CherryPickStatus::Pending,
        },
        CherryPickItem {
            commit_id: "conflict123".to_string(),
            pr_id: 103,
            pr_title: "Database schema changes".to_string(),
            status: CherryPickStatus::Conflict,
        },
    ]
}

/// Create a sample migration analysis for testing
pub fn create_test_migration_analysis() -> MigrationAnalysis {
    let prs = create_test_pull_requests();
    MigrationAnalysis {
        eligible_prs: vec![prs[0].clone(), prs[1].clone()],
        unsure_prs: vec![],
        not_merged_prs: vec![prs[2].clone()],
        terminal_states: vec!["Closed".to_string(), "Resolved".to_string()],
        unsure_details: Default::default(),
        all_details: Default::default(),
        manual_overrides: Default::default(),
    }
}

/// Create post-completion task items with various statuses for testing
pub fn create_test_post_completion_tasks() -> Vec<crate::ui::state::PostCompletionTaskItem> {
    use crate::ui::state::{PostCompletionTask, PostCompletionTaskItem, TaskStatus};

    vec![
        PostCompletionTaskItem {
            task: PostCompletionTask::TaggingPR {
                pr_id: 100,
                pr_title: "Fix login bug".to_string(),
            },
            status: TaskStatus::Success,
        },
        PostCompletionTaskItem {
            task: PostCompletionTask::UpdatingWorkItem {
                work_item_id: 1001,
                work_item_title: "Implement user authentication".to_string(),
            },
            status: TaskStatus::InProgress,
        },
        PostCompletionTaskItem {
            task: PostCompletionTask::TaggingPR {
                pr_id: 101,
                pr_title: "Update user profile page design".to_string(),
            },
            status: TaskStatus::Pending,
        },
        PostCompletionTaskItem {
            task: PostCompletionTask::UpdatingWorkItem {
                work_item_id: 1002,
                work_item_title: "User profile redesign".to_string(),
            },
            status: TaskStatus::Failed(
                "403 Forbidden: Insufficient permissions to update work item".to_string(),
            ),
        },
        PostCompletionTaskItem {
            task: PostCompletionTask::TaggingPR {
                pr_id: 102,
                pr_title: "Add analytics tracking".to_string(),
            },
            status: TaskStatus::Failed(
                "Network error: Connection timed out after 30 seconds".to_string(),
            ),
        },
        PostCompletionTaskItem {
            task: PostCompletionTask::UpdatingWorkItem {
                work_item_id: 1003,
                work_item_title: "Analytics implementation".to_string(),
            },
            status: TaskStatus::Pending,
        },
        PostCompletionTaskItem {
            task: PostCompletionTask::TaggingPR {
                pr_id: 103,
                pr_title: "Database schema changes".to_string(),
            },
            status: TaskStatus::Pending,
        },
    ]
}

/// Create a large list of pull requests for scrolling tests (50+ items)
pub fn create_large_pr_list() -> Vec<PullRequestWithWorkItems> {
    let mut prs = Vec::new();

    for i in 0..60 {
        prs.push(PullRequestWithWorkItems {
            pr: PullRequest {
                id: 1000 + i,
                title: format!(
                    "Pull Request #{}: Feature implementation for component {}",
                    1000 + i,
                    i
                ),
                closed_date: Some("2024-01-15T10:30:00Z".to_string()),
                created_by: CreatedBy {
                    display_name: format!("Developer {}", i % 10),
                },
                last_merge_commit: Some(MergeCommit {
                    commit_id: format!("commit{:08x}", i * 12345),
                }),
                labels: Some(vec![]),
            },
            work_items: if i % 3 == 0 {
                vec![WorkItem {
                    id: 5000 + i,
                    fields: WorkItemFields {
                        title: Some(format!("Work Item {}", i)),
                        state: Some(
                            ["Active", "Resolved", "Closed", "New"][i as usize % 4].to_string(),
                        ),
                        work_item_type: Some("Task".to_string()),
                        assigned_to: if i % 2 == 0 {
                            Some(CreatedBy {
                                display_name: format!("Developer {}", i % 5),
                            })
                        } else {
                            None
                        },
                        iteration_path: Some("Project\\Sprint 1".to_string()),
                        description: Some("<div>Test work item</div>".to_string()),
                        repro_steps: None,
                    },
                    history: vec![],
                }]
            } else {
                vec![]
            },
            selected: (20..=25).contains(&i), // Select some items in the middle
        });
    }

    prs
}

/// Create a list of work item states for state selection dialog tests
pub fn create_test_work_item_states() -> Vec<String> {
    vec![
        "Active".to_string(),
        "Resolved".to_string(),
        "Closed".to_string(),
        "New".to_string(),
        "Removed".to_string(),
    ]
}
