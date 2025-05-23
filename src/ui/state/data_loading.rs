use crate::{
    api,
    models::PullRequestWithWorkItems,
    ui::App,
    ui::state::{AppState, StateChange, PullRequestSelectionState},
};
use async_trait::async_trait;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub struct DataLoadingState {
    loading_stage: LoadingStage,
    loaded: bool,
}

#[derive(Debug, Clone)]
enum LoadingStage {
    NotStarted,
    FetchingPullRequests,
    FetchingWorkItems,
    FetchingCommitInfo,
    Complete,
}

impl DataLoadingState {
    pub fn new() -> Self {
        Self {
            loading_stage: LoadingStage::NotStarted,
            loaded: false,
        }
    }

    async fn fetch_pull_requests(&mut self, app: &mut App) -> Result<(), String> {
        self.loading_stage = LoadingStage::FetchingPullRequests;
        
        // Fetch pull requests
        let prs = match app.client.fetch_pull_requests(&app.dev_branch).await {
            Ok(prs) => prs,
            Err(e) => return Err(format!("Failed to fetch pull requests: {}", e)),
        };

        let filtered_prs = api::filter_prs_without_merged_tag(prs);

        if filtered_prs.is_empty() {
            return Err("No pull requests found without merged tags.".to_string());
        }

        // Initialize PRs with empty work items for now
        app.pull_requests = filtered_prs
            .into_iter()
            .map(|pr| PullRequestWithWorkItems {
                pr,
                work_items: Vec::new(),
                selected: false,
            })
            .collect();

        Ok(())
    }

    async fn fetch_work_items(&mut self, app: &mut App) -> Result<(), String> {
        self.loading_stage = LoadingStage::FetchingWorkItems;

        // Fetch work items for each PR
        for pr_with_wi in &mut app.pull_requests {
            let work_items = app.client
                .fetch_work_items_for_pr(pr_with_wi.pr.id)
                .await
                .unwrap_or_default();
            pr_with_wi.work_items = work_items;
        }

        Ok(())
    }

    async fn fetch_commit_info(&mut self, app: &mut App) -> Result<(), String> {
        self.loading_stage = LoadingStage::FetchingCommitInfo;

        for pr_with_wi in &mut app.pull_requests {
            if pr_with_wi.pr.last_merge_commit.is_none() {
                match app.client.fetch_pr_commit(pr_with_wi.pr.id).await {
                    Ok(commit_info) => {
                        pr_with_wi.pr.last_merge_commit = Some(commit_info);
                    }
                    Err(e) => {
                        return Err(format!(
                            "Failed to fetch commit for PR #{}: {}",
                            pr_with_wi.pr.id, e
                        ));
                    }
                }
            }
        }

        self.loading_stage = LoadingStage::Complete;
        Ok(())
    }

    fn get_loading_message(&self) -> &'static str {
        match self.loading_stage {
            LoadingStage::NotStarted => "Initializing...",
            LoadingStage::FetchingPullRequests => "Fetching pull requests...",
            LoadingStage::FetchingWorkItems => "Fetching work items...",
            LoadingStage::FetchingCommitInfo => "Fetching commit information...",
            LoadingStage::Complete => "Loading complete",
        }
    }
}

#[async_trait]
impl AppState for DataLoadingState {
    fn ui(&mut self, f: &mut Frame, _app: &App) {
        let loading = Paragraph::new(self.get_loading_message())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Loading"))
            .alignment(Alignment::Center);
        f.render_widget(loading, f.area());
    }

    async fn process_key(&mut self, code: KeyCode, app: &mut App) -> StateChange {
        // Start loading on first render
        if !self.loaded && code == KeyCode::Null {
            self.loaded = true;
            return StateChange::Keep;
        }

        // Process loading stages
        if self.loaded && code == KeyCode::Null {
            match self.loading_stage {
                LoadingStage::NotStarted => {
                    if let Err(e) = self.fetch_pull_requests(app).await {
                        app.error_message = Some(e);
                        return StateChange::Change(Box::new(super::ErrorState::new()));
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingPullRequests => {
                    if let Err(e) = self.fetch_work_items(app).await {
                        app.error_message = Some(e);
                        return StateChange::Change(Box::new(super::ErrorState::new()));
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingWorkItems => {
                    if let Err(e) = self.fetch_commit_info(app).await {
                        app.error_message = Some(e);
                        return StateChange::Change(Box::new(super::ErrorState::new()));
                    }
                    return StateChange::Keep;
                }
                LoadingStage::FetchingCommitInfo => {
                    // Loading is complete, transition to PR selection
                    return StateChange::Change(Box::new(PullRequestSelectionState::new()));
                }
                LoadingStage::Complete => {
                    // Should not reach here, but transition to PR selection just in case
                    return StateChange::Change(Box::new(PullRequestSelectionState::new()));
                }
            }
        }

        // Allow quitting during loading
        match code {
            KeyCode::Char('q') => StateChange::Exit,
            _ => StateChange::Keep,
        }
    }
}