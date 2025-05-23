use crate::{
    api::AzureDevOpsClient,
    models::{CherryPickItem, PullRequestWithWorkItems},
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
    pub client: AzureDevOpsClient,

    // Runtime state
    pub version: Option<String>,
    pub repo_path: Option<std::path::PathBuf>,
    pub _temp_dir: Option<TempDir>, // Keeps temp directory alive
    pub cherry_pick_items: Vec<CherryPickItem>,
    pub current_cherry_pick_index: usize,
    pub error_message: Option<String>,
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
            client,
            version: None,
            repo_path: None,
            _temp_dir: None,
            cherry_pick_items: Vec::new(),
            current_cherry_pick_index: 0,
            error_message: None,
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
}
