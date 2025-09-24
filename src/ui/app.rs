use crate::{
    api::AzureDevOpsClient,
    models::{AppConfig, CherryPickItem, MigrationAnalysis, PullRequestWithWorkItems},
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
    pub _temp_dir: Option<TempDir>, // Keeps temp directory alive
    pub cherry_pick_items: Vec<CherryPickItem>,
    pub current_cherry_pick_index: usize,
    pub error_message: Option<String>,

    // Migration state
    pub migration_analysis: Option<MigrationAnalysis>,

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
            _temp_dir: None,
            cherry_pick_items: Vec::new(),
            current_cherry_pick_index: 0,
            error_message: None,
            migration_analysis: None,
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
}
