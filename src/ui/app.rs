use crate::{
    api::AzureDevOpsClient,
    models::{
        AppConfig, CherryPickItem, CleanupBranch, MigrationAnalysis, PullRequestWithWorkItems,
    },
    ui::state::AppState,
};
use std::{process::Command, sync::Arc};
use tempfile::TempDir;

pub struct App {
    pub config: Arc<AppConfig>,
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    pub client: AzureDevOpsClient,

    // Runtime state
    pub version: Option<String>,
    pub repo_path: Option<std::path::PathBuf>,
    pub base_repo_path: Option<std::path::PathBuf>, // Original repo path (for worktree cleanup)
    pub _temp_dir: Option<TempDir>,                 // Keeps temp directory alive
    pub cherry_pick_items: Vec<CherryPickItem>,
    pub current_cherry_pick_index: usize,
    pub error_message: Option<String>,

    // Migration state
    pub migration_analysis: Option<MigrationAnalysis>,

    // Cleanup state
    pub cleanup_branches: Vec<CleanupBranch>,

    // Initial state for state machine
    pub initial_state: Option<Box<dyn AppState>>,
}

impl App {
    pub fn new(
        pull_requests: Vec<PullRequestWithWorkItems>,
        config: Arc<AppConfig>,
        client: AzureDevOpsClient,
    ) -> Self {
        Self {
            config,
            pull_requests,
            client,
            version: None,
            repo_path: None,
            base_repo_path: None,
            _temp_dir: None,
            cherry_pick_items: Vec::new(),
            current_cherry_pick_index: 0,
            error_message: None,
            migration_analysis: None,
            cleanup_branches: Vec::new(),
            initial_state: None,
        }
    }

    // Configuration getters
    pub fn organization(&self) -> &str {
        self.config.shared().organization.value()
    }

    pub fn project(&self) -> &str {
        self.config.shared().project.value()
    }

    pub fn repository(&self) -> &str {
        self.config.shared().repository.value()
    }

    pub fn dev_branch(&self) -> &str {
        self.config.shared().dev_branch.value()
    }

    pub fn target_branch(&self) -> &str {
        self.config.shared().target_branch.value()
    }

    pub fn local_repo(&self) -> Option<&str> {
        self.config
            .shared()
            .local_repo
            .as_ref()
            .map(|p| p.value().as_str())
    }

    pub fn work_item_state(&self) -> &str {
        match &*self.config {
            AppConfig::Default { default, .. } => default.work_item_state.value(),
            AppConfig::Migration { .. } => "Next Merged", // Default fallback for migration mode
            AppConfig::Cleanup { .. } => "Next Merged",   // Default fallback for cleanup mode
        }
    }

    pub fn max_concurrent_network(&self) -> usize {
        *self.config.shared().max_concurrent_network.value()
    }

    pub fn max_concurrent_processing(&self) -> usize {
        *self.config.shared().max_concurrent_processing.value()
    }

    pub fn tag_prefix(&self) -> &str {
        self.config.shared().tag_prefix.value()
    }

    pub fn since(&self) -> Option<&str> {
        self.config
            .shared()
            .since
            .as_ref()
            .and_then(|d| d.original())
    }

    pub fn get_selected_prs(&self) -> Vec<&PullRequestWithWorkItems> {
        let mut prs = self
            .pull_requests
            .iter()
            .filter(|pr| pr.selected)
            .collect::<Vec<_>>();
        prs.sort_by_key(|pr| pr.pr.closed_date.as_ref().unwrap());
        prs
    }

    pub fn open_pr_in_browser(&self, pr_id: i32) {
        let url = format!(
            "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{}",
            self.organization(),
            self.project(),
            self.repository(),
            pr_id
        );

        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg(&url).spawn();

        #[cfg(target_os = "linux")]
        let _ = Command::new("xdg-open").arg(&url).spawn();

        #[cfg(target_os = "windows")]
        let _ = Command::new("cmd").args(&["/C", "start", &url]).spawn();
    }

    pub fn open_work_items_in_browser(&self, work_items: &[crate::models::WorkItem]) {
        for wi in work_items {
            let url = format!(
                "https://dev.azure.com/{}/{}/_workitems/edit/{}",
                self.organization(),
                self.project(),
                wi.id
            );

            #[cfg(target_os = "macos")]
            let _ = Command::new("open").arg(&url).spawn();

            #[cfg(target_os = "linux")]
            let _ = Command::new("xdg-open").arg(&url).spawn();

            #[cfg(target_os = "windows")]
            let _ = Command::new("cmd").args(&["/C", "start", &url]).spawn();
        }
    }

    /// Mark a PR as manually eligible - moves it to eligible regardless of automatic analysis
    pub fn mark_pr_as_eligible(&mut self, pr_id: i32) {
        if let Some(analysis) = &mut self.migration_analysis {
            // Remove from not eligible set if present
            analysis
                .manual_overrides
                .marked_as_not_eligible
                .remove(&pr_id);
            // Add to eligible set
            analysis.manual_overrides.marked_as_eligible.insert(pr_id);
            // Recategorize with new overrides
            self.recategorize_prs();
        }
    }

    /// Mark a PR as manually not eligible - moves it to not merged regardless of automatic analysis  
    pub fn mark_pr_as_not_eligible(&mut self, pr_id: i32) {
        if let Some(analysis) = &mut self.migration_analysis {
            // Remove from eligible set if present
            analysis.manual_overrides.marked_as_eligible.remove(&pr_id);
            // Add to not eligible set
            analysis
                .manual_overrides
                .marked_as_not_eligible
                .insert(pr_id);
            // Recategorize with new overrides
            self.recategorize_prs();
        }
    }

    /// Remove manual override for a PR - returns it to automatic categorization
    pub fn remove_manual_override(&mut self, pr_id: i32) {
        if let Some(analysis) = &mut self.migration_analysis {
            analysis.manual_overrides.marked_as_eligible.remove(&pr_id);
            analysis
                .manual_overrides
                .marked_as_not_eligible
                .remove(&pr_id);
            // Recategorize with updated overrides
            self.recategorize_prs();
        }
    }

    /// Check if a PR has a manual override
    pub fn has_manual_override(&self, pr_id: i32) -> Option<bool> {
        if let Some(analysis) = &self.migration_analysis {
            if analysis
                .manual_overrides
                .marked_as_eligible
                .contains(&pr_id)
            {
                Some(true) // manually marked eligible
            } else if analysis
                .manual_overrides
                .marked_as_not_eligible
                .contains(&pr_id)
            {
                Some(false) // manually marked not eligible
            } else {
                None // no manual override
            }
        } else {
            None
        }
    }

    /// Recategorize all PRs with current manual overrides
    fn recategorize_prs(&mut self) {
        if let Some(analysis) = self.migration_analysis.take() {
            // Create a new analyzer instance with the same parameters
            let analyzer = crate::migration::MigrationAnalyzer::new(
                self.client.clone(),
                analysis.terminal_states.clone(),
            );

            // Recategorize with current overrides
            if let Ok(new_analysis) = analyzer.categorize_prs_with_overrides(
                analysis.all_details.clone(),
                analysis.manual_overrides.clone(),
            ) {
                self.migration_analysis = Some(new_analysis);
            } else {
                // If recategorization fails, restore the original analysis
                self.migration_analysis = Some(analysis);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::AzureDevOpsClient,
        models::{DefaultModeConfig, SharedConfig},
        parsed_property::ParsedProperty,
    };

    /// # App Parallel Limit Configuration
    ///
    /// Tests that the application correctly configures parallel processing limits.
    ///
    /// ## Test Scenario
    /// - Creates an app instance with specific parallel limit settings
    /// - Validates that parallel limits are properly applied
    ///
    /// ## Expected Outcome
    /// - Parallel processing limits are correctly configured
    /// - App respects the specified concurrency constraints
    #[test]
    fn test_app_parallel_limit_configuration() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        // Create shared configuration for testing
        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("test_org".to_string()),
            project: ParsedProperty::Default("test_project".to_string()),
            repository: ParsedProperty::Default("test_repo".to_string()),
            pat: ParsedProperty::Default("test_pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("next".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        // Test default configuration
        let config_default = AppConfig::Default {
            shared: shared_config.clone(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        };
        let app_default = App::new(Vec::new(), Arc::new(config_default), client.clone());
        assert_eq!(app_default.max_concurrent_network(), 100);
        assert_eq!(app_default.max_concurrent_processing(), 10);

        // Test custom configuration
        let shared_config_custom = SharedConfig {
            max_concurrent_network: ParsedProperty::Default(150),
            max_concurrent_processing: ParsedProperty::Default(20),
            ..shared_config
        };
        let config_custom = AppConfig::Default {
            shared: shared_config_custom,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        };
        let app_custom = App::new(Vec::new(), Arc::new(config_custom), client);
        assert_eq!(app_custom.max_concurrent_network(), 150);
        assert_eq!(app_custom.max_concurrent_processing(), 20);
    }

    /// # App Configuration Property Accessors
    ///
    /// Tests all configuration property accessor methods on App struct.
    ///
    /// ## Test Scenario
    /// - Creates App with various configuration values
    /// - Tests all property accessor methods
    ///
    /// ## Expected Outcome
    /// - All accessors return correct values from Arc<AppConfig>
    /// - Property access is consistent and reliable
    #[test]
    fn test_app_configuration_property_accessors() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Cli("my_org".to_string(), "my_org".to_string()),
            project: ParsedProperty::Env("my_project".to_string(), "my_project".to_string()),
            repository: ParsedProperty::Git("my_repo".to_string(), "git_url".to_string()),
            pat: ParsedProperty::File(
                "my_pat".to_string(),
                std::path::PathBuf::from("config.toml"),
                "my_pat".to_string(),
            ),
            dev_branch: ParsedProperty::Default("develop".to_string()),
            target_branch: ParsedProperty::Cli("production".to_string(), "production".to_string()),
            local_repo: Some(ParsedProperty::Cli(
                "/path/to/repo".to_string(),
                "/path/to/repo".to_string(),
            )),
            parallel_limit: ParsedProperty::Default(500),
            max_concurrent_network: ParsedProperty::Env(200, "200".to_string()),
            max_concurrent_processing: ParsedProperty::File(
                25,
                std::path::PathBuf::from("config.toml"),
                "25".to_string(),
            ),
            tag_prefix: ParsedProperty::Git("release-".to_string(), "git_url".to_string()),
            since: Some(ParsedProperty::Cli(
                chrono::Utc::now() - chrono::Duration::days(7),
                "1w".to_string(),
            )),
            skip_confirmation: true,
        };

        let config = AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Cli(
                    "Completed".to_string(),
                    "Completed".to_string(),
                ),
            },
        };

        let app = App::new(Vec::new(), Arc::new(config), client);

        // Test basic property accessors
        assert_eq!(app.organization(), "my_org");
        assert_eq!(app.project(), "my_project");
        assert_eq!(app.repository(), "my_repo");
        assert_eq!(app.dev_branch(), "develop");
        assert_eq!(app.target_branch(), "production");
        assert_eq!(app.tag_prefix(), "release-");
        assert_eq!(app.work_item_state(), "Completed");

        // Test numeric property accessors
        assert_eq!(app.max_concurrent_network(), 200);
        assert_eq!(app.max_concurrent_processing(), 25);

        // Test optional property accessors
        assert_eq!(app.local_repo(), Some("/path/to/repo"));
        assert!(app.since().is_some());
    }

    /// # App Property Accessors with Optional Fields
    ///
    /// Tests property accessors when optional fields are None.
    ///
    /// ## Test Scenario
    /// - Creates App config with optional fields set to None
    /// - Tests accessor behavior with missing values
    ///
    /// ## Expected Outcome
    /// - Optional accessors return None appropriately
    /// - No panics or errors when accessing missing fields
    #[test]
    fn test_app_property_accessors_with_optional_none() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("test_org".to_string()),
            project: ParsedProperty::Default("test_project".to_string()),
            repository: ParsedProperty::Default("test_repo".to_string()),
            pat: ParsedProperty::Default("test_pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: None, // Explicitly None
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None, // Explicitly None
            skip_confirmation: false,
        };

        let config = AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Next Merged".to_string()),
            },
        };

        let app = App::new(Vec::new(), Arc::new(config), client);

        // Test that None optional fields return None
        assert_eq!(app.local_repo(), None);
        assert_eq!(app.since(), None);

        // Test that required fields still work
        assert_eq!(app.organization(), "test_org");
        assert_eq!(app.project(), "test_project");
        assert_eq!(app.repository(), "test_repo");
    }

    /// # App Mode-Specific Property Access
    ///
    /// Tests property access behavior in different app modes (Default vs Migration).
    ///
    /// ## Test Scenario
    /// - Creates App instances in Default and Migration modes
    /// - Tests mode-specific property access
    ///
    /// ## Expected Outcome
    /// - Mode-specific properties return appropriate values
    /// - Shared properties work consistently across modes
    #[test]
    fn test_app_mode_specific_property_access() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("test_org".to_string()),
            project: ParsedProperty::Default("test_project".to_string()),
            repository: ParsedProperty::Default("test_repo".to_string()),
            pat: ParsedProperty::Default("test_pat".to_string()),
            dev_branch: ParsedProperty::Default("dev".to_string()),
            target_branch: ParsedProperty::Default("main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("merged-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        // Test Default mode
        let default_config = AppConfig::Default {
            shared: shared_config.clone(),
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Cli(
                    "Custom State".to_string(),
                    "Custom State".to_string(),
                ),
            },
        };
        let default_app = App::new(Vec::new(), Arc::new(default_config), client.clone());
        assert_eq!(default_app.work_item_state(), "Custom State");
        assert!(!default_app.config.is_migration_mode());

        // Test Migration mode
        let migration_config = AppConfig::Migration {
            shared: shared_config,
            migration: crate::models::MigrationModeConfig {
                terminal_states: ParsedProperty::Default(vec![
                    "Closed".to_string(),
                    "Done".to_string(),
                ]),
            },
        };
        let migration_app = App::new(Vec::new(), Arc::new(migration_config), client);
        assert_eq!(migration_app.work_item_state(), "Next Merged"); // Default fallback for migration mode
        assert!(migration_app.config.is_migration_mode());

        // Test that shared properties work the same in both modes
        assert_eq!(default_app.organization(), migration_app.organization());
        assert_eq!(default_app.project(), migration_app.project());
        assert_eq!(default_app.repository(), migration_app.repository());
    }

    /// # App Property Access Consistency
    ///
    /// Tests that property accessors return consistent values across multiple calls.
    ///
    /// ## Test Scenario
    /// - Creates App and calls property accessors multiple times
    /// - Tests that Arc sharing doesn't affect consistency
    ///
    /// ## Expected Outcome
    /// - Property accessors return same values on multiple calls
    /// - Arc<AppConfig> provides stable, consistent access
    #[test]
    fn test_app_property_access_consistency() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("stable_org".to_string()),
            project: ParsedProperty::Default("stable_project".to_string()),
            repository: ParsedProperty::Default("stable_repo".to_string()),
            pat: ParsedProperty::Default("stable_pat".to_string()),
            dev_branch: ParsedProperty::Default("stable_dev".to_string()),
            target_branch: ParsedProperty::Default("stable_main".to_string()),
            local_repo: Some(ParsedProperty::Default("/stable/path".to_string())),
            parallel_limit: ParsedProperty::Default(400),
            max_concurrent_network: ParsedProperty::Default(120),
            max_concurrent_processing: ParsedProperty::Default(15),
            tag_prefix: ParsedProperty::Default("stable-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Stable State".to_string()),
            },
        };

        let app = App::new(Vec::new(), Arc::new(config), client);

        // Call accessors multiple times and verify consistency
        for _ in 0..5 {
            assert_eq!(app.organization(), "stable_org");
            assert_eq!(app.project(), "stable_project");
            assert_eq!(app.repository(), "stable_repo");
            assert_eq!(app.dev_branch(), "stable_dev");
            assert_eq!(app.target_branch(), "stable_main");
            assert_eq!(app.local_repo(), Some("/stable/path"));
            assert_eq!(app.tag_prefix(), "stable-");
            assert_eq!(app.work_item_state(), "Stable State");
            assert_eq!(app.max_concurrent_network(), 120);
            assert_eq!(app.max_concurrent_processing(), 15);
        }
    }

    /// # Arc Config Sharing Behavior
    ///
    /// Tests that Arc<AppConfig> is truly shared between multiple instances.
    ///
    /// ## Test Scenario
    /// - Creates multiple App instances sharing the same Arc<AppConfig>
    /// - Verifies memory sharing and reference counting
    ///
    /// ## Expected Outcome
    /// - All instances point to the same config memory location
    /// - Arc reference counting works correctly
    #[test]
    fn test_arc_config_sharing_behavior() {
        let client1 = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let client2 = AzureDevOpsClient::new(
            "test_org2".to_string(),
            "test_project2".to_string(),
            "test_repo2".to_string(),
            "test_pat2".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("shared_org".to_string()),
            project: ParsedProperty::Default("shared_project".to_string()),
            repository: ParsedProperty::Default("shared_repo".to_string()),
            pat: ParsedProperty::Default("shared_pat".to_string()),
            dev_branch: ParsedProperty::Default("shared_dev".to_string()),
            target_branch: ParsedProperty::Default("shared_main".to_string()),
            local_repo: Some(ParsedProperty::Default("/shared/path".to_string())),
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("shared-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = Arc::new(AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Shared State".to_string()),
            },
        });

        // Create multiple App instances sharing the same Arc
        let app1 = App::new(Vec::new(), Arc::clone(&config), client1);
        let app2 = App::new(Vec::new(), Arc::clone(&config), client2);

        // Test that both apps access the same config values
        assert_eq!(app1.organization(), app2.organization());
        assert_eq!(app1.project(), app2.project());
        assert_eq!(app1.repository(), app2.repository());
        assert_eq!(app1.dev_branch(), app2.dev_branch());
        assert_eq!(app1.target_branch(), app2.target_branch());
        assert_eq!(app1.local_repo(), app2.local_repo());
        assert_eq!(app1.work_item_state(), app2.work_item_state());
        assert_eq!(app1.max_concurrent_network(), app2.max_concurrent_network());
        assert_eq!(
            app1.max_concurrent_processing(),
            app2.max_concurrent_processing()
        );
        assert_eq!(app1.tag_prefix(), app2.tag_prefix());

        // Test that the Arc instances point to the same memory
        assert!(Arc::ptr_eq(&app1.config, &app2.config));

        // Test Arc reference count (should be at least 3: original + app1 + app2)
        let strong_count = Arc::strong_count(&config);
        assert!(strong_count >= 3);
    }

    /// # Arc Config Memory Efficiency
    ///
    /// Tests memory efficiency of Arc<AppConfig> sharing.
    ///
    /// ## Test Scenario
    /// - Creates many App instances sharing the same Arc<AppConfig>
    /// - Verifies that config is not duplicated in memory
    ///
    /// ## Expected Outcome
    /// - All instances share the same config memory
    /// - Memory usage is efficient through Arc sharing
    #[test]
    fn test_arc_config_memory_efficiency() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("memory_test_org".to_string()),
            project: ParsedProperty::Default("memory_test_project".to_string()),
            repository: ParsedProperty::Default("memory_test_repo".to_string()),
            pat: ParsedProperty::Default("memory_test_pat".to_string()),
            dev_branch: ParsedProperty::Default("memory_test_dev".to_string()),
            target_branch: ParsedProperty::Default("memory_test_main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("memory-test-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = Arc::new(AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Memory Test State".to_string()),
            },
        });

        // Create multiple App instances
        let mut apps = Vec::new();
        for _ in 0..10 {
            apps.push(App::new(Vec::new(), Arc::clone(&config), client.clone()));
        }

        // Verify all apps share the same config memory
        for app in &apps {
            assert!(Arc::ptr_eq(&config, &app.config));
            assert_eq!(app.organization(), "memory_test_org");
            assert_eq!(app.work_item_state(), "Memory Test State");
        }

        // Verify Arc reference count is as expected (original + 10 apps)
        let strong_count = Arc::strong_count(&config);
        assert_eq!(strong_count, 11); // 1 original + 10 apps
    }

    /// # Arc Config Immutability
    ///
    /// Tests that Arc<AppConfig> provides immutable access to configuration.
    ///
    /// ## Test Scenario
    /// - Attempts to verify config immutability through Arc
    /// - Tests that config values cannot be modified
    ///
    /// ## Expected Outcome
    /// - Arc provides read-only access to config
    /// - Config values remain stable and immutable
    #[test]
    fn test_arc_config_immutability() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("immutable_org".to_string()),
            project: ParsedProperty::Default("immutable_project".to_string()),
            repository: ParsedProperty::Default("immutable_repo".to_string()),
            pat: ParsedProperty::Default("immutable_pat".to_string()),
            dev_branch: ParsedProperty::Default("immutable_dev".to_string()),
            target_branch: ParsedProperty::Default("immutable_main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("immutable-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = Arc::new(AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Immutable State".to_string()),
            },
        });

        let app = App::new(Vec::new(), Arc::clone(&config), client);

        // Store original values
        let original_org = app.organization().to_string();
        let original_project = app.project().to_string();
        let original_state = app.work_item_state().to_string();

        // Note: The following operations would be compile-time errors
        // demonstrating that Arc provides immutable access:
        // app.config.shared.organization = ParsedProperty::Default("modified".to_string()); // Error
        // *app.config.shared().organization.value() = "modified".to_string(); // Error

        // Verify values remain unchanged (demonstrating immutability)
        assert_eq!(app.organization(), original_org);
        assert_eq!(app.project(), original_project);
        assert_eq!(app.work_item_state(), original_state);

        // Test with another reference to same Arc
        let app2 = App::new(
            Vec::new(),
            Arc::clone(&config),
            AzureDevOpsClient::new(
                "test_org2".to_string(),
                "test_project2".to_string(),
                "test_repo2".to_string(),
                "test_pat2".to_string(),
            )
            .unwrap(),
        );

        // Both apps should see the same immutable values
        assert_eq!(app.organization(), app2.organization());
        assert_eq!(app.project(), app2.project());
        assert_eq!(app.work_item_state(), app2.work_item_state());
    }

    /// # Arc Config Thread-Safe Access
    ///
    /// Tests thread-safe access to Arc<AppConfig> from multiple threads.
    ///
    /// ## Test Scenario
    /// - Spawns multiple threads accessing the same Arc<AppConfig>
    /// - Verifies concurrent read access works correctly
    ///
    /// ## Expected Outcome
    /// - Arc enables safe concurrent access from multiple threads
    /// - All threads read consistent config values
    #[test]
    fn test_arc_config_thread_safe_access() {
        use std::sync::mpsc;
        use std::thread;

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("thread_safe_org".to_string()),
            project: ParsedProperty::Default("thread_safe_project".to_string()),
            repository: ParsedProperty::Default("thread_safe_repo".to_string()),
            pat: ParsedProperty::Default("thread_safe_pat".to_string()),
            dev_branch: ParsedProperty::Default("thread_safe_dev".to_string()),
            target_branch: ParsedProperty::Default("thread_safe_main".to_string()),
            local_repo: Some(ParsedProperty::Default("/thread/safe/path".to_string())),
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("thread-safe-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = Arc::new(AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Thread Safe State".to_string()),
            },
        });

        // Create channel for collecting results from threads
        let (tx, rx) = mpsc::channel();
        let num_threads = 5;

        // Spawn multiple threads that access the config
        let mut handles = Vec::new();
        for thread_id in 0..num_threads {
            let config_clone = Arc::clone(&config);
            let tx_clone = tx.clone();

            let handle = thread::spawn(move || {
                // Create client in thread
                let client = AzureDevOpsClient::new(
                    format!("thread_org_{}", thread_id),
                    format!("thread_project_{}", thread_id),
                    format!("thread_repo_{}", thread_id),
                    format!("thread_pat_{}", thread_id),
                )
                .unwrap();

                // Create app in thread
                let app = App::new(Vec::new(), config_clone, client);

                // Access config properties
                let results = (
                    thread_id,
                    app.organization().to_string(),
                    app.project().to_string(),
                    app.repository().to_string(),
                    app.dev_branch().to_string(),
                    app.target_branch().to_string(),
                    app.work_item_state().to_string(),
                    app.max_concurrent_network(),
                    app.max_concurrent_processing(),
                    app.tag_prefix().to_string(),
                    app.local_repo().map(|s| s.to_string()),
                );

                tx_clone.send(results).unwrap();
            });

            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Collect all results
        drop(tx); // Close the sending end
        let mut results = Vec::new();
        while let Ok(result) = rx.recv() {
            results.push(result);
        }

        // Verify all threads got the same config values
        assert_eq!(results.len(), num_threads);

        let expected = (
            "thread_safe_org".to_string(),
            "thread_safe_project".to_string(),
            "thread_safe_repo".to_string(),
            "thread_safe_dev".to_string(),
            "thread_safe_main".to_string(),
            "Thread Safe State".to_string(),
            100,
            10,
            "thread-safe-".to_string(),
            Some("/thread/safe/path".to_string()),
        );

        for (
            _thread_id,
            org,
            project,
            repo,
            dev_branch,
            target_branch,
            work_item_state,
            max_net,
            max_proc,
            tag_prefix,
            local_repo,
        ) in results
        {
            assert_eq!(org, expected.0);
            assert_eq!(project, expected.1);
            assert_eq!(repo, expected.2);
            assert_eq!(dev_branch, expected.3);
            assert_eq!(target_branch, expected.4);
            assert_eq!(work_item_state, expected.5);
            assert_eq!(max_net, expected.6);
            assert_eq!(max_proc, expected.7);
            assert_eq!(tag_prefix, expected.8);
            assert_eq!(local_repo, expected.9);
        }
    }

    /// # Arc Config Concurrent Reference Counting
    ///
    /// Tests Arc reference counting under concurrent access.
    ///
    /// ## Test Scenario
    /// - Creates and drops Arc references concurrently from multiple threads
    /// - Verifies Arc reference counting remains consistent
    ///
    /// ## Expected Outcome
    /// - Arc reference counting works correctly under concurrent access
    /// - No memory leaks or use-after-free issues occur
    #[test]
    fn test_arc_config_concurrent_reference_counting() {
        use std::sync::{Barrier, mpsc};
        use std::thread;

        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("concurrent_org".to_string()),
            project: ParsedProperty::Default("concurrent_project".to_string()),
            repository: ParsedProperty::Default("concurrent_repo".to_string()),
            pat: ParsedProperty::Default("concurrent_pat".to_string()),
            dev_branch: ParsedProperty::Default("concurrent_dev".to_string()),
            target_branch: ParsedProperty::Default("concurrent_main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("concurrent-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = Arc::new(AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Concurrent State".to_string()),
            },
        });

        let num_threads = 10;
        let barrier = Arc::new(Barrier::new(num_threads));
        let (tx, rx) = mpsc::channel();

        // Spawn threads that create and drop Arc references simultaneously
        let mut handles = Vec::new();
        for thread_id in 0..num_threads {
            let config_clone = Arc::clone(&config);
            let barrier_clone = Arc::clone(&barrier);
            let tx_clone = tx.clone();

            let handle = thread::spawn(move || {
                // Wait for all threads to be ready
                barrier_clone.wait();

                // Create multiple Arc clones in this thread
                let mut arcs = Vec::new();
                for _ in 0..5 {
                    arcs.push(Arc::clone(&config_clone));
                }

                // Access config through each Arc
                for (i, arc) in arcs.iter().enumerate() {
                    let org = arc.shared().organization.value();
                    assert_eq!(org, "concurrent_org");

                    // Send confirmation that this access worked
                    tx_clone.send((thread_id, i, org.to_string())).unwrap();
                }

                // Arcs will be dropped when this thread exits
            });

            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        drop(tx);

        // Collect results to ensure all accesses succeeded
        let mut access_count = 0;
        while let Ok((_thread_id, _arc_id, org)) = rx.recv() {
            assert_eq!(org, "concurrent_org");
            access_count += 1;
        }

        // Verify all accesses succeeded (10 threads × 5 arcs each)
        assert_eq!(access_count, 50);

        // Original Arc should still be valid and accessible
        assert_eq!(config.shared().organization.value(), "concurrent_org");
        assert_eq!(config.shared().project.value(), "concurrent_project");
    }

    /// # Arc Config Drop Behavior
    ///
    /// Tests proper cleanup when Arc<AppConfig> references are dropped.
    ///
    /// ## Test Scenario
    /// - Creates Arc references and verifies cleanup on drop
    /// - Tests that the last reference properly cleans up
    ///
    /// ## Expected Outcome
    /// - Arc properly manages reference counting
    /// - Memory is cleaned up when last reference is dropped
    #[test]
    fn test_arc_config_drop_behavior() {
        let shared_config = SharedConfig {
            organization: ParsedProperty::Default("drop_test_org".to_string()),
            project: ParsedProperty::Default("drop_test_project".to_string()),
            repository: ParsedProperty::Default("drop_test_repo".to_string()),
            pat: ParsedProperty::Default("drop_test_pat".to_string()),
            dev_branch: ParsedProperty::Default("drop_test_dev".to_string()),
            target_branch: ParsedProperty::Default("drop_test_main".to_string()),
            local_repo: None,
            parallel_limit: ParsedProperty::Default(300),
            max_concurrent_network: ParsedProperty::Default(100),
            max_concurrent_processing: ParsedProperty::Default(10),
            tag_prefix: ParsedProperty::Default("drop-test-".to_string()),
            since: None,
            skip_confirmation: false,
        };

        let config = Arc::new(AppConfig::Default {
            shared: shared_config,
            default: DefaultModeConfig {
                work_item_state: ParsedProperty::Default("Drop Test State".to_string()),
            },
        });

        // Initially should have reference count of 1
        assert_eq!(Arc::strong_count(&config), 1);

        // Create additional references
        let config2 = Arc::clone(&config);
        let config3 = Arc::clone(&config);
        assert_eq!(Arc::strong_count(&config), 3);

        // Test access through all references
        assert_eq!(config.shared().organization.value(), "drop_test_org");
        assert_eq!(config2.shared().organization.value(), "drop_test_org");
        assert_eq!(config3.shared().organization.value(), "drop_test_org");

        // Drop references one by one
        drop(config2);
        assert_eq!(Arc::strong_count(&config), 2);

        drop(config3);
        assert_eq!(Arc::strong_count(&config), 1);

        // Original reference should still work
        assert_eq!(config.shared().organization.value(), "drop_test_org");

        // When config is dropped at end of scope, reference count goes to 0
        // and memory is cleaned up (this is verified by the test not crashing)
    }
}
