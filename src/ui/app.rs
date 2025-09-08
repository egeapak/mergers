use crate::{
    api::AzureDevOpsClient,
    models::{CherryPickItem, MigrationAnalysis, PullRequestWithWorkItems},
    ui::state::AppState,
};
use std::process::Command;
use tempfile::TempDir;

pub struct App {
    pub pull_requests: Vec<PullRequestWithWorkItems>,
    pub organization: String,
    pub project: String,
    pub repository: String,
    pub dev_branch: String,
    pub target_branch: String,
    pub local_repo: Option<String>,
    pub work_item_state: String,
    pub parallel_limit: usize,
    pub max_concurrent_network: usize,
    pub max_concurrent_processing: usize,
    pub tag_prefix: String,
    pub since: Option<String>,
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
        organization: String,
        project: String,
        repository: String,
        dev_branch: String,
        target_branch: String,
        local_repo: Option<String>,
        work_item_state: String,
        parallel_limit: usize,
        max_concurrent_network: usize,
        max_concurrent_processing: usize,
        tag_prefix: String,
        since: Option<String>,
        client: AzureDevOpsClient,
    ) -> Self {
        Self {
            pull_requests,
            organization,
            project,
            repository,
            dev_branch,
            target_branch,
            local_repo,
            work_item_state,
            parallel_limit,
            max_concurrent_network,
            max_concurrent_processing,
            tag_prefix,
            since,
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
            self.organization, self.project, self.repository, pr_id
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
                self.organization, self.project, wi.id
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
                analysis.symmetric_diff.clone(),
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
    use crate::api::AzureDevOpsClient;

    #[test]
    fn test_app_parallel_limit_configuration() {
        let client = AzureDevOpsClient::new(
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "test_pat".to_string(),
        )
        .unwrap();

        // Test default parallel limit
        let app_default = App::new(
            Vec::new(),
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            None,
            "Next Merged".to_string(),
            300,
            100,
            10,
            "merged-".to_string(),
            None,
            client.clone(),
        );
        assert_eq!(app_default.parallel_limit, 300);

        // Test custom parallel limit
        let app_custom = App::new(
            Vec::new(),
            "test_org".to_string(),
            "test_project".to_string(),
            "test_repo".to_string(),
            "dev".to_string(),
            "next".to_string(),
            None,
            "Next Merged".to_string(),
            500,
            100,
            20,
            "merged-".to_string(),
            None,
            client,
        );
        assert_eq!(app_custom.parallel_limit, 500);
    }
}
